//! # photocraft-mobile-shell
//!
//! [`ModeShell`] wraps PhotoCraft's [`PhotocraftApp`] (unchanged) and adds the mode switcher: a
//! segmented Vector | Pixel | Photo control above the editor, plus Alt+1 / Alt+2 / Alt+3.
//!
//! A switch only goes through PhotoCraft's public surface: the Window › Workspace command lays
//! out the panels, `edit.toolbar` hides the tools the mode doesn't use, and the remembered tool
//! becomes active. The decisions live in `photocraft-mobile-modes`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use egui::{Color32, FontId, Key, Margin, Modifiers, Sense, Stroke, StrokeKind, vec2};
use photocraft_mobile_modes::{Mode, Modes};
use photocraft_ui_egui::theme::Tokens;
use photocraft_ui_egui::{PhotocraftApp, Tool};
use serde_json::json;

pub use photocraft_mobile_modes;

pub mod services;

/// Called with the new state after every switch, to save it.
pub type SaveModesFn = Box<dyn FnMut(&Modes)>;

/// Height of the switcher bar, in points.
pub const BAR_HEIGHT: f32 = 40.0;

/// Every PhotoCraft tool, by the name `Tool::from_name` and `edit.toolbar` read.
pub fn tool_names() -> Vec<String> {
    Tool::ALL.iter().map(|t| format!("{t:?}")).collect()
}

/// PhotoCraft with the mode switcher on top.
pub struct ModeShell {
    pub app: PhotocraftApp,
    pub modes: Modes,
    save: Option<SaveModesFn>,
    /// The saved mode is applied once, after PhotoCraft's first `logic` pass.
    applied: bool,
}

impl ModeShell {
    pub fn new(app: PhotocraftApp, modes: Modes) -> Self {
        Self { app, modes, save: None, applied: false }
    }

    /// Persist the modes through `save` after each switch.
    pub fn on_change(mut self, save: SaveModesFn) -> Self {
        self.save = Some(save);
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

    /// The switcher bar.
    pub fn bar(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let lang = photocraft_ui_egui::i18n::current().code();
        let current = self.modes.mode();
        let mut picked = None;
        egui::Panel::top("photocraft-mobile-modes")
            .resizable(false)
            .exact_size(BAR_HEIGHT)
            .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(Margin::symmetric(8, 5)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(mode) = segmented(ui, &t, current, lang) {
                        picked = Some(mode);
                    }
                });
            });
        if let Some(mode) = picked.filter(|&m| m != current) {
            self.switch_logged(ui.ctx(), mode);
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

/// A pill of mode buttons; returns the one clicked.
fn segmented(ui: &mut egui::Ui, t: &Tokens, current: Mode, lang: &str) -> Option<Mode> {
    const H: f32 = 28.0;
    const ICON: f32 = 15.0;
    const PAD: f32 = 12.0;
    const GAP: f32 = 6.0;
    let font = FontId::proportional(13.0);
    let mut picked = None;
    let galleys: Vec<_> = Mode::ALL.iter().map(|m| ui.painter().layout_no_wrap(m.label(lang).to_string(), font.clone(), Color32::WHITE)).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| PAD + ICON + GAP + g.size().x + PAD).collect();
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
        let text_pos = egui::pos2(icon.max.x + GAP, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(text_pos, galley, ink);
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
        self.app.logic(ctx, frame);
        if !self.applied {
            self.applied = true;
            self.switch_logged(ctx, self.modes.mode());
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.bar(ui);
        self.app.ui(ui, frame);
    }
}

#[cfg(test)]
mod tests;
