//! The daemon's Gemini Live session and its IPC-facing event stream.

use std::time::Duration;

use gemini_live::session::{ReconnectPolicy, Session, SessionConfig};
use gemini_live::transport::{Auth, Endpoint, TransportConfig};
use gemini_live::types::{
    AudioTranscriptionConfig, ClientContent, Content, GenerationConfig, Modality, Part,
    PrebuiltVoiceConfig, ServerEvent as GeminiEvent, SetupConfig, SpeechConfig, VoiceConfig,
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
    token_source: GoogleTokenSource,
    project: String,
    location: String,
    model: String,
}

impl LiveConfig {
    fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let project = std::env::var("PROJECT_ID")?;
        if project.trim().is_empty() {
            return Err("PROJECT_ID is empty".into());
        }
        Ok(Self {
            token_source: GoogleTokenSource::new(GoogleOAuthCredentials::from_env()?)?,
            project,
            location: std::env::var("VERTEX_LOCATION").unwrap_or_else(|_| "us-central1".into()),
            model: std::env::var("VERTEX_MODEL").unwrap_or_else(|_| "gemini-3.8-live".into()),
        })
    }

    fn session_config(&self, voice_name: Option<&str>) -> SessionConfig {
        let resource = format!(
            "projects/{}/locations/{}/publishers/google/models/{}",
            self.project, self.location, self.model
        );
        SessionConfig {
            transport: TransportConfig {
                endpoint: Endpoint::VertexAi {
                    location: self.location.clone(),
                },
                auth: Auth::BearerTokenProvider(self.token_source.bearer_provider()),
                ..Default::default()
            },
            setup: SetupConfig {
                model: resource,
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
    let config = match LiveConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            hub.set_state(TurnState::Error, None);
            hub.publish(ServerEvent::Error {
                message: format!("Live configuration failed: {error}"),
            });
            warn!(%error, "Live configuration failed");
            return;
        }
    };

    loop {
        hub.set_state(TurnState::Connecting, None);
        let selected_voice = hub.snapshot().gemini_voice_name;
        let connected = tokio::time::timeout(
            Duration::from_secs(45),
            Session::connect(config.session_config(selected_voice.as_deref())),
        )
        .await;
        match connected {
            Ok(Ok(session)) => {
                let session_id = Uuid::new_v4();
                info!(%session_id, "Gemini Live session ready");
                hub.set_state(TurnState::Ready, Some(session_id));
                handle_session(session, session_id, &hub, &mut commands).await;
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
) {
    let mut transcript = AssistantTranscript::new();
    let mut busy = false;
    let (audio_sender, mut audio_chunks) = mpsc::channel::<AudioChunk>(8);
    let mut microphone: Option<MicCapture> = None;
    let mut playback: Option<AudioPlayback> = None;
    let mut speaker_muted = hub.snapshot().speaker_muted;
    let mut selected_output_device_id = hub.snapshot().selected_output_device_id;
    let mut audio_level_counter = 0_u8;
    let mut last_output_spectrum: Option<tokio::time::Instant> = None;
    let mut mic_turn_deadline: Option<tokio::time::Instant> = None;
    loop {
        tokio::select! {
            Some(command) = commands.recv() => {
                match command {
                    LiveCommand::SendText { text, reply } => {
                        if reply.is_closed() {
                            continue;
                        }
                        if busy {
                            let _ = reply.send(Err(SubmitError::Busy));
                            continue;
                        }
                        let result = session.send_client_content(ClientContent {
                            turns: Some(vec![Content {
                                role: Some("user".into()),
                                parts: vec![Part { text: Some(text), inline_data: None }],
                            }]),
                            turn_complete: Some(true),
                        }).await;
                        match result {
                            Ok(()) => {
                                busy = true;
                                hub.set_state(TurnState::Thinking, Some(session_id));
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
                        if busy || microphone.is_some() {
                            let _ = reply.send(Err(SubmitError::Busy));
                            continue;
                        }
                        match MicCapture::start(audio_sender.clone()).await {
                            Ok(capture) => {
                                microphone = Some(capture);
                                busy = true;
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
                        let Some(capture) = microphone.take() else {
                            let _ = reply.send(Err(SubmitError::Unavailable));
                            continue;
                        };
                        let capture_id = capture.id;
                        capture.finish().await;
                        hub.set_microphone_active(false);
                        let mut send_failed = false;
                        while let Ok(chunk) = audio_chunks.try_recv() {
                            if chunk.capture_id == capture_id
                                && let Err(error) = session.send_audio_at_rate(&chunk.pcm, chunk.sample_rate).await {
                                    warn!(%error, "final microphone audio send failed");
                                    send_failed = true;
                                    break;
                                }
                        }
                        if send_failed {
                            let _ = reply.send(Err(SubmitError::Unavailable));
                            break;
                        }
                        match session.audio_stream_end().await {
                            Ok(()) => {
                                hub.set_state(TurnState::Thinking, Some(session_id));
                                mic_turn_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(15));
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
                        if busy || microphone.is_some() || playback.as_ref().is_some_and(|player| !player.is_idle()) {
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
            _ = async {
                if let Some(deadline) = mic_turn_deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                mic_turn_deadline = None;
                busy = false;
                hub.set_state(TurnState::Ready, Some(session_id));
            }
            event = session.next_event() => {
                let Some(event) = event else { break };
                if let Some(text_event) = transcript.observe(&event) {
                    hub.publish(text_event);
                }
                match event {
                    GeminiEvent::ModelAudio(bytes) => {
                        mic_turn_deadline = None;
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
                                        speaker_muted = true;
                                        hub.set_speaker_muted(true);
                                    }
                                }
                            }
                            if let Some(player) = playback.as_ref()
                                && let Err(error) = player.push_pcm(&bytes) {
                                    hub.publish(ServerEvent::Error { message: format!("Speaker playback failed: {error}") });
                            }
                        }
                    }
                    GeminiEvent::OutputTranscription(_) => {
                        mic_turn_deadline = None;
                        hub.set_state(TurnState::Speaking, Some(session_id));
                    }
                    GeminiEvent::InputTranscription(text) => hub.publish(ServerEvent::InputText { text }),
                    GeminiEvent::ToolCall(calls) => {
                        hub.set_state(TurnState::ToolQueued, Some(session_id));
                        for call in calls {
                            hub.publish(ServerEvent::ToolCallRequested { call_id: call.id, name: call.name });
                        }
                    }
                    GeminiEvent::TurnComplete | GeminiEvent::Interrupted => {
                        mic_turn_deadline = None;
                        if matches!(event, GeminiEvent::Interrupted)
                            && let Some(player) = playback.as_ref() {
                                player.clear();
                        }
                        busy = microphone.is_some();
                        hub.set_state(if busy { TurnState::Listening } else { TurnState::Ready }, Some(session_id));
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
    fn selected_voice_is_sent_in_live_setup() {
        let credentials = GoogleOAuthCredentials::new("client", "secret", "refresh").unwrap();
        let config = LiveConfig {
            token_source: GoogleTokenSource::new(credentials).unwrap(),
            project: "project".into(),
            location: "us-central1".into(),
            model: "gemini-3.8-live".into(),
        };
        let session = config.session_config(Some("Kore"));
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
