//! Press-and-hold scrubbing of numeric fields on a touch screen.
//!
//! PhotoCraft's value fields (Opacity, Flow, Smoothing, …) scrub with a horizontal mouse drag and
//! step with the arrow keys while focused. With a finger or the pen, [`ValueScrub::filter`]
//! adds a touch gesture: hold a field still for [`HOLD_SECONDS`], then slide up or down.
//!
//! - The hold ends the press early (a synthetic release where it started), so the field takes
//!   focus as on a tap.
//! - Each [`STEP_POINTS`] of vertical travel becomes an ArrowUp or ArrowDown, ±1 per step
//!   (Shift, ±10, once the contact is far to the side of where it started, for big changes).
//! - Lifting presses Enter, which commits the value and leaves the field.
//!
//! Only touches outside the canvas are considered (the canvas paints or navigates), and any
//! movement before the hold makes it an ordinary drag.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, TouchId, TouchPhase};

/// How long a contact must stay still before it scrubs (seconds).
pub const HOLD_SECONDS: f64 = 0.35;
/// Moving farther than this before the hold makes it an ordinary drag (points).
pub const HOLD_SLOP: f32 = 8.0;
/// Vertical travel per step (points).
pub const STEP_POINTS: f32 = 6.0;
/// Sideways distance past which each step is ten (points).
pub const COARSE_POINTS: f32 = 80.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    Pressed { id: TouchId, start: Pos2, since: f64 },
    Scrubbing { id: TouchId, start: Pos2, last_y: f32, carry: f32 },
}

/// What the scrub did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrubFrame {
    /// The hold just started a scrub at this point (the field was pressed there).
    pub began: Option<Pos2>,
    /// A scrub is in progress (after this frame's events).
    pub active: bool,
    /// Steps taken this frame (up positive), already turned into arrow keys.
    pub steps: i32,
    /// The scrub ended this frame (Enter was pressed).
    pub ended: bool,
    /// A hold is pending: check again after this many seconds.
    pub wake_in: Option<f64>,
}

#[derive(Debug)]
pub struct ValueScrub {
    state: State,
}

impl Default for ValueScrub {
    fn default() -> Self {
        Self { state: State::Idle }
    }
}

fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

impl ValueScrub {
    pub fn active(&self) -> bool {
        matches!(self.state, State::Scrubbing { .. })
    }

    /// While scrubbing: how far the contact is above where it started (points; below is negative).
    pub fn offset(&self) -> Option<f32> {
        match self.state {
            State::Scrubbing { start, last_y, .. } => Some(start.y - last_y),
            _ => None,
        }
    }

    /// Turns a press-and-hold on a field into scrubbing. `scrubbable` says whether a contact
    /// starting at a point may scrub (outside the canvas, not part of a finger gesture).
    pub fn filter(&mut self, events: &mut Vec<Event>, now: f64, scrubbable: &dyn Fn(Pos2) -> bool, field_focused: bool) -> ScrubFrame {
        let mut out = ScrubFrame::default();
        let scrubbing_at_start = self.active();
        let mut kept = Vec::with_capacity(events.len());
        for event in events.drain(..) {
            match (self.state, &event) {
                (State::Idle, Event::Touch { id, phase: TouchPhase::Start, pos, .. }) if scrubbable(*pos) => {
                    self.state = State::Pressed { id: *id, start: *pos, since: now };
                    kept.push(event);
                }
                (State::Pressed { id, start, .. }, Event::Touch { id: tid, phase, pos, .. }) if *tid == id => {
                    let moved = start.distance(*pos) > HOLD_SLOP || !pos.x.is_finite() || !pos.y.is_finite();
                    if moved || matches!(phase, TouchPhase::End | TouchPhase::Cancel) {
                        self.state = State::Idle;
                    }
                    kept.push(event);
                }
                (State::Scrubbing { id, start, last_y, carry }, Event::Touch { id: tid, phase, pos, .. }) if *tid == id => match phase {
                    TouchPhase::Start | TouchPhase::Move => {
                        if pos.y.is_finite() {
                            let mut carry = carry + (last_y - pos.y);
                            let coarse = (pos.x - start.x).abs() > COARSE_POINTS;
                            let mods = if coarse { Modifiers::SHIFT } else { Modifiers::NONE };
                            while carry.abs() >= STEP_POINTS {
                                let up = carry > 0.0;
                                carry -= STEP_POINTS.copysign(carry);
                                out.steps += if up { 1 } else { -1 };
                                // Arrow keys only reach a focused text field: elsewhere they are
                                // shortcuts (nudging a layer), so a hold on anything else just ends.
                                if field_focused {
                                    let k = if up { Key::ArrowUp } else { Key::ArrowDown };
                                    kept.push(key(k, true, mods));
                                    kept.push(key(k, false, mods));
                                }
                            }
                            self.state = State::Scrubbing { id, start, last_y: pos.y, carry };
                        }
                    }
                    TouchPhase::End | TouchPhase::Cancel => {
                        if field_focused {
                            kept.push(key(Key::Enter, true, Modifiers::NONE));
                            kept.push(key(Key::Enter, false, Modifiers::NONE));
                        }
                        kept.push(Event::PointerGone);
                        out.ended = true;
                        self.state = State::Idle;
                    }
                },
                // The contact's emulated pointer while scrubbing: the field keeps focus, nothing drags.
                (_, Event::PointerMoved(_) | Event::PointerButton { .. } | Event::PointerGone) if scrubbing_at_start || self.active() => {}
                _ => kept.push(event),
            }
        }
        *events = kept;
        // A contact held still long enough: end its press where it began, so the field focuses.
        if let State::Pressed { id, start, since } = self.state {
            let held = now - since;
            if held >= HOLD_SECONDS {
                events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
                self.state = State::Scrubbing { id, start, last_y: start.y, carry: 0.0 };
                out.began = Some(start);
            } else if held.is_finite() {
                out.wake_in = Some((HOLD_SECONDS - held).max(0.01));
            }
        }
        out.active = self.active();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{TouchDeviceId, pos2};

    fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> Event {
        Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id), phase, pos: pos2(x, y), force: None }
    }

    fn anywhere(_: Pos2) -> bool {
        true
    }

    fn arrows(events: &[Event]) -> (usize, usize) {
        let count = |k| events.iter().filter(|e| matches!(e, Event::Key { key, pressed: true, .. } if *key == k)).count();
        (count(Key::ArrowUp), count(Key::ArrowDown))
    }

    #[test]
    fn hold_then_slide_steps_the_value_and_lift_commits() {
        let mut s = ValueScrub::default();
        let mut e = vec![touch(1, TouchPhase::Start, 100.0, 50.0)];
        let f = s.filter(&mut e, 0.0, &anywhere, true);
        assert_eq!(e.len(), 1, "the press goes through: the field sees it");
        assert!(f.wake_in.is_some_and(|w| (w - HOLD_SECONDS).abs() < 1e-9));
        // Still held, no events: the hold fires.
        let mut e = vec![];
        let f = s.filter(&mut e, 0.4, &anywhere, true);
        assert_eq!(f.began, Some(pos2(100.0, 50.0)));
        assert!(matches!(e.as_slice(), [Event::PointerButton { pressed: false, .. }]), "{e:?}");
        assert!(s.active());
        // 20 points up: three steps up, the emulated pointer swallowed.
        let mut e = vec![touch(1, TouchPhase::Move, 101.0, 30.0), Event::PointerMoved(pos2(101.0, 30.0))];
        let f = s.filter(&mut e, 0.5, &anywhere, true);
        assert_eq!(f.steps, 3);
        assert_eq!(arrows(&e), (3, 0));
        assert!(!e.iter().any(|e| matches!(e, Event::PointerMoved(_) | Event::Touch { .. })));
        // Back down 40 (the 2 left over count): six steps down.
        let mut e = vec![touch(1, TouchPhase::Move, 101.0, 70.0)];
        assert_eq!(s.filter(&mut e, 0.6, &anywhere, true).steps, -6);
        // Lift: Enter, no click.
        let mut e = vec![
            touch(1, TouchPhase::End, 101.0, 70.0),
            Event::PointerButton { pos: pos2(101.0, 70.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        ];
        let f = s.filter(&mut e, 0.7, &anywhere, true);
        assert!(f.ended && !s.active());
        assert!(e.iter().any(|e| matches!(e, Event::Key { key: Key::Enter, pressed: true, .. })));
        assert!(!e.iter().any(|e| matches!(e, Event::PointerButton { .. })));
    }

    #[test]
    fn far_to_the_side_steps_are_coarse() {
        let mut s = ValueScrub::default();
        s.filter(&mut vec![touch(1, TouchPhase::Start, 100.0, 50.0)], 0.0, &anywhere, true);
        s.filter(&mut vec![], 0.4, &anywhere, true);
        let mut e = vec![touch(1, TouchPhase::Move, 300.0, 44.0)];
        s.filter(&mut e, 0.5, &anywhere, true);
        assert!(e.iter().any(|e| matches!(e, Event::Key { key: Key::ArrowUp, modifiers, .. } if modifiers.shift)));
    }

    #[test]
    fn a_tap_or_an_early_drag_is_left_alone() {
        let mut s = ValueScrub::default();
        s.filter(&mut vec![touch(1, TouchPhase::Start, 100.0, 50.0)], 0.0, &anywhere, true);
        let mut e = vec![touch(1, TouchPhase::End, 100.0, 50.0)];
        let f = s.filter(&mut e, 0.1, &anywhere, true);
        assert_eq!(e.len(), 1);
        assert_eq!(f, ScrubFrame::default());

        s.filter(&mut vec![touch(2, TouchPhase::Start, 100.0, 50.0)], 1.0, &anywhere, true);
        let mut e = vec![touch(2, TouchPhase::Move, 130.0, 50.0), Event::PointerMoved(pos2(130.0, 50.0))];
        s.filter(&mut e, 1.1, &anywhere, true);
        assert_eq!(e.len(), 2, "a drag before the hold stays a drag");
        let f = s.filter(&mut vec![], 1.6, &anywhere, true);
        assert_eq!(f.began, None);
    }

    #[test]
    fn without_a_focused_field_no_keys_are_sent() {
        let mut s = ValueScrub::default();
        s.filter(&mut vec![touch(1, TouchPhase::Start, 100.0, 50.0)], 0.0, &anywhere, false);
        s.filter(&mut vec![], 0.4, &anywhere, false);
        let mut e = vec![touch(1, TouchPhase::Move, 100.0, 20.0)];
        let f = s.filter(&mut e, 0.5, &anywhere, false);
        assert_eq!(f.steps, 5);
        assert_eq!(s.offset(), Some(30.0));
        assert!(e.is_empty(), "arrow keys would be shortcuts outside a field: {e:?}");
        let mut e = vec![touch(1, TouchPhase::End, 100.0, 20.0)];
        s.filter(&mut e, 0.6, &anywhere, false);
        assert_eq!(e, vec![Event::PointerGone]);
    }

    #[test]
    fn contacts_outside_the_allowed_area_never_scrub() {
        let mut s = ValueScrub::default();
        s.filter(&mut vec![touch(1, TouchPhase::Start, 100.0, 50.0)], 0.0, &|_| false, true);
        assert_eq!(s.filter(&mut vec![], 1.0, &|_| false, true).began, None);
    }
}
