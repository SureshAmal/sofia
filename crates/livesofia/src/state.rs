//! Shared daemon state and ordered IPC event fanout.

use std::sync::{Arc, Mutex};

use sofia_protocol::{
    PROTOCOL_VERSION, ServerBody, ServerEvent, ServerMessage, StateSnapshot, TurnState,
};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Clone)]
pub struct EventHub {
    inner: Arc<Mutex<HubState>>,
    sender: broadcast::Sender<ServerMessage>,
}

struct HubState {
    snapshot: StateSnapshot,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(512);
        Self {
            inner: Arc::new(Mutex::new(HubState {
                snapshot: StateSnapshot::default(),
            })),
            sender,
        }
    }

    pub fn snapshot(&self) -> StateSnapshot {
        self.inner.lock().unwrap().snapshot.clone()
    }

    /// Subscribe and capture the next sequence under the same lock as publish.
    pub fn subscribe(&self) -> (u64, broadcast::Receiver<ServerMessage>) {
        let state = self.inner.lock().unwrap();
        let receiver = self.sender.subscribe();
        (state.snapshot.sequence + 1, receiver)
    }

    pub fn set_state(&self, state: TurnState, session_id: Option<Uuid>) {
        let mut inner = self.inner.lock().unwrap();
        if inner.snapshot.state == state && inner.snapshot.active_session_id == session_id {
            return;
        }
        inner.snapshot.state = state;
        inner.snapshot.active_session_id = session_id;
        self.publish_locked(&mut inner, ServerEvent::TurnStateChanged { state });
    }

    pub fn set_microphone_active(&self, active: bool) {
        let mut inner = self.inner.lock().unwrap();
        if inner.snapshot.microphone_active == active {
            return;
        }
        inner.snapshot.microphone_active = active;
        self.publish_audio_status(&mut inner);
    }

    pub fn set_speaker_muted(&self, muted: bool) {
        let mut inner = self.inner.lock().unwrap();
        if inner.snapshot.speaker_muted == muted {
            return;
        }
        inner.snapshot.speaker_muted = muted;
        self.publish_audio_status(&mut inner);
    }

    pub fn set_output_device(&self, device_id: Option<String>) {
        let mut inner = self.inner.lock().unwrap();
        if inner.snapshot.selected_output_device_id == device_id {
            return;
        }
        inner.snapshot.selected_output_device_id = device_id.clone();
        self.publish_locked(
            &mut inner,
            ServerEvent::AudioOutputChanged {
                selected_device_id: device_id,
            },
        );
    }

    pub fn set_gemini_voice(&self, voice_name: Option<String>) {
        let mut inner = self.inner.lock().unwrap();
        if inner.snapshot.gemini_voice_name == voice_name {
            return;
        }
        inner.snapshot.gemini_voice_name = voice_name.clone();
        self.publish_locked(&mut inner, ServerEvent::GeminiVoiceChanged { voice_name });
    }

    fn publish_audio_status(&self, inner: &mut HubState) {
        self.publish_locked(
            inner,
            ServerEvent::AudioStatusChanged {
                microphone_active: inner.snapshot.microphone_active,
                speaker_muted: inner.snapshot.speaker_muted,
            },
        );
    }

    pub fn publish(&self, event: ServerEvent) {
        let mut inner = self.inner.lock().unwrap();
        self.publish_locked(&mut inner, event);
    }

    fn publish_locked(&self, inner: &mut HubState, event: ServerEvent) {
        inner.snapshot.sequence += 1;
        let _ = self.sender.send(ServerMessage {
            version: PROTOCOL_VERSION,
            request_id: None,
            sequence: Some(inner.snapshot.sequence),
            body: ServerBody::Event(event),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscription_sequence_matches_first_event() {
        let hub = EventHub::new();
        hub.set_state(TurnState::Connecting, None);
        let (next, mut events) = hub.subscribe();
        hub.set_state(TurnState::Ready, Some(Uuid::new_v4()));
        assert_eq!(events.recv().await.unwrap().sequence, Some(next));
        assert_eq!(hub.snapshot().sequence, next);
        assert_eq!(hub.snapshot().state, TurnState::Ready);
    }

    #[test]
    fn audio_status_is_reflected_in_snapshot() {
        let hub = EventHub::new();
        hub.set_microphone_active(true);
        hub.set_speaker_muted(true);
        let snapshot = hub.snapshot();
        assert!(snapshot.microphone_active);
        assert!(snapshot.speaker_muted);
        assert_eq!(snapshot.sequence, 2);
    }
}
