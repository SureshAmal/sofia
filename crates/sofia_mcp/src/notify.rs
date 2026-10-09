use futures_util::{SinkExt, StreamExt};
use sofia_protocol::{ClientCapabilities, ClientMessage, ClientRequest, ServerBody, ServerMessage};
use std::sync::OnceLock;
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LinesCodec};

static NOTIFIER_TX: OnceLock<mpsc::Sender<String>> = OnceLock::new();

fn get_notifier() -> &'static mpsc::Sender<String> {
    NOTIFIER_TX.get_or_init(|| {
        let (tx, mut rx) = mpsc::channel::<String>(100);
        tokio::spawn(async move {
            loop {
                let Some(first_id) = rx.recv().await else {
                    break;
                };

                // Connect once and perform handshake
                match connect_and_handshake().await {
                    Ok(mut stream) => {
                        // Send the initial changed notification
                        if send_notification(&mut stream, first_id).await.is_err() {
                            continue;
                        }

                        // Keep processing on this persistent connection
                        while let Some(id) = rx.recv().await {
                            if send_notification(&mut stream, id).await.is_err() {
                                // Break to outer loop to reconnect on connection loss
                                break;
                            }
                        }
                    }
                    Err(_) => {
                        // Back off briefly before trying again on next notification
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                }
            }
        });
        tx
    })
}

#[cfg(unix)]
type IpcStream = tokio::net::UnixStream;

#[cfg(windows)]
type IpcStream = tokio::net::windows::named_pipe::NamedPipeClient;

async fn open_transport() -> Result<IpcStream, Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(unix)]
    {
        let path = std::env::var_os("SOFIA_IPC_PATH")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_RUNTIME_DIR")
                    .map(|path| std::path::PathBuf::from(path).join("sofia/livesofia.sock"))
            })
            .ok_or("No IPC path")?;
        Ok(tokio::net::UnixStream::connect(path).await?)
    }
    #[cfg(windows)]
    {
        Ok(tokio::net::windows::named_pipe::ClientOptions::new()
            .open(r"\\.\pipe\sofia-livesofia")?)
    }
}

async fn connect_and_handshake() -> Result<Framed<IpcStream, LinesCodec>, Box<dyn std::error::Error + Send + Sync>> {
    let transport = open_transport().await?;
    let mut stream = Framed::new(
        transport,
        LinesCodec::new_with_max_length(sofia_protocol::MAX_MESSAGE_BYTES),
    );

    let hello_req = ClientMessage::new(ClientRequest::Hello {
        client_name: "sofia-mcp".into(),
        capabilities: ClientCapabilities::default(),
    });
    let request_id = hello_req.request_id;
    stream.send(serde_json::to_string(&hello_req)?).await?;

    let reply: ServerMessage = serde_json::from_str(&stream.next().await.ok_or("IPC closed")??)?;
    if reply.request_id != Some(request_id) || matches!(reply.body, ServerBody::Error(_)) {
        return Err("IPC handshake rejected".into());
    }

    Ok(stream)
}

async fn send_notification(
    stream: &mut Framed<IpcStream, LinesCodec>,
    id: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let message = ClientMessage::new(ClientRequest::ContentChanged { id });
    let request_id = message.request_id;
    stream.send(serde_json::to_string(&message)?).await?;

    let reply: ServerMessage = serde_json::from_str(&stream.next().await.ok_or("IPC closed")??)?;
    if reply.request_id != Some(request_id) || matches!(reply.body, ServerBody::Error(_)) {
        return Err("IPC notification rejected".into());
    }
    Ok(())
}

pub async fn changed(id: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = get_notifier().send(id).await;
    Ok(())
}
