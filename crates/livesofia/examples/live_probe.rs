//! Credential and Gemini 3.8 Live probe. Never prints bearer or refresh tokens.

use std::time::Duration;

use gemini_live::session::{ReconnectPolicy, Session, SessionConfig};
use gemini_live::transport::{Auth, Endpoint, TransportConfig};
use gemini_live::types::{
    AudioTranscriptionConfig, ClientContent, Content, GenerationConfig, Modality, Part,
    ServerEvent, SetupConfig,
};
use livesofia::oauth::{GoogleOAuthCredentials, GoogleTokenSource};
use livesofia::transcript::AssistantTranscript;
use sofia_protocol::ServerEvent as UiEvent;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::from_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env")).ok();
    let token_source = GoogleTokenSource::new(GoogleOAuthCredentials::from_env()?)?;
    token_source.access_token().await?;
    println!("OAuth refresh succeeded");

    if std::env::args().any(|arg| arg == "--token-only") {
        return Ok(());
    }

    let project = std::env::var("PROJECT_ID")?;
    if project.trim().is_empty() {
        return Err("PROJECT_ID is empty".into());
    }
    let location = std::env::var("VERTEX_LOCATION").unwrap_or_else(|_| "us-central1".into());
    let model = std::env::var("VERTEX_MODEL").unwrap_or_else(|_| "gemini-3.8-live".into());
    let model_resource =
        format!("projects/{project}/locations/{location}/publishers/google/models/{model}");

    let mut session = tokio::time::timeout(
        Duration::from_secs(45),
        Session::connect(SessionConfig {
            transport: TransportConfig {
                endpoint: Endpoint::VertexAi { location },
                auth: Auth::BearerTokenProvider(token_source.bearer_provider()),
                ..Default::default()
            },
            setup: SetupConfig {
                model: model_resource,
                generation_config: Some(GenerationConfig {
                    response_modalities: Some(vec![Modality::Audio]),
                    ..Default::default()
                }),
                output_audio_transcription: Some(AudioTranscriptionConfig::default()),
                ..Default::default()
            },
            reconnect: ReconnectPolicy {
                enabled: false,
                ..Default::default()
            },
        }),
    )
    .await??;
    println!("Gemini Live setup succeeded");

    session
        .send_client_content(ClientContent {
            turns: Some(vec![Content {
                role: Some("user".into()),
                parts: vec![Part {
                    text: Some("Say hello in one short sentence.".into()),
                    inline_data: None,
                }],
            }]),
            turn_complete: Some(true),
        })
        .await?;

    let (audio_bytes, transcript) = tokio::time::timeout(Duration::from_secs(45), async {
        let mut audio_bytes = 0usize;
        let mut transcript = String::new();
        let mut output_text = AssistantTranscript::new();
        while let Some(event) = session.next_event().await {
            if let Some(UiEvent::AssistantTextDelta { text, .. }) = output_text.observe(&event) {
                println!("Text delta: {text}");
                transcript.push_str(&text);
            }
            match event {
                ServerEvent::ModelAudio(bytes) => audio_bytes += bytes.len(),
                ServerEvent::TurnComplete => return Ok((audio_bytes, transcript)),
                ServerEvent::Error(error) => return Err(format!("model error: {}", error.message)),
                ServerEvent::Closed { reason } => {
                    return Err(format!("connection closed: {reason}"));
                }
                _ => {}
            }
        }
        Err("event stream ended before turn completion".into())
    })
    .await??;
    println!("Gemini Live turn complete; audio bytes: {audio_bytes}");
    if !transcript.is_empty() {
        println!("Transcript: {transcript}");
    }
    if audio_bytes == 0 || transcript.is_empty() {
        return Err("turn must contain both spoken audio and output transcription".into());
    }
    Ok(())
}
