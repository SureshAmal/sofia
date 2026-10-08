//! Local IPC transport. The GPUI view receives only small state updates.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sofia_protocol::{
    ClientCapabilities, ClientMessage, ClientRequest, ServerBody, ServerEvent, ServerMessage,
    StateSnapshot,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc as async_mpsc;
use tokio_util::codec::{Framed, LinesCodec};

pub enum UiUpdate {
    Snapshot(StateSnapshot),
    Event(ServerEvent),
    Disconnected,
}

#[derive(Clone)]
pub struct IpcClient {
    commands: async_mpsc::UnboundedSender<ClientRequest>,
}

impl IpcClient {
    pub fn start() -> (Self, Receiver<UiUpdate>) {
        let (updates, receiver) = mpsc::channel();
        let (commands, command_rx) = async_mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("sofia-pill-ipc".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("IPC runtime");
                runtime.block_on(run(updates, command_rx));
            })
            .expect("IPC thread");
        (Self { commands }, receiver)
    }

    pub fn send(&self, command: ClientRequest) {
        let _ = self.commands.send(command);
    }
}

async fn run(
    updates: Sender<UiUpdate>,
    mut commands: async_mpsc::UnboundedReceiver<ClientRequest>,
) {
    loop {
        if let Err(error) = connect_and_run(&updates, &mut commands).await {
            eprintln!("Sofia IPC: {error}");
        }
        if updates.send(UiUpdate::Disconnected).is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(unix)]
async fn connect_and_run(
    updates: &Sender<UiUpdate>,
    commands: &mut async_mpsc::UnboundedReceiver<ClientRequest>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::var_os("SOFIA_IPC_PATH")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(|base| std::path::PathBuf::from(base).join("sofia/livesofia.sock"))
        })
        .ok_or("SOFIA_IPC_PATH and XDG_RUNTIME_DIR are unset")?;
    run_stream(
        tokio::net::UnixStream::connect(path).await?,
        updates,
        commands,
    )
    .await
}

#[cfg(windows)]
async fn connect_and_run(
    updates: &Sender<UiUpdate>,
    commands: &mut async_mpsc::UnboundedReceiver<ClientRequest>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pipe =
        tokio::net::windows::named_pipe::ClientOptions::new().open(r"\\.\pipe\sofia-livesofia")?;
    run_stream(pipe, updates, commands).await
}

async fn run_stream<S>(
    stream: S,
    updates: &Sender<UiUpdate>,
    commands: &mut async_mpsc::UnboundedReceiver<ClientRequest>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut framed = Framed::new(
        stream,
        LinesCodec::new_with_max_length(sofia_protocol::MAX_MESSAGE_BYTES),
    );
    let hello = request(
        &mut framed,
        ClientRequest::Hello {
            client_name: "sofia-ui-pill".into(),
            capabilities: ClientCapabilities::default(),
        },
        updates,
    )
    .await?;
    if !matches!(hello, ServerBody::Hello { .. }) {
        return Err("unexpected IPC handshake reply".into());
    }
    let snapshot = request(&mut framed, ClientRequest::GetSnapshot, updates).await?;
    if let ServerBody::Snapshot(snapshot) = snapshot {
        updates.send(UiUpdate::Snapshot(snapshot))?;
    } else {
        return Err("unexpected IPC snapshot reply".into());
    }
    let subscription = request(&mut framed, ClientRequest::SubscribeEvents, updates).await?;
    let ServerBody::Subscribed { next_sequence } = subscription else {
        return Err("unexpected IPC subscription reply".into());
    };
    let mut expected_sequence = next_sequence;

    loop {
        tokio::select! {
            message = framed.next() => {
                let Some(line) = message else { return Err("IPC connection closed".into()); };
                let message: ServerMessage = serde_json::from_str(&line?)?;
                validate_version(&message)?;
                if matches!(message.body, ServerBody::Event(_)) {
                    if message.sequence != Some(expected_sequence) {
                        return Err("IPC event gap; reconnecting for a fresh snapshot".into());
                    }
                    expected_sequence += 1;
                }
                if matches!(message.body, ServerBody::Error(ref error) if error.code == sofia_protocol::ErrorCode::ResyncRequired) {
                    return Err("IPC resync requested; reconnecting for a fresh snapshot".into());
                }
                handle_message(message, updates)?;
            }
            Some(command) = commands.recv() => {
                let message = ClientMessage::new(command);
                framed.send(serde_json::to_string(&message)?).await?;
            }
        }
    }
}

async fn request<S>(
    framed: &mut Framed<S, LinesCodec>,
    body: ClientRequest,
    updates: &Sender<UiUpdate>,
) -> Result<ServerBody, Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let message = ClientMessage::new(body);
    framed.send(serde_json::to_string(&message)?).await?;
    loop {
        let line = framed.next().await.ok_or("IPC connection closed")??;
        let reply: ServerMessage = serde_json::from_str(&line)?;
        validate_version(&reply)?;
        if reply.request_id == Some(message.request_id) {
            if let ServerBody::Error(error) = &reply.body {
                return Err(error.message.clone().into());
            }
            return Ok(reply.body);
        }
        handle_message(reply, updates)?;
    }
}

fn validate_version(
    message: &ServerMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if message.version != sofia_protocol::PROTOCOL_VERSION {
        return Err(format!("unsupported IPC version: {}", message.version).into());
    }
    Ok(())
}

fn handle_message(
    message: ServerMessage,
    updates: &Sender<UiUpdate>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match message.body {
        ServerBody::Event(event) => updates.send(UiUpdate::Event(event))?,
        ServerBody::Error(error) => updates.send(UiUpdate::Event(ServerEvent::Error {
            message: error.message,
        }))?,
        _ => {}
    }
    Ok(())
}
