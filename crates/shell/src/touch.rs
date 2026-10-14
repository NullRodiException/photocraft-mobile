//! Finger gestures on the canvas, for pen tablets (S Pen): the pen paints, fingers navigate.
//!
//! [`TouchRouter::filter`] runs on each frame's raw input before PhotoCraft sees it. A finger that
//! lands on the canvas starts a gesture: its touch events and the pointer events egui emulates
//! from them are taken out, so nothing paints, and the router reports what the fingers did
//! instead ([`Gesture`]): one finger pans, two or more pan and pinch, and a quick tap with two or
//! three fingers is an undo or redo. The pen, fingers that land outside the canvas (panels,
//! menus) and every touch while finger navigation is off pass through untouched.
//!
//! Which contact is a finger comes from the platform ([`ToolKind`]): winit drops the tool type,
//! so on Android `MainActivity` reports it for each new pointer.

use egui::{Event, Pos2, TouchId, TouchPhase, Vec2};

/// What touched the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Finger,
    Pen,
}

/// A tap is shorter than this (seconds)…
const TAP_SECONDS: f64 = 0.3;
/// …and no finger moves farther than this (points).
const TAP_SLOP: f32 = 12.0;

/// What a frame of finger input asks of the view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gesture {
    /// Move the image by this much (screen points, the fingers' direction).
    pub pan: Vec2,
    /// Multiply the zoom by this factor (1 = no change)…
    pub zoom: f32,
    /// …around this screen point (the fingers' centre).
    pub anchor: Option<Pos2>,
    /// A gesture ended as a tap with this many fingers.
    pub tap: Option<usize>,
}

impl Default for Gesture {
    fn default() -> Self {
        Self { pan: Vec2::ZERO, zoom: 1.0, anchor: None, tap: None }
    }
}

#[derive(Clone, Copy, Debug)]
struct Contact {
    id: TouchId,
    start: Pos2,
    pos: Pos2,
}

/// Tracks the fingers of the gesture in progress.
#[derive(Debug, Default)]
pub struct TouchRouter {
    contacts: Vec<Contact>,
    /// A gesture runs from its first finger down until the last one lifts.
    active: bool,
    started: f64,
    max_fingers: usize,
    /// The farthest any finger of the gesture moved from where it landed (points).
    travel: f32,
}

/// Centre and mean distance to it.
fn spread(points: &[Pos2]) -> Option<(Pos2, f32)> {
    if points.is_empty() {
        return None;
    }
    let n = points.len() as f32;
    let c = points.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / n;
    let c = c.to_pos2();
    Some((c, points.iter().map(|p| p.distance(c)).sum::<f32>() / n))
}

impl TouchRouter {
    /// A finger gesture is in progress.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Takes this frame's finger gesture out of `events` and returns what it does to the view.
    ///
    /// `now` is the frame time (seconds); `on_canvas` says whether a finger landing at a screen
    /// point starts a gesture (on an open image, under no window, dialog or menu); `kind` tells
    /// fingers from the pen; and `finger_navigates` off lets every touch through.
    pub fn filter(
        &mut self,
        events: &mut Vec<Event>,
        now: f64,
        on_canvas: &dyn Fn(Pos2) -> bool,
        kind: &dyn Fn(TouchId) -> Option<ToolKind>,
        finger_navigates: bool,
    ) -> Gesture {
        let before: Vec<Contact> = self.contacts.clone();
        let mut gesture_frame = self.active;
        let mut kept = Vec::with_capacity(events.len());
        for event in events.drain(..) {
            match event {
                Event::Touch { id, phase, pos, .. } => {
                    if let Some(i) = self.contacts.iter().position(|c| c.id == id) {
                        match phase {
                            TouchPhase::Start | TouchPhase::Move => {
                                if let Some(c) = self.contacts.get_mut(i) {
                                    c.pos = pos;
                                    self.travel = self.travel.max(c.start.distance(pos));
                                }
                            }
                            TouchPhase::End | TouchPhase::Cancel => {
                                self.contacts.remove(i);
                            }
                        }
                    } else if phase == TouchPhase::Start
                        && finger_navigates
                        && kind(id) == Some(ToolKind::Finger)
                        && (self.active || on_canvas(pos))
                        && self.contacts.len() < 10
                    {
                        if !self.active {
                            self.active = true;
                            self.started = now;
                            self.max_fingers = 0;
                            self.travel = 0.0;
                            gesture_frame = true;
                        }
                        self.contacts.push(Contact { id, start: pos, pos });
                        self.max_fingers = self.max_fingers.max(self.contacts.len());
                    } else {
                        kept.push(event);
                    }
                }
                // The pointer egui emulates from the gesture's first finger: never a press on the canvas.
                Event::PointerMoved(_) | Event::PointerButton { .. } | Event::PointerGone if gesture_frame => {}
                other => kept.push(other),
            }
        }
        *events = kept;

        let mut gesture = Gesture::default();
        // Movement of the fingers that were down before this frame and still are.
        let (old, new): (Vec<Pos2>, Vec<Pos2>) = before.iter().filter_map(|b| self.contacts.iter().find(|c| c.id == b.id).map(|c| (b.pos, c.pos))).unzip();
        if let (Some((c0, d0)), Some((c1, d1))) = (spread(&old), spread(&new)) {
            gesture.pan = c1 - c0;
            if new.len() >= 2 && d0 > 1.0 && d1.is_finite() {
                gesture.zoom = d1 / d0;
                gesture.anchor = Some(c1);
            }
        }
        if !(gesture.pan.x.is_finite() && gesture.pan.y.is_finite()) {
            gesture.pan = Vec2::ZERO;
        }
        if !(gesture.zoom.is_finite() && gesture.zoom > 0.0) {
            gesture.zoom = 1.0;
        }
        if self.active && self.contacts.is_empty() {
            self.active = false;
            if now - self.started < TAP_SECONDS && self.travel < TAP_SLOP {
                gesture.tap = Some(self.max_fingers);
            }
            events.push(Event::PointerGone);
        }
        gesture
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{PointerButton, Rect, TouchDeviceId, pos2, vec2};

    const PEN: u64 = 99;

    fn canvas() -> Rect {
        Rect::from_min_max(pos2(100.0, 100.0), pos2(900.0, 700.0))
    }

    fn on_canvas(p: Pos2) -> bool {
        canvas().contains(p)
    }

    fn kind(id: TouchId) -> Option<ToolKind> {
        Some(if id.0 == PEN { ToolKind::Pen } else { ToolKind::Finger })
    }

    fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> Event {
        Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id), phase, pos: pos2(x, y), force: Some(0.5) }
    }

    /// A touch as egui-winit delivers it: the touch, then the emulated pointer for the first contact.
    fn with_pointer(id: u64, phase: TouchPhase, x: f32, y: f32) -> Vec<Event> {
        let mut v = vec![touch(id, phase, x, y), Event::PointerMoved(pos2(x, y))];
        match phase {
            TouchPhase::Start => v.push(Event::PointerButton { pos: pos2(x, y), button: PointerButton::Primary, pressed: true, modifiers: Default::default() }),
            TouchPhase::End => {
                v.push(Event::PointerButton { pos: pos2(x, y), button: PointerButton::Primary, pressed: false, modifiers: Default::default() });
                v.push(Event::PointerGone);
            }
            _ => {}
        }
        v
    }

    fn run(r: &mut TouchRouter, mut events: Vec<Event>, now: f64) -> (Vec<Event>, Gesture) {
        let g = r.filter(&mut events, now, &on_canvas, &kind, true);
        (events, g)
    }

    fn presses(events: &[Event]) -> usize {
        events.iter().filter(|e| matches!(e, Event::PointerButton { pressed: true, .. })).count()
    }

    #[test]
    fn a_finger_on_the_canvas_pans_and_never_presses() {
        let mut r = TouchRouter::default();
        let (e, g) = run(&mut r, with_pointer(1, TouchPhase::Start, 400.0, 400.0), 0.0);
        assert!(e.is_empty(), "{e:?}");
        assert_eq!(g.pan, Vec2::ZERO);
        assert!(r.active());
        let (e, g) = run(&mut r, with_pointer(1, TouchPhase::Move, 430.0, 380.0), 0.1);
        assert!(e.is_empty());
        assert_eq!(g.pan, vec2(30.0, -20.0));
        assert_eq!(g.zoom, 1.0);
        let (e, g) = run(&mut r, with_pointer(1, TouchPhase::End, 430.0, 380.0), 0.5);
        assert_eq!(e, vec![Event::PointerGone]);
        assert_eq!(g.tap, None, "it moved: not a tap");
        assert!(!r.active());
    }

    #[test]
    fn the_pen_passes_through() {
        let mut r = TouchRouter::default();
        let input = with_pointer(PEN, TouchPhase::Start, 400.0, 400.0);
        let (e, g) = run(&mut r, input.clone(), 0.0);
        assert_eq!(e, input);
        assert_eq!(g, Gesture::default());
        assert!(!r.active());
    }

    #[test]
    fn a_finger_outside_the_canvas_passes_through() {
        let mut r = TouchRouter::default();
        let input = with_pointer(1, TouchPhase::Start, 950.0, 50.0);
        let (e, _) = run(&mut r, input.clone(), 0.0);
        assert_eq!(e, input);
        assert_eq!(presses(&e), 1);
    }

    #[test]
    fn with_navigation_off_fingers_paint() {
        let mut r = TouchRouter::default();
        let mut e = with_pointer(1, TouchPhase::Start, 400.0, 400.0);
        let g = r.filter(&mut e, 0.0, &on_canvas, &kind, false);
        assert_eq!(presses(&e), 1);
        assert_eq!(g, Gesture::default());
    }

    #[test]
    fn an_unknown_contact_passes_through() {
        let mut r = TouchRouter::default();
        let mut e = with_pointer(1, TouchPhase::Start, 400.0, 400.0);
        r.filter(&mut e, 0.0, &on_canvas, &|_| None, true);
        assert_eq!(presses(&e), 1);
    }

    #[test]
    fn two_fingers_pinch_around_their_centre_and_pan() {
        let mut r = TouchRouter::default();
        let mut start = with_pointer(1, TouchPhase::Start, 300.0, 400.0);
        start.push(touch(2, TouchPhase::Start, 500.0, 400.0));
        run(&mut r, start, 0.0);
        // Spread from 200 to 300 apart, centre moves 10 right.
        let (e, g) = run(&mut r, vec![touch(1, TouchPhase::Move, 260.0, 400.0), touch(2, TouchPhase::Move, 560.0, 400.0)], 0.1);
        assert!(e.is_empty());
        assert!((g.zoom - 1.5).abs() < 1e-5, "{g:?}");
        assert_eq!(g.anchor, Some(pos2(410.0, 400.0)));
        assert_eq!(g.pan, vec2(10.0, 0.0));
    }

    #[test]
    fn a_second_finger_joins_without_a_jump() {
        let mut r = TouchRouter::default();
        run(&mut r, with_pointer(1, TouchPhase::Start, 300.0, 400.0), 0.0);
        // The second finger lands far away (even off the canvas): no pan or zoom from its arrival.
        let (e, g) = run(&mut r, vec![touch(2, TouchPhase::Start, 950.0, 400.0)], 0.05);
        assert!(e.is_empty(), "a finger joining a gesture belongs to it");
        assert_eq!(g, Gesture::default());
    }

    #[test]
    fn quick_taps_with_two_and_three_fingers() {
        for n in [2u64, 3] {
            let mut r = TouchRouter::default();
            let mut down = with_pointer(1, TouchPhase::Start, 400.0, 400.0);
            for i in 2..=n {
                down.push(touch(i, TouchPhase::Start, 400.0 + 50.0 * i as f32, 400.0));
            }
            run(&mut r, down, 1.0);
            let mut up = with_pointer(1, TouchPhase::End, 401.0, 400.0);
            for i in 2..=n {
                up.push(touch(i, TouchPhase::End, 400.0 + 50.0 * i as f32, 402.0));
            }
            let (e, g) = run(&mut r, up, 1.15);
            assert_eq!(g.tap, Some(n as usize));
            assert_eq!(e, vec![Event::PointerGone]);
        }
    }

    #[test]
    fn a_slow_touch_is_no_tap() {
        let mut r = TouchRouter::default();
        let mut down = with_pointer(1, TouchPhase::Start, 400.0, 400.0);
        down.push(touch(2, TouchPhase::Start, 500.0, 400.0));
        run(&mut r, down, 0.0);
        let mut up = with_pointer(1, TouchPhase::End, 400.0, 400.0);
        up.push(touch(2, TouchPhase::End, 500.0, 400.0));
        assert_eq!(run(&mut r, up, 0.8).1.tap, None);
    }

    #[test]
    fn other_events_are_kept_in_order() {
        let mut r = TouchRouter::default();
        let mut e = vec![Event::Text("a".into())];
        e.extend(with_pointer(1, TouchPhase::Start, 400.0, 400.0));
        e.push(Event::Text("b".into()));
        let (e, _) = run(&mut r, e, 0.0);
        assert_eq!(e, vec![Event::Text("a".into()), Event::Text("b".into())]);
    }

    #[test]
    fn non_finite_input_never_leaks_into_the_view() {
        let mut r = TouchRouter::default();
        let mut start = vec![touch(1, TouchPhase::Start, 300.0, 300.0), touch(2, TouchPhase::Start, 300.0, 300.0)];
        r.filter(&mut start, 0.0, &on_canvas, &kind, true);
        let mut mv = vec![touch(1, TouchPhase::Move, f32::NAN, 300.0), touch(2, TouchPhase::Move, 300.0, 300.0)];
        let g = r.filter(&mut mv, 0.1, &on_canvas, &kind, true);
        assert!(g.pan.x.is_finite() && g.zoom.is_finite() && g.zoom > 0.0, "{g:?}");
    }
}
