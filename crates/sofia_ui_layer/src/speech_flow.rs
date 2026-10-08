//! Display provider transcription directly, without a separate word timer.
use gpui_kit::*;
use std::time::Instant;

#[derive(Default)]
pub(crate) struct SpeechFlow {
    turn: Option<String>,
    text: String,
    words: Vec<String>,
    active_start: usize,
    finished_at: Option<Instant>,
}
impl SpeechFlow {
    pub fn update(&mut self, turn: String, text: &str, final_text: bool, now: Instant) {
        if self.turn.as_ref() != Some(&turn) {
            *self = Self {
                turn: Some(turn),
                ..Default::default()
            };
        }
        if final_text {
            self.text = text.into();
            self.finished_at.get_or_insert(now);
        } else if !text.trim().is_empty() {
            // A fragment may finish the previous word, or introduce several words.
            self.active_start = self
                .text
                .split_whitespace()
                .count()
                .saturating_sub(usize::from(
                    !self.text.is_empty() && !self.text.ends_with(char::is_whitespace),
                ));
            self.text.push_str(text);
        } else {
            self.text.push_str(text);
        }
        self.words = self.text.split_whitespace().map(str::to_string).collect();
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn tick(&mut self, now: Instant) -> bool {
        if self
            .finished_at
            .is_some_and(|time| now.duration_since(time).as_secs_f32() > 1.2)
        {
            self.clear();
            return true;
        }
        false
    }
    pub fn visible(&self) -> bool {
        !self.words.is_empty()
    }
    pub fn render(&self, color: Hsla, foreground: Hsla) -> AnyElement {
        let words = self.words.clone();
        let active_start = self.active_start;
        canvas(
            |_, _, _| {},
            move |frame, _, window, cx| {
                let font_size = window.rem_size();
                let height = font_size * 1.1;
                let font = window.text_style().font();
                let shape = |text: String, active: usize| {
                    let len = text.len();
                    window.text_system().shape_line(
                        text.into(),
                        font_size,
                        &[
                            TextRun {
                                len: len - active,
                                font: font.clone(),
                                color: foreground,
                                ..Default::default()
                            },
                            TextRun {
                                len: active,
                                font: font.clone(),
                                color,
                                ..Default::default()
                            },
                        ],
                        None,
                    )
                };
                let (lines, clipped) = visible_lines(&words, f32::from(frame.size.width), |text| {
                    f32::from(shape(text.into(), 0).width)
                });
                let y = frame.origin.y + (frame.size.height - height * lines.len() as f32) / 2.;
                let mut shaped = Vec::new();
                let mut offset = words.len() - lines.iter().map(Vec::len).sum::<usize>();
                for (index, line) in lines.iter().enumerate() {
                    let text = format!(
                        "{}{}",
                        if index == 0 && clipped { ".. " } else { "" },
                        line.join(" ")
                    );
                    let first_active = active_start.saturating_sub(offset).min(line.len());
                    let active = line[first_active..].join(" ").len();
                    offset += line.len();
                    shaped.push(shape(text, active));
                }
                for (index, line) in shaped.iter().enumerate() {
                    let _ = line.paint(
                        point(frame.origin.x, y + height * index as f32),
                        height,
                        TextAlign::Center,
                        Some(frame.size.width),
                        window,
                        cx,
                    );
                }
            },
        )
        .size_full()
        .into_any_element()
    }
}
/// Keep the newest context in two lines, measuring the ellipsis as part of line one.
fn visible_lines(
    words: &[String],
    width: f32,
    measure: impl Fn(&str) -> f32,
) -> (Vec<Vec<String>>, bool) {
    for start in words.len().saturating_sub(64)..words.len() {
        let mut lines = vec![Vec::<String>::new()];
        for word in &words[start..] {
            let index = lines.len() - 1;
            let prefix = if start > 0 && index == 0 { ".. " } else { "" };
            let candidate = format!(
                "{prefix}{}{}{}",
                lines[index].join(" "),
                if lines[index].is_empty() { "" } else { " " },
                word
            );
            if !lines[index].is_empty() && measure(&candidate) > width {
                lines.push(vec![]);
            }
            lines.last_mut().unwrap().push(word.clone());
            if lines.len() > 2 {
                break;
            }
        }
        if lines.len() <= 2 {
            return (lines, start > 0);
        }
    }
    (vec![], false)
}
#[cfg(test)]
mod tests {
    use super::{SpeechFlow, visible_lines};
    use std::time::{Duration, Instant};
    #[test]
    fn fragments_appear_immediately_and_completed_turn_clears() {
        let now = Instant::now();
        let mut flow = SpeechFlow::default();
        flow.update("a".into(), "Hel", false, now);
        assert!(flow.visible());
        flow.update("a".into(), "lo world", false, now);
        assert_eq!(flow.words, vec!["Hello", "world"]);
        flow.update("a".into(), "Hello world", true, now);
        flow.tick(now + Duration::from_secs(2));
        assert!(!flow.visible());
    }
    #[test]
    fn highlights_provider_phrase_and_handles_split_words() {
        let now = Instant::now();
        let mut flow = SpeechFlow::default();
        flow.update("a".into(), "Hello ", false, now);
        flow.update("a".into(), "this is a phrase", false, now);
        assert_eq!(
            &flow.words[flow.active_start..],
            &["this", "is", "a", "phrase"]
        );
        flow.update("b".into(), "Hel", false, now);
        flow.update("b".into(), "lo world", false, now);
        assert_eq!(flow.active_start, 0);
    }
    #[test]
    fn long_transcript_keeps_newest_two_lines_and_left_ellipsis() {
        let words = "one two three four five six seven"
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let (lines, clipped) = visible_lines(&words, 14., |text| text.len() as f32);
        assert!(clipped);
        assert!(lines.len() <= 2);
        assert_eq!(lines.last().unwrap().last().unwrap(), "seven");
        for (index, line) in lines.iter().enumerate() {
            assert!(line.join(" ").len() + if index == 0 { 3 } else { 0 } <= 14);
        }
    }
}
