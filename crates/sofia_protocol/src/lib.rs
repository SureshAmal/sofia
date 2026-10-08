//! Versioned messages shared by `livesofia` and desktop UI clients.
//!
//! The wire format is one JSON value per line. Audio samples are deliberately
//! outside this control protocol; future spectrum updates contain bounded bins.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 10;
pub const AUDIO_SPECTRUM_BANDS: usize = 21;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ClientCapabilities {
    pub window_kinds: Vec<String>,
    pub can_own_windows: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientMessage {
    pub version: u16,
    pub request_id: Uuid,
    pub body: ClientRequest,
}

impl ClientMessage {
    pub fn new(body: ClientRequest) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: Uuid::new_v4(),
            body,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ClientRequest {
    Hello {
        client_name: String,
        capabilities: ClientCapabilities,
    },
    GetSnapshot,
    SubscribeEvents,
    SendText {
        text: String,
    },
    StartListening,
    StopListening,
    ReloadSettings,
    SetSpeakerMuted {
        muted: bool,
    },
    ListAudioOutputs,
    SelectAudioOutput {
        device_id: Option<String>,
    },
    ListGeminiVoices,
    SelectGeminiVoice {
        voice_name: Option<String>,
    },
    ContentChanged {
        id: String,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerMessage {
    pub version: u16,
    pub request_id: Option<Uuid>,
    pub sequence: Option<u64>,
    pub body: ServerBody,
}

impl ServerMessage {
    pub fn reply(request_id: Uuid, body: ServerBody) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: Some(request_id),
            sequence: None,
            body,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ServerBody {
    Hello {
        server_name: String,
        client_id: Uuid,
    },
    Snapshot(StateSnapshot),
    Subscribed {
        next_sequence: u64,
    },
    Accepted,
    AudioOutputs {
        devices: Vec<AudioOutputDevice>,
        selected_device_id: Option<String>,
    },
    GeminiVoices {
        voices: Vec<GeminiVoice>,
        selected_voice_name: Option<String>,
    },
    Pong,
    Error(ProtocolError),
    Event(ServerEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateSnapshot {
    pub sequence: u64,
    pub state: TurnState,
    pub active_session_id: Option<Uuid>,
    pub microphone_active: bool,
    pub speaker_muted: bool,
    pub selected_output_device_id: Option<String>,
    pub gemini_voice_name: Option<String>,
}

impl Default for StateSnapshot {
    fn default() -> Self {
        Self {
            sequence: 0,
            state: TurnState::Disconnected,
            active_session_id: None,
            microphone_active: false,
            speaker_muted: false,
            selected_output_device_id: None,
            gemini_voice_name: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Disconnected,
    Connecting,
    Ready,
    Listening,
    Thinking,
    Speaking,
    ToolQueued,
    ToolRunning,
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ServerEvent {
    ContentChanged {
        id: String,
    },
    TurnStateChanged {
        state: TurnState,
    },
    AssistantTextDelta {
        turn_id: Uuid,
        text: String,
    },
    AssistantTextFinal {
        turn_id: Uuid,
        text: String,
        interrupted: bool,
    },
    InputText {
        text: String,
    },
    InputAudioLevel {
        rms: f32,
    },
    AudioSpectrum {
        source: AudioSource,
        bins: Vec<f32>,
    },
    AudioStatusChanged {
        microphone_active: bool,
        speaker_muted: bool,
    },
    AudioOutputChanged {
        selected_device_id: Option<String>,
    },
    GeminiVoiceChanged {
        voice_name: Option<String>,
    },
    ToolCallRequested {
        call_id: String,
        name: String,
    },
    ToolCallFinished {
        call_id: String,
        name: String,
        success: bool,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioSource {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioOutputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeminiVoice {
    pub name: String,
    pub style: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    VersionMismatch,
    HandshakeRequired,
    BadRequest,
    Unavailable,
    Busy,
    AudioDevice,
    ResyncRequired,
    Internal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_message_round_trips() {
        let message = ClientMessage::new(ClientRequest::Hello {
            client_name: "sofia_ui_layer".into(),
            capabilities: ClientCapabilities {
                window_kinds: vec!["visualizerwindow".into()],
                can_own_windows: true,
            },
        });
        let json = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serde_json::from_str::<ClientMessage>(&json).unwrap(),
            message
        );
    }

    #[test]
    fn server_message_round_trips() {
        let message = ServerMessage::reply(
            Uuid::new_v4(),
            ServerBody::Snapshot(StateSnapshot::default()),
        );
        let json = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serde_json::from_str::<ServerMessage>(&json).unwrap(),
            message
        );
    }

    #[test]
    fn assistant_text_event_round_trips() {
        let message = ServerMessage {
            version: PROTOCOL_VERSION,
            request_id: None,
            sequence: Some(4),
            body: ServerBody::Event(ServerEvent::AssistantTextFinal {
                turn_id: Uuid::new_v4(),
                text: "Hello, Sofia user.".into(),
                interrupted: false,
            }),
        };
        let json = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serde_json::from_str::<ServerMessage>(&json).unwrap(),
            message
        );
    }
}
