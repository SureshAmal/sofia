use futures_util::{SinkExt, StreamExt};
use sofia_protocol::{ClientCapabilities, ClientMessage, ClientRequest, ServerBody, ServerMessage};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LinesCodec};
async fn send<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    id: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = Framed::new(
        stream,
        LinesCodec::new_with_max_length(sofia_protocol::MAX_MESSAGE_BYTES),
    );
    for request in [
        ClientRequest::Hello {
            client_name: "sofia-mcp".into(),
            capabilities: ClientCapabilities::default(),
        },
        ClientRequest::ContentChanged { id },
    ] {
        let message = ClientMessage::new(request);
        let request_id = message.request_id;
        stream.send(serde_json::to_string(&message)?).await?;
        let reply: ServerMessage =
            serde_json::from_str(&stream.next().await.ok_or("IPC closed")??)?;
        if reply.request_id != Some(request_id) || matches!(reply.body, ServerBody::Error(_)) {
            return Err("IPC notification rejected".into());
        }
    }
    Ok(())
}
pub async fn changed(id: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        #[cfg(unix)]
        {
            let path = std::env::var_os("SOFIA_IPC_PATH")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("XDG_RUNTIME_DIR")
                        .map(|path| std::path::PathBuf::from(path).join("sofia/livesofia.sock"))
                })
                .ok_or("No IPC path")?;
            send(tokio::net::UnixStream::connect(path).await?, id).await
        }
        #[cfg(windows)]
        {
            send(
                tokio::net::windows::named_pipe::ClientOptions::new()
                    .open(r"\\.\pipe\sofia-livesofia")?,
                id,
            )
            .await
        }
    })
    .await?
}
