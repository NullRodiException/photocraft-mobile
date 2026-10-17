//! Text from an on-screen keyboard that edits a text field of the platform's own (Android's
//! GameActivity).
//!
//! Such a keyboard doesn't send keys: it edits the platform's field, and winit 0.30 passes none of
//! those edits on to egui. [`ImeMirror`] compares the platform's field with what it held at the
//! last check, and turns the difference into the events egui's text fields take: a Backspace for
//! each character removed from the end, then the new text, with a line break as Enter.
//!
//! This assumes the egui field's cursor is at the end of what the keyboard typed, which is where
//! typing leaves it. The platform's field is cleared whenever another egui widget takes the focus
//! ([`ImeMirror::focus`]), so each field starts from an empty platform field.

use egui::{Event, Key, Modifiers};

/// What the platform's text field held at the last check.
#[derive(Debug, Default)]
pub struct ImeMirror {
    text: String,
    focus: Option<egui::Id>,
}

/// A key press and release.
fn tap(key: Key) -> [Event; 2] {
    let event = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    [event(true), event(false)]
}

impl ImeMirror {
    /// The platform's field now holds `now`: the events that make the focused egui field follow it.
    pub fn update(&mut self, now: &str) -> Vec<Event> {
        if now == self.text {
            return Vec::new();
        }
        // The byte length of the common start, on a character boundary of both.
        let common = self.text.char_indices().zip(now.chars()).find(|((_, a), b)| a != b).map_or_else(|| self.text.len().min(now.len()), |((i, _), _)| i);
        let removed = self.text.get(common..).map_or(0, |s| s.chars().count());
        let added = now.get(common..).unwrap_or_default();
        let mut events = Vec::new();
        for _ in 0..removed {
            events.extend(tap(Key::Backspace));
        }
        for (i, line) in added.split('\n').enumerate() {
            if i > 0 {
                events.extend(tap(Key::Enter));
            }
            if !line.is_empty() {
                events.push(Event::Text(line.to_string()));
            }
        }
        self.text = now.to_string();
        events
    }

    /// The egui widget with the keyboard focus is `focus`. Returns true when that changed: the
    /// platform's field must then be cleared, and the mirror is cleared with it.
    pub fn focus(&mut self, focus: Option<egui::Id>) -> bool {
        if focus == self.focus {
            return false;
        }
        self.focus = focus;
        self.text.clear();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backspaces(events: &[Event]) -> usize {
        events.iter().filter(|e| matches!(e, Event::Key { key: Key::Backspace, pressed: true, .. })).count()
    }

    fn texts(events: &[Event]) -> String {
        events.iter().filter_map(|e| if let Event::Text(t) = e { Some(t.as_str()) } else { None }).collect()
    }

    #[test]
    fn typing_adds_only_the_new_text() {
        let mut m = ImeMirror::default();
        assert_eq!(m.update("c"), [Event::Text("c".into())]);
        assert_eq!(m.update("cam"), [Event::Text("am".into())]);
        assert!(m.update("cam").is_empty(), "no change, no events");
    }

    #[test]
    fn deleting_and_corrections_backspace_then_type() {
        let mut m = ImeMirror::default();
        m.update("teh ");
        // An autocorrection replaces the word: back to the common start, then the fix.
        let e = m.update("the ");
        assert_eq!(backspaces(&e), 3);
        assert_eq!(texts(&e), "he ");
        let e = m.update("th");
        assert_eq!((backspaces(&e), texts(&e).as_str()), (2, ""));
    }

    #[test]
    fn accents_count_as_one_character() {
        let mut m = ImeMirror::default();
        m.update("camada");
        let e = m.update("camadã");
        assert_eq!((backspaces(&e), texts(&e).as_str()), (1, "ã"));
        let e = m.update("cama");
        assert_eq!(backspaces(&e), 2, "ã is one character, not two bytes");
    }

    #[test]
    fn a_line_break_is_enter() {
        let mut m = ImeMirror::default();
        let e = m.update("ok\n");
        assert_eq!(e.first(), Some(&Event::Text("ok".into())));
        assert!(matches!(e.get(1), Some(Event::Key { key: Key::Enter, pressed: true, .. })));
        assert_eq!(e.len(), 3);
    }

    #[test]
    fn a_new_focus_starts_from_an_empty_field() {
        let mut m = ImeMirror::default();
        let field = egui::Id::new("a");
        assert!(m.focus(Some(field)));
        assert!(!m.focus(Some(field)), "same field: nothing to clear");
        m.update("abc");
        assert!(m.focus(Some(egui::Id::new("b"))));
        assert_eq!(m.update("x"), [Event::Text("x".into())], "nothing of the last field is deleted");
    }
}
