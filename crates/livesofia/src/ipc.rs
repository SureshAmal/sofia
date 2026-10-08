use futures_util::{SinkExt, StreamExt};
use livesofia::state::EventHub;
use livesofia::voice::{available_voices, canonical_voice_name};
use sofia_protocol::{
    ClientMessage, ClientRequest, ErrorCode, MAX_MESSAGE_BYTES, PROTOCOL_VERSION, ProtocolError,
    ServerBody, ServerMessage,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{info, warn};
use uuid::Uuid;

use crate::audio;
use crate::live::{LiveCommand, SubmitError};

#[cfg(unix)]
pub async fn run_server(
    hub: EventHub,
    commands: mpsc::Sender<LiveCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use tokio::net::UnixListener;

    let path = match std::env::var_os("SOFIA_IPC_PATH") {
        Some(path) => std::path::PathBuf::from(path),
        None => std::path::PathBuf::from(
            std::env::var_os("XDG_RUNTIME_DIR")
                .ok_or("XDG_RUNTIME_DIR is unset; set SOFIA_IPC_PATH to a private socket path")?,
        )
        .join("sofia/livesofia.sock"),
    };
    let directory = path.parent().ok_or("IPC socket needs a parent directory")?;
    std::fs::create_dir_all(directory)?;
    if std::env::var_os("SOFIA_IPC_PATH").is_none() {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    if path.exists() {
        if !std::fs::symlink_metadata(&path)?.file_type().is_socket() {
            return Err(format!("IPC path is not a socket: {}", path.display()).into());
        }
        if tokio::net::UnixStream::connect(&path).await.is_ok() {
            return Err(format!("livesofia is already listening at {}", path.display()).into());
        }
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    info!(socket = %path.display(), "IPC server ready");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let hub = hub.clone();
                let commands = commands.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_connection(stream, hub, commands).await {
                        warn!(%error, "IPC client disconnected with error");
                    }
                });
            }
            result = tokio::signal::ctrl_c() => {
                result?;
                break;
            }
        }
    }
    drop(listener);
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(windows)]
pub async fn run_server(
    hub: EventHub,
    commands: mpsc::Sender<LiveCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::net::windows::named_pipe::ServerOptions;

    // Restrict the pipe ACL before this endpoint carries credentials or user data.
    let name = r"\\.\pipe\sofia-livesofia";
    info!(pipe = name, "IPC server ready");
    loop {
        let server = ServerOptions::new().create(name)?;
        tokio::select! {
            result = server.connect() => {
                result?;
                let hub = hub.clone();
                let commands = commands.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_connection(server, hub, commands).await {
                        warn!(%error, "IPC client disconnected with error");
                    }
                });
            }
            result = tokio::signal::ctrl_c() => {
                result?;
                break;
            }
        }
    }
    Ok(())
}

async fn handle_connection<S>(
    stream: S,
    hub: EventHub,
    commands: mpsc::Sender<LiveCommand>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut framed = Framed::new(stream, LinesCodec::new_with_max_length(MAX_MESSAGE_BYTES));
    let mut client_id: Option<Uuid> = None;
    let mut subscription: Option<broadcast::Receiver<ServerMessage>> = None;

    loop {
        let line = if let Some(receiver) = subscription.as_mut() {
            tokio::select! {
                line = framed.next() => line,
                event = receiver.recv() => {
                    match event {
                        Ok(event) => send(&mut framed, event).await?,
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            subscription = None;
                            send(&mut framed, ServerMessage {
                                version: PROTOCOL_VERSION,
                                request_id: None,
                                sequence: None,
                                body: protocol_error(ErrorCode::ResyncRequired, "event buffer overflow; fetch a snapshot and subscribe again"),
                            }).await?;
                        }
                        Err(broadcast::error::RecvError::Closed) => subscription = None,
                    }
                    continue;
                }
            }
        } else {
            framed.next().await
        };
        let Some(line) = line else { break };
        let line = line?;
        let request: ClientMessage = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                send(
                    &mut framed,
                    ServerMessage {
                        version: PROTOCOL_VERSION,
                        request_id: None,
                        sequence: None,
                        body: ServerBody::Error(ProtocolError {
                            code: ErrorCode::BadRequest,
                            message: format!("invalid JSON request: {error}"),
                        }),
                    },
                )
                .await?;
                continue;
            }
        };

        let body = if request.version != PROTOCOL_VERSION {
            ServerBody::Error(ProtocolError {
                code: ErrorCode::VersionMismatch,
                message: format!(
                    "unsupported protocol version {}; server supports {}",
                    request.version, PROTOCOL_VERSION
                ),
            })
        } else {
            match request.body {
                ClientRequest::Hello {
                    client_name,
                    capabilities: _,
                } if !client_name.trim().is_empty() && client_name.len() <= 128 => {
                    let id = *client_id.get_or_insert_with(Uuid::new_v4);
                    info!(client = %client_name, client_id = %id, "IPC client connected");
                    ServerBody::Hello {
                        server_name: "livesofia".into(),
                        client_id: id,
                    }
                }
                ClientRequest::Hello { .. } => ServerBody::Error(ProtocolError {
                    code: ErrorCode::BadRequest,
                    message: "client_name must be 1–128 bytes".into(),
                }),
                _ if client_id.is_none() => ServerBody::Error(ProtocolError {
                    code: ErrorCode::HandshakeRequired,
                    message: "send hello before other requests".into(),
                }),
                ClientRequest::GetSnapshot => ServerBody::Snapshot(hub.snapshot()),
                ClientRequest::SubscribeEvents => {
                    let (next_sequence, receiver) = hub.subscribe();
                    subscription = Some(receiver);
                    ServerBody::Subscribed { next_sequence }
                }
                ClientRequest::SendText { text } => {
                    if text.trim().is_empty() || text.len() > 4096 {
                        protocol_error(ErrorCode::BadRequest, "text must be 1–4096 bytes")
                    } else {
                        submit_text(&commands, text).await
                    }
                }
                ClientRequest::StartListening => {
                    submit_command(&commands, |reply| LiveCommand::StartListening { reply }).await
                }
                ClientRequest::StopListening => {
                    submit_command(&commands, |reply| LiveCommand::StopListening { reply }).await
                }
                ClientRequest::ReloadSettings => {
                    submit_command(&commands, |reply| LiveCommand::ReloadSettings { reply }).await
                }
                ClientRequest::SetSpeakerMuted { muted } => {
                    submit_command(&commands, |reply| LiveCommand::SetSpeakerMuted {
                        muted,
                        reply,
                    })
                    .await
                }
                ClientRequest::ListAudioOutputs => {
                    match tokio::task::spawn_blocking(audio::list_output_devices).await {
                        Ok(Ok(devices)) => ServerBody::AudioOutputs {
                            devices,
                            selected_device_id: hub.snapshot().selected_output_device_id,
                        },
                        _ => protocol_error(
                            ErrorCode::AudioDevice,
                            "audio outputs could not be listed",
                        ),
                    }
                }
                ClientRequest::SelectAudioOutput { device_id } => {
                    if device_id
                        .as_ref()
                        .is_some_and(|id| id.len() > 512 || id.is_empty())
                    {
                        protocol_error(ErrorCode::BadRequest, "device_id must be 1–512 bytes")
                    } else {
                        submit_command(&commands, |reply| LiveCommand::SelectAudioOutput {
                            device_id,
                            reply,
                        })
                        .await
                    }
                }
                ClientRequest::ListGeminiVoices => ServerBody::GeminiVoices {
                    voices: available_voices(),
                    selected_voice_name: hub.snapshot().gemini_voice_name,
                },
                ClientRequest::SelectGeminiVoice { voice_name } => {
                    let canonical = voice_name.as_deref().map(canonical_voice_name);
                    if canonical.as_ref().is_some_and(Option::is_none) {
                        protocol_error(ErrorCode::BadRequest, "unknown Gemini voice")
                    } else {
                        let voice_name = canonical.flatten().map(str::to_owned);
                        submit_command(&commands, |reply| LiveCommand::SelectGeminiVoice {
                            voice_name,
                            reply,
                        })
                        .await
                    }
                }
                ClientRequest::ContentChanged { id } => {
                    if uuid::Uuid::parse_str(&id).is_err() {
                        protocol_error(ErrorCode::BadRequest, "Invalid document ID")
                    } else {
                        hub.publish(sofia_protocol::ServerEvent::ContentChanged { id });
                        ServerBody::Accepted
                    }
                }
                ClientRequest::Ping => ServerBody::Pong,
            }
        };
        send(&mut framed, ServerMessage::reply(request.request_id, body)).await?;
    }
    Ok(())
}

async fn submit_text(commands: &mpsc::Sender<LiveCommand>, text: String) -> ServerBody {
    submit_command(commands, |reply| LiveCommand::SendText { text, reply }).await
}

async fn submit_command(
    commands: &mpsc::Sender<LiveCommand>,
    make: impl FnOnce(oneshot::Sender<Result<(), SubmitError>>) -> LiveCommand,
) -> ServerBody {
    let (reply, result) = oneshot::channel();
    if commands.send(make(reply)).await.is_err() {
        return protocol_error(ErrorCode::Unavailable, "Live session is unavailable");
    }
    match tokio::time::timeout(std::time::Duration::from_secs(5), result).await {
        Ok(Ok(Ok(()))) => ServerBody::Accepted,
        Ok(Ok(Err(SubmitError::Busy))) => {
            protocol_error(ErrorCode::Busy, "Live turn is in progress")
        }
        Ok(Ok(Err(SubmitError::AudioDevice))) => {
            protocol_error(ErrorCode::AudioDevice, "audio device could not start")
        }
        Ok(Ok(Err(SubmitError::Settings))) => protocol_error(
            ErrorCode::Internal,
            "audio output selection could not be saved",
        ),
        _ => protocol_error(ErrorCode::Unavailable, "Live session is unavailable"),
    }
}

fn protocol_error(code: ErrorCode, message: &str) -> ServerBody {
    ServerBody::Error(ProtocolError {
        code,
        message: message.into(),
    })
}

async fn send<S>(
    framed: &mut Framed<S, LinesCodec>,
    message: ServerMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    framed.send(serde_json::to_string(&message)?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sofia_protocol::{ClientCapabilities, StateSnapshot, TurnState};

    #[tokio::test]
    async fn handshake_and_snapshot() {
        let (server, client) = tokio::io::duplex(4096);
        let hub = EventHub::new();
        let (commands, _receiver) = mpsc::channel(1);
        let task = tokio::spawn(handle_connection(server, hub, commands));
        let mut client = Framed::new(client, LinesCodec::new());

        let hello = ClientMessage::new(ClientRequest::Hello {
            client_name: "test-ui".into(),
            capabilities: ClientCapabilities::default(),
        });
        client
            .send(serde_json::to_string(&hello).unwrap())
            .await
            .unwrap();
        let reply: ServerMessage =
            serde_json::from_str(&client.next().await.unwrap().unwrap()).unwrap();
        assert_eq!(reply.request_id, Some(hello.request_id));
        assert!(matches!(reply.body, ServerBody::Hello { .. }));

        let snapshot = ClientMessage::new(ClientRequest::GetSnapshot);
        client
            .send(serde_json::to_string(&snapshot).unwrap())
            .await
            .unwrap();
        let reply: ServerMessage =
            serde_json::from_str(&client.next().await.unwrap().unwrap()).unwrap();
        assert_eq!(reply.request_id, Some(snapshot.request_id));
        assert!(matches!(
            reply.body,
            ServerBody::Snapshot(StateSnapshot {
                state: TurnState::Disconnected,
                ..
            })
        ));
        drop(client);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn rejects_requests_before_hello() {
        let (server, client) = tokio::io::duplex(4096);
        let hub = EventHub::new();
        let (commands, _receiver) = mpsc::channel(1);
        let task = tokio::spawn(handle_connection(server, hub, commands));
        let mut client = Framed::new(client, LinesCodec::new());
        let request = ClientMessage::new(ClientRequest::GetSnapshot);
        client
            .send(serde_json::to_string(&request).unwrap())
            .await
            .unwrap();
        let reply: ServerMessage =
            serde_json::from_str(&client.next().await.unwrap().unwrap()).unwrap();
        assert!(matches!(
            reply.body,
            ServerBody::Error(ProtocolError {
                code: ErrorCode::HandshakeRequired,
                ..
            })
        ));
        drop(client);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn subscribed_client_receives_ordered_events() {
        let (server, client) = tokio::io::duplex(4096);
        let hub = EventHub::new();
        let (commands, _receiver) = mpsc::channel(1);
        let task = tokio::spawn(handle_connection(server, hub.clone(), commands));
        let mut client = Framed::new(client, LinesCodec::new());

        let hello = ClientMessage::new(ClientRequest::Hello {
            client_name: "test-ui".into(),
            capabilities: ClientCapabilities::default(),
        });
        client
            .send(serde_json::to_string(&hello).unwrap())
            .await
            .unwrap();
        let _: ServerMessage =
            serde_json::from_str(&client.next().await.unwrap().unwrap()).unwrap();

        let subscribe = ClientMessage::new(ClientRequest::SubscribeEvents);
        client
            .send(serde_json::to_string(&subscribe).unwrap())
            .await
            .unwrap();
        let reply: ServerMessage =
            serde_json::from_str(&client.next().await.unwrap().unwrap()).unwrap();
        let ServerBody::Subscribed { next_sequence } = reply.body else {
            panic!("expected subscription response");
        };

        hub.set_state(TurnState::Thinking, Some(Uuid::new_v4()));
        hub.publish(sofia_protocol::ServerEvent::AssistantTextDelta {
            turn_id: Uuid::new_v4(),
            text: "Hello".into(),
        });
        for sequence in [next_sequence, next_sequence + 1] {
            let event: ServerMessage =
                serde_json::from_str(&client.next().await.unwrap().unwrap()).unwrap();
            assert_eq!(event.sequence, Some(sequence));
            assert!(matches!(event.body, ServerBody::Event(_)));
        }

        let snapshot = ClientMessage::new(ClientRequest::GetSnapshot);
        client
            .send(serde_json::to_string(&snapshot).unwrap())
            .await
            .unwrap();
        let reply: ServerMessage =
            serde_json::from_str(&client.next().await.unwrap().unwrap()).unwrap();
        assert!(
            matches!(reply.body, ServerBody::Snapshot(StateSnapshot { sequence, state: TurnState::Thinking, .. }) if sequence == next_sequence + 1)
        );
        drop(client);
        task.await.unwrap().unwrap();
    }
}
