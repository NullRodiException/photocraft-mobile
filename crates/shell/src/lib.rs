//! # photocraft-mobile-shell
//!
//! [`ModeShell`] wraps PhotoCraft's [`PhotocraftApp`] (unchanged) in a minimal, Procreate-style
//! chrome for a tablet without a keyboard ([`studio`]): a slim top bar (Gallery, Actions,
//! Adjustments, Selection, Transform, the Vector | Pixel | Photo mode switcher, the mode's main
//! tools, Layers and the colour), a floating side bar (size, eyedropper, opacity, undo, redo) and
//! popovers that reach every menu and tool. Alt+1 / Alt+2 / Alt+3 switch modes.
//!
//! A mode switch only goes through PhotoCraft's public surface: the Window › Workspace command
//! lays out the panels, `edit.toolbar` hides the tools the mode doesn't use, and the remembered
//! tool becomes active. The decisions live in `photocraft-mobile-modes`.
//!
//! On a pen tablet the shell also turns fingers on the canvas into navigation ([`touch`]): one
//! finger pans, two pan and pinch, a two- or three-finger tap is Undo or Redo. The platform says
//! which contacts are fingers ([`ModeShell::with_touch_kind`]); without it every touch reaches
//! PhotoCraft as before.
//!
//! On a touch platform, holding a numeric field and sliding up or down scrubs its value
//! ([`scrub`]), and the on-screen keyboard only opens when a text field is tapped, not when a
//! dialog focuses one by itself.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use egui::{Event, Key, Modifiers, Stroke, StrokeKind, Vec2, vec2};
use photocraft_mobile_modes::{Mode, Modes};
use photocraft_ui_egui::canvas::ViewXform;
use photocraft_ui_egui::theme::Tokens;
use photocraft_ui_egui::{PhotocraftApp, Tool};
use serde_json::json;
use touch::Hold;

/// Two fingers that leave the view this close to a right angle (degrees) snap it there.
pub const SNAP_DEGREES: f32 = 5.0;

pub use photocraft_mobile_modes;
pub use services::{ColorSelector, ShellSettings};
pub use studio::{BAR_HEIGHT, COMPACT_BELOW, Popover, SidePanel};
pub use touch::ToolKind;

pub mod clipboard;
pub mod crash;
pub mod ime;
pub mod pen;
pub mod saf;
pub mod scrub;
pub mod services;
pub mod share;
pub mod studio;
pub mod touch;

/// Called with the new state after every switch, to save it.
pub type SaveModesFn = Box<dyn FnMut(&Modes)>;
/// Called with the shell settings after every change, to save them.
pub type SaveSettingsFn = Box<dyn FnMut(&ShellSettings)>;
/// Tells what made a touch contact (by egui touch id), where the platform knows.
pub type TouchKindFn = Box<dyn Fn(u64) -> Option<ToolKind>>;
/// Platform input added before anything else sees a frame's raw input (pen hover, pen samples),
/// with the current points-per-pixel.
pub type InputHookFn = Box<dyn FnMut(&mut egui::RawInput, f32)>;
/// What the platform asked of the app since the last frame.
pub type PlatformEventsFn = Box<dyn FnMut() -> Vec<PlatformEvent>>;
/// Sends a crash report file somewhere the user picks (Android's share sheet).
pub type SendReportFn = Box<dyn FnMut(&std::path::Path)>;

/// Something the platform asks of the app between frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatformEvent {
    /// The app went to the background, where the system may end it to free memory: autosave
    /// every document with unsaved changes now, for crash recovery.
    Background,
}

/// Every PhotoCraft tool, by the name `Tool::from_name` and `edit.toolbar` read.
pub fn tool_names() -> Vec<String> {
    named_tools().iter().map(|(n, _)| n.clone()).collect()
}

/// Every PhotoCraft tool and its name, built once.
fn named_tools() -> &'static [(String, Tool)] {
    static NAMES: std::sync::OnceLock<Vec<(String, Tool)>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| Tool::ALL.iter().map(|&t| (format!("{t:?}"), t)).collect())
}

/// The tool called `name`, as `Tool::from_name` finds it, but cheap for the names the modes list.
/// `from_name` builds a few strings for each of PhotoCraft's tools on every call, and the top bar
/// and the tool popovers look tools up on every frame.
pub fn tool_named(name: &str) -> Option<Tool> {
    named_tools().iter().find(|(n, _)| n == name).map(|(_, t)| *t).or_else(|| Tool::from_name(name))
}

/// `tool`'s name, as the modes list it.
pub fn tool_name(tool: Tool) -> &'static str {
    named_tools().iter().find(|(_, t)| *t == tool).map_or("", |(n, _)| n.as_str())
}

/// Preferences › Interface › UI Scale values (PhotoCraft's `UiScale`), as display scales.
const UI_SCALES: [(&str, f32); 12] = [
    ("75", 0.75),
    ("80", 0.8),
    ("85", 0.85),
    ("90", 0.9),
    ("95", 0.95),
    ("100", 1.0),
    ("125", 1.25),
    ("150", 1.5),
    ("175", 1.75),
    ("200", 2.0),
    ("250", 2.5),
    ("300", 3.0),
];

/// The UI Scale value for a touch screen whose system scale is `native` (points per pixel): the
/// one closest to a quarter larger, so controls are easier to hit with a finger. `None` for a
/// scale that makes no sense.
pub fn ui_scale_for(native: f32) -> Option<&'static str> {
    if !(native.is_finite() && native > 0.0) {
        return None;
    }
    let target = native * 1.25;
    UI_SCALES.iter().min_by(|a, b| (a.1 - target).abs().total_cmp(&(b.1 - target).abs())).map(|s| s.0)
}

/// PhotoCraft in the studio chrome (top bar, side bar, popovers).
pub struct ModeShell {
    pub app: PhotocraftApp,
    pub modes: Modes,
    pub settings: ShellSettings,
    save: Option<SaveModesFn>,
    save_settings: Option<SaveSettingsFn>,
    touch_kind: Option<TouchKindFn>,
    router: touch::TouchRouter,
    /// Finger pan waiting for the next `logic` pass (screen points).
    pending_pan: Vec2,
    /// A finger tap waiting for the next `logic` pass (finger count).
    pending_tap: Option<usize>,
    /// Finger turn waiting for the next `logic` pass (degrees, clockwise), around this point.
    pending_turn: f32,
    turn_anchor: Option<egui::Pos2>,
    /// A turning gesture ended: snap the rotation near a right angle.
    pending_settle: bool,
    /// A quick pinch: fit the image on screen.
    pending_fit: bool,
    /// The tool to bring back after a finger hold's eyedropper, and when (after the frame that
    /// takes the hold's release).
    hold_return: Option<Tool>,
    restore_after_frame: bool,
    /// A finger is held still: repaint after this many seconds to see the hold through.
    gesture_wake: Option<f64>,
    /// Focus (full screen): only the image and a button to come back.
    focus: bool,
    /// The top bar, side bar and popovers.
    pub studio: studio::Studio,
    /// The saved mode is applied once, after PhotoCraft's first `logic` pass.
    applied: bool,
    input_hook: Option<InputHookFn>,
    platform_events: Option<PlatformEventsFn>,
    /// The report the last crash left (see [`crash`]), and how to send it.
    crash_report: Option<std::path::PathBuf>,
    send_report: Option<SendReportFn>,
    /// Share image… and Save to Gallery, where the platform has them.
    sharing: Option<share::Sharing>,
    scrub: scrub::ValueScrub,
    /// This frame's input ended a press the user made (a tap or a lift), not one the shell did.
    tapped: bool,
    /// The widget that had keyboard focus last frame.
    last_focus: Option<egui::Id>,
    /// The focused text field may open the on-screen keyboard (the user tapped it).
    keyboard_ok: bool,
    /// A hold is pending: repaint after this many seconds to see it through.
    scrub_wake: Option<f64>,
    /// Frames drawn since the scrub in progress began.
    scrub_frames: u8,
}

impl ModeShell {
    pub fn new(app: PhotocraftApp, modes: Modes) -> Self {
        Self {
            app,
            modes,
            settings: ShellSettings::default(),
            save: None,
            save_settings: None,
            touch_kind: None,
            router: touch::TouchRouter::default(),
            pending_pan: Vec2::ZERO,
            pending_tap: None,
            pending_turn: 0.0,
            turn_anchor: None,
            pending_settle: false,
            pending_fit: false,
            hold_return: None,
            restore_after_frame: false,
            gesture_wake: None,
            focus: false,
            studio: studio::Studio::default(),
            applied: false,
            input_hook: None,
            platform_events: None,
            crash_report: None,
            send_report: None,
            sharing: None,
            scrub: scrub::ValueScrub::default(),
            tapped: false,
            last_focus: None,
            keyboard_ok: false,
            scrub_wake: None,
            scrub_frames: 0,
        }
    }

    /// Platform input to add to each frame before the shell and PhotoCraft see it.
    pub fn with_input_hook(mut self, hook: InputHookFn) -> Self {
        self.input_hook = Some(hook);
        self
    }

    /// Take the platform's requests ([`PlatformEvent`]) at the start of each frame.
    pub fn with_platform_events(mut self, events: PlatformEventsFn) -> Self {
        self.platform_events = Some(events);
        self
    }

    /// The last crash left the report at `path` ([`crash::take_report`]): a notice says so at start-up,
    /// and Actions › Quick offers to send it through `send`, where the platform can.
    pub fn with_crash_report(mut self, path: std::path::PathBuf, send: Option<SendReportFn>) -> Self {
        self.crash_report = Some(path);
        self.send_report = send;
        self
    }

    /// Actions › Quick offers to share the open image and save it to the gallery: exports go into
    /// folders under `dir`, then to `share` (see [`share`]).
    pub fn with_share(mut self, dir: std::path::PathBuf, share: share::ShareFn) -> Self {
        self.sharing = Some(share::Sharing::new(dir, share));
        self
    }

    /// The platform can share images; `Some(true)` while an export is under way.
    pub(crate) fn sharing_busy(&self) -> Option<bool> {
        self.sharing.as_ref().map(share::Sharing::busy)
    }

    /// Export the active document for `target` (Actions › Quick).
    pub(crate) fn share(&mut self, ctx: &egui::Context, target: share::ShareTarget) {
        let (Some(sharing), Some(doc)) = (self.sharing.as_mut(), self.app.session.active().map(|d| d.doc.clone())) else { return };
        let wake = ctx.clone();
        if let Err(e) = sharing.start(doc, target, move || wake.request_repaint()) {
            log::warn!("couldn't share the image: {e}");
        }
    }

    /// Tells the user when an export for sharing failed.
    fn shared(&mut self) {
        let Some(Err(e)) = self.sharing.as_mut().and_then(share::Sharing::poll) else { return };
        let title = studio::tr("Couldn't export the image", "Não foi possível exportar a imagem");
        photocraft_ui_egui::notices::post(&mut self.app, title, vec![e], true, None);
    }

    /// The report a crash left can be sent (Actions › Quick).
    pub(crate) fn can_send_report(&self) -> bool {
        self.crash_report.is_some() && self.send_report.is_some()
    }

    pub(crate) fn send_report(&mut self) {
        if let (Some(path), Some(send)) = (self.crash_report.as_deref(), self.send_report.as_mut()) {
            send(path);
        }
    }

    /// Tells the user that the app closed with an error last time.
    pub(crate) fn post_crash_notice(&mut self) {
        let Some(path) = self.crash_report.as_ref() else { return };
        let mut lines = vec![format!("{}: {}", studio::tr("The report is in", "O relatório está em"), path.display())];
        if self.send_report.is_some() {
            lines.push(studio::tr("To send it: Actions › Quick › Send crash report…", "Para enviar: Ações › Rápido › Enviar relatório de falha…").to_string());
        }
        let title = studio::tr("The app closed with an error last time", "O app fechou com um erro na última vez");
        photocraft_ui_egui::notices::post(&mut self.app, title, lines, true, None);
    }

    /// Carry out the platform's requests since the last frame.
    fn platform_events(&mut self) {
        let Some(poll) = self.platform_events.as_mut() else { return };
        for event in poll() {
            match event {
                PlatformEvent::Background => {
                    let dirty = self.app.session.documents().iter().filter(|d| d.is_dirty()).count();
                    log::info!("in the background: autosaving {dirty} document(s) with unsaved changes");
                    // PhotoCraft's autosave runs in this frame's `logic`, on a background thread.
                    photocraft_ui_egui::prefs_ui::autosave_now(&mut self.app);
                }
            }
        }
    }

    /// Persist the modes through `save` after each switch.
    pub fn on_change(mut self, save: SaveModesFn) -> Self {
        self.save = Some(save);
        self
    }

    /// Start from saved shell settings and persist them through `save` after each change.
    pub fn with_settings(mut self, settings: ShellSettings, save: SaveSettingsFn) -> Self {
        self.settings = settings;
        self.save_settings = Some(save);
        self
    }

    /// Tell fingers from the pen (pen tablets): fingers on the canvas then navigate.
    pub fn with_touch_kind(mut self, kind: TouchKindFn) -> Self {
        self.touch_kind = Some(kind);
        self
    }

    /// Enter `mode`: its workspace, its tools, its last tool.
    pub fn switch_to(&mut self, ctx: &egui::Context, mode: Mode) -> Result<(), String> {
        let names = tool_names();
        let all: Vec<&str> = names.iter().map(String::as_str).collect();
        let current = format!("{:?}", self.app.ui.tool);
        let plan = self.modes.switch(mode, &current, &all);
        photocraft_ui_egui::menus::invoke(&mut self.app, ctx, plan.workspace_command, json!({}))?;
        self.app.run("edit.toolbar", json!({ "hidden": plan.hidden }))?;
        if let Some(tool) = tool_named(&plan.tool) {
            self.app.ui.tool = tool;
        }
        self.chrome_relayout();
        if let Some(save) = self.save.as_mut() {
            save(&self.modes);
        }
        Ok(())
    }

    fn switch_logged(&mut self, ctx: &egui::Context, mode: Mode) {
        if let Err(e) = self.switch_to(ctx, mode) {
            log::warn!("couldn't switch to the {} mode: {e}", mode.id());
        }
    }

    /// Run a PhotoCraft command by id; a refusal (nothing to undo, no document) is only logged.
    fn command(&mut self, ctx: &egui::Context, id: &str) {
        if let Err(e) = photocraft_ui_egui::menus::invoke(&mut self.app, ctx, id, json!({})) {
            log::info!("{id}: {e}");
        }
    }

    /// Focus is on: only the image shows.
    pub fn focused(&self) -> bool {
        self.focus
    }

    /// Hide every bar, panel and popover (full screen), or bring them back.
    pub fn toggle_focus(&mut self) {
        self.focus = !self.focus;
        self.studio.popover = None;
        self.apply_chrome();
    }

    /// Turn finger navigation on or off, and save it.
    pub fn set_finger_navigates(&mut self, on: bool) {
        self.settings.finger_navigates = on;
        self.save_settings();
    }

    fn save_settings(&mut self) {
        if let Some(save) = self.save_settings.as_mut() {
            save(&self.settings);
        }
    }

    /// Move the active document's view by `d` screen points, as a finger drags it (the image
    /// follows the finger). The same mapping as the canvas's own scroll pan (`ViewXform::unmap_vec`:
    /// flip, then rotation); pixel aspect correction is left out.
    pub fn pan_view(&mut self, pixels_per_point: f32, d: Vec2) {
        let Some(idx) = self.app.session.active_index() else { return };
        let flip = self.app.ui.view.flip_horizontal;
        let Some(view) = self.app.ui.views.get_mut(idx) else { return };
        let scale = view.zoom / pixels_per_point;
        if !(scale.is_finite() && scale > 0.0 && d.x.is_finite() && d.y.is_finite()) {
            return;
        }
        let d = if flip { vec2(-d.x, d.y) } else { d };
        let (c, s) = if view.rotation.abs() < 1e-8 {
            (1.0, 0.0)
        } else {
            let r = view.rotation.to_radians();
            (r.cos(), r.sin())
        };
        view.center[0] -= (d.x * c + d.y * s) / scale;
        view.center[1] -= (-d.x * s + d.y * c) / scale;
    }

    /// Turn the active document's view by `degrees` (clockwise on screen) around the screen point
    /// `anchor`, which keeps showing the same spot of the image, as two fingers turn it.
    pub fn rotate_view(&mut self, degrees: f32, anchor: egui::Pos2) {
        let Some(idx) = self.app.session.active_index() else { return };
        let Some(xf) = ViewXform::active(&self.app) else { return };
        if !(degrees.is_finite() && xf.zoom.is_finite() && xf.zoom > 0.0 && anchor.x.is_finite() && anchor.y.is_finite()) {
            return;
        }
        let doc = xf.to_doc(anchor);
        // A mirrored view turns the other way on screen.
        let delta = if xf.flip { -degrees } else { degrees };
        let rotation = (xf.rotation + delta + 180.0).rem_euclid(360.0) - 180.0;
        let turned = ViewXform { rotation, ..xf };
        let u = turned.unmap_vec((anchor - xf.rect.center()) / xf.zoom);
        let center = [doc[0] as f32 - u.x, doc[1] as f32 - u.y];
        if let Some(view) = self.app.ui.views.get_mut(idx)
            && center.iter().all(|c| c.is_finite())
        {
            view.rotation = rotation;
            view.center = center;
        }
    }

    /// Snap the view's rotation to the nearest right angle when it is within
    /// [`SNAP_DEGREES`] of it (after two fingers turned it), keeping `anchor` in place.
    fn settle_rotation(&mut self, anchor: egui::Pos2) {
        let Some(rotation) = self.app.session.active_index().and_then(|i| self.app.ui.views.get(i)).map(|v| v.rotation) else { return };
        let nearest = (rotation / 90.0).round() * 90.0;
        let off = nearest - rotation;
        if off != 0.0 && off.abs() <= SNAP_DEGREES {
            let flip = self.app.ui.view.flip_horizontal;
            self.rotate_view(if flip { -off } else { off }, anchor);
        }
    }

    /// Apply the finger gesture gathered by `raw_input_hook`.
    fn apply_gesture(&mut self, ctx: &egui::Context) {
        let pan = std::mem::take(&mut self.pending_pan);
        if pan != Vec2::ZERO {
            self.pan_view(ctx.pixels_per_point(), pan);
            ctx.request_repaint();
        }
        let turn = std::mem::take(&mut self.pending_turn);
        let anchor = self.turn_anchor.unwrap_or_else(|| self.app.last_canvas_rect.center());
        if turn != 0.0 {
            self.rotate_view(turn, anchor);
            ctx.request_repaint();
        }
        if std::mem::take(&mut self.pending_settle) {
            self.settle_rotation(anchor);
            ctx.request_repaint();
        }
        if std::mem::take(&mut self.pending_fit) {
            self.command(ctx, "view.fitOnScreen");
        }
        match self.pending_tap.take() {
            Some(2) => self.command(ctx, "edit.undo"),
            Some(3) => self.command(ctx, "edit.redo"),
            Some(4) => self.toggle_focus(),
            _ => {}
        }
    }

    /// A one-finger hold on the canvas: the eyedropper picks where the finger is, as a press and
    /// drag of the pointer, and the tool comes back after the frame that takes the release.
    fn hold(&mut self, hold: Hold, events: &mut Vec<Event>) {
        let press = |pos, pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        match hold {
            Hold::Start(pos) => {
                if self.app.ui.tool != Tool::Eyedropper {
                    self.hold_return = Some(self.app.ui.tool);
                    self.app.ui.tool = Tool::Eyedropper;
                }
                events.extend([Event::PointerMoved(pos), press(pos, true)]);
            }
            Hold::Move(pos) => events.push(Event::PointerMoved(pos)),
            Hold::End(pos) => {
                // Before the gesture's closing PointerGone, so the release lands where the finger was.
                let at = if matches!(events.last(), Some(Event::PointerGone)) { events.len() - 1 } else { events.len() };
                events.insert(at, press(pos, false));
                self.restore_after_frame = self.hold_return.is_some();
            }
        }
    }

    /// Alt+1 / Alt+2 / Alt+3, taken before PhotoCraft's own shortcuts see them, and Esc closing a
    /// popover. Left alone while a text field has the keyboard, so typing isn't interrupted.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.studio.popover.is_some() && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.studio.popover = None;
            return;
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let picked = Mode::ALL.into_iter().find(|&mode| ctx.input_mut(|i| i.consume_key(Modifiers::ALT, shortcut_key(mode))));
        if let Some(mode) = picked {
            self.switch_logged(ctx, mode);
        }
    }
}

fn shortcut_key(mode: Mode) -> Key {
    match mode {
        Mode::Vector => Key::Num1,
        Mode::Pixel => Key::Num2,
        Mode::Photo => Key::Num3,
    }
}

impl eframe::App for ModeShell {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.platform_events();
        self.shared();
        self.shortcuts(ctx);
        self.apply_gesture(ctx);
        for wait in [self.scrub_wake.take(), self.gesture_wake.take()].into_iter().flatten() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        self.app.logic(ctx, frame);
        if !self.applied {
            self.applied = true;
            self.switch_logged(ctx, self.modes.mode());
            self.post_crash_notice();
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(hook) = self.input_hook.as_mut() {
            hook(raw_input, ctx.pixels_per_point());
        }
        self.tapped = false;
        // A press outside the open popover closes it, without painting under it.
        self.studio.filter(&mut raw_input.events);
        if let Some(kind) = self.touch_kind.as_ref() {
            let now = raw_input.time.unwrap_or_else(|| ctx.input(|i| i.time));
            // A finger navigates only over an open image: the start screen's buttons, and windows,
            // dialogs or menus over the canvas (last frame's layers), take it as a tap.
            let canvas = self.app.last_canvas_rect;
            let has_document = self.app.session.active_index().is_some();
            let on_canvas = |p: egui::Pos2| has_document && canvas.contains(p) && ctx.layer_id_at(p).is_none_or(|l| l.order == egui::Order::Background);
            let gesture = self.router.filter(&mut raw_input.events, now, &on_canvas, &|id| kind(id.0), self.settings.finger_navigates);
            // Hold-and-slide on a numeric field, anywhere but the canvas. Keys go out only once the
            // hold has focused a text field.
            self.tapped = raw_input.events.iter().any(|e| matches!(e, Event::PointerButton { pressed: false, .. }));
            let field_focused = ctx.memory(|m| m.focused()).is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
            let navigating = self.router.active();
            // The shell's sliders and lists take a slow press as a drag, not a field scrub.
            let studio = &self.studio;
            let scrubbable = |p: egui::Pos2| !navigating && !on_canvas(p) && !studio.owns(p);
            let scrub = self.scrub.filter(&mut raw_input.events, now, &scrubbable, field_focused);
            if scrub.began.is_some() {
                self.tapped = false;
            }
            if let Some(wait) = scrub.wake_in {
                // Asked for in `logic`: a repaint requested before the pass begins is dropped.
                self.scrub_wake = Some(wait);
            }
            // PhotoCraft zooms on a pinch (`Event::Zoom`) around the pointer, within its own limits.
            if gesture.zoom != 1.0
                && let Some(anchor) = gesture.anchor
            {
                raw_input.events.push(Event::PointerMoved(anchor));
                raw_input.events.push(Event::Zoom(gesture.zoom));
            }
            self.pending_pan += gesture.pan;
            if gesture.tap.is_some() {
                self.pending_tap = gesture.tap;
            }
            if gesture.rotate != 0.0 {
                self.pending_turn += gesture.rotate;
            }
            if gesture.anchor.is_some() {
                self.turn_anchor = gesture.anchor;
            }
            self.pending_settle |= gesture.settle;
            self.pending_fit |= gesture.fit;
            if let Some(hold) = gesture.hold {
                self.hold(hold, &mut raw_input.events);
            }
            if let Some(wait) = gesture.wake_in {
                // Asked for in `logic`, like the scrub's.
                self.gesture_wake = Some(wait);
            }
        }
        self.app.raw_input_hook(ctx, raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_chrome();
        if !self.focus {
            self.top_bar(ui);
        }
        self.app.ui(ui, frame);
        if std::mem::take(&mut self.restore_after_frame)
            && let Some(tool) = self.hold_return.take()
        {
            // PhotoCraft has taken the hold's release with the eyedropper: the tool comes back.
            self.app.ui.tool = tool;
        }
        if self.focus {
            self.focus_exit(&ctx);
        } else {
            self.side_bar(&ctx);
            self.popover(&ctx);
        }
        if self.touch_kind.is_some() {
            self.touch_keyboard_and_scrub(ui.ctx());
        }
    }
}

impl ModeShell {
    /// Touch platforms: the on-screen keyboard opens only for a text field the user tapped (a
    /// dialog focusing its first field, or a hold-to-scrub, keeps it closed so it doesn't cover
    /// the dialog), and a scrub in progress shows its slider beside the field.
    fn touch_keyboard_and_scrub(&mut self, ctx: &egui::Context) {
        let focus = ctx.memory(|m| m.focused());
        let scrubbing = self.scrub.active();
        if focus != self.last_focus {
            self.keyboard_ok = focus.is_some() && self.tapped && !scrubbing;
            self.last_focus = focus;
        } else if focus.is_some() && self.tapped && !scrubbing {
            self.keyboard_ok = true;
        }
        if !self.keyboard_ok {
            ctx.output_mut(|o| o.ime = None);
        }
        let Some(offset) = self.scrub.offset() else {
            self.scrub_frames = 0;
            return;
        };
        // The first frames of a scrub are asked for: the field the hold focused may only show as a
        // text field on the next one. After that the knob moves with the contact, whose events
        // bring the frames, so a finger held still costs none.
        if self.scrub_frames < 2 {
            self.scrub_frames += 1;
            ctx.request_repaint();
        }
        let Some(field) = focus.filter(|id| egui::text_edit::TextEditState::load(ctx, *id).is_some()).and_then(|id| ctx.read_response(id)).map(|r| r.rect)
        else {
            return;
        };
        let t = Tokens::get(ctx);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("photocraft-mobile-scrub")));
        painter.rect_stroke(field.expand(2.0), t.radius_sm, Stroke::new(2.0, t.accent), StrokeKind::Outside);
        // A vertical track beside the field, the knob following the finger.
        const W: f32 = 36.0;
        const H: f32 = 140.0;
        let screen = ctx.content_rect();
        let x = if field.right() + 10.0 + W <= screen.right() { field.right() + 10.0 } else { field.left() - 10.0 - W };
        let y = (field.center().y - H / 2.0).clamp(screen.top(), (screen.bottom() - H).max(screen.top()));
        let track = egui::Rect::from_min_size(egui::pos2(x, y), vec2(W, H));
        painter.rect(track, W / 2.0, t.card, Stroke::new(1.0, t.accent_border), StrokeKind::Inside);
        let arrow = |center: egui::Pos2, up: bool| {
            let d = if up { -1.0 } else { 1.0 };
            let pts = vec![center + vec2(-6.0, 3.0 * -d), center + vec2(6.0, 3.0 * -d), center + vec2(0.0, 5.0 * d)];
            painter.add(egui::Shape::convex_polygon(pts, t.text_dim, Stroke::NONE));
        };
        arrow(egui::pos2(track.center().x, track.top() + 14.0), true);
        arrow(egui::pos2(track.center().x, track.bottom() - 14.0), false);
        let reach = H / 2.0 - 30.0;
        let knob = egui::pos2(track.center().x, track.center().y - offset.clamp(-reach, reach));
        painter.circle(knob, 11.0, t.accent, Stroke::new(2.0, studio::ink_on(t.accent)));
    }
}

#[cfg(test)]
mod tests;
