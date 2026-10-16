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
use photocraft_ui_egui::theme::Tokens;
use photocraft_ui_egui::{PhotocraftApp, Tool};
use serde_json::json;

pub use photocraft_mobile_modes;
pub use services::{ColorSelector, ShellSettings};
pub use studio::{BAR_HEIGHT, COMPACT_BELOW, Popover, SidePanel};
pub use touch::ToolKind;

pub mod pen;
pub mod scrub;
pub mod services;
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
    /// Focus (full screen): only the image and a button to come back.
    focus: bool,
    /// The top bar, side bar and popovers.
    pub studio: studio::Studio,
    /// The saved mode is applied once, after PhotoCraft's first `logic` pass.
    applied: bool,
    input_hook: Option<InputHookFn>,
    scrub: scrub::ValueScrub,
    /// This frame's input ended a press the user made (a tap or a lift), not one the shell did.
    tapped: bool,
    /// The widget that had keyboard focus last frame.
    last_focus: Option<egui::Id>,
    /// The focused text field may open the on-screen keyboard (the user tapped it).
    keyboard_ok: bool,
    /// A hold is pending: repaint after this many seconds to see it through.
    scrub_wake: Option<f64>,
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
            focus: false,
            studio: studio::Studio::default(),
            applied: false,
            input_hook: None,
            scrub: scrub::ValueScrub::default(),
            tapped: false,
            last_focus: None,
            keyboard_ok: false,
            scrub_wake: None,
        }
    }

    /// Platform input to add to each frame before the shell and PhotoCraft see it.
    pub fn with_input_hook(mut self, hook: InputHookFn) -> Self {
        self.input_hook = Some(hook);
        self
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

    /// Apply the finger gesture gathered by `raw_input_hook`.
    fn apply_gesture(&mut self, ctx: &egui::Context) {
        let pan = std::mem::take(&mut self.pending_pan);
        if pan != Vec2::ZERO {
            self.pan_view(ctx.pixels_per_point(), pan);
            ctx.request_repaint();
        }
        match self.pending_tap.take() {
            Some(2) => self.command(ctx, "edit.undo"),
            Some(3) => self.command(ctx, "edit.redo"),
            _ => {}
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
        self.shortcuts(ctx);
        self.apply_gesture(ctx);
        if let Some(wait) = self.scrub_wake.take() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        self.app.logic(ctx, frame);
        if !self.applied {
            self.applied = true;
            self.switch_logged(ctx, self.modes.mode());
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
        let Some(offset) = self.scrub.offset() else { return };
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
        // No repaint asked for: the knob only moves with the contact, whose events bring the
        // next frame. A finger held still costs no frames.
        painter.circle(knob, 11.0, t.accent, Stroke::new(2.0, studio::ink_on(t.accent)));
    }
}

#[cfg(test)]
mod tests;
