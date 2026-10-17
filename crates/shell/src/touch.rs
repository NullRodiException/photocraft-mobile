//! Finger gestures on the canvas, for pen tablets (S Pen): the pen paints, fingers navigate.
//!
//! [`TouchRouter::filter`] runs on each frame's raw input before PhotoCraft sees it. A finger that
//! lands on the canvas starts a gesture: its touch events and the pointer events egui emulates
//! from them are taken out, so nothing paints, and the router reports what the fingers did
//! instead ([`Gesture`]), as Procreate does:
//!
//! - one finger pans, two or more pan, pinch and turn the view (the turn starts once the fingers
//!   have twisted past [`ROTATE_START_DEGREES`], so a pinch doesn't tilt the image);
//! - a quick tap with two, three or four fingers is undo, redo or full screen;
//! - a quick pinch closed fits the image on screen;
//! - one finger held still for [`HOLD_SECONDS`] picks a colour where it is (the eyedropper), and
//!   keeps picking as it moves until it lifts.
//!
//! The pen, fingers that land outside the canvas (panels, menus) and every touch while finger
//! navigation is off pass through untouched.
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
/// A finger held still this long (seconds) starts the eyedropper.
pub const HOLD_SECONDS: f64 = 0.5;
/// Two fingers turn the view once they have twisted this far (degrees).
pub const ROTATE_START_DEGREES: f32 = 10.0;
/// A pinch is quick if it ends within this long (seconds)…
const QUICK_PINCH_SECONDS: f64 = 0.4;
/// …and closes the fingers to less than this share of their spread: it fits the image on screen.
const QUICK_PINCH_ZOOM: f32 = 0.6;

/// A one-finger hold on the canvas: the eyedropper picks where the finger is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hold {
    /// The finger was held still long enough: start picking here.
    Start(Pos2),
    /// The held finger moved.
    Move(Pos2),
    /// The held finger lifted here.
    End(Pos2),
}

/// What a frame of finger input asks of the view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gesture {
    /// Move the image by this much (screen points, the fingers' direction).
    pub pan: Vec2,
    /// Multiply the zoom by this factor (1 = no change)…
    pub zoom: f32,
    /// …around this screen point (the fingers' centre).
    pub anchor: Option<Pos2>,
    /// Turn the view by this much (degrees, clockwise on screen) around `anchor`.
    pub rotate: f32,
    /// A gesture that turned the view ended: snap the rotation to a right angle when it is close.
    pub settle: bool,
    /// A gesture ended as a tap with this many fingers.
    pub tap: Option<usize>,
    /// A quick pinch closed: fit the image on screen.
    pub fit: bool,
    /// The one-finger hold (eyedropper).
    pub hold: Option<Hold>,
    /// A finger is held still: check again after this many seconds, for the hold.
    pub wake_in: Option<f64>,
}

impl Default for Gesture {
    fn default() -> Self {
        Self { pan: Vec2::ZERO, zoom: 1.0, anchor: None, rotate: 0.0, settle: false, tap: None, fit: false, hold: None, wake_in: None }
    }
}

/// The angle (degrees) from `a` to `b` on screen, clockwise from the x axis (y points down).
fn angle(a: Pos2, b: Pos2) -> f32 {
    (b.y - a.y).atan2(b.x - a.x).to_degrees()
}

/// `degrees` in −180…180.
fn wrap(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
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
    /// A contact of the gesture was cancelled (the pen took over, or the system found a palm): it
    /// is no tap.
    cancelled: bool,
    /// How far the fingers have twisted (degrees), and whether that turns the view yet.
    twist: f32,
    rotating: bool,
    /// The gesture's zoom so far (the product of each frame's).
    zoom_total: f32,
    /// The one finger is held: it picks colours instead of panning.
    holding: bool,
    /// Where the last finger was.
    last_pos: Pos2,
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
                                self.cancelled |= phase == TouchPhase::Cancel;
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
                            self.cancelled = false;
                            self.twist = 0.0;
                            self.rotating = false;
                            self.zoom_total = 1.0;
                            self.holding = false;
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
        if let ([a0, b0, ..], [a1, b1, ..]) = (old.as_slice(), new.as_slice()) {
            let turn = wrap(angle(*a1, *b1) - angle(*a0, *b0));
            if turn.is_finite() {
                self.twist += turn;
                if self.rotating {
                    gesture.rotate = turn;
                } else if self.twist.abs() > ROTATE_START_DEGREES {
                    self.rotating = true;
                }
            }
        }
        if !(gesture.pan.x.is_finite() && gesture.pan.y.is_finite()) {
            gesture.pan = Vec2::ZERO;
        }
        if !(gesture.zoom.is_finite() && gesture.zoom > 0.0) {
            gesture.zoom = 1.0;
        }
        self.zoom_total *= gesture.zoom;
        if let Some(c) = self.contacts.first() {
            self.last_pos = c.pos;
        }
        self.hold(&mut gesture, now);
        if self.active && self.contacts.is_empty() {
            self.active = false;
            let quick = now - self.started < TAP_SECONDS && !self.cancelled;
            if self.holding {
                gesture.hold = Some(Hold::End(self.last_pos));
            } else if quick && self.travel < TAP_SLOP {
                gesture.tap = Some(self.max_fingers);
            } else if self.max_fingers >= 2 && now - self.started < QUICK_PINCH_SECONDS && self.zoom_total < QUICK_PINCH_ZOOM {
                gesture.fit = true;
            }
            gesture.settle = self.rotating;
            events.push(Event::PointerGone);
        }
        gesture
    }

    /// The one-finger hold: a single finger that stays still for [`HOLD_SECONDS`] picks colours
    /// (and no longer pans) until it lifts.
    fn hold(&mut self, gesture: &mut Gesture, now: f64) {
        let single = self.active && self.max_fingers == 1 && self.contacts.len() == 1 && !self.cancelled;
        if self.holding {
            // The view stays put while the finger picks colours.
            *gesture = Gesture { tap: gesture.tap, ..Gesture::default() };
            if single {
                gesture.hold = Some(Hold::Move(self.last_pos));
            }
            return;
        }
        if !single || self.travel >= TAP_SLOP {
            return;
        }
        let held = now - self.started;
        if held >= HOLD_SECONDS {
            self.holding = true;
            gesture.pan = Vec2::ZERO;
            gesture.hold = Some(Hold::Start(self.last_pos));
        } else {
            gesture.wake_in = Some(HOLD_SECONDS - held);
        }
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

    /// Two fingers at `a` and `b` around (400, 400), as a frame's touch events.
    fn two(phase: TouchPhase, a: (f32, f32), b: (f32, f32)) -> Vec<Event> {
        vec![touch(1, phase, a.0, a.1), touch(2, phase, b.0, b.1)]
    }

    /// The pair turned by `deg` degrees around (400, 400), 100 points apart.
    fn turned(deg: f32) -> ((f32, f32), (f32, f32)) {
        let (s, c) = deg.to_radians().sin_cos();
        ((400.0 - 50.0 * c, 400.0 - 50.0 * s), (400.0 + 50.0 * c, 400.0 + 50.0 * s))
    }

    #[test]
    fn two_fingers_turn_the_view_once_they_twist_far_enough() {
        let mut r = TouchRouter::default();
        let (a, b) = turned(0.0);
        run(&mut r, two(TouchPhase::Start, a, b), 0.0);
        // A small twist (a pinch's wobble) turns nothing.
        let (a, b) = turned(6.0);
        assert_eq!(run(&mut r, two(TouchPhase::Move, a, b), 0.1).1.rotate, 0.0);
        // Past the threshold, the view turns with the fingers, frame by frame.
        let (a, b) = turned(12.0);
        run(&mut r, two(TouchPhase::Move, a, b), 0.2);
        let (a, b) = turned(20.0);
        let g = run(&mut r, two(TouchPhase::Move, a, b), 0.3).1;
        assert!((g.rotate - 8.0).abs() < 1e-3, "{g:?}");
        assert_eq!(g.anchor, Some(pos2(400.0, 400.0)));
        // Lifting settles the rotation (snaps it when near a right angle).
        let (_, g) = run(&mut r, two(TouchPhase::End, a, b), 0.4);
        assert!(g.settle);
        // Turning back across ±180° stays a small step.
        assert!((wrap(179.0 - -179.0) + 2.0).abs() < 1e-3);
    }

    #[test]
    fn a_quick_pinch_closed_fits_the_image() {
        let mut r = TouchRouter::default();
        run(&mut r, two(TouchPhase::Start, (200.0, 400.0), (600.0, 400.0)), 0.0);
        run(&mut r, two(TouchPhase::Move, (350.0, 400.0), (450.0, 400.0)), 0.15);
        let (_, g) = run(&mut r, two(TouchPhase::End, (350.0, 400.0), (450.0, 400.0)), 0.25);
        assert!(g.fit && g.tap.is_none(), "{g:?}");
        // The same pinch, slowly: just a zoom.
        let mut r = TouchRouter::default();
        run(&mut r, two(TouchPhase::Start, (200.0, 400.0), (600.0, 400.0)), 0.0);
        run(&mut r, two(TouchPhase::Move, (350.0, 400.0), (450.0, 400.0)), 0.6);
        assert!(!run(&mut r, two(TouchPhase::End, (350.0, 400.0), (450.0, 400.0)), 0.9).1.fit);
    }

    #[test]
    fn a_four_finger_tap_is_reported() {
        let mut r = TouchRouter::default();
        let down: Vec<Event> = (1..=4).map(|i| touch(i, TouchPhase::Start, 300.0 + 40.0 * i as f32, 400.0)).collect();
        run(&mut r, down, 0.0);
        let up: Vec<Event> = (1..=4).map(|i| touch(i, TouchPhase::End, 300.0 + 40.0 * i as f32, 400.0)).collect();
        assert_eq!(run(&mut r, up, 0.1).1.tap, Some(4));
    }

    #[test]
    fn a_finger_held_still_picks_colours_until_it_lifts() {
        let mut r = TouchRouter::default();
        let (_, g) = run(&mut r, with_pointer(1, TouchPhase::Start, 400.0, 400.0), 0.0);
        assert_eq!(g.wake_in, Some(HOLD_SECONDS), "a frame is asked for when the hold is due");
        // A frame with no events once the hold is due: the eyedropper starts where the finger is.
        let (e, g) = run(&mut r, Vec::new(), HOLD_SECONDS + 0.01);
        assert!(e.is_empty());
        assert_eq!(g.hold, Some(Hold::Start(pos2(400.0, 400.0))));
        // Moving now picks along the way instead of panning.
        let (_, g) = run(&mut r, with_pointer(1, TouchPhase::Move, 460.0, 420.0), 0.8);
        assert_eq!(g.hold, Some(Hold::Move(pos2(460.0, 420.0))));
        assert_eq!(g.pan, Vec2::ZERO);
        let (_, g) = run(&mut r, with_pointer(1, TouchPhase::End, 460.0, 420.0), 0.9);
        assert_eq!(g.hold, Some(Hold::End(pos2(460.0, 420.0))));
        assert_eq!(g.tap, None);
        // A finger that moves before the hold is due just pans.
        let mut r = TouchRouter::default();
        run(&mut r, with_pointer(1, TouchPhase::Start, 400.0, 400.0), 0.0);
        run(&mut r, with_pointer(1, TouchPhase::Move, 440.0, 400.0), 0.1);
        let (_, g) = run(&mut r, Vec::new(), 1.0);
        assert_eq!((g.hold, g.wake_in), (None, None));
    }

    #[test]
    fn a_cancelled_contact_is_no_tap() {
        // Two quick contacts, one cancelled (the pen took over, or the system saw a palm): no undo.
        let mut r = TouchRouter::default();
        let mut down = with_pointer(1, TouchPhase::Start, 400.0, 400.0);
        down.push(touch(2, TouchPhase::Start, 500.0, 400.0));
        run(&mut r, down, 0.0);
        let mut up = vec![touch(1, TouchPhase::Cancel, 400.0, 400.0)];
        up.push(touch(2, TouchPhase::End, 500.0, 400.0));
        let (_, g) = run(&mut r, up, 0.1);
        assert_eq!(g.tap, None);
        assert!(!r.active());
        // The next gesture starts clean.
        let mut down = with_pointer(1, TouchPhase::Start, 400.0, 400.0);
        down.push(touch(2, TouchPhase::Start, 500.0, 400.0));
        run(&mut r, down, 1.0);
        let up = vec![touch(1, TouchPhase::End, 400.0, 400.0), touch(2, TouchPhase::End, 500.0, 400.0)];
        assert_eq!(run(&mut r, up, 1.1).1.tap, Some(2));
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
