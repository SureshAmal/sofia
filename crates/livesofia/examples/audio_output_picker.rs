//! Choose the speaker used for Gemini Live replies through Sofia IPC.

use futures_util::{SinkExt, StreamExt};
use sofia_protocol::{ClientCapabilities, ClientMessage, ClientRequest, ServerBody, ServerMessage};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LinesCodec};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        let path = std::env::var_os("SOFIA_IPC_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
                    .join("sofia/livesofia.sock")
            });
        run(tokio::net::UnixStream::connect(path).await?).await
    }
    #[cfg(windows)]
    {
        let pipe = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(r"\\.\pipe\sofia-livesofia")?;
        run(pipe).await
    }
}

async fn run<S>(stream: S) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut framed = Framed::new(stream, LinesCodec::new());
    let hello = ClientMessage::new(ClientRequest::Hello {
        client_name: "audio-output-picker".into(),
        capabilities: ClientCapabilities::default(),
    });
    send(&mut framed, &hello).await?;
    wait_reply(&mut framed, hello.request_id).await?;

    let list = ClientMessage::new(ClientRequest::ListAudioOutputs);
    send(&mut framed, &list).await?;
    let reply = wait_reply(&mut framed, list.request_id).await?;
    let ServerBody::AudioOutputs {
        devices,
        selected_device_id,
    } = reply.body
    else {
        return Err("daemon did not return audio outputs".into());
    };
    println!(
        "0. Follow system default{}",
        if selected_device_id.is_none() {
            " [selected]"
        } else {
            ""
        }
    );
    for (index, device) in devices.iter().enumerate() {
        let selected = selected_device_id.as_deref() == Some(device.id.as_str());
        let default = if device.is_default {
            " [system default]"
        } else {
            ""
        };
        let selected = if selected { " [selected]" } else { "" };
        println!(
            "{}. {}{}{} ({})",
            index + 1,
            device.name,
            default,
            selected,
            device.id
        );
    }
    println!("Choose an output number:");
    let choice = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok::<_, std::io::Error>(line)
    })
    .await??;
    let index = choice.trim().parse::<usize>()?;
    let device_id = if index == 0 {
        None
    } else {
        Some(
            devices
                .get(index - 1)
                .ok_or("output number is out of range")?
                .id
                .clone(),
        )
    };
    let select = ClientMessage::new(ClientRequest::SelectAudioOutput { device_id });
    send(&mut framed, &select).await?;
    let reply = wait_reply(&mut framed, select.request_id).await?;
    if !matches!(reply.body, ServerBody::Accepted) {
        return Err("output selection was not accepted".into());
    }
    println!("Gemini audio output saved.");
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

async fn wait_reply<S>(
    framed: &mut Framed<S, LinesCodec>,
    id: Uuid,
) -> Result<ServerMessage, Box<dyn std::error::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let line = framed.next().await.ok_or("IPC connection closed")??;
        let message: ServerMessage = serde_json::from_str(&line)?;
        if message.request_id != Some(id) {
            continue;
        }
        if let ServerBody::Error(error) = &message.body {
            return Err(format!("IPC request failed: {}", error.message).into());
        }
        return Ok(message);
    }
}
