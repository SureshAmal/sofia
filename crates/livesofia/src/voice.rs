//! Prebuilt Gemini Live voices exposed to settings and IPC clients.

use sofia_protocol::GeminiVoice;

const VOICES: &[(&str, &str)] = &[
    ("Zephyr", "Bright"),
    ("Puck", "Upbeat"),
    ("Charon", "Informative"),
    ("Kore", "Firm"),
    ("Fenrir", "Excitable"),
    ("Leda", "Youthful"),
    ("Orus", "Firm"),
    ("Aoede", "Breezy"),
    ("Callirrhoe", "Easy-going"),
    ("Autonoe", "Bright"),
    ("Enceladus", "Breathy"),
    ("Iapetus", "Clear"),
    ("Umbriel", "Easy-going"),
    ("Algieba", "Smooth"),
    ("Despina", "Smooth"),
    ("Erinome", "Clear"),
    ("Algenib", "Gravelly"),
    ("Rasalgethi", "Informative"),
    ("Laomedeia", "Upbeat"),
    ("Achernar", "Soft"),
    ("Alnilam", "Firm"),
    ("Schedar", "Even"),
    ("Gacrux", "Mature"),
    ("Pulcherrima", "Forward"),
    ("Achird", "Friendly"),
    ("Zubenelgenubi", "Casual"),
    ("Vindemiatrix", "Gentle"),
    ("Sadachbia", "Lively"),
    ("Sadaltager", "Knowledgeable"),
    ("Sulafat", "Warm"),
];

pub fn available_voices() -> Vec<GeminiVoice> {
    VOICES
        .iter()
        .map(|(name, style)| GeminiVoice {
            name: (*name).into(),
            style: (*style).into(),
        })
        .collect()
}

pub fn canonical_voice_name(name: &str) -> Option<&'static str> {
    VOICES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(known, _)| *known)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_selection_uses_canonical_provider_name() {
        assert_eq!(available_voices().len(), 30);
        assert_eq!(canonical_voice_name("kore"), Some("Kore"));
        assert_eq!(canonical_voice_name("unknown"), None);
    }
}
