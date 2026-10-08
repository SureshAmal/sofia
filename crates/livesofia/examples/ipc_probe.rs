//! Connect to the running daemon and verify a Live text turn over IPC.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sofia_protocol::{
    ClientCapabilities, ClientMessage, ClientRequest, ServerBody, ServerEvent, ServerMessage,
    StateSnapshot, TurnState,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LinesCodec};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prompt = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let prompt = if prompt.is_empty() {
        "Say hello in one short sentence.".to_string()
    } else {
        prompt
    };

    #[cfg(unix)]
    {
        let path = std::env::var_os("SOFIA_IPC_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
                    .join("sofia/livesofia.sock")
            });
        run(tokio::net::UnixStream::connect(path).await?, prompt).await
    }
    #[cfg(windows)]
    {
        let pipe = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(r"\\.\pipe\sofia-livesofia")?;
        run(pipe, prompt).await
    }
}

async fn run<S>(stream: S, prompt: String) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut framed = Framed::new(stream, LinesCodec::new());
    let hello = ClientMessage::new(ClientRequest::Hello {
        client_name: "ipc-probe".into(),
        capabilities: ClientCapabilities::default(),
    });
    send(&mut framed, &hello).await?;
    expect_reply(&mut framed, hello.request_id).await?;

    let snapshot = ClientMessage::new(ClientRequest::GetSnapshot);
    send(&mut framed, &snapshot).await?;
    let reply = expect_reply(&mut framed, snapshot.request_id).await?;
    let ServerBody::Snapshot(StateSnapshot {
        state: TurnState::Ready,
        ..
    }) = reply.body
    else {
        return Err("Live session is not ready; inspect the daemon log and retry".into());
    };
    println!("Live state: ready");

    let subscribe = ClientMessage::new(ClientRequest::SubscribeEvents);
    send(&mut framed, &subscribe).await?;
    let reply = expect_reply(&mut framed, subscribe.request_id).await?;
    let ServerBody::Subscribed { next_sequence } = reply.body else {
        return Err("event subscription failed".into());
    };

    let request = ClientMessage::new(ClientRequest::SendText { text: prompt });
    send(&mut framed, &request).await?;
    let mut accepted = false;
    let mut final_text = None;
    let mut expected_sequence = next_sequence;
    tokio::time::timeout(Duration::from_secs(60), async {
        while !accepted || final_text.is_none() {
            let message = read(&mut framed).await?;
            if message.request_id == Some(request.request_id) {
                match message.body {
                    ServerBody::Accepted => accepted = true,
                    ServerBody::Error(error) => {
                        return Err(format!("prompt rejected: {}", error.message).into());
                    }
                    _ => return Err("unexpected prompt reply".into()),
                }
            } else if let ServerBody::Event(event) = message.body {
                if message.sequence != Some(expected_sequence) {
                    return Err("IPC event sequence gap".into());
                }
                expected_sequence += 1;
                match event {
                    ServerEvent::TurnStateChanged { state } => println!("State: {state:?}"),
                    ServerEvent::AssistantTextDelta { text, .. } => println!("Text delta: {text}"),
                    ServerEvent::AssistantTextFinal { text, .. } => final_text = Some(text),
                    ServerEvent::Error { message } => {
                        return Err(format!("Live error: {message}").into());
                    }
                    _ => {}
                }
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await??;
    println!("Final text: {}", final_text.unwrap());
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

async fn expect_reply<S>(
    framed: &mut Framed<S, LinesCodec>,
    request_id: Uuid,
) -> Result<ServerMessage, Box<dyn std::error::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let reply = read(framed).await?;
    if reply.request_id != Some(request_id) {
        return Err("unexpected IPC reply".into());
    }
    if let ServerBody::Error(error) = &reply.body {
        return Err(format!("IPC error: {}", error.message).into());
    }
    Ok(reply)
}
