//! Exercise Gemini Live calling an actual local MCP fixture.

use std::time::Duration;

use gemini_live::session::{ReconnectPolicy, Session, SessionConfig};
use gemini_live::transport::{Auth, Endpoint, TransportConfig};
use gemini_live::types::{
    AudioTranscriptionConfig, ClientContent, Content, FunctionResponse, GenerationConfig, Modality,
    Part, ServerEvent, SetupConfig, Tool,
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

    let documents = std::env::args().any(|arg| arg == "--documents");
    let sandbox = std::env::temp_dir().join(format!("sofia-live-documents-{}", std::process::id()));
    let config = if documents {
        let command = std::env::current_exe()?
            .parent()
            .ok_or("No executable directory")?
            .parent()
            .ok_or("No target directory")?
            .join("sofia-mcp");
        sofia_config::McpServerConfig {
            id: "probe".into(),
            transport: sofia_config::McpTransport::Stdio {
                command: command.to_string_lossy().into_owned(),
                args: vec![],
                env: [
                    (
                        "SOFIA_CONTENT_DB".into(),
                        sandbox.join("content.db").to_string_lossy().into_owned(),
                    ),
                    (
                        "SOFIA_IPC_PATH".into(),
                        sandbox
                            .join("unavailable.sock")
                            .to_string_lossy()
                            .into_owned(),
                    ),
                ]
                .into(),
            },
            ..Default::default()
        }
    } else {
        sofia_config::McpServerConfig {
            name: String::new(),
            id: "probe".into(),
            enabled: true,
            transport: sofia_config::McpTransport::Stdio {
                command: "python3".into(),
                args: vec![format!(
                    "{}/../sofia_mcp_client/tests/fixtures/server.py",
                    env!("CARGO_MANIFEST_DIR")
                )],
                env: Default::default(),
            },
            disabled_tools: vec!["second".into()],
        }
    };
    let (bridge, reports) = sofia_mcp_client::McpBridge::connect(&[config]).await;
    if reports[0].error.is_some() {
        return Err("MCP fixture failed".into());
    }
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
                tools: Some(vec![Tool::FunctionDeclarations(bridge.declarations())]),
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

    let prompt = if documents {
        format!(
            "Call {} to create a note titled Live MCP probe with markdown exactly sofia-documents-ok and open false. You must use the tool, then confirm briefly.",
            sofia_mcp_client::tool_name("probe", "sofia_create_document")
        )
    } else {
        "Call mcp_probe__echo with text exactly sofia-mcp-ok, then say the tool result in one short sentence. You must call the tool.".into()
    };
    session
        .send_client_content(ClientContent {
            turns: Some(vec![Content {
                role: Some("user".into()),
                parts: vec![Part {
                    text: Some(prompt),
                    inline_data: None,
                }],
            }]),
            turn_complete: Some(true),
        })
        .await?;

    let (audio_bytes, transcript) = tokio::time::timeout(Duration::from_secs(45), async {
        let mut audio_bytes = 0usize;
        let mut tool_called = false;
        let mut transcript = String::new();
        let mut output_text = AssistantTranscript::new();
        while let Some(event) = session.next_event().await {
            if let Some(UiEvent::AssistantTextDelta { text, .. }) = output_text.observe(&event) {
                println!("Text delta: {text}");
                transcript.push_str(&text);
            }
            match event {
                ServerEvent::ToolCall(calls) => {
                    let mut responses = Vec::new();
                    for call in calls {
                        let response = bridge.call(&call.name, call.args).await;
                        if response["content"][0]["text"] == "sofia-mcp-ok"
                            || (documents
                                && response["isError"] != true
                                && response["content"][0]["text"]
                                    .as_str()
                                    .is_some_and(|text| text.contains("sofia-documents-ok")))
                        {
                            tool_called = true;
                        }
                        responses.push(FunctionResponse {
                            id: call.id,
                            name: call.name,
                            response,
                        });
                    }
                    session
                        .send_tool_response(responses)
                        .await
                        .map_err(|_| "Tool response failed".to_string())?;
                }
                ServerEvent::ModelAudio(bytes) => audio_bytes += bytes.len(),
                ServerEvent::TurnComplete => {
                    if !tool_called {
                        return Err("Gemini did not complete the MCP tool call".into());
                    }
                    println!("MCP tool call and response succeeded");
                    if audio_bytes > 0 && !transcript.is_empty() {
                        return Ok((audio_bytes, transcript));
                    }
                }
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
    if documents {
        std::fs::remove_dir_all(sandbox)?;
    }
    Ok(())
}
