//! Turn text extracted from Gemini's spoken-output transcription.

use gemini_live::types::ServerEvent as GeminiEvent;
use sofia_protocol::ServerEvent as UiEvent;
use uuid::Uuid;

pub struct AssistantTranscript {
    turn_id: Uuid,
    text: String,
}

impl Default for AssistantTranscript {
    fn default() -> Self {
        Self::new()
    }
}

impl AssistantTranscript {
    pub fn new() -> Self {
        Self {
            turn_id: Uuid::new_v4(),
            text: String::new(),
        }
    }

    /// Translate spoken-output transcription into UI text events.
    /// `ModelText` is deliberately ignored for this native-audio model.
    pub fn observe(&mut self, event: &GeminiEvent) -> Option<UiEvent> {
        match event {
            GeminiEvent::OutputTranscription(fragment) if !fragment.is_empty() => {
                self.text.push_str(fragment);
                Some(UiEvent::AssistantTextDelta {
                    turn_id: self.turn_id,
                    text: fragment.clone(),
                })
            }
            GeminiEvent::TurnComplete => self.finish(false),
            GeminiEvent::Interrupted => self.finish(true),
            _ => None,
        }
    }

    fn finish(&mut self, interrupted: bool) -> Option<UiEvent> {
        let text = std::mem::take(&mut self.text);
        let turn_id = std::mem::replace(&mut self.turn_id, Uuid::new_v4());
        if text.is_empty() {
            return None;
        }
        Some(UiEvent::AssistantTextFinal {
            turn_id,
            text,
            interrupted,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_chunks_then_final_text() {
        let mut transcript = AssistantTranscript::new();
        let first = transcript
            .observe(&GeminiEvent::OutputTranscription("Hello".into()))
            .unwrap();
        let second = transcript
            .observe(&GeminiEvent::OutputTranscription(" world".into()))
            .unwrap();
        let final_event = transcript.observe(&GeminiEvent::TurnComplete).unwrap();
        let turn_id = match first {
            UiEvent::AssistantTextDelta { turn_id, text } => {
                assert_eq!(text, "Hello");
                turn_id
            }
            _ => panic!("expected a text delta"),
        };
        assert!(
            matches!(second, UiEvent::AssistantTextDelta { turn_id: id, text } if id == turn_id && text == " world")
        );
        assert!(
            matches!(final_event, UiEvent::AssistantTextFinal { turn_id: id, text, interrupted: false } if id == turn_id && text == "Hello world")
        );
        assert!(transcript.observe(&GeminiEvent::TurnComplete).is_none());
    }

    #[test]
    fn ignores_model_text_and_marks_interruption() {
        let mut transcript = AssistantTranscript::new();
        assert!(
            transcript
                .observe(&GeminiEvent::ModelText("internal".into()))
                .is_none()
        );
        transcript.observe(&GeminiEvent::OutputTranscription("Hi".into()));
        assert!(
            matches!(transcript.observe(&GeminiEvent::Interrupted), Some(UiEvent::AssistantTextFinal { text, interrupted: true, .. }) if text == "Hi")
        );
    }
}
