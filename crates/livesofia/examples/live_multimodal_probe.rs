//! Live function-call, still-image, and streamed-video probe.
//! Run scripts/create_multimodal_fixtures.sh first for the default media files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use gemini_live::session::{ReconnectPolicy, Session, SessionConfig};
use gemini_live::transport::{Auth, Endpoint, TransportConfig};
use gemini_live::types::{
    AudioTranscriptionConfig, ClientContent, Content, FunctionDeclaration, FunctionResponse,
    GenerationConfig, MediaResolution, Modality, Part, ServerEvent, SetupConfig, Tool,
};
use livesofia::oauth::{GoogleOAuthCredentials, GoogleTokenSource};
use livesofia::transcript::AssistantTranscript;
use serde_json::json;
use sofia_protocol::ServerEvent as UiEvent;
use uuid::Uuid;

#[derive(Clone, Copy)]
enum Probe {
    Tools,
    Image,
    Video,
}

impl Probe {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "tools" => Ok(Self::Tools),
            "image" => Ok(Self::Image),
            "video" => Ok(Self::Video),
            _ => Err("choose tools, image, or video".into()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::from_path(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env")).ok();
    let mut args = std::env::args().skip(1);
    let mode = Probe::parse(
        &args
            .next()
            .ok_or("usage: live_multimodal_probe <tools|image|video> [path]")?,
    )?;
    let media_path = args.next().map(PathBuf::from);
    let using_fixture = media_path.is_none();
    if args.next().is_some() {
        return Err("too many arguments".into());
    }

    let token_source = GoogleTokenSource::new(GoogleOAuthCredentials::from_env()?)?;
    let project = std::env::var("PROJECT_ID")?;
    if project.trim().is_empty() {
        return Err("PROJECT_ID is empty".into());
    }
    let location = std::env::var("VERTEX_LOCATION").unwrap_or_else(|_| "us-central1".into());
    let model = std::env::var("VERTEX_MODEL").unwrap_or_else(|_| "gemini-3.8-live".into());
    let resource =
        format!("projects/{project}/locations/{location}/publishers/google/models/{model}");

    let tools = if matches!(mode, Probe::Tools) {
        Some(vec![Tool::FunctionDeclarations(vec![FunctionDeclaration {
            name: "lookup_demo_object".into(),
            description: "Look up the demo object's shape, color, and position. Call this before answering questions about the demo object.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }),
            scheduling: None,
            behavior: None,
        }])])
    } else {
        None
    };

    let mut session = tokio::time::timeout(
        Duration::from_secs(45),
        Session::connect(SessionConfig {
            transport: TransportConfig {
                endpoint: Endpoint::VertexAi { location },
                auth: Auth::BearerTokenProvider(token_source.bearer_provider()),
                ..Default::default()
            },
            setup: SetupConfig {
                model: resource,
                generation_config: Some(GenerationConfig {
                    response_modalities: Some(vec![Modality::Audio]),
                    media_resolution: Some(MediaResolution::MediaResolutionLow),
                    ..Default::default()
                }),
                output_audio_transcription: Some(AudioTranscriptionConfig::default()),
                tools,
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

    match mode {
        Probe::Tools => {
            send_prompt(&session, "Call lookup_demo_object with name desk_target. After the tool result, describe that object's shape, color, and position in one short sentence.").await?;
        }
        Probe::Image => {
            let path = media_path.unwrap_or_else(|| fixture_path("still.png"));
            let bytes = fs::read(&path)?;
            ensure_small_frame(&bytes)?;
            session.send_video(&bytes, mime_for_image(&path)?).await?;
            println!("Sent one still-image frame");
            tokio::time::sleep(Duration::from_secs(1)).await;
            session.send_text("Describe the shapes, colors, and their left-to-right positions in the image I just sent.").await?;
        }
        Probe::Video => {
            let path = media_path.unwrap_or_else(|| fixture_path("moving_circle.mp4"));
            let frames = ExtractedFrames::from_video(&path)?;
            println!("Streaming {} video frames at 1 fps", frames.paths.len());
            for (index, frame) in frames.paths.iter().enumerate() {
                let bytes = fs::read(frame)?;
                ensure_small_frame(&bytes)?;
                session.send_video(&bytes, "image/jpeg").await?;
                println!("Sent frame {}", index + 1);
                if index + 1 < frames.paths.len() {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            session.send_text("Describe how the object moves across the video frames I just streamed. Include where it starts and ends.").await?;
        }
    }

    let deadline = Instant::now() + Duration::from_secs(75);
    let mut tool_calls = 0usize;
    let mut audio_bytes = 0usize;
    let mut transcript = String::new();
    let mut output_text = AssistantTranscript::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for a completed model turn".into());
        }
        let event = tokio::time::timeout(remaining, session.next_event())
            .await?
            .ok_or("event stream ended before turn completion")?;
        if let Some(UiEvent::AssistantTextDelta { text, .. }) = output_text.observe(&event) {
            println!("Text delta: {text}");
            transcript.push_str(&text);
        }
        match event {
            ServerEvent::ToolCall(calls) => {
                for call in calls {
                    if call.name != "lookup_demo_object" || !matches!(mode, Probe::Tools) {
                        return Err(format!("unexpected tool call: {}", call.name).into());
                    }
                    tool_calls += 1;
                    println!("Tool called: {}", call.name);
                    session.send_tool_response(vec![FunctionResponse {
                        id: call.id,
                        name: call.name,
                        response: json!({"name":"desk_target","shape":"circle","color":"red","position":"left side of the desk"}),
                    }]).await?;
                }
            }
            ServerEvent::ModelAudio(bytes) => audio_bytes += bytes.len(),
            ServerEvent::TurnComplete => {
                if !matches!(mode, Probe::Tools)
                    || (tool_calls > 0 && audio_bytes > 0 && !transcript.is_empty())
                {
                    break;
                }
                println!("Tool turn completed; waiting for the answer after the tool result");
            }
            ServerEvent::Error(error) => {
                return Err(format!("model error: {}", error.message).into());
            }
            ServerEvent::Closed { reason } => {
                return Err(format!("connection closed: {reason}").into());
            }
            _ => {}
        }
    }

    println!("Turn complete; tool calls: {tool_calls}; audio bytes: {audio_bytes}");
    if !transcript.is_empty() {
        println!("Transcript: {transcript}");
    }
    if matches!(mode, Probe::Tools) && tool_calls == 0 {
        return Err("model did not call the declared tool".into());
    }
    if audio_bytes == 0 || transcript.is_empty() {
        return Err("model must return both audio and output transcription".into());
    }
    if using_fixture && matches!(mode, Probe::Image) {
        let description = transcript.to_ascii_lowercase();
        if !description.contains("red") || !description.contains("blue") {
            return Err(
                "image description did not identify the fixture's red and blue objects".into(),
            );
        }
    }
    if using_fixture && matches!(mode, Probe::Video) {
        let description = transcript.to_ascii_lowercase();
        if !description.contains("left") || !description.contains("right") {
            return Err("video description did not identify left-to-right motion".into());
        }
    }
    Ok(())
}

async fn send_prompt(
    session: &Session,
    prompt: &str,
) -> Result<(), gemini_live::error::SessionError> {
    session
        .send_client_content(ClientContent {
            turns: Some(vec![Content {
                role: Some("user".into()),
                parts: vec![Part {
                    text: Some(prompt.into()),
                    inline_data: None,
                }],
            }]),
            turn_complete: Some(true),
        })
        .await
}

fn mime_for_image(path: &Path) -> Result<&'static str, Box<dyn std::error::Error>> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Ok("image/png"),
        Some("jpg" | "jpeg") => Ok("image/jpeg"),
        _ => Err("image must be PNG or JPEG".into()),
    }
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/generated")
        .join(name)
}

fn ensure_small_frame(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if bytes.is_empty() || bytes.len() > 200_000 {
        return Err("frame must be nonempty and no larger than 200 KB".into());
    }
    Ok(())
}

struct ExtractedFrames {
    directory: PathBuf,
    paths: Vec<PathBuf>,
}

impl ExtractedFrames {
    fn from_video(video: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if !video.is_file() {
            return Err(format!("video not found: {}", video.display()).into());
        }
        let directory = std::env::temp_dir().join(format!("sofia-video-probe-{}", Uuid::new_v4()));
        fs::create_dir(&directory)?;
        let output = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(video)
            .args(["-vf", "fps=1,scale=320:-2", "-frames:v", "5", "-q:v", "4"])
            .arg(directory.join("frame_%03d.jpg"))
            .output()?;
        if !output.status.success() {
            fs::remove_dir_all(&directory).ok();
            return Err("ffmpeg could not decode the video".into());
        }
        let mut paths: Vec<_> = fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        paths.sort();
        if paths.is_empty() {
            fs::remove_dir_all(&directory).ok();
            return Err("video contained no decoded frames".into());
        }
        Ok(Self { directory, paths })
    }
}

impl Drop for ExtractedFrames {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).ok();
    }
}
