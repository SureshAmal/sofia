//! Exercise livesofia through the same IPC client used by the GPUI pill.
use std::time::{Duration, Instant};

use sofia_protocol::{AUDIO_SPECTRUM_BANDS, AudioSource, ClientRequest, ServerEvent, TurnState};
use sofia_ui_layer::ipc_client::{IpcClient, UiUpdate};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let microphone = std::env::args().any(|arg| arg == "--microphone");
    let capture_seconds = std::env::args()
        .find_map(|arg| {
            arg.strip_prefix("--seconds=")
                .and_then(|value| value.parse::<u64>().ok())
        })
        .unwrap_or(3);
    let (client, updates) = IpcClient::start();
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut submitted = false;
    let mut mic_started = None;
    let mut mic_stopped = false;
    let mut user_frames = 0;
    let mut assistant_frames = 0;
    let mut text_chunks = 0;
    let mut final_text = None;
    let mut states = Vec::new();
    let mut peak_rms = 0.0_f32;

    while Instant::now() < deadline {
        if mic_started
            .is_some_and(|start: Instant| start.elapsed() >= Duration::from_secs(capture_seconds))
            && !mic_stopped
        {
            client.send(ClientRequest::StopListening);
            mic_stopped = true;
        }
        let update = match updates.recv_timeout(Duration::from_millis(100)) {
            Ok(update) => update,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(error) => return Err(error.into()),
        };
        let state = match update {
            UiUpdate::Snapshot(snapshot) => {
                println!("Snapshot: {:?}", snapshot.state);
                if snapshot.microphone_active {
                    mic_started = Some(Instant::now());
                }
                Some(snapshot.state)
            }
            UiUpdate::Disconnected => {
                println!("Disconnected; waiting for automatic reconnect");
                None
            }
            UiUpdate::Event(event) => match event {
                ServerEvent::TurnStateChanged { state } => {
                    println!("State: {state:?}");
                    states.push(state);
                    Some(state)
                }
                ServerEvent::AudioStatusChanged {
                    microphone_active,
                    speaker_muted,
                } => {
                    println!(
                        "Audio: microphone={microphone_active}, speaker_muted={speaker_muted}"
                    );
                    if microphone_active {
                        mic_started = Some(Instant::now());
                    }
                    None
                }
                ServerEvent::AudioSpectrum { source, bins } => {
                    if bins.len() != AUDIO_SPECTRUM_BANDS
                        || bins
                            .iter()
                            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                    {
                        return Err("Invalid FFT frame".into());
                    }
                    match source {
                        AudioSource::User => user_frames += 1,
                        AudioSource::Assistant => assistant_frames += 1,
                    }
                    None
                }
                ServerEvent::AssistantTextDelta { text, .. } => {
                    text_chunks += 1;
                    println!("Text delta: {text}");
                    None
                }
                ServerEvent::InputAudioLevel { rms } => {
                    peak_rms = peak_rms.max(rms);
                    None
                }
                ServerEvent::InputText { text } => {
                    println!("Input transcript: {text}");
                    None
                }
                ServerEvent::AssistantTextFinal { text, .. } => {
                    println!("Final text: {text}");
                    final_text = Some(text);
                    None
                }
                ServerEvent::Error { message } => return Err(message.into()),
                _ => None,
            },
        };
        if matches!(state, Some(TurnState::Ready | TurnState::Listening)) && !submitted {
            submitted = true;
            if microphone && mic_started.is_none() {
                client.send(ClientRequest::StartListening);
            } else if !microphone {
                client.send(ClientRequest::SendText {
                    text: "Say exactly: Sofia is connected to the desktop interface.".into(),
                });
            }
        }
        if microphone && mic_stopped && state == Some(TurnState::Ready) {
            if user_frames == 0 {
                return Err("Missing user FFT frames".into());
            }
            println!(
                "PASS: microphone start/stop and {user_frames} valid user FFT frames; peak_rms={peak_rms:.5}; assistant_reply={}",
                final_text.is_some()
            );
            return Ok(());
        }
        if submitted
            && !microphone
            && matches!(state, Some(TurnState::Ready | TurnState::Listening))
            && final_text.is_some()
        {
            if assistant_frames == 0 || text_chunks == 0 {
                return Err("Missing assistant FFT or streamed text".into());
            }
            if microphone && (!mic_stopped || user_frames == 0) {
                return Err("Missing microphone stop or user FFT".into());
            }
            println!(
                "PASS: states={states:?}, text_chunks={text_chunks}, user_fft={user_frames}, assistant_fft={assistant_frames}"
            );
            return Ok(());
        }
    }
    Err("Timed out waiting for a complete UI client turn".into())
}
