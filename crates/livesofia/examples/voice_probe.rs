//! Speak through the daemon, then hear and read the Gemini Live response.
//! Optional argument: number of seconds to capture before stopping.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sofia_protocol::{
    ClientCapabilities, ClientMessage, ClientRequest, ServerBody, ServerEvent, ServerMessage,
    TurnState,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LinesCodec};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seconds = std::env::args()
        .nth(1)
        .map(|value| value.parse::<u64>())
        .transpose()?;
    #[cfg(unix)]
    {
        let path = std::env::var_os("SOFIA_IPC_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
                    .join("sofia/livesofia.sock")
            });
        run(tokio::net::UnixStream::connect(path).await?, seconds).await
    }
    #[cfg(windows)]
    {
        let pipe = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(r"\\.\pipe\sofia-livesofia")?;
        run(pipe, seconds).await
    }
}

async fn run<S>(stream: S, seconds: Option<u64>) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut framed = Framed::new(stream, LinesCodec::new());
    let hello = ClientMessage::new(ClientRequest::Hello {
        client_name: "voice-probe".into(),
        capabilities: ClientCapabilities::default(),
    });
    send(&mut framed, &hello).await?;
    wait_reply(&mut framed, hello.request_id).await?;

    let snapshot = ClientMessage::new(ClientRequest::GetSnapshot);
    send(&mut framed, &snapshot).await?;
    let reply = wait_reply(&mut framed, snapshot.request_id).await?;
    if !matches!(reply.body, ServerBody::Snapshot(ref state) if state.state == TurnState::Ready) {
        return Err("Live session is not ready; inspect the daemon log and retry".into());
    }

    let subscribe = ClientMessage::new(ClientRequest::SubscribeEvents);
    send(&mut framed, &subscribe).await?;
    wait_reply(&mut framed, subscribe.request_id).await?;

    let start = ClientMessage::new(ClientRequest::StartListening);
    send(&mut framed, &start).await?;
    wait_reply(&mut framed, start.request_id).await?;
    println!("Microphone active. Speak now.");
    if seconds.is_none() {
        println!("Press Enter to stop recording.");
    }
    let stop_signal = async {
        if let Some(seconds) = seconds {
            tokio::time::sleep(Duration::from_secs(seconds)).await;
        } else {
            let _ = tokio::task::spawn_blocking(|| {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)
            })
            .await;
        }
    };
    tokio::pin!(stop_signal);
    let mut max_rms = 0.0_f32;
    let mut answer = None;
    loop {
        tokio::select! {
            _ = &mut stop_signal => break,
            message = read(&mut framed) => {
                let message = message?;
                if let ServerBody::Event(event) = message.body {
                    observe(event, &mut max_rms, &mut answer)?;
                }
            }
        }
    }

    let stop = ClientMessage::new(ClientRequest::StopListening);
    send(&mut framed, &stop).await?;
    wait_reply(&mut framed, stop.request_id).await?;
    println!("Microphone stopped; peak RMS: {max_rms:.3}");

    tokio::time::timeout(Duration::from_secs(45), async {
        while answer.is_none() {
            let message = read(&mut framed).await?;
            if let ServerBody::Event(event) = message.body {
                observe(event, &mut max_rms, &mut answer)?;
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await
    .map_err(
        |_| "no spoken response received within 45 seconds; try speaking clearly before stopping",
    )??;
    println!("Answer: {}", answer.unwrap());
    Ok(())
}

fn observe(
    event: ServerEvent,
    max_rms: &mut f32,
    answer: &mut Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    match event {
        ServerEvent::InputAudioLevel { rms } => *max_rms = max_rms.max(rms),
        ServerEvent::InputText { text } => println!("You: {text}"),
        ServerEvent::AssistantTextDelta { text, .. } => println!("Sofia text: {text}"),
        ServerEvent::AssistantTextFinal { text, .. } => *answer = Some(text),
        ServerEvent::TurnStateChanged { state } => println!("State: {state:?}"),
        ServerEvent::Error { message } => return Err(format!("Live error: {message}").into()),
        _ => {}
    }
    Ok(())
}

async fn send<S>(
    framed: &mut Framed<S, LinesCodec>,
    message: &ClientMessage,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    framed.send(serde_json::to_string(message)?).await?;
    Ok(())
}

async fn read<S>(
    framed: &mut Framed<S, LinesCodec>,
) -> Result<ServerMessage, Box<dyn std::error::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let line = framed.next().await.ok_or("IPC connection closed")??;
    Ok(serde_json::from_str(&line)?)
}

async fn wait_reply<S>(
    framed: &mut Framed<S, LinesCodec>,
    id: Uuid,
) -> Result<ServerMessage, Box<dyn std::error::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let message = read(framed).await?;
        if message.request_id != Some(id) {
            continue;
        }
        if let ServerBody::Error(error) = &message.body {
            return Err(format!("IPC request failed: {}", error.message).into());
        }
        return Ok(message);
    }
}
