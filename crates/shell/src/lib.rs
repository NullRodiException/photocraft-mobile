//! # photocraft-mobile-shell
//!
//! [`ModeShell`] wraps PhotoCraft's [`PhotocraftApp`] (unchanged) and adds a touch-friendly bar
//! above the editor: the Vector | Pixel | Photo mode switcher (also Alt+1 / Alt+2 / Alt+3) and
//! large Undo, Redo, Fit on Screen and Focus buttons, for a tablet without a keyboard.
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

use egui::{Align, Color32, Event, FontId, Key, Layout, Margin, Modifiers, Sense, Stroke, StrokeKind, Vec2, vec2};
use photocraft_mobile_modes::{Mode, Modes};
use photocraft_ui_egui::theme::Tokens;
use photocraft_ui_egui::{PhotocraftApp, Tool};
use serde_json::json;

pub use photocraft_mobile_modes;
pub use services::ShellSettings;
pub use touch::ToolKind;

pub mod pen;
pub mod scrub;
pub mod services;
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

/// Height of the bar, in points.
pub const BAR_HEIGHT: f32 = 40.0;
/// Narrower than this (points), the mode switcher shows icons without labels.
pub const COMPACT_BELOW: f32 = 700.0;

/// Every PhotoCraft tool, by the name `Tool::from_name` and `edit.toolbar` read.
pub fn tool_names() -> Vec<String> {
    Tool::ALL.iter().map(|t| format!("{t:?}")).collect()
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

/// A bar button's action, run after the bar is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Mode(Mode),
    Undo,
    Redo,
    Fit,
    Focus,
    FingerNavigates(bool),
}

/// PhotoCraft with the mode switcher and touch bar on top.
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
    /// Toolbar, options bar and dock as they were before Focus hid them.
    focus_saved: Option<[bool; 3]>,
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
            focus_saved: None,
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
        if let Some(tool) = Tool::from_name(&plan.tool) {
            self.app.ui.tool = tool;
        }
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

    /// Focus is on: the toolbar, options bar and dock are hidden.
    pub fn focused(&self) -> bool {
        self.focus_saved.is_some()
    }

    /// Hide the toolbar, options bar and dock (Photoshop's Tab), or bring back what was shown.
    pub fn toggle_focus(&mut self) {
        let p = &mut self.app.ui.panels;
        match self.focus_saved.take() {
            Some([toolbar, options_bar, dock]) => {
                p.toolbar = toolbar;
                p.options_bar = options_bar;
                p.dock = dock;
            }
            None => {
                self.focus_saved = Some([p.toolbar, p.options_bar, p.dock]);
                p.toolbar = false;
                p.options_bar = false;
                p.dock = false;
            }
        }
    }

    /// Turn finger navigation on or off, and save it.
    pub fn set_finger_navigates(&mut self, on: bool) {
        self.settings.finger_navigates = on;
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

    /// Alt+1 / Alt+2 / Alt+3, taken before PhotoCraft's own shortcuts see them. Left alone while a
    /// text field has the keyboard, so typing isn't interrupted.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let picked = Mode::ALL.into_iter().find(|&mode| ctx.input_mut(|i| i.consume_key(Modifiers::ALT, shortcut_key(mode))));
        if let Some(mode) = picked {
            self.switch_logged(ctx, mode);
        }
    }

    /// The bar: mode switcher on the left, touch actions on the right.
    pub fn bar(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let lang = photocraft_ui_egui::i18n::current().code();
        let pt = lang.to_ascii_lowercase().starts_with("pt");
        let tr = |en: &'static str, pt_text: &'static str| if pt { pt_text } else { en };
        let current = self.modes.mode();
        let focused = self.focused();
        let fingers = self.touch_kind.is_some().then_some(self.settings.finger_navigates);
        let mut action = None;
        egui::Panel::top("photocraft-mobile-modes")
            .resizable(false)
            .exact_size(BAR_HEIGHT)
            .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(Margin::symmetric(8, 5)))
            .show(ui, |ui| {
                let compact = ui.available_width() < COMPACT_BELOW;
                ui.horizontal_centered(|ui| {
                    if let Some(mode) = segmented(ui, &t, current, lang, compact) {
                        action = Some(Action::Mode(mode));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        const B: f32 = 30.0;
                        let button = |ui: &mut egui::Ui, icon: &str, on: bool, tip: &str| photocraft_ui_egui::icons::button(ui, icon, B, on, tip).clicked();
                        if let Some(on) = fingers {
                            let (icon, tip) = if on {
                                ("hand", tr("Fingers navigate the canvas (tap to let them paint)", "Dedos navegam no canvas (toque para desenharem)"))
                            } else {
                                ("brush", tr("Fingers paint (tap so they navigate)", "Dedos desenham (toque para navegarem)"))
                            };
                            if button(ui, icon, on, tip) {
                                action = Some(Action::FingerNavigates(!on));
                            }
                        }
                        if button(ui, "panels-top-left", focused, tr("Focus: hide or show the panels", "Foco: esconder ou mostrar os painéis")) {
                            action = Some(Action::Focus);
                        }
                        if button(ui, "maximize-2", false, tr("Fit on Screen", "Encaixar na tela")) {
                            action = Some(Action::Fit);
                        }
                        ui.add_space(8.0);
                        if button(ui, "redo-2", false, tr("Redo (three-finger tap)", "Refazer (toque com três dedos)")) {
                            action = Some(Action::Redo);
                        }
                        if button(ui, "undo-2", false, tr("Undo (two-finger tap)", "Desfazer (toque com dois dedos)")) {
                            action = Some(Action::Undo);
                        }
                    });
                });
            });
        let ctx = ui.ctx().clone();
        match action {
            Some(Action::Mode(mode)) if mode != current => self.switch_logged(&ctx, mode),
            Some(Action::Undo) => self.command(&ctx, "edit.undo"),
            Some(Action::Redo) => self.command(&ctx, "edit.redo"),
            Some(Action::Fit) => self.command(&ctx, "view.fitOnScreen"),
            Some(Action::Focus) => self.toggle_focus(),
            Some(Action::FingerNavigates(on)) => self.set_finger_navigates(on),
            _ => {}
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

/// Black or white, whichever reads better on `fill`.
fn ink_on(fill: Color32) -> Color32 {
    let l = 0.2126 * f32::from(fill.r()) + 0.7152 * f32::from(fill.g()) + 0.0722 * f32::from(fill.b());
    if l > 150.0 { Color32::from_gray(16) } else { Color32::WHITE }
}

/// A pill of mode buttons (icons only when `compact`); returns the one clicked.
fn segmented(ui: &mut egui::Ui, t: &Tokens, current: Mode, lang: &str, compact: bool) -> Option<Mode> {
    const H: f32 = 28.0;
    const ICON: f32 = 15.0;
    const PAD: f32 = 12.0;
    const GAP: f32 = 6.0;
    let font = FontId::proportional(13.0);
    let mut picked = None;
    let galleys: Vec<_> = Mode::ALL.iter().map(|m| ui.painter().layout_no_wrap(m.label(lang).to_string(), font.clone(), Color32::WHITE)).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| if compact { PAD + ICON + PAD } else { PAD + ICON + GAP + g.size().x + PAD }).collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total, H + 4.0), Sense::hover());
    let radius = (H + 4.0) / 2.0;
    ui.painter().rect(outer, radius, t.field, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let mut x = outer.min.x + 2.0;
    for ((mode, galley), w) in Mode::ALL.into_iter().zip(galleys).zip(widths) {
        let rect = egui::Rect::from_min_size(egui::pos2(x, outer.min.y + 2.0), vec2(w, H));
        x += w;
        let id = ui.id().with(("photocraft-mobile-mode", mode.id()));
        let resp = ui.interact(rect, id, Sense::click());
        let selected = mode == current;
        let (fill, ink) = if selected {
            (t.accent, ink_on(t.accent))
        } else if resp.hovered() {
            (t.hover, t.text)
        } else {
            (Color32::TRANSPARENT, t.text_dim)
        };
        if fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, H / 2.0, fill);
        }
        let icon = egui::Rect::from_min_size(egui::pos2(rect.min.x + PAD, rect.center().y - ICON / 2.0), vec2(ICON, ICON));
        photocraft_ui_egui::icons::paint(ui, icon, mode.icon(), ICON, ink);
        if !compact {
            let text_pos = egui::pos2(icon.max.x + GAP, rect.center().y - galley.size().y / 2.0);
            ui.painter().galley(text_pos, galley, ink);
        }
        let label = mode.label(lang);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label));
        let resp = resp.on_hover_text(format!("{label} (Alt+{})", mode.index() + 1));
        if resp.clicked() {
            picked = Some(mode);
        }
    }
    picked
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
            let scrubbable = |p: egui::Pos2| !navigating && !on_canvas(p);
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
        self.bar(ui);
        self.app.ui(ui, frame);
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
        painter.circle(knob, 11.0, t.accent, Stroke::new(2.0, ink_on(t.accent)));
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests;
