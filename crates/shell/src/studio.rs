//! The Procreate-style chrome around PhotoCraft's canvas.
//!
//! PhotoCraft's menu bar, toolbar, status bar and dock are hidden. The shell draws instead:
//! - a slim **top bar**: Gallery, Actions (every menu), Adjustments, Selection and Transform on
//!   the left, then the mode switcher; on the right, every tool of the mode, its three main tools
//!   (Brush, Smudge, Eraser in Pixel), Layers and the current colour;
//! - a floating **side bar** over the canvas: brush size, the modify button (eyedropper),
//!   opacity, undo and redo;
//! - one **popover** at a time under its button: Actions, Adjustments, Selection, all tools, the
//!   Brush Library (tap the active brush again) and the colour disc. A press outside closes it
//!   without reaching the canvas, as in Procreate.
//!
//! PhotoCraft's options bar comes back under the top bar for the tools whose settings the side
//! bar doesn't cover (selections, shapes, type, crop…) and while a transform or a text edit needs
//! its Commit / Cancel. Layers opens the right dock with just the Layers panel; a panel opened from
//! Window › … shows there too.
//!
//! Everything goes through PhotoCraft's public surface: commands, `tools.setBrush`,
//! `tools.setColors` and the panel flags.

use egui::{Align, Align2, Color32, Event, FontId, Id, Layout, Margin, Order, Pos2, Rect, Sense, Stroke, StrokeKind, TouchPhase, Vec2, pos2, vec2};
use photocraft_mobile_modes::{Mode, SELECTION_TOOLS};
use photocraft_ui_egui::menus::{self, MenuItem};
use photocraft_ui_egui::state::Panels;
use photocraft_ui_egui::theme::Tokens;
use photocraft_ui_egui::{PhotocraftApp, Tool, i18n, icons};
use serde_json::json;

use crate::ModeShell;
use crate::services::ColorSelector;

/// Height of the top bar, in points.
pub const BAR_HEIGHT: f32 = 46.0;
/// Narrower than this (points), the top bar shows icons only (Gallery, the mode names).
pub const COMPACT_BELOW: f32 = 760.0;
/// The side bar's largest brush size: the options bar's Size range for the Brush and Eraser.
pub const MAX_BRUSH_SIZE: f32 = 2500.0;
/// Colours kept in the colour popover's history.
pub const RECENT_COLORS: usize = 10;
/// The View › Screen Mode the studio keeps: no document tabs, no scroll bars.
const STUDIO_SCREEN_MODE: &str = "fullScreenWithMenuBar";

/// Tools whose settings the side bar covers (size, opacity), or that have none worth a bar:
/// PhotoCraft's options bar stays hidden for them.
const SIDE_BAR_TOOLS: &[Tool] = &[
    Tool::Brush,
    Tool::Pencil,
    Tool::MixerBrush,
    Tool::Eraser,
    Tool::Smudge,
    Tool::Blur,
    Tool::Sharpen,
    Tool::HistoryBrush,
    Tool::Move,
    Tool::Hand,
    Tool::Zoom,
    Tool::RotateView,
    Tool::Eyedropper,
];

/// A popover hanging from a top bar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popover {
    Actions,
    Adjustments,
    Selection,
    Tools,
    Brushes,
    Color,
}

/// What the right dock shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidePanel {
    /// Just the Layers panel (the top bar's Layers button).
    Layers,
    /// What PhotoCraft opened itself (Window › History, …).
    Window,
}

/// Something a control asked for, applied once the frame's controls are drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    Mode(Mode),
    Tool(Tool),
    Command(String),
    Popover(Popover),
    Layers,
    Gallery,
    Transform,
    Modify,
    Fit,
    Focus,
    FingerNavigates(bool),
    OptionsBar(bool),
    Preset(String),
    Color([f32; 3]),
    SwapColors,
    MoreColors,
    ColorSelector(ColorSelector),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slider {
    Size,
    Opacity,
}

/// `dock` and the dock groups' flags, to notice when PhotoCraft changes them.
type DockFlags = [bool; 7];

fn dock_flags(p: &Panels) -> DockFlags {
    [p.dock, p.layers, p.color, p.history, p.properties, p.navigator, p.character]
}

/// The chrome's state between frames.
#[derive(Debug)]
pub struct Studio {
    pub popover: Option<Popover>,
    pub side: Option<SidePanel>,
    /// Last frame's popover buttons on the top bar: each popover hangs from its own.
    buttons: Vec<(Popover, Rect)>,
    /// Last frame's rects: the popover (with its arrow), the top bar and the side bar.
    popover_rect: Option<Rect>,
    top_rect: Rect,
    side_rect: Option<Rect>,
    /// The popover a press outside just closed, and whether that press has lifted: its own button
    /// must not open it again on that click.
    closed_by_press: Option<(Popover, bool)>,
    /// A press outside the popover closed it: its events are dropped until it lifts.
    swallowing: bool,
    /// The dock flags as the chrome last left them.
    applied: Option<DockFlags>,
    /// Actions popover: the menu shown, and the search text.
    category: Option<String>,
    search: String,
    /// Brush Library: the group shown.
    brush_group: Option<String>,
    /// The colour disc's hue (degrees), saturation and value, and the colour they were made for.
    hsv: [f32; 3],
    hsv_rgb: Option<[f32; 3]>,
    /// The press on the colour picker has picked a colour (it goes into the history on release).
    picking: bool,
    /// Newest first.
    pub recent_colors: Vec<[f32; 3]>,
    /// The tool the modify button returns to after the eyedropper.
    modify_return: Option<Tool>,
    /// The side bar slider being dragged and its thumb (the preview bubble follows it).
    dragging: Option<(Slider, Pos2)>,
}

impl Default for Studio {
    fn default() -> Self {
        Self {
            popover: None,
            side: None,
            buttons: Vec::new(),
            popover_rect: None,
            top_rect: Rect::NOTHING,
            side_rect: None,
            closed_by_press: None,
            swallowing: false,
            applied: None,
            category: None,
            search: String::new(),
            brush_group: None,
            hsv: [0.0, 0.0, 0.0],
            hsv_rgb: None,
            picking: false,
            recent_colors: Vec::new(),
            modify_return: None,
            dragging: None,
        }
    }
}

impl Studio {
    /// A press outside the open popover closes it. Unless it is on the top bar (where it may open
    /// another popover), the press is dropped until it lifts, so closing a popover never paints.
    pub(crate) fn filter(&mut self, events: &mut Vec<Event>) {
        if matches!(self.closed_by_press, Some((_, true))) {
            self.closed_by_press = None;
        }
        let mut kept = Vec::with_capacity(events.len());
        for event in events.drain(..) {
            let press = match &event {
                Event::PointerButton { pos, pressed: true, .. } | Event::Touch { pos, phase: TouchPhase::Start, .. } => Some(*pos),
                _ => None,
            };
            if let (Some(pos), Some(open)) = (press, self.popover)
                && !self.popover_rect.is_some_and(|r| r.contains(pos))
            {
                self.popover = None;
                self.closed_by_press = Some((open, false));
                self.swallowing = !self.top_rect.contains(pos);
            }
            let lift = matches!(
                event,
                Event::PointerButton { pressed: false, .. } | Event::PointerGone | Event::Touch { phase: TouchPhase::End | TouchPhase::Cancel, .. }
            );
            if lift && let Some((p, false)) = self.closed_by_press {
                self.closed_by_press = Some((p, true));
            }
            if self.swallowing {
                if matches!(event, Event::PointerButton { pressed: false, .. } | Event::PointerGone) {
                    self.swallowing = false;
                }
                if matches!(event, Event::PointerButton { .. } | Event::PointerMoved(_) | Event::PointerGone | Event::Touch { .. }) {
                    continue;
                }
            }
            kept.push(event);
        }
        *events = kept;
    }

    /// The shell's own controls are under `p` (last frame): a hold there never scrubs a field.
    pub(crate) fn owns(&self, p: Pos2) -> bool {
        self.top_rect.contains(p) || self.side_rect.is_some_and(|r| r.contains(p)) || self.popover_rect.is_some_and(|r| r.contains(p))
    }
}

/// Whether PhotoCraft's options bar should show for the app's state: for a tool the side bar
/// doesn't cover, and while a transform, a text edit or a crop frame waits for Commit / Cancel.
pub fn needs_options_bar(app: &PhotocraftApp) -> bool {
    app.ui.transform.is_some() || app.ui.text_edit.is_some() || app.ui.crop_rect.is_some() || !SIDE_BAR_TOOLS.contains(&app.ui.tool)
}

fn pt() -> bool {
    i18n::current().code().to_ascii_lowercase().starts_with("pt")
}

/// `en`, or `pt_text` when the UI is in Portuguese.
fn tr(en: &'static str, pt_text: &'static str) -> &'static str {
    if pt() { pt_text } else { en }
}

fn color32(rgb: [f32; 3]) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(q(rgb[0]), q(rgb[1]), q(rgb[2]))
}

fn rgb_of(c: [f32; 4]) -> [f32; 3] {
    [c[0], c[1], c[2]]
}

/// Black or white, whichever reads better on `fill`.
pub(crate) fn ink_on(fill: Color32) -> Color32 {
    let l = 0.2126 * f32::from(fill.r()) + 0.7152 * f32::from(fill.g()) + 0.0722 * f32::from(fill.b());
    if l > 150.0 { Color32::from_gray(16) } else { Color32::WHITE }
}

/// The icon of a top-level menu in the Actions popover.
fn menu_icon(top: &str) -> &'static str {
    match top {
        "File" => "file",
        "Edit" => "copy",
        "Image" => "image",
        "Layer" => "layers",
        "Type" => "type",
        "Select" => "square-dashed",
        "Filter" => "sparkles",
        "3D" => "diamond",
        "View" => "eye",
        "Plugins" => "settings",
        "Window" => "app-window",
        "Help" => "info",
        _ => "ellipsis",
    }
}

/// A top bar icon: blue when `on` (Procreate's active tool), a soft fill under the pointer.
fn bar_icon(ui: &mut egui::Ui, t: &Tokens, icon: &str, on: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 36.0), Sense::click());
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 10.0, t.hover);
    }
    let ink = if on { t.accent } else { t.text };
    icons::paint(ui, Rect::from_center_size(rect.center(), vec2(21.0, 21.0)), icon, 21.0, ink);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, tip));
    resp.on_hover_text(tip)
}

/// "Gallery": a text button, like Procreate's.
fn text_button(ui: &mut egui::Ui, t: &Tokens, text: &str, on: bool) -> egui::Response {
    let font = FontId::proportional(16.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 20.0, 36.0), Sense::click());
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 10.0, t.hover);
    }
    let ink = if on { t.accent } else { t.text };
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    resp
}

/// The current colour as a disc (the top bar's last button).
fn color_button(ui: &mut egui::Ui, t: &Tokens, rgb: [f32; 3], on: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 36.0), Sense::click());
    let c = rect.center();
    if on {
        ui.painter().circle_stroke(c, 16.0, Stroke::new(2.0, t.accent));
    }
    ui.painter().circle(c, 12.5, color32(rgb), Stroke::new(1.5, t.text_dim));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    resp.on_hover_text(tip)
}

/// The Vector | Pixel | Photo pill (icons only when `compact`); returns the mode clicked.
fn segmented(ui: &mut egui::Ui, t: &Tokens, current: Mode, lang: &str, compact: bool) -> Option<Mode> {
    const H: f32 = 28.0;
    const ICON: f32 = 15.0;
    const PAD: f32 = 11.0;
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
        let rect = Rect::from_min_size(pos2(x, outer.min.y + 2.0), vec2(w, H));
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
        let icon = Rect::from_min_size(pos2(rect.min.x + PAD, rect.center().y - ICON / 2.0), vec2(ICON, ICON));
        icons::paint(ui, icon, mode.icon(), ICON, ink);
        if !compact {
            let text_pos = pos2(icon.max.x + GAP, rect.center().y - galley.size().y / 2.0);
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

/// A vertical slider of the side bar: `value` 0..1 from the bottom. Returns its response and the
/// thumb's centre.
fn v_slider(ui: &mut egui::Ui, t: &Tokens, id: &str, value: &mut f32, height: f32, enabled: bool, tip: &str) -> (egui::Response, Pos2) {
    const W: f32 = 30.0;
    const THUMB: f32 = 18.0;
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, _) = ui.allocate_exact_size(vec2(W, height), Sense::hover());
    let mut resp = ui.interact(rect, ui.id().with(("photocraft-mobile-slider", id)), sense);
    let travel = (height - THUMB).max(1.0);
    if enabled && let Some(p) = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()) {
        let v = ((rect.bottom() - THUMB / 2.0 - p.y) / travel).clamp(0.0, 1.0);
        if (v - *value).abs() > f32::EPSILON {
            *value = v;
            resp.mark_changed();
        }
    }
    let v = value.clamp(0.0, 1.0);
    let painter = ui.painter();
    painter.rect_filled(rect, W / 2.0, t.field);
    let thumb_y = rect.bottom() - THUMB / 2.0 - v * travel;
    let thumb = Rect::from_center_size(pos2(rect.center().x, thumb_y), vec2(W - 4.0, THUMB));
    let ink = if !enabled {
        t.text_faint
    } else if resp.is_pointer_button_down_on() {
        t.accent
    } else {
        t.text
    };
    painter.rect_filled(thumb, THUMB / 2.0, ink);
    resp.widget_info(|| egui::WidgetInfo::slider(enabled, f64::from(v), tip));
    let resp = resp.on_hover_text(tip);
    (resp, thumb.center())
}

/// A row of a popover list (a menu item, a quick action).
fn list_row(ui: &mut egui::Ui, t: &Tokens, label: &str, detail: Option<&str>, enabled: bool, checked: Option<bool>) -> egui::Response {
    let h = if detail.is_some() { 48.0 } else { 40.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && (resp.hovered() || resp.is_pointer_button_down_on()) {
        ui.painter().rect_filled(rect, 10.0, t.hover);
    }
    let ink = if enabled { t.text } else { t.text_faint };
    let font = FontId::proportional(15.0);
    let right = if checked.is_some() { rect.right() - 36.0 } else { rect.right() - 8.0 };
    let label_y = if detail.is_some() { rect.top() + 15.0 } else { rect.center().y };
    let galley = ui.painter().layout(label.to_string(), font, ink, (right - rect.left() - 12.0).max(10.0));
    ui.painter().galley(pos2(rect.left() + 12.0, label_y - galley.size().y / 2.0), galley, ink);
    if let Some(d) = detail {
        ui.painter().text(pos2(rect.left() + 12.0, rect.bottom() - 13.0), Align2::LEFT_CENTER, d, FontId::proportional(11.5), t.text_faint);
    }
    if let Some(on) = checked {
        let r = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(18.0, 18.0));
        if on {
            icons::paint(ui, r, "check", 18.0, t.accent);
        } else {
            ui.painter().circle_stroke(r.center(), 7.0, Stroke::new(1.5, t.text_faint));
        }
    }
    resp.widget_info(|| match checked {
        Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, label),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label),
    });
    resp
}

/// A category of a popover's left column (Procreate's Brush Library groups).
fn category_row(ui: &mut egui::Ui, t: &Tokens, icon: Option<&str>, label: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
    let (fill, ink) = if selected {
        (t.accent, ink_on(t.accent))
    } else if resp.hovered() || resp.is_pointer_button_down_on() {
        (t.hover, t.text)
    } else {
        (t.field, t.text)
    };
    ui.painter().rect_filled(rect, 10.0, fill);
    let mut x = rect.left() + 12.0;
    if let Some(icon) = icon {
        icons::paint(ui, Rect::from_center_size(pos2(x + 9.0, rect.center().y), vec2(17.0, 17.0)), icon, 17.0, ink);
        x += 28.0;
    }
    let galley = ui.painter().layout(label.to_string(), FontId::proportional(14.5), ink, (rect.right() - x - 6.0).max(10.0));
    ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, ink);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label));
    resp
}

/// A small heading inside a popover list.
fn section(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, text.to_uppercase(), FontId::proportional(11.5), t.text_faint);
}

/// A popover's title row.
fn title(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.label(egui::RichText::new(text).font(FontId::proportional(22.0)).color(t.text));
    ui.add_space(8.0);
}

/// A tool's name without the "Tool" every name carries ("Move Tool" → "Move", "Ferramenta
/// Laço" → "Laço"), for the tool grids.
pub fn short_tool_name(full: &str) -> String {
    let short = ["Ferramenta ", "Herramienta ", "Outil ", "Werkzeug "]
        .iter()
        .find_map(|p| full.strip_prefix(p))
        .or_else(|| [" Tool", " tool", "-Werkzeug", "werkzeug"].iter().find_map(|s| full.strip_suffix(s)))
        .unwrap_or(full)
        .trim();
    let mut chars = short.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => full.to_string(),
    }
}

/// A tool tile of a popover grid: the icon over its name.
fn tool_tile(ui: &mut egui::Ui, t: &Tokens, tool: Tool, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(TILE.x, TILE.y), Sense::click());
    let (fill, ink) = if selected {
        (t.accent, ink_on(t.accent))
    } else if resp.hovered() || resp.is_pointer_button_down_on() {
        (t.hover, t.text)
    } else {
        (t.field, t.text)
    };
    ui.painter().rect_filled(rect, 12.0, fill);
    icons::paint(ui, Rect::from_center_size(pos2(rect.center().x, rect.top() + 24.0), vec2(22.0, 22.0)), icons::tool_icon(tool), 22.0, ink);
    let full = i18n::t(tool.label());
    let label = short_tool_name(full);
    let galley = ui.painter().layout(label.clone(), FontId::proportional(11.5), ink, rect.width() - 10.0);
    let galley_pos = pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 42.0);
    ui.painter().with_clip_rect(rect.shrink(2.0)).galley(galley_pos, galley, ink);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, full));
    resp.on_hover_text(full)
}

/// A tool tile's size; four fit a row of the Tools popover.
const TILE: Vec2 = vec2(96.0, 76.0);

/// The live menu items (what PhotoCraft implements), without separators.
fn live_items(app: &PhotocraftApp) -> Vec<MenuItem> {
    menus::menu_items(app).into_iter().filter(|i| !i.path.is_empty() && !i.label.is_empty() && i.label != "-" && menus::is_live(&i.id)).collect()
}

fn item_label(item: &MenuItem) -> String {
    i18n::tr_id(i18n::current(), &item.id, &item.label).to_string()
}

fn path_label(path: &[String]) -> String {
    path.iter().map(|s| i18n::t(s)).collect::<Vec<_>>().join(" › ")
}

/// `items` as rows, with a heading where the submenu changes below `depth` path segments.
fn menu_rows<'a>(ui: &mut egui::Ui, t: &Tokens, items: impl Iterator<Item = &'a MenuItem>, depth: usize, acts: &mut Vec<Act>) {
    let mut group: Option<&[String]> = None;
    for item in items {
        let sub = item.path.get(depth..).unwrap_or_default();
        if group != Some(sub) {
            if !sub.is_empty() {
                section(ui, t, &path_label(sub));
            }
            group = Some(sub);
        }
        if list_row(ui, t, &item_label(item), None, item.enabled, item.checked).clicked() {
            acts.push(Act::Command(item.id.clone()));
        }
    }
}

impl ModeShell {
    /// Hide PhotoCraft's chrome and show what the studio wants of it. Runs every frame before
    /// PhotoCraft draws, so a workspace, Tab or a saved layout can't bring the old chrome back.
    pub(crate) fn apply_chrome(&mut self) {
        let options = !self.focus && (self.settings.options_bar || needs_options_bar(&self.app));
        // PhotoCraft's "full screen with menu bar" keeps its chrome but drops the document tabs
        // and the scroll bars: one image at a time, panned with the fingers, as in Procreate (the
        // Gallery and Window › … switch documents). Set as state, not with the command, which
        // would also make the window full screen.
        if self.app.ui.view.screen_mode != STUDIO_SCREEN_MODE {
            self.app.ui.view.screen_mode = STUDIO_SCREEN_MODE.into();
        }
        let studio = &mut self.studio;
        let p = &mut self.app.ui.panels;
        p.menu_bar = false;
        p.toolbar = false;
        p.status_bar = false;
        p.rail = false;
        p.hidden_by_tab = None;
        p.options_bar = options;
        let now = dock_flags(p);
        if let Some(before) = studio.applied
            && before != now
        {
            // PhotoCraft changed the dock itself (Window › History, a shortcut): follow it.
            let opened = (1..now.len()).any(|i| now[i] && !before[i]);
            if opened {
                studio.side = Some(SidePanel::Window);
            } else if !now[1..].iter().any(|&on| on) || (studio.side == Some(SidePanel::Layers) && !now[1]) {
                studio.side = None;
            }
        }
        match studio.side {
            None => p.dock = false,
            Some(SidePanel::Layers) => {
                p.dock = true;
                p.layers = true;
                p.color = false;
                p.history = false;
                p.properties = false;
                p.navigator = false;
                p.character = false;
            }
            Some(SidePanel::Window) => p.dock = true,
        }
        if self.focus {
            p.dock = false;
        }
        studio.applied = Some(dock_flags(p));
    }

    /// Forget the dock flags the chrome set: a mode switch lays out its workspace, which must not
    /// read as the user opening panels.
    pub(crate) fn chrome_relayout(&mut self) {
        self.studio.applied = None;
    }

    /// Open popover `p`, or close it when it is open (its button's tap).
    pub fn toggle_popover(&mut self, p: Popover) {
        if self.studio.closed_by_press.is_some_and(|(closed, _)| closed == p) {
            // The press on this very button closed it: the click must not reopen it.
            return;
        }
        self.studio.popover = if self.studio.popover == Some(p) { None } else { Some(p) };
        if self.studio.popover == Some(Popover::Color) {
            self.sync_hsv();
        }
    }

    /// The colour disc starts from the foreground colour (keeping its hue for a grey).
    fn sync_hsv(&mut self) {
        let rgb = rgb_of(self.app.session.tools.foreground);
        if self.studio.hsv_rgb == Some(rgb) {
            return;
        }
        let hsv = photocraft_ui_egui::color_picker_ui::rgb_to_hsv(rgb);
        let hue = if hsv[1] > 0.0 && hsv[2] > 0.0 { hsv[0] } else { self.studio.hsv[0] };
        self.studio.hsv = [hue, hsv[1], hsv[2]];
        self.studio.hsv_rgb = Some(rgb);
    }

    fn remember_color(&mut self, rgb: [f32; 3]) {
        let recent = &mut self.studio.recent_colors;
        recent.retain(|c| color32(*c) != color32(rgb));
        recent.insert(0, rgb);
        recent.truncate(RECENT_COLORS);
    }

    pub(crate) fn apply_act(&mut self, ctx: &egui::Context, act: Act) {
        match act {
            Act::Mode(mode) => {
                if mode != self.modes.mode() {
                    self.switch_logged(ctx, mode);
                }
            }
            Act::Tool(tool) => {
                self.app.ui.tool = tool;
                self.studio.popover = None;
                self.studio.modify_return = None;
            }
            Act::Command(id) => {
                self.studio.popover = None;
                self.command(ctx, &id);
            }
            Act::Popover(p) => self.toggle_popover(p),
            Act::Layers => {
                self.studio.popover = None;
                if self.studio.side.is_some() {
                    self.studio.side = None;
                } else {
                    self.studio.side = Some(SidePanel::Layers);
                    photocraft_ui_egui::dock::reveal(&mut self.app, photocraft_ui_egui::dock::Group::Layers);
                }
            }
            Act::Gallery => {
                self.studio.popover = None;
                let n = self.app.session.documents().len();
                let auto = self.app.session.prefs().general.auto_show_home_screen;
                let on = self.app.ui.chrome.shows_home(n, auto);
                // With nothing open and Home shown automatically, there is nothing behind it.
                if n > 0 || !auto {
                    self.app.ui.chrome.home = if on { None } else { Some(n) };
                }
            }
            Act::Transform => {
                self.studio.popover = None;
                if self.app.ui.transform.is_some() {
                    photocraft_ui_egui::transform_tool::commit(&mut self.app);
                } else if menus::is_enabled(&self.app, "edit.freeTransform") {
                    self.command(ctx, "edit.freeTransform");
                } else {
                    self.app.ui.tool = Tool::Move;
                }
            }
            Act::Modify => {
                if self.app.ui.tool == Tool::Eyedropper {
                    if let Some(back) = self.studio.modify_return.take() {
                        self.app.ui.tool = back;
                    }
                } else {
                    self.studio.modify_return = Some(self.app.ui.tool);
                    self.app.ui.tool = Tool::Eyedropper;
                }
            }
            Act::Fit => {
                self.studio.popover = None;
                self.command(ctx, "view.fitOnScreen");
            }
            Act::Focus => {
                self.studio.popover = None;
                self.toggle_focus();
            }
            Act::FingerNavigates(on) => self.set_finger_navigates(on),
            Act::OptionsBar(on) => {
                self.settings.options_bar = on;
                self.save_settings();
            }
            Act::Preset(name) => {
                if let Err(e) = self.app.run("tools.setBrush", json!({ "preset": name })) {
                    self.app.ui.status = e;
                }
            }
            Act::Color(rgb) => {
                if let Err(e) = self.app.run("tools.setColors", json!({ "foreground": [rgb[0], rgb[1], rgb[2], 1.0] })) {
                    log::info!("tools.setColors: {e}");
                }
                let [h, s, v] = self.studio.hsv;
                if color32(photocraft_ui_egui::color_picker_ui::hsv_to_rgb(h, s, v)) == color32(rgb) {
                    // Picked on the disc: keep its exact hue and position.
                    self.studio.hsv_rgb = Some(rgb_of(self.app.session.tools.foreground));
                } else {
                    self.studio.hsv_rgb = None;
                    self.sync_hsv();
                }
            }
            Act::SwapColors => {
                if let Err(e) = self.app.run("tools.swapColors", json!({})) {
                    log::info!("tools.swapColors: {e}");
                }
                self.sync_hsv();
            }
            Act::MoreColors => {
                self.studio.popover = None;
                photocraft_ui_egui::color_picker_ui::open(&mut self.app, "foreground");
            }
            Act::ColorSelector(kind) => {
                self.settings.color_selector = kind;
                self.save_settings();
            }
        }
    }

    /// The top bar.
    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let lang = i18n::current().code();
        let mode = self.modes.mode();
        let tool = self.app.ui.tool;
        let open = self.studio.popover;
        let fg = rgb_of(self.app.session.tools.foreground);
        let transform_on = self.app.ui.transform.is_some() || tool == Tool::Move;
        let selecting = SELECTION_TOOLS.contains(&format!("{tool:?}").as_str());
        let home_on = {
            let n = self.app.session.documents().len();
            let auto = self.app.session.prefs().general.auto_show_home_screen;
            self.app.ui.chrome.shows_home(n, auto)
        };
        let layers_on = self.studio.side.is_some();
        let quick: Vec<Tool> = mode.quick_tools().iter().filter_map(|n| Tool::from_name(n)).collect();
        let mut acts = Vec::new();
        let mut buttons: Vec<(Popover, Rect)> = Vec::new();
        let resp = egui::Panel::top("photocraft-mobile-top")
            .resizable(false)
            .exact_size(BAR_HEIGHT)
            .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(Margin::symmetric(10, 5)))
            .show(ui, |ui| {
                let compact = ui.available_width() < COMPACT_BELOW;
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let gallery = tr("Gallery", "Galeria");
                    let r = if compact { bar_icon(ui, &t, "house", home_on, gallery) } else { text_button(ui, &t, gallery, home_on) };
                    if r.clicked() {
                        acts.push(Act::Gallery);
                    }
                    ui.add_space(6.0);
                    let popovers = [
                        ("settings", Popover::Actions, false, tr("Actions: every menu", "Ações: todos os menus")),
                        ("wand-sparkles", Popover::Adjustments, false, tr("Adjustments and filters", "Ajustes e filtros")),
                        ("lasso", Popover::Selection, selecting, tr("Selection", "Seleção")),
                    ];
                    for (icon, p, on, tip) in popovers {
                        let r = bar_icon(ui, &t, icon, on || open == Some(p), tip);
                        if r.clicked() {
                            acts.push(Act::Popover(p));
                        }
                        buttons.push((p, r.rect));
                    }
                    let tip = tr("Transform (tap again to apply)", "Transformar (toque de novo para aplicar)");
                    if bar_icon(ui, &t, "mouse-pointer-2", transform_on, tip).clicked() {
                        acts.push(Act::Transform);
                    }
                    ui.add_space(10.0);
                    if let Some(m) = segmented(ui, &t, mode, lang, compact) {
                        acts.push(Act::Mode(m));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let r = color_button(ui, &t, fg, open == Some(Popover::Color), tr("Color", "Cor"));
                        if r.clicked() {
                            acts.push(Act::Popover(Popover::Color));
                        }
                        buttons.push((Popover::Color, r.rect));
                        if bar_icon(ui, &t, "layers", layers_on, tr("Layers", "Camadas")).clicked() {
                            acts.push(Act::Layers);
                        }
                        ui.add_space(8.0);
                        for &qt in quick.iter().rev() {
                            let on = tool == qt;
                            let tip = if on && qt.is_brushlike() {
                                tr("Tap again for the Brush Library", "Toque de novo para a Biblioteca de pincéis")
                            } else {
                                i18n::t(qt.label())
                            };
                            let r = bar_icon(ui, &t, icons::tool_icon(qt), on, tip);
                            if on && qt.is_brushlike() {
                                buttons.push((Popover::Brushes, r.rect));
                            }
                            if r.clicked() {
                                acts.push(if on && qt.is_brushlike() { Act::Popover(Popover::Brushes) } else { Act::Tool(qt) });
                            }
                        }
                        // Every tool of the mode. It shows the active tool when that isn't one of the three.
                        let other = !quick.contains(&tool);
                        let icon = if other { icons::tool_icon(tool) } else { "grid-2x2" };
                        let r = bar_icon(ui, &t, icon, other || open == Some(Popover::Tools), tr("All tools", "Todas as ferramentas"));
                        if r.clicked() {
                            acts.push(Act::Popover(Popover::Tools));
                        }
                        buttons.push((Popover::Tools, r.rect));
                    });
                });
            });
        self.studio.top_rect = resp.response.rect;
        self.studio.buttons = buttons;
        let ctx = ui.ctx().clone();
        for act in acts {
            self.apply_act(&ctx, act);
        }
    }

    /// The floating side bar: size, modify (eyedropper), opacity, undo, redo.
    pub(crate) fn side_bar(&mut self, ctx: &egui::Context) {
        self.studio.side_rect = None;
        let canvas = self.app.last_canvas_rect;
        if self.focus || self.app.session.active_index().is_none() || !(canvas.width() > 120.0 && canvas.height() > 240.0) {
            self.studio.dragging = None;
            return;
        }
        let t = Tokens::get(ctx);
        // The brush of the tool now active, before reading its size and opacity.
        photocraft_ui_egui::paint_mouse::sync_tool_brush(&mut self.app);
        let tool = self.app.ui.tool;
        let brushlike = tool.is_brushlike();
        let fill = matches!(tool, Tool::Gradient | Tool::PaintBucket);
        let before = self.app.session.tools.brush.clone();
        let mut brush = before.clone();
        let mut size = (brush.size.max(1.0).ln() / MAX_BRUSH_SIZE.ln()).clamp(0.0, 1.0);
        let mut opacity = if fill { self.app.ui.tool_options.fill_opacity / 100.0 } else { brush.opacity };
        const BUTTON: f32 = 36.0;
        const GAP: f32 = 10.0;
        let fixed = 3.0 * BUTTON + 5.0 * GAP + 18.0;
        let slider_h = ((canvas.height() - 48.0).min(560.0) - fixed).max(140.0) / 2.0;
        let slider_h = slider_h.min(200.0);
        let total = 2.0 * slider_h + fixed;
        let pos = pos2(canvas.left() + 10.0, canvas.center().y - total / 2.0);
        let mut acts = Vec::new();
        let (mut size_changed, mut opacity_changed) = (false, false);
        let mut dragging = None;
        let resp = egui::Area::new(Id::new("photocraft-mobile-side-bar")).order(Order::Middle).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::NONE
                .fill(t.chrome)
                .corner_radius(16.0)
                .stroke(Stroke::new(1.0, t.card_border))
                .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow })
                .inner_margin(Margin::symmetric(6, 9))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = GAP;
                    ui.vertical_centered(|ui| {
                        let (r, thumb) = v_slider(ui, &t, "size", &mut size, slider_h, brushlike, tr("Brush size", "Tamanho do pincel"));
                        size_changed = r.changed();
                        if r.is_pointer_button_down_on() && brushlike {
                            dragging = Some((Slider::Size, thumb));
                        }
                        let on = tool == Tool::Eyedropper;
                        let (rect, mr) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click());
                        let square = Rect::from_center_size(rect.center(), vec2(24.0, 24.0));
                        let ink = if on {
                            t.accent
                        } else if mr.hovered() {
                            t.text
                        } else {
                            t.text_dim
                        };
                        ui.painter().rect_stroke(square, 6.0, Stroke::new(2.0, ink), StrokeKind::Inside);
                        if on {
                            ui.painter().rect_filled(square.shrink(5.0), 3.0, ink);
                        }
                        let tip = tr("Modify: eyedropper (tap again to go back)", "Modificar: conta-gotas (toque de novo para voltar)");
                        mr.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, tip));
                        if mr.on_hover_text(tip).clicked() {
                            acts.push(Act::Modify);
                        }
                        let (r, thumb) = v_slider(ui, &t, "opacity", &mut opacity, slider_h, brushlike || fill, tr("Opacity", "Opacidade"));
                        opacity_changed = r.changed();
                        if r.is_pointer_button_down_on() && (brushlike || fill) {
                            dragging = Some((Slider::Opacity, thumb));
                        }
                        if bar_icon(ui, &t, "undo-2", false, tr("Undo (two-finger tap)", "Desfazer (toque com dois dedos)")).clicked() {
                            acts.push(Act::Command("edit.undo".into()));
                        }
                        if bar_icon(ui, &t, "redo-2", false, tr("Redo (three-finger tap)", "Refazer (toque com três dedos)")).clicked() {
                            acts.push(Act::Command("edit.redo".into()));
                        }
                    });
                });
        });
        let bar = resp.response.rect;
        self.studio.side_rect = Some(bar);
        if brushlike {
            if size_changed {
                brush.size = (size * MAX_BRUSH_SIZE.ln()).exp().round().clamp(1.0, MAX_BRUSH_SIZE);
            }
            if opacity_changed {
                brush.opacity = (opacity * 100.0).round().clamp(1.0, 100.0) / 100.0;
            }
            photocraft_ui_egui::brush_panel::commit_gesture(&mut self.app, ctx, &before, &brush);
        } else if fill && opacity_changed {
            self.app.ui.tool_options.fill_opacity = (opacity * 100.0).round().clamp(1.0, 100.0);
        }
        self.studio.dragging = dragging;
        if let Some((which, thumb)) = dragging {
            self.slider_bubble(ctx, &t, which, bar, thumb);
        }
        for act in acts {
            self.apply_act(ctx, act);
        }
    }

    /// While a side bar slider moves: the brush at its size on screen, or the opacity.
    fn slider_bubble(&self, ctx: &egui::Context, t: &Tokens, which: Slider, bar: Rect, thumb: Pos2) {
        let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new("photocraft-mobile-bubble")));
        let fill = self.app.ui.tool_options.fill_opacity;
        let brush = &self.app.session.tools.brush;
        let fg = color32(rgb_of(self.app.session.tools.foreground));
        let (side, text) = match which {
            Slider::Size => (132.0, format!("{} px", brush.size.round())),
            Slider::Opacity => {
                let pct = if self.app.ui.tool.is_brushlike() { brush.opacity * 100.0 } else { fill };
                (92.0, format!("{}%", pct.round()))
            }
        };
        let center = pos2(bar.right() + 16.0 + side / 2.0, thumb.y);
        let card = Rect::from_center_size(center, vec2(side, side));
        painter.rect(card, 18.0, t.chrome, Stroke::new(1.0, t.card_border), StrokeKind::Inside);
        match which {
            Slider::Size => {
                let zoom = self.app.session.active_index().and_then(|i| self.app.ui.views.get(i)).map_or(1.0, |v| v.zoom);
                let d = (brush.size * zoom / ctx.pixels_per_point()).clamp(3.0, side - 40.0);
                painter.circle_filled(pos2(center.x, card.top() + (side - 24.0) / 2.0 + 2.0), d / 2.0, fg);
                painter.text(pos2(center.x, card.bottom() - 14.0), Align2::CENTER_CENTER, text, FontId::proportional(13.0), t.text);
            }
            Slider::Opacity => {
                painter.text(center, Align2::CENTER_CENTER, text, FontId::proportional(22.0), t.text);
            }
        }
    }

    /// Focus mode: just the image, and a button to bring the chrome back.
    pub(crate) fn focus_exit(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let mut exit = false;
        egui::Area::new(Id::new("photocraft-mobile-focus-exit")).order(Order::Foreground).fixed_pos(ctx.content_rect().min + vec2(10.0, 10.0)).show(
            ctx,
            |ui| {
                egui::Frame::NONE.fill(t.chrome.gamma_multiply(0.85)).corner_radius(12.0).inner_margin(Margin::same(2)).show(ui, |ui| {
                    exit = bar_icon(ui, &t, "maximize-2", true, tr("Show the tools again", "Mostrar as ferramentas de novo")).clicked();
                });
            },
        );
        if exit {
            self.toggle_focus();
        }
    }

    /// The open popover, hanging from its button.
    pub(crate) fn popover(&mut self, ctx: &egui::Context) {
        let Some(which) = self.studio.popover.filter(|_| !self.focus) else {
            self.studio.popover_rect = None;
            return;
        };
        if which == Popover::Brushes && !self.app.ui.tool.is_brushlike() {
            self.studio.popover = None;
            self.studio.popover_rect = None;
            return;
        }
        let t = Tokens::get(ctx);
        let screen = ctx.content_rect();
        // Hung from its button; the Brush Library of a tool the bar doesn't show, from All tools.
        let button = |p: Popover| self.studio.buttons.iter().find(|(b, _)| *b == p).map(|(_, r)| *r);
        let anchor =
            button(which).or_else(|| button(Popover::Tools)).unwrap_or(Rect::from_min_size(screen.right_top() - vec2(60.0, 0.0), vec2(40.0, BAR_HEIGHT)));
        let want = match which {
            Popover::Actions => vec2(640.0, 640.0),
            Popover::Adjustments => vec2(380.0, 600.0),
            Popover::Selection => vec2(452.0, 580.0),
            Popover::Tools => vec2(452.0, 600.0),
            Popover::Brushes => vec2(600.0, 660.0),
            Popover::Color => vec2(340.0, 660.0),
        };
        let top = anchor.bottom() + 12.0;
        let size = vec2(want.x.min(screen.width() - 16.0), want.y.min(screen.bottom() - top - 10.0)).max(vec2(220.0, 220.0));
        let x = (anchor.center().x - size.x / 2.0).clamp(screen.left() + 8.0, (screen.right() - 8.0 - size.x).max(screen.left() + 8.0));
        const PAD: i8 = 14;
        let inner = size - Vec2::splat(2.0 * f32::from(PAD));
        let mut acts = Vec::new();
        let resp = egui::Area::new(Id::new("photocraft-mobile-popover")).order(Order::Foreground).fixed_pos(pos2(x, top)).show(ctx, |ui| {
            egui::Frame::NONE
                .fill(t.chrome)
                .corner_radius(18.0)
                .stroke(Stroke::new(1.0, t.card_border))
                .shadow(egui::Shadow { offset: [0, 10], blur: 32, spread: 0, color: t.shadow })
                .inner_margin(Margin::same(PAD))
                .show(ui, |ui| {
                    ui.set_min_size(inner);
                    ui.set_max_size(inner);
                    match which {
                        Popover::Actions => self.actions_body(ui, &t, &mut acts),
                        Popover::Adjustments => self.adjustments_body(ui, &t, &mut acts),
                        Popover::Selection => self.selection_body(ui, &t, &mut acts),
                        Popover::Tools => self.tools_body(ui, &t, &mut acts),
                        Popover::Brushes => self.brushes_body(ui, &t, &mut acts),
                        Popover::Color => self.color_body(ui, &t, &mut acts),
                    }
                });
        });
        let rect = resp.response.rect;
        // The little arrow pointing at the button.
        let tip = anchor.center().x.clamp(rect.left() + 26.0, rect.right() - 26.0);
        let painter = ctx.layer_painter(resp.response.layer_id);
        let arrow = vec![pos2(tip - 10.0, rect.top() + 1.0), pos2(tip, rect.top() - 9.0), pos2(tip + 10.0, rect.top() + 1.0)];
        painter.add(egui::Shape::convex_polygon(arrow, t.chrome, Stroke::NONE));
        self.studio.popover_rect = Some(Rect::from_min_max(pos2(rect.left(), rect.top() - 10.0), rect.max));
        for act in acts {
            self.apply_act(ctx, act);
        }
    }

    /// Actions: the shell's quick actions and every PhotoCraft menu, a column of menus beside
    /// the items of the one picked; a search over all of them.
    fn actions_body(&mut self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>) {
        let items = live_items(&self.app);
        let mut tops: Vec<String> = Vec::new();
        for item in &items {
            if let Some(top) = item.path.first()
                && !tops.contains(top)
            {
                tops.push(top.clone());
            }
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("Actions", "Ações")).font(FontId::proportional(22.0)).color(t.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let w = (ui.available_width() - 8.0).clamp(120.0, 260.0);
                ui.add(egui::TextEdit::singleline(&mut self.studio.search).hint_text(tr("Search commands", "Buscar comandos")).desired_width(w));
            });
        });
        ui.add_space(10.0);
        let query = self.studio.search.trim().to_string();
        let lang = i18n::current();
        if !query.is_empty() {
            egui::ScrollArea::vertical().id_salt("photocraft-mobile-actions-search").auto_shrink([false, false]).show(ui, |ui| {
                let found = menus::search_items(&items, &query, lang);
                if found.is_empty() {
                    ui.label(egui::RichText::new(tr("Nothing found", "Nada encontrado")).color(t.text_faint));
                }
                for item in found.into_iter().take(80) {
                    let path = path_label(&item.path);
                    if list_row(ui, t, &item_label(item), Some(&path), item.enabled, item.checked).clicked() {
                        acts.push(Act::Command(item.id.clone()));
                    }
                }
            });
            return;
        }
        let quick = "";
        let current = self.studio.category.clone().filter(|c| c.is_empty() || tops.contains(c)).unwrap_or_default();
        let left_w = (ui.available_width() * 0.34).clamp(120.0, 190.0);
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(left_w, height), Layout::top_down(Align::Min), |ui| {
                egui::ScrollArea::vertical().id_salt("photocraft-mobile-actions-menus").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    if category_row(ui, t, Some("star"), tr("Quick", "Rápido"), current == quick).clicked() {
                        self.studio.category = Some(String::new());
                    }
                    for top in &tops {
                        if category_row(ui, t, Some(menu_icon(top)), i18n::t(top), &current == top).clicked() {
                            self.studio.category = Some(top.clone());
                        }
                    }
                });
            });
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), height), Layout::top_down(Align::Min), |ui| {
                egui::ScrollArea::vertical().id_salt(("photocraft-mobile-actions-items", current.clone())).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if current.is_empty() {
                        self.quick_rows(ui, t, &items, acts);
                    } else {
                        menu_rows(ui, t, items.iter().filter(|i| i.path.first() == Some(&current)), 1, acts);
                    }
                });
            });
        });
    }

    /// Actions › Quick: documents, view and the shell's own settings.
    fn quick_rows(&mut self, ui: &mut egui::Ui, t: &Tokens, items: &[MenuItem], acts: &mut Vec<Act>) {
        section(ui, t, tr("Document", "Documento"));
        for id in ["file.new", "file.open", "file.save", "file.saveAs", "file.export.exportAs"] {
            if let Some(item) = items.iter().find(|i| i.id == id)
                && list_row(ui, t, &item_label(item), None, item.enabled, None).clicked()
            {
                acts.push(Act::Command(id.into()));
            }
        }
        section(ui, t, tr("View", "Exibir"));
        if list_row(ui, t, tr("Fit on Screen", "Encaixar na tela"), None, true, None).clicked() {
            acts.push(Act::Fit);
        }
        if list_row(ui, t, tr("Full screen (just the image)", "Tela cheia (só a imagem)"), None, true, None).clicked() {
            acts.push(Act::Focus);
        }
        section(ui, t, tr("Interface", "Interface"));
        let always = self.settings.options_bar;
        let label = tr("Always show the tool options bar", "Sempre mostrar a barra de opções");
        if list_row(ui, t, label, None, true, Some(always)).clicked() {
            acts.push(Act::OptionsBar(!always));
        }
        if self.touch_kind.is_some() {
            let on = self.settings.finger_navigates;
            let label = tr("Fingers navigate (the pen paints)", "Dedos navegam (a caneta desenha)");
            if list_row(ui, t, label, None, true, Some(on)).clicked() {
                acts.push(Act::FingerNavigates(!on));
            }
        }
        if let Some(item) = items.iter().find(|i| i.id.starts_with("edit.preferences")) {
            let label = item_label(item);
            if list_row(ui, t, &label, None, item.enabled, None).clicked() {
                acts.push(Act::Command(item.id.clone()));
            }
        }
    }

    /// Adjustments: Image › Adjustments and the Filter menu.
    fn adjustments_body(&mut self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>) {
        title(ui, t, tr("Adjustments", "Ajustes"));
        let items = live_items(&self.app);
        egui::ScrollArea::vertical().id_salt("photocraft-mobile-adjustments").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let adjust = items.iter().filter(|i| i.path.len() >= 2 && i.path[0] == "Image" && i.path[1] == "Adjustments");
            menu_rows(ui, t, adjust, 2, acts);
            section(ui, t, i18n::t("Filter"));
            menu_rows(ui, t, items.iter().filter(|i| i.path.first().is_some_and(|p| p == "Filter")), 1, acts);
        });
    }

    /// Selection: the mode's selection tools and the Select menu.
    fn selection_body(&mut self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>) {
        title(ui, t, tr("Selection", "Seleção"));
        let tool = self.app.ui.tool;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for name in self.modes.mode().selection_tools() {
                if let Some(st) = Tool::from_name(name)
                    && tool_tile(ui, t, st, st == tool).clicked()
                {
                    acts.push(Act::Tool(st));
                }
            }
        });
        ui.add_space(8.0);
        let items = live_items(&self.app);
        egui::ScrollArea::vertical().id_salt("photocraft-mobile-selection").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            menu_rows(ui, t, items.iter().filter(|i| i.path.first().is_some_and(|p| p == "Select")), 1, acts);
        });
    }

    /// Every tool of the mode.
    fn tools_body(&mut self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>) {
        let mode = self.modes.mode();
        title(ui, t, &format!("{} · {}", tr("Tools", "Ferramentas"), mode.label(i18n::current().code())));
        let tool = self.app.ui.tool;
        egui::ScrollArea::vertical().id_salt("photocraft-mobile-tools").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for name in mode.tools() {
                    if let Some(tt) = Tool::from_name(name)
                        && tool_tile(ui, t, tt, tt == tool).clicked()
                    {
                        acts.push(Act::Tool(tt));
                    }
                }
            });
        });
    }

    /// The Brush Library: preset groups on the left, the group's brushes with their stroke on the right.
    fn brushes_body(&mut self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>) {
        let presets = &self.app.session.tools.presets;
        let current = self.app.session.tools.current_preset.clone();
        let groups = photocraft_ui_egui::brush_panel::grouped_presets(presets);
        let current_group =
            current.as_ref().and_then(|name| groups.iter().find(|(_, idx)| idx.iter().any(|&i| presets.get(i).is_some_and(|p| &p.name == name))));
        let shown = self
            .studio
            .brush_group
            .clone()
            .filter(|g| groups.iter().any(|(name, _)| name == g))
            .or_else(|| current_group.map(|(g, _)| g.clone()))
            .or_else(|| groups.first().map(|(g, _)| g.clone()))
            .unwrap_or_default();
        title(ui, t, tr("Brush Library", "Biblioteca de pincéis"));
        if groups.is_empty() {
            ui.label(egui::RichText::new(tr("No brushes", "Nenhum pincel")).color(t.text_faint));
            return;
        }
        let left_w = (ui.available_width() * 0.36).clamp(130.0, 200.0);
        let height = ui.available_height();
        let mut pick_group = None;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(left_w, height), Layout::top_down(Align::Min), |ui| {
                egui::ScrollArea::vertical().id_salt("photocraft-mobile-brush-groups").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for (group, _) in &groups {
                        if category_row(ui, t, Some("brush"), group, group == &shown).clicked() {
                            pick_group = Some(group.clone());
                        }
                    }
                });
            });
            ui.add_space(10.0);
            let indices: Vec<usize> = groups.iter().find(|(g, _)| g == &shown).map(|(_, i)| i.clone()).unwrap_or_default();
            ui.allocate_ui_with_layout(vec2(ui.available_width(), height), Layout::top_down(Align::Min), |ui| {
                const ROW: f32 = 78.0;
                let ppp = ui.ctx().pixels_per_point();
                egui::ScrollArea::vertical().id_salt(("photocraft-mobile-brushes", shown.clone())).auto_shrink([false, false]).show_rows(
                    ui,
                    ROW,
                    indices.len(),
                    |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for &i in indices.get(range).unwrap_or_default() {
                            let Some(preset) = presets.get(i) else { continue };
                            let selected = current.as_deref() == Some(preset.name.as_str());
                            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                            let card = rect.shrink2(vec2(0.0, 3.0));
                            let (fill, ink) = if selected {
                                (t.accent, ink_on(t.accent))
                            } else if resp.hovered() || resp.is_pointer_button_down_on() {
                                (t.hover, t.text)
                            } else {
                                (Color32::TRANSPARENT, t.text)
                            };
                            if fill != Color32::TRANSPARENT {
                                ui.painter().rect_filled(card, 12.0, fill);
                            }
                            ui.painter().text(pos2(card.left() + 14.0, card.top() + 15.0), Align2::LEFT_CENTER, &preset.name, FontId::proportional(14.0), ink);
                            let stroke = Rect::from_min_max(pos2(card.left() + 10.0, card.top() + 28.0), pos2(card.right() - 10.0, card.bottom() - 4.0));
                            let (w, h) = ((stroke.width() * ppp).round().max(8.0) as u32, (stroke.height() * ppp).round().max(8.0) as u32);
                            let slot = format!("photocraft-mobile-library:{}", preset.name);
                            let tex = photocraft_ui_egui::brush_preview::stroke_texture(ui.ctx(), &slot, &preset.brush, w, h, ink);
                            ui.painter().image(tex.id(), stroke, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
                            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &preset.name));
                            if resp.clicked() {
                                acts.push(Act::Preset(preset.name.clone()));
                            }
                        }
                    },
                );
            });
        });
        if let Some(g) = pick_group {
            self.studio.brush_group = Some(g);
        }
    }

    /// The colour popover: the picker chosen in its header (a hue ring around a square, or around
    /// a triangle with H, S, V bars), the foreground and background colours, the recent colours
    /// and PhotoCraft's full Color Picker.
    fn color_body(&mut self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>) {
        use photocraft_ui_egui::color_picker_ui::{hex, hsv_to_rgb};
        self.sync_hsv();
        let selector = self.settings.color_selector;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("Colors", "Cores")).font(FontId::proportional(22.0)).color(t.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let kinds = [
                    (ColorSelector::Triangle, "triangle", tr("Triangle selector", "Seletor em triângulo")),
                    (ColorSelector::Square, "square", tr("Square selector", "Seletor em quadrado")),
                ];
                for (kind, icon, tip) in kinds {
                    if bar_icon(ui, t, icon, selector == kind, tip).clicked() && selector != kind {
                        acts.push(Act::ColorSelector(kind));
                    }
                }
            });
        });
        ui.add_space(6.0);
        let mut hsv = self.studio.hsv;
        let reserve = if selector == ColorSelector::Triangle { 236.0 } else { 150.0 };
        let side = ui.available_width().min(ui.available_height() - reserve).clamp(150.0, 300.0);
        let (mut changed, mut released) = match selector {
            ColorSelector::Square => square_picker(ui, &mut hsv, side),
            ColorSelector::Triangle => triangle_picker(ui, &mut hsv, side),
        };
        if selector == ColorSelector::Triangle {
            ui.add_space(10.0);
            let (c, r) = hsv_bars(ui, t, &mut hsv);
            changed |= c;
            released |= r;
        }
        if changed {
            self.studio.hsv = hsv;
            self.studio.picking = true;
            acts.push(Act::Color(hsv_to_rgb(hsv[0], hsv[1], hsv[2])));
        }
        // A pick that ends goes into the history; a tap that picked nothing doesn't.
        if released && std::mem::take(&mut self.studio.picking) {
            self.remember_color(hsv_to_rgb(hsv[0], hsv[1], hsv[2]));
        }
        let fg = rgb_of(self.app.session.tools.foreground);
        let bg = rgb_of(self.app.session.tools.background);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            // Foreground over background; a tap swaps them.
            let (r, swap) = ui.allocate_exact_size(vec2(64.0, 44.0), Sense::click());
            let back = Rect::from_min_size(r.min + vec2(24.0, 10.0), vec2(34.0, 34.0));
            let front = Rect::from_min_size(r.min, vec2(34.0, 34.0));
            ui.painter().rect(back, 8.0, color32(bg), Stroke::new(1.5, t.text_dim), StrokeKind::Inside);
            ui.painter().rect(front, 8.0, color32(fg), Stroke::new(2.0, t.text), StrokeKind::Inside);
            let tip = tr("Swap foreground and background", "Trocar cor de frente e de fundo");
            swap.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
            if swap.on_hover_text(tip).clicked() {
                acts.push(Act::SwapColors);
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new(hex(fg).to_uppercase()).font(FontId::monospace(15.0)).color(t.text_dim));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(tr("More…", "Mais…")).on_hover_text(tr("PhotoCraft's Color Picker", "Seletor de cores do PhotoCraft")).clicked() {
                    acts.push(Act::MoreColors);
                }
            });
        });
        ui.add_space(10.0);
        section(ui, t, tr("Recent", "Recentes"));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            if self.studio.recent_colors.is_empty() {
                ui.label(egui::RichText::new(tr("Colors you pick show up here", "As cores que você escolher aparecem aqui")).color(t.text_faint));
            }
            for &rgb in &self.studio.recent_colors {
                let (r, chip) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                ui.painter().circle(r.center(), 13.0, color32(rgb), Stroke::new(1.0, t.text_dim));
                let label = hex(rgb);
                chip.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
                if chip.clicked() {
                    acts.push(Act::Color(rgb));
                }
            }
        });
    }
}

/// Width of the hue ring around both colour pickers (points).
const RING: f32 = 26.0;

fn hsv_color(h: f32, s: f32, v: f32) -> Color32 {
    color32(photocraft_ui_egui::color_picker_ui::hsv_to_rgb(h, s, v))
}

/// The hue ring between `inner` and `outer` around `c`, with its knob at hue `h` (degrees,
/// clockwise from the right).
fn hue_ring(painter: &egui::Painter, c: Pos2, inner: f32, outer: f32, h: f32) {
    let mut mesh = egui::Mesh::default();
    const N: u32 = 120;
    for i in 0..=N {
        let deg = i as f32 * 360.0 / N as f32;
        let a = deg.to_radians();
        let dir = vec2(a.cos(), a.sin());
        let col = hsv_color(deg, 1.0, 1.0);
        mesh.colored_vertex(c + dir * inner, col);
        mesh.colored_vertex(c + dir * outer, col);
        if i > 0 {
            let k = 2 * i;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    let a = h.to_radians();
    let knob = c + vec2(a.cos(), a.sin()) * (inner + outer) / 2.0;
    painter.circle(knob, (outer - inner) / 2.0 - 3.0, hsv_color(h, 1.0, 1.0), Stroke::new(3.0, Color32::WHITE));
}

/// Where the press that is dragging `resp` began, and where the pointer is now.
fn drag_points(ui: &egui::Ui, resp: &egui::Response) -> Option<(Pos2, Pos2)> {
    if !resp.is_pointer_button_down_on() {
        return None;
    }
    Some((ui.input(|i| i.pointer.press_origin())?, resp.interact_pointer_pos()?))
}

/// A hue ring with the saturation (across) and brightness (up) square inside. Returns whether
/// `hsv` changed and whether a pick just ended.
fn square_picker(ui: &mut egui::Ui, hsv: &mut [f32; 3], side: f32) -> (bool, bool) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), side), Sense::click_and_drag());
    let c = rect.center();
    let outer = side / 2.0;
    let inner = outer - RING;
    let half = (inner - 10.0) / std::f32::consts::SQRT_2;
    let square = Rect::from_center_size(c, Vec2::splat(2.0 * half));
    let [mut h, mut s, mut v] = *hsv;
    let mut changed = false;
    if let Some((origin, p)) = drag_points(ui, &resp) {
        let d0 = (origin - c).length();
        if d0 >= inner - 6.0 && d0 <= outer + 8.0 {
            let d = p - c;
            h = d.y.atan2(d.x).to_degrees().rem_euclid(360.0);
            changed = true;
        } else if square.expand(10.0).contains(origin) {
            s = ((p.x - square.left()) / square.width()).clamp(0.0, 1.0);
            v = 1.0 - ((p.y - square.top()) / square.height()).clamp(0.0, 1.0);
            changed = true;
        }
    }
    let painter = ui.painter_at(rect.expand(4.0));
    hue_ring(&painter, c, inner, outer, h);
    let mut mesh = egui::Mesh::default();
    const G: u32 = 14;
    for j in 0..=G {
        for i in 0..=G {
            let (fx, fy) = (i as f32 / G as f32, j as f32 / G as f32);
            let p = pos2(square.left() + fx * square.width(), square.top() + fy * square.height());
            mesh.colored_vertex(p, hsv_color(h, fx, 1.0 - fy));
            if i > 0 && j > 0 {
                let k = j * (G + 1) + i;
                mesh.add_triangle(k - G - 2, k - G - 1, k - 1);
                mesh.add_triangle(k - G - 1, k, k - 1);
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    let knob = pos2(square.left() + s * square.width(), square.top() + (1.0 - v) * square.height());
    painter.circle(knob, 9.0, hsv_color(h, s, v), Stroke::new(3.0, Color32::WHITE));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, tr("Color disc", "Disco de cor")));
    *hsv = [h, s, v];
    (changed, resp.drag_stopped() || resp.clicked())
}

/// The barycentric weights of `p` for the triangle's hue, white and black corners.
pub fn triangle_weights(p: Pos2, hue: Pos2, white: Pos2, black: Pos2) -> [f32; 3] {
    let (v0, v1, v2) = (white - hue, black - hue, p - hue);
    let (d00, d01, d11) = (v0.dot(v0), v0.dot(v1), v1.dot(v1));
    let (d20, d21) = (v2.dot(v0), v2.dot(v1));
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() <= f32::EPSILON || !denom.is_finite() {
        return [1.0, 0.0, 0.0];
    }
    let w = (d11 * d20 - d01 * d21) / denom;
    let k = (d00 * d21 - d01 * d20) / denom;
    [1.0 - w - k, w, k]
}

/// Saturation and value for weights inside the triangle (clamped onto it first); `s` is kept
/// where black leaves it undefined.
pub fn triangle_sv(weights: [f32; 3], s: f32) -> (f32, f32) {
    let [a, b, k] = weights.map(|x| if x.is_finite() { x.max(0.0) } else { 0.0 });
    let sum = a + b + k;
    if sum <= f32::EPSILON {
        return (s, 0.0);
    }
    let (a, b) = (a / sum, b / sum);
    let v = (a + b).clamp(0.0, 1.0);
    (if v > 1e-4 { (a / v).clamp(0.0, 1.0) } else { s }, v)
}

/// A hue ring around a triangle whose corners are the pure hue (right), white (left) and black
/// (top), like Krita's Advanced Color Selector. Returns whether `hsv` changed and whether a pick
/// just ended.
fn triangle_picker(ui: &mut egui::Ui, hsv: &mut [f32; 3], side: f32) -> (bool, bool) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), side), Sense::click_and_drag());
    let c = rect.center();
    let outer = side / 2.0;
    let inner = outer - RING;
    let r = inner - 8.0;
    let at = |deg: f32| {
        let a = deg.to_radians();
        c + vec2(a.cos(), a.sin()) * r
    };
    let (hue_at, white_at, black_at) = (at(30.0), at(150.0), at(-90.0));
    let [mut h, mut s, mut v] = *hsv;
    let mut changed = false;
    if let Some((origin, p)) = drag_points(ui, &resp) {
        let d0 = (origin - c).length();
        let [a, b, k] = triangle_weights(origin, hue_at, white_at, black_at);
        if d0 >= inner - 6.0 && d0 <= outer + 8.0 {
            let d = p - c;
            h = d.y.atan2(d.x).to_degrees().rem_euclid(360.0);
            changed = true;
        } else if a > -0.1 && b > -0.1 && k > -0.1 {
            (s, v) = triangle_sv(triangle_weights(p, hue_at, white_at, black_at), s);
            changed = true;
        }
    }
    let painter = ui.painter_at(rect.expand(4.0));
    hue_ring(&painter, c, inner, outer, h);
    // The triangle, subdivided so each vertex has its exact colour (hue · s·v + white · (1−s)·v).
    let mut mesh = egui::Mesh::default();
    const N: u32 = 16;
    let idx = |i: u32, j: u32| i * (i + 1) / 2 + j;
    for i in 0..=N {
        for j in 0..=i {
            let (a, b) = (j as f32 / N as f32, (i - j) as f32 / N as f32);
            let k = 1.0 - a - b;
            let p = pos2(hue_at.x * a + white_at.x * b + black_at.x * k, hue_at.y * a + white_at.y * b + black_at.y * k);
            let vv = a + b;
            let ss = if vv > 0.0 { a / vv } else { 0.0 };
            mesh.colored_vertex(p, hsv_color(h, ss, vv));
            if i < N {
                mesh.add_triangle(idx(i, j), idx(i + 1, j), idx(i + 1, j + 1));
                if j < i {
                    mesh.add_triangle(idx(i, j), idx(i + 1, j + 1), idx(i, j + 1));
                }
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    let (a, b, k) = (s * v, (1.0 - s) * v, 1.0 - v);
    let knob = pos2(hue_at.x * a + white_at.x * b + black_at.x * k, hue_at.y * a + white_at.y * b + black_at.y * k);
    let ring = if v > 0.6 && s < 0.5 { Color32::from_gray(30) } else { Color32::WHITE };
    painter.circle(knob, 8.0, hsv_color(h, s, v), Stroke::new(2.5, ring));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, tr("Color triangle", "Triângulo de cor")));
    *hsv = [h, s, v];
    (changed, resp.drag_stopped() || resp.clicked())
}

/// Hue, saturation and value bars, each showing what it would pick at this colour. Returns
/// whether `hsv` changed and whether a pick just ended.
fn hsv_bars(ui: &mut egui::Ui, t: &Tokens, hsv: &mut [f32; 3]) -> (bool, bool) {
    let [h, s, v] = *hsv;
    let hue_bar = |f: f32| hsv_color(f * 360.0, 1.0, 1.0);
    let sat_bar = |f: f32| hsv_color(h, f, v);
    let val_bar = |f: f32| hsv_color(h, s, f);
    /// A bar: where the colour sits on it (0..1), its name, and its colour along it.
    type Bar<'a> = (f32, &'a str, &'a dyn Fn(f32) -> Color32);
    let bars: [Bar; 3] = [(h / 360.0, tr("Hue", "Matiz"), &hue_bar), (s, tr("Saturation", "Saturação"), &sat_bar), (v, tr("Value", "Valor"), &val_bar)];
    let (mut changed, mut released) = (false, false);
    ui.spacing_mut().item_spacing.y = 8.0;
    for (i, (value, label, color_at)) in bars.into_iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
        let mut f = value.clamp(0.0, 1.0);
        if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()) {
            f = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            match i {
                0 => hsv[0] = (f * 360.0).min(359.9),
                1 => hsv[1] = f,
                _ => hsv[2] = f,
            }
            changed = true;
        }
        released |= resp.drag_stopped() || resp.clicked();
        let mut mesh = egui::Mesh::default();
        const N: u32 = 32;
        for k in 0..=N {
            let fx = k as f32 / N as f32;
            let x = rect.left() + fx * rect.width();
            let col = color_at(fx);
            mesh.colored_vertex(pos2(x, rect.top()), col);
            mesh.colored_vertex(pos2(x, rect.bottom()), col);
            if k > 0 {
                let j = 2 * k;
                mesh.add_triangle(j - 2, j - 1, j);
                mesh.add_triangle(j - 1, j + 1, j);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(rect, 3.0, Stroke::new(1.0, t.card_border), StrokeKind::Inside);
        let x = rect.left() + f * rect.width();
        let mark = Rect::from_center_size(pos2(x, rect.center().y), vec2(6.0, rect.height() + 4.0));
        ui.painter().rect_stroke(mark, 2.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Middle);
        resp.widget_info(|| egui::WidgetInfo::slider(true, f64::from(f), label));
    }
    (changed, released)
}
