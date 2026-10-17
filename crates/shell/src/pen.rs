//! Pen and memory helpers for touch platforms, kept here (pure and tested) rather than in the
//! Android crate, which only builds for Android.

/// Android's stylus angles as PhotoCraft's tilt (Pointer Events' `tiltX`/`tiltY`, degrees in
/// −90…90): `tilt` is `AXIS_TILT` (radians from perpendicular) and `orientation` is
/// `AXIS_ORIENTATION` (radians, 0 = pointing up the screen, clockwise positive). Same conversion
/// as Chromium's Android pointer events.
pub fn tilt_from_android(tilt: f32, orientation: f32) -> (f32, f32) {
    if !(tilt.is_finite() && orientation.is_finite()) {
        return (0.0, 0.0);
    }
    let tilt = tilt.clamp(0.0, std::f32::consts::FRAC_PI_2);
    let (r, z) = (tilt.sin(), tilt.cos());
    let tilt_x = ((-orientation).sin() * r).atan2(z).to_degrees();
    let tilt_y = ((-orientation).cos() * r).atan2(z).to_degrees();
    (tilt_x.clamp(-90.0, 90.0), tilt_y.clamp(-90.0, 90.0))
}

/// Preferences › Performance › Memory Usage for a device with `total_mb` of RAM: a third of it,
/// between 1 and 4 GB in 256 MB steps. A phone or tablet shares that RAM with the system and other
/// apps, and Android ends an app that takes too much; PhotoCraft's desktop default is 8 GB.
pub fn memory_budget_mb(total_mb: u64) -> u32 {
    let third = total_mb / 3;
    let stepped = third / 256 * 256;
    stepped.clamp(1024, 4096) as u32
}

/// `MemTotal` from `/proc/meminfo` text, in MB.
pub fn mem_total_mb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// Batches of pen positions kept at most (a stalled frame drops the oldest).
const MAX_BATCHES: usize = 64;

/// One motion event's batched positions of a pen, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
struct Batch {
    id: u64,
    /// The positions Android batched before the event's own, oldest first.
    history: Vec<[f32; 2]>,
    /// The event's own position: the one winit passes on.
    current: [f32; 2],
}

/// The pen positions that winit drops on Android.
///
/// Android batches the samples of a fast pen into one motion event, with the earlier ones in the
/// event's history. winit 0.30 turns each event into one touch move at its last position, so a
/// quick curve reached PhotoCraft as a few straight segments. The platform reports every sample
/// here ([`PenHistory::historical`], then [`PenHistory::current`]) before the event reaches winit.
/// [`PenHistory::insert`] then puts the batched positions back in a frame's input, as touch moves
/// (and pointer moves, for the touch that drives egui's pointer) just before winit's move of the
/// same event. The pressure and tilt of each sample go through PhotoCraft's stylus feed as before,
/// which interpolates them for each pointer move.
#[derive(Debug, Default)]
pub struct PenHistory {
    /// Samples of the event being reported, by pen.
    open: Vec<(u64, [f32; 2])>,
    /// Events waiting for winit's move, oldest first.
    batches: std::collections::VecDeque<Batch>,
}

impl PenHistory {
    /// A sample from the history of pen `id`'s next motion event (physical pixels).
    pub fn historical(&mut self, id: u64, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() {
            self.open.push((id, [x, y]));
        }
    }

    /// The motion event's own sample for pen `id`: its batch is complete.
    pub fn current(&mut self, id: u64, x: f32, y: f32) {
        let history: Vec<[f32; 2]> = self.open.iter().filter(|(i, _)| *i == id).map(|(_, p)| *p).collect();
        self.open.retain(|(i, _)| *i != id);
        if history.is_empty() || !(x.is_finite() && y.is_finite()) {
            return;
        }
        if self.batches.len() >= MAX_BATCHES {
            self.batches.pop_front();
        }
        self.batches.push_back(Batch { id, history, current: [x, y] });
    }

    /// Pen `id` lifted or was cancelled: nothing of it waits any more.
    pub fn lifted(&mut self, id: u64) {
        self.open.retain(|(i, _)| *i != id);
        self.batches.retain(|b| b.id != id);
    }

    /// Positions waiting for winit's move (tests).
    pub fn waiting(&self) -> usize {
        self.batches.iter().map(|b| b.history.len()).sum()
    }

    /// Puts the batched positions back into a frame's `events`, before winit's move of each event
    /// they belong to. Returns how many positions it added.
    pub fn insert(&mut self, events: &mut Vec<egui::Event>, pixels_per_point: f32) -> usize {
        if self.batches.is_empty() || !(pixels_per_point.is_finite() && pixels_per_point > 0.0) {
            return 0;
        }
        let points = |p: [f32; 2]| egui::pos2(p[0] / pixels_per_point, p[1] / pixels_per_point);
        let mut out = Vec::with_capacity(events.len());
        let mut added = 0;
        let mut iter = std::mem::take(events).into_iter().peekable();
        while let Some(event) = iter.next() {
            if let egui::Event::Touch { device_id, id, phase: egui::TouchPhase::Move, pos, .. } = event
                && let Some(at) = self.batches.iter().position(|b| b.id == id.0 && points(b.current).distance(pos) < 0.5)
            {
                // Earlier batches of this pen, whose move never came, are stale.
                let mut index = 0;
                self.batches.retain(|b| {
                    let keep = b.id != id.0 || index >= at;
                    index += 1;
                    keep
                });
                let drives_pointer = matches!(iter.peek(), Some(egui::Event::PointerMoved(p)) if p.distance(pos) < 0.5);
                if let Some(index) = self.batches.iter().position(|b| b.id == id.0)
                    && let Some(batch) = self.batches.remove(index)
                {
                    for p in batch.history {
                        let p = points(p);
                        out.push(egui::Event::Touch { device_id, id, phase: egui::TouchPhase::Move, pos: p, force: None });
                        if drives_pointer {
                            out.push(egui::Event::PointerMoved(p));
                        }
                        added += 1;
                    }
                }
            }
            out.push(event);
        }
        *events = out;
        added
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

    #[test]
    fn upright_pen_has_no_tilt() {
        assert_eq!(tilt_from_android(0.0, 1.0), (0.0, 0.0));
    }

    #[test]
    fn tilt_follows_the_orientation() {
        // Leaning 45° with orientation 0 (up the screen): all of it on the y axis.
        let (x, y) = tilt_from_android(FRAC_PI_4, 0.0);
        assert!(x.abs() < 1e-3 && (y - 45.0).abs() < 1e-3, "{x} {y}");
        // Orientation a quarter turn clockwise: all of it on the x axis, the other sign.
        let (x, y) = tilt_from_android(FRAC_PI_4, FRAC_PI_2);
        assert!((x + 45.0).abs() < 1e-3 && y.abs() < 1e-3, "{x} {y}");
    }

    #[test]
    fn nonsense_angles_are_upright() {
        assert_eq!(tilt_from_android(f32::NAN, 0.0), (0.0, 0.0));
        let (x, y) = tilt_from_android(10.0, 0.0);
        assert!(x.abs() <= 90.0 && y.abs() <= 90.0);
    }

    #[test]
    fn memory_budget_is_a_third_within_limits() {
        assert_eq!(memory_budget_mb(12 * 1024), 4096);
        assert_eq!(memory_budget_mb(8 * 1024), 2560);
        assert_eq!(memory_budget_mb(2 * 1024), 1024);
        assert_eq!(memory_budget_mb(64 * 1024), 4096);
    }

    fn pen_move(id: u64, x: f32, y: f32) -> egui::Event {
        egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase: egui::TouchPhase::Move, pos: egui::pos2(x, y), force: None }
    }

    fn positions(events: &[egui::Event]) -> Vec<(char, f32, f32)> {
        events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Touch { pos, .. } => Some(('t', pos.x, pos.y)),
                egui::Event::PointerMoved(p) => Some(('p', p.x, p.y)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn batched_positions_go_back_before_winits_move() {
        let mut h = PenHistory::default();
        // One event at (40, 40) px batching (10, 10) and (20, 20); 2 px per point.
        h.historical(3, 10.0, 10.0);
        h.historical(3, 20.0, 20.0);
        h.current(3, 40.0, 40.0);
        let mut events = vec![pen_move(3, 20.0, 20.0), egui::Event::PointerMoved(egui::pos2(20.0, 20.0))];
        assert_eq!(h.insert(&mut events, 2.0), 2);
        assert_eq!(positions(&events), [('t', 5.0, 5.0), ('p', 5.0, 5.0), ('t', 10.0, 10.0), ('p', 10.0, 10.0), ('t', 20.0, 20.0), ('p', 20.0, 20.0)]);
        assert_eq!(h.waiting(), 0);
    }

    #[test]
    fn a_pen_not_driving_the_pointer_gets_touch_moves_only() {
        let mut h = PenHistory::default();
        h.historical(3, 10.0, 10.0);
        h.current(3, 20.0, 20.0);
        let mut events = vec![pen_move(3, 20.0, 20.0)];
        assert_eq!(h.insert(&mut events, 1.0), 1);
        assert_eq!(positions(&events), [('t', 10.0, 10.0), ('t', 20.0, 20.0)]);
    }

    #[test]
    fn a_batch_waits_for_its_move_and_stale_ones_go() {
        let mut h = PenHistory::default();
        h.historical(3, 1.0, 1.0);
        h.current(3, 2.0, 2.0); // its move never comes
        h.historical(3, 5.0, 5.0);
        h.current(3, 6.0, 6.0);
        // A frame before winit delivers either event: nothing changes.
        let mut early = vec![pen_move(3, 9.0, 9.0)];
        assert_eq!(h.insert(&mut early, 1.0), 0);
        assert_eq!(h.waiting(), 2);
        // The second event's move: its batch goes in, the first batch was stale.
        let mut events = vec![pen_move(3, 6.0, 6.0)];
        assert_eq!(h.insert(&mut events, 1.0), 1);
        assert_eq!(positions(&events), [('t', 5.0, 5.0), ('t', 6.0, 6.0)]);
        assert_eq!(h.waiting(), 0);
    }

    #[test]
    fn an_event_without_history_or_a_lifted_pen_leaves_nothing() {
        let mut h = PenHistory::default();
        h.current(3, 2.0, 2.0);
        assert_eq!(h.waiting(), 0);
        h.historical(3, 1.0, 1.0);
        h.current(3, 2.0, 2.0);
        h.historical(4, 1.0, 1.0);
        h.lifted(3);
        h.lifted(4);
        assert_eq!(h.waiting(), 0);
        h.current(4, 2.0, 2.0);
        assert_eq!(h.waiting(), 0, "the lifted pen's open samples went too");
        // Other pens and other events are left alone.
        h.historical(5, 1.0, 1.0);
        h.current(5, 2.0, 2.0);
        let mut events = vec![pen_move(6, 2.0, 2.0), egui::Event::Text("a".into())];
        assert_eq!(h.insert(&mut events, 1.0), 0);
        assert_eq!(events.len(), 2);
        assert_eq!(h.insert(&mut events, f32::NAN), 0);
    }

    #[test]
    fn reads_mem_total() {
        assert_eq!(mem_total_mb("MemTotal:       11534336 kB\nMemFree: 1 kB\n"), Some(11264));
        assert_eq!(mem_total_mb("nothing"), None);
    }
}
