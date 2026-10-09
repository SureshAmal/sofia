//! The daemon's Gemini Live session and its IPC-facing event stream.

use std::time::Duration;

use gemini_live::session::{ReconnectPolicy, Session, SessionConfig};
use gemini_live::transport::{Auth, Endpoint, TransportConfig};
use gemini_live::types::{
    AudioTranscriptionConfig, ClientContent, Content, FunctionResponse, GenerationConfig, Modality,
    Part, PrebuiltVoiceConfig, ServerEvent as GeminiEvent, SetupConfig, SpeechConfig, Tool,
    VoiceConfig,
};
use livesofia::oauth::{GoogleOAuthCredentials, GoogleTokenSource};
use livesofia::state::EventHub;
use livesofia::transcript::AssistantTranscript;
use sofia_protocol::{AudioSource, ServerEvent, TurnState};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};
use uuid::Uuid;

use crate::audio::{AudioChunk, AudioPlayback, MicCapture};

pub enum LiveCommand {
    ReloadSettings {
        reply: oneshot::Sender<Result<(), SubmitError>>,
    },
    SendText {
        text: String,
        reply: oneshot::Sender<Result<(), SubmitError>>,
    },
    StartListening {
        reply: oneshot::Sender<Result<(), SubmitError>>,
    },
    StopListening {
        reply: oneshot::Sender<Result<(), SubmitError>>,
    },
    SetSpeakerMuted {
        muted: bool,
        reply: oneshot::Sender<Result<(), SubmitError>>,
    },
    SelectAudioOutput {
        device_id: Option<String>,
        reply: oneshot::Sender<Result<(), SubmitError>>,
    },
    SelectGeminiVoice {
        voice_name: Option<String>,
        reply: oneshot::Sender<Result<(), SubmitError>>,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum SubmitError {
    Busy,
    Unavailable,
    AudioDevice,
    Settings,
}

struct LiveConfig {
    token_source: Option<GoogleTokenSource>,
    api_key: String,
    project: String,
    location: String,
    model: String,
    system_prompt: String,
}

impl LiveConfig {
    fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let settings = sofia_config::load()?;
        settings.validate()?;
        if settings
            .gemini
            .voice_name
            .as_deref()
            .is_some_and(|name| livesofia::voice::canonical_voice_name(name).is_none())
        {
            return Err("Unknown Gemini voice. Choose a supported prebuilt voice.".into());
        }
        if !settings.uses_vertex_ai() {
            let api_key = if settings.generative.api_key.trim().is_empty() {
                std::env::var("GEMINI_API_KEY").unwrap_or_default()
            } else {
                settings.generative.api_key
            };
            if api_key.trim().is_empty() {
                return Err("Gemini API key is required when Vertex AI is disabled.".into());
            }
            return Ok(Self {
                token_source: None,
                api_key,
                project: String::new(),
                location: String::new(),
                model: settings.generative.model,
                system_prompt: settings.assistant.system_prompt,
            });
        }
        let vertex = settings.vertex;
        let setting_or_env = |value: String, key: &str| {
            if value.trim().is_empty() {
                std::env::var(key).unwrap_or_default()
            } else {
                value
            }
        };
        let project = setting_or_env(vertex.project_id, "PROJECT_ID");
        if project.trim().is_empty() {
            return Err("PROJECT_ID is empty".into());
        }
        Ok(Self {
            token_source: Some(GoogleTokenSource::new(GoogleOAuthCredentials::new(
                setting_or_env(vertex.client_id, "CLIENT_ID"),
                setting_or_env(vertex.client_secret, "CLIENT_SECRET"),
                setting_or_env(vertex.refresh_token, "REFRESH_TOKEN"),
            )?)?),
            api_key: String::new(),
            project,
            location: vertex.location,
            model: vertex.model,
            system_prompt: settings.assistant.system_prompt,
        })
    }

    fn session_config(&self, voice_name: Option<&str>) -> SessionConfig {
        let resource = if self.token_source.is_some() {
            format!(
                "projects/{}/locations/{}/publishers/google/models/{}",
                self.project, self.location, self.model
            )
        } else {
            format!("models/{}", self.model.trim_start_matches("models/"))
        };
        let (endpoint, auth) = if let Some(source) = &self.token_source {
            (
                Endpoint::VertexAi {
                    location: self.location.clone(),
                },
                Auth::BearerTokenProvider(source.bearer_provider()),
            )
        } else {
            (Endpoint::GeminiApi, Auth::ApiKey(self.api_key.clone()))
        };
        SessionConfig {
            transport: TransportConfig {
                endpoint,
                auth,
                ..Default::default()
            },
            setup: SetupConfig {
                model: resource,
                system_instruction: Some(Content {
                    role: None,
                    parts: vec![Part {
                        text: Some(self.system_prompt.clone()),
                        inline_data: None,
                    }],
                }),
                generation_config: Some(GenerationConfig {
                    response_modalities: Some(vec![Modality::Audio]),
                    speech_config: voice_name.map(|name| SpeechConfig {
                        voice_config: VoiceConfig {
                            prebuilt_voice_config: PrebuiltVoiceConfig {
                                voice_name: name.into(),
                            },
                        },
                    }),
                    ..Default::default()
                }),
                input_audio_transcription: Some(AudioTranscriptionConfig::default()),
                output_audio_transcription: Some(AudioTranscriptionConfig::default()),
                ..Default::default()
            },
            reconnect: ReconnectPolicy {
                enabled: false,
                ..Default::default()
            },
        }
    }
}

pub async fn run(hub: EventHub, mut commands: mpsc::Receiver<LiveCommand>) {
    loop {
        let config = match LiveConfig::from_env() {
            Ok(config) => config,
            Err(error) => {
                hub.set_state(TurnState::Error, None);
                hub.publish(ServerEvent::Error {
                    message: format!("Live configuration failed: {error}"),
                });
                warn!(%error, "Live configuration failed");
                while let Some(command) = commands.recv().await {
                    if let LiveCommand::ReloadSettings { reply } = command {
                        let _ = reply.send(Ok(()));
                        break;
                    }
                    reject(command, SubmitError::Unavailable);
                }
                continue;
            }
        };

        let settings = sofia_config::load().ok();
        if let Some(settings) = &settings {
            hub.set_output_device(settings.audio.output_device_id.clone());
            hub.set_gemini_voice(settings.gemini.voice_name.clone());
        }
        hub.set_state(TurnState::Connecting, None);
        let mcp_configs = settings
            .as_ref()
            .map(|s| s.mcp_servers.clone())
            .unwrap_or_default();
        let (mcp, reports) = sofia_mcp_client::McpBridge::connect(&mcp_configs).await;
        for report in reports {
            info!(server = %report.id, tools = report.tools, "MCP discovery finished");
            if let Some(error) = report.error {
                hub.publish(ServerEvent::Error {
                    message: format!("MCP {}: {error}", report.id),
                });
            }
        }
        let mcp = std::sync::Arc::new(mcp);
        let selected_voice = hub.snapshot().gemini_voice_name;
        let mut session_config = config.session_config(selected_voice.as_deref());
        let declarations = mcp.declarations();
        if !declarations.is_empty() {
            session_config.setup.tools = Some(vec![Tool::FunctionDeclarations(declarations)]);
        }
        let auto_listen = settings.as_ref().map(|s| s.audio.auto_listen).unwrap_or(true);
        let connected =
            tokio::time::timeout(Duration::from_secs(45), Session::connect(session_config)).await;
        match connected {
            Ok(Ok(session)) => {
                let session_id = Uuid::new_v4();
                info!(%session_id, "Gemini Live session ready");
                hub.set_state(TurnState::Ready, Some(session_id));
                handle_session(session, session_id, &hub, &mut commands, mcp, auto_listen).await;
            }
            Ok(Err(error)) => {
                warn!(%error, "Gemini Live connection failed");
                hub.set_state(TurnState::Error, None);
                hub.publish(ServerEvent::Error {
                    message: format!("Live connection failed: {error}"),
                });
            }
            Err(_) => {
                hub.set_state(TurnState::Error, None);
                hub.publish(ServerEvent::Error {
                    message: "Live connection timed out".into(),
                });
            }
        }
        hub.set_state(TurnState::Reconnecting, None);
        let delay = tokio::time::sleep(Duration::from_secs(3));
        tokio::pin!(delay);
        loop {
            tokio::select! {
                _ = &mut delay => break,
                Some(command) = commands.recv() => reject(command, SubmitError::Unavailable),
            }
        }
    }
}

async fn handle_session(
    mut session: Session,
    session_id: Uuid,
    hub: &EventHub,
    commands: &mut mpsc::Receiver<LiveCommand>,
    mcp: std::sync::Arc<sofia_mcp_client::McpBridge>,
    auto_listen: bool,
) {
    let mut tool_tasks: tokio::task::JoinSet<(String, String, sofia_mcp_client::ToolOutput)> =
        tokio::task::JoinSet::new();
    let mut pending_tools = std::collections::HashMap::<String, tokio::task::AbortHandle>::new();
    let mut completed_tool_responses = Vec::<FunctionResponse>::new();
    let mut completed_tool_images = Vec::<sofia_mcp_client::ToolImage>::new();
    let mut transcript = AssistantTranscript::new();
    let mut busy = false;
    let mut paused = false;
    let (audio_sender, mut audio_chunks) = mpsc::channel::<AudioChunk>(8);
    let mut microphone = if auto_listen {
        match MicCapture::start(audio_sender.clone()).await {
            Ok(capture) => {
                hub.set_microphone_active(true);
                hub.set_state(TurnState::Listening, Some(session_id));
                Some(capture)
            }
            Err(error) => {
                hub.publish(ServerEvent::Error {
                    message: format!("Microphone could not start: {error}"),
                });
                None
            }
        }
    } else {
        None
    };
    let mut playback: Option<AudioPlayback> = None;
    // Tool turns may cause Gemini to narrate each call/result. Keep those
    // intermediate announcements in the UI only; speech resumes for the
    // final response after all tools have returned.
    let mut tool_audio_suppressed = false;
    let mut speaker_muted = hub.snapshot().speaker_muted;
    let mut selected_output_device_id = hub.snapshot().selected_output_device_id;
    let mut audio_level_counter = 0_u8;
    let mut last_output_spectrum: Option<tokio::time::Instant> = None;
    let mut audio_health = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            Some(result) = tool_tasks.join_next(), if !tool_tasks.is_empty() => {
                if let Ok((id, name, output)) = result {
                    pending_tools.remove(&id);
                    if paused { continue; }
                    let response = output.response;
                    completed_tool_images.extend(output.images);
                    let success = response.get("isError").and_then(serde_json::Value::as_bool) != Some(true);
                    hub.publish(ServerEvent::ToolCallFinished { call_id: id.clone(), name: name.clone(), success });
                    if !success {
                        let detail = response.get("error").and_then(serde_json::Value::as_str)
                            .or_else(|| response.get("content")?.get(0)?.get("text")?.as_str())
                            .unwrap_or("Tool returned an error");
                        hub.publish(ServerEvent::Error {
                            message: format!("{}: {}", name, detail.chars().take(512).collect::<String>()),
                        });
                    }
                    // Keep all results from one Gemini tool turn together.
                    // Sending each result immediately can make Gemini start a
                    // separate spoken answer for every completed call.
                    completed_tool_responses.push(FunctionResponse { id, name, response });
                    if pending_tools.is_empty() {
                        for image in std::mem::take(&mut completed_tool_images) {
                            if let Err(error) = session.send_video(&image.bytes, &image.mime_type).await {
                                warn!(%error, "MCP image could not be sent to Gemini");
                                hub.publish(ServerEvent::Error {
                                    message: format!("MCP image could not be sent to Gemini: {error}"),
                                });
                            }
                        }
                        let responses = std::mem::take(&mut completed_tool_responses);
                        if let Err(error) = session.send_tool_response(responses).await {
                            warn!(%error,"MCP result could not be sent to Gemini");
                            break;
                        }
                        tool_audio_suppressed = false;
                        hub.set_state(TurnState::Thinking, Some(session_id));
                    }
                } else if pending_tools.is_empty() {
                    // A cancelled or failed task must not leave all later
                    // model audio suppressed for the rest of the session.
                    tool_audio_suppressed = false;
                    hub.set_state(TurnState::Thinking, Some(session_id));
                }
            }
            _ = audio_health.tick() => {
                if microphone.as_ref().is_some_and(MicCapture::has_stopped) {
                    microphone.take();
                    hub.set_microphone_active(false);
                    hub.publish(ServerEvent::Error { message: "Microphone stream stopped unexpectedly; start listening again.".into() });
                    busy = false;
                    hub.set_state(TurnState::Ready, Some(session_id));
                }
            }
            Some(command) = commands.recv() => {
                match command {
                    LiveCommand::ReloadSettings { reply } => {
                        let _ = reply.send(Ok(()));
                        break;
                    }
                    LiveCommand::SendText { text, reply } => {
                        if reply.is_closed() {
                            continue;
                        }
                        if busy {
                            let _ = reply.send(Err(SubmitError::Busy));
                            continue;
                        }
                        paused = false;
                        let result = session.send_client_content(ClientContent {
                            turns: Some(vec![Content {
                                role: Some("user".into()),
                                parts: vec![Part { text: Some(text.clone()), inline_data: None }],
                            }]),
                            turn_complete: Some(true),
                        }).await;
                        match result {
                            Ok(()) => {
                                busy = true;
                                hub.set_state(TurnState::Thinking, Some(session_id));
                                hub.publish(ServerEvent::InputText { text });
                                let _ = reply.send(Ok(()));
                            }
                            Err(error) => {
                                warn!(%error, "Live text send failed");
                                let _ = reply.send(Err(SubmitError::Unavailable));
                                hub.publish(ServerEvent::Error { message: format!("Live text send failed: {error}") });
                                break;
                            }
                        }
                    }
                    LiveCommand::StartListening { reply } => {
                        if reply.is_closed() { continue; }
                        if microphone.is_some() {
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        if busy {
                            let _ = reply.send(Err(SubmitError::Busy));
                            continue;
                        }
                        match MicCapture::start(audio_sender.clone()).await {
                            Ok(capture) => {
                                paused = false;
                                microphone = Some(capture);
                                hub.set_microphone_active(true);
                                hub.set_state(TurnState::Listening, Some(session_id));
                                let _ = reply.send(Ok(()));
                            }
                            Err(error) => {
                                warn!(%error, "microphone could not start");
                                hub.publish(ServerEvent::Error { message: format!("Microphone could not start: {error}") });
                                let _ = reply.send(Err(SubmitError::AudioDevice));
                            }
                        }
                    }
                    LiveCommand::StopListening { reply } => {
                        paused = true;
                        tool_tasks.abort_all();
                        pending_tools.clear();
                        busy = false;
                        if let Some(player) = playback.as_ref() { player.clear(); }
                        let Some(capture) = microphone.take() else {
                            hub.set_state(TurnState::Ready, Some(session_id));
                            let _ = reply.send(Ok(()));
                            continue;
                        };
                        capture.finish().await;
                        hub.set_microphone_active(false);
                        while audio_chunks.try_recv().is_ok() {}
                        match session.audio_stream_end().await {
                            Ok(()) => {
                                hub.set_state(TurnState::Ready, Some(session_id));
                                let _ = reply.send(Ok(()));
                            }
                            Err(error) => {
                                warn!(%error, "audio stream end failed");
                                let _ = reply.send(Err(SubmitError::Unavailable));
                                break;
                            }
                        }
                    }
                    LiveCommand::SetSpeakerMuted { muted, reply } => {
                        speaker_muted = muted;
                        if muted && let Some(player) = playback.as_ref() {
                            player.clear();
                        }
                        hub.set_speaker_muted(muted);
                        let _ = reply.send(Ok(()));
                    }
                    LiveCommand::SelectAudioOutput { device_id, reply } => {
                        if reply.is_closed() { continue; }
                        if busy || playback.as_ref().is_some_and(|player| !player.is_idle()) {
                            let _ = reply.send(Err(SubmitError::Busy));
                            continue;
                        }
                        match AudioPlayback::open(device_id.as_deref()) {
                            Ok(new_playback) => {
                                if let Err(error) = sofia_config::save_output_device_id(device_id.as_deref()) {
                                    hub.publish(ServerEvent::Error { message: format!("Could not save audio output selection: {error}") });
                                    let _ = reply.send(Err(SubmitError::Settings));
                                    continue;
                                }
                                playback = Some(new_playback);
                                selected_output_device_id = device_id.clone();
                                hub.set_output_device(device_id);
                                let _ = reply.send(Ok(()));
                            }
                            Err(error) => {
                                hub.publish(ServerEvent::Error { message: format!("Output device could not open: {error}") });
                                let _ = reply.send(Err(SubmitError::AudioDevice));
                            }
                        }
                    }
                    LiveCommand::SelectGeminiVoice { voice_name, reply } => {
                        if reply.is_closed() { continue; }
                        if busy || playback.as_ref().is_some_and(|player| !player.is_idle()) {
                            let _ = reply.send(Err(SubmitError::Busy));
                            continue;
                        }
                        if hub.snapshot().gemini_voice_name == voice_name {
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        if let Err(error) = sofia_config::save_gemini_voice_name(voice_name.as_deref()) {
                            hub.publish(ServerEvent::Error { message: format!("Could not save Gemini voice: {error}") });
                            let _ = reply.send(Err(SubmitError::Settings));
                            continue;
                        }
                        hub.set_gemini_voice(voice_name);
                        let _ = reply.send(Ok(()));
                        break;
                    }
                }
            }
            Some(chunk) = audio_chunks.recv() => {
                if microphone.as_ref().is_none_or(|capture| capture.id != chunk.capture_id) {
                    continue;
                }
                if let Err(error) = session.send_audio_at_rate(&chunk.pcm, chunk.sample_rate).await {
                    warn!(%error, "microphone audio send failed");
                    hub.publish(ServerEvent::Error { message: format!("Microphone audio send failed: {error}") });
                    break;
                }
                audio_level_counter = audio_level_counter.wrapping_add(1);
                if audio_level_counter.is_multiple_of(2) {
                    hub.publish(ServerEvent::InputAudioLevel { rms: chunk.rms });
                    hub.publish(ServerEvent::AudioSpectrum {
                        source: AudioSource::User,
                        bins: livesofia::spectrum::pcm_i16_bands(&chunk.pcm, chunk.sample_rate),
                    });
                }
            }
            event = session.next_event() => {
                let Some(event) = event else { break };
                if paused && !matches!(event, GeminiEvent::Error(_) | GeminiEvent::Closed { .. }) {
                    continue;
                }
                if let Some(text_event) = transcript.observe(&event) {
                    hub.publish(text_event);
                }
                match event {
                    GeminiEvent::ModelAudio(bytes) => {
                        if tool_audio_suppressed {
                            continue;
                        }
                        busy = true;
                        hub.set_state(TurnState::Speaking, Some(session_id));
                        let now = tokio::time::Instant::now();
                        if last_output_spectrum.is_none_or(|last| now.duration_since(last) >= Duration::from_millis(80)) {
                            hub.publish(ServerEvent::AudioSpectrum {
                                source: AudioSource::Assistant,
                                bins: livesofia::spectrum::pcm_i16_bands(&bytes, 24_000),
                            });
                            last_output_spectrum = Some(now);
                        }
                        if !speaker_muted {
                            if playback.is_none() {
                                match AudioPlayback::open(selected_output_device_id.as_deref()) {
                                    Ok(player) => playback = Some(player),
                                    Err(error) => {
                                        hub.publish(ServerEvent::Error { message: format!("Speaker could not open: {error}") });
                                    }
                                }
                            }
                            if let Some(player) = playback.as_ref()
                                && let Err(error) = player.push_pcm(&bytes) {
                                    hub.publish(ServerEvent::Error { message: format!("Speaker playback failed: {error}") });
                                    playback = None;
                            }
                        }
                    }
                    GeminiEvent::OutputTranscription(_) => {
                        hub.set_state(TurnState::Speaking, Some(session_id));
                    }
                    GeminiEvent::InputTranscription(text) => hub.publish(ServerEvent::InputText { text }),
                    GeminiEvent::ToolCall(calls) => {
                        busy = true;
                        tool_audio_suppressed = true;
                        if let Some(player) = playback.as_ref() {
                            player.clear();
                        }
                        hub.set_state(TurnState::ToolQueued, Some(session_id));
                        for call in calls {
                            hub.publish(ServerEvent::ToolCallRequested { call_id: call.id.clone(), name: call.name.clone() });
                            if pending_tools.contains_key(&call.id) { continue; }
                            if pending_tools.len() >= 32 {
                                let _ = session.send_tool_response(vec![FunctionResponse { id: call.id, name: call.name, response: serde_json::json!({"isError":true,"error":"Too many simultaneous tool calls"}) }]).await;
                                continue;
                            }
                            let bridge = mcp.clone();
                            let id = call.id.clone();
                            let task = tool_tasks.spawn(async move {
                                let response = bridge.call_with_media(&call.name,call.args).await;
                                (call.id,call.name,response)
                            });
                            pending_tools.insert(id,task);
                        }
                        hub.set_state(TurnState::ToolRunning, Some(session_id));
                    }
                    GeminiEvent::ToolCallCancellation(ids) => {
                        for id in ids {
                            if let Some(task) = pending_tools.remove(&id) {
                                task.abort();
                            }
                        }
                        if pending_tools.is_empty() {
                            tool_audio_suppressed = false;
                        }
                    }
                    GeminiEvent::TurnComplete | GeminiEvent::Interrupted => {
                        if matches!(event, GeminiEvent::Interrupted)
                            && let Some(player) = playback.as_ref() {
                                player.clear();
                        }
                        busy = false;
                        hub.set_state(if microphone.is_some() { TurnState::Listening } else { TurnState::Ready }, Some(session_id));
                    }
                    GeminiEvent::Error(error) => {
                        hub.publish(ServerEvent::Error { message: error.message });
                        break;
                    }
                    GeminiEvent::Closed { reason } => {
                        hub.publish(ServerEvent::Error { message: format!("Live session closed: {reason}") });
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    drop(microphone);
    hub.set_microphone_active(false);
    hub.set_state(TurnState::Disconnected, None);
}

fn reject(command: LiveCommand, reason: SubmitError) {
    match command {
        LiveCommand::SendText { reply, .. }
        | LiveCommand::StartListening { reply }
        | LiveCommand::StopListening { reply }
        | LiveCommand::ReloadSettings { reply }
        | LiveCommand::SetSpeakerMuted { reply, .. }
        | LiveCommand::SelectAudioOutput { reply, .. }
        | LiveCommand::SelectGeminiVoice { reply, .. } => {
            let _ = reply.send(Err(reason));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_mode_uses_generative_endpoint_and_model_resource() {
        let config = LiveConfig {
            token_source: None,
            api_key: "test-key".into(),
            project: String::new(),
            location: String::new(),
            model: "models/test-live".into(),
            system_prompt: sofia_config::DEFAULT_SYSTEM_PROMPT.into(),
        };
        let session = config.session_config(Some("Kore"));
        assert_eq!(session.transport.endpoint, Endpoint::GeminiApi);
        assert!(matches!(session.transport.auth, Auth::ApiKey(ref key) if key == "test-key"));
        assert_eq!(session.setup.model, "models/test-live");
    }

    #[test]
    fn selected_voice_is_sent_in_live_setup() {
        let credentials = GoogleOAuthCredentials::new("client", "secret", "refresh").unwrap();
        let config = LiveConfig {
            token_source: Some(GoogleTokenSource::new(credentials).unwrap()),
            api_key: String::new(),
            project: "project".into(),
            location: "us-central1".into(),
            model: "gemini-3.8-live".into(),
            system_prompt: sofia_config::DEFAULT_SYSTEM_PROMPT.into(),
        };
        let session = config.session_config(Some("Kore"));
        assert!(matches!(
            session.transport.endpoint,
            Endpoint::VertexAi { .. }
        ));
        assert!(matches!(
            session.transport.auth,
            Auth::BearerTokenProvider(_)
        ));
        assert_eq!(
            session.setup.system_instruction.as_ref().unwrap().parts[0]
                .text
                .as_deref(),
            Some(sofia_config::DEFAULT_SYSTEM_PROMPT)
        );
        let voice = session
            .setup
            .generation_config
            .unwrap()
            .speech_config
            .unwrap()
            .voice_config
            .prebuilt_voice_config
            .voice_name;
        assert_eq!(voice, "Kore");
    }
}
