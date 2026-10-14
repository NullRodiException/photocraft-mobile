//! # photocraft-mobile-modes
//!
//! Quick-switch modes for PhotoCraft, in the spirit of Affinity's Vector / Pixel studios: one tap
//! changes the workspace (which panels are open), trims the toolbar to the mode's tools and picks
//! the tool last used in that mode.
//!
//! This crate is pure data and decisions, with no UI and no PhotoCraft dependency, so it builds
//! for every target (desktop, Android, wasm). Tools are named the way PhotoCraft's
//! `Tool::from_name` and `edit.toolbar` read them (the `Tool` variant names). The shell crate
//! checks these names against PhotoCraft's real tool list in its tests.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A mode of the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Paths, shapes, type, artboards.
    Vector,
    /// Selections, brushes, erasers, fills.
    Pixel,
    /// Crop, healing, retouching, toning.
    Photo,
}

/// Navigation and sampling tools every mode keeps.
pub const COMMON_TOOLS: &[&str] = &["Move", "Hand", "Zoom", "RotateView", "Eyedropper"];

/// Tools that make or edit a selection (pixels or paths), whatever mode lists them.
pub const SELECTION_TOOLS: &[&str] = &[
    "RectMarquee",
    "EllipseMarquee",
    "SingleRowMarquee",
    "SingleColumnMarquee",
    "Lasso",
    "PolygonLasso",
    "MagneticLasso",
    "MagicWand",
    "QuickSelection",
    "ObjectSelection",
    "PathSelection",
    "DirectSelection",
];

const VECTOR_TOOLS: &[&str] = &[
    "Pen",
    "FreeformPen",
    "CurvaturePen",
    "AddAnchorPoint",
    "DeleteAnchorPoint",
    "ConvertPoint",
    "PathSelection",
    "DirectSelection",
    "Rectangle",
    "EllipseShape",
    "Triangle",
    "Polygon",
    "Line",
    "CustomShape",
    "Type",
    "VerticalType",
    "Gradient",
    "Artboard",
    "Frame",
    "Slice",
    "SliceSelect",
    "Ruler",
];

const PIXEL_TOOLS: &[&str] = &[
    "RectMarquee",
    "EllipseMarquee",
    "SingleRowMarquee",
    "SingleColumnMarquee",
    "Lasso",
    "PolygonLasso",
    "MagneticLasso",
    "MagicWand",
    "QuickSelection",
    "ObjectSelection",
    "Brush",
    "Pencil",
    "MixerBrush",
    "ColorReplacement",
    "Eraser",
    "BackgroundEraser",
    "MagicEraser",
    "Gradient",
    "PaintBucket",
    "Smudge",
    "Blur",
    "Sharpen",
    "CloneStamp",
    "PatternStamp",
    "HistoryBrush",
    "Type",
    "HorizontalTypeMask",
    "VerticalTypeMask",
    "Crop",
];

const PHOTO_TOOLS: &[&str] = &[
    "Crop",
    "PerspectiveCrop",
    "RectMarquee",
    "EllipseMarquee",
    "Lasso",
    "PolygonLasso",
    "QuickSelection",
    "ObjectSelection",
    "MagicWand",
    "Remove",
    "SpotHealing",
    "Healing",
    "Patch",
    "ContentAwareMove",
    "RedEye",
    "CloneStamp",
    "Dodge",
    "Burn",
    "Sponge",
    "Blur",
    "Sharpen",
    "ColorSampler",
    "Ruler",
    "Note",
    "Count",
];

impl Mode {
    /// In switcher order: Alt+1, Alt+2, Alt+3.
    pub const ALL: [Mode; 3] = [Mode::Vector, Mode::Pixel, Mode::Photo];

    /// Stable machine id (`vector`, `pixel`, `photo`).
    pub fn id(self) -> &'static str {
        match self {
            Mode::Vector => "vector",
            Mode::Pixel => "pixel",
            Mode::Photo => "photo",
        }
    }

    pub fn from_id(id: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.id().eq_ignore_ascii_case(id.trim()))
    }

    /// The switcher label in a UI language (a code such as `pt-BR`, `es` or `en`).
    pub fn label(self, lang: &str) -> &'static str {
        let lang = lang.to_ascii_lowercase();
        let (pt, es) = (lang.starts_with("pt"), lang.starts_with("es"));
        match self {
            Mode::Vector if pt => "Vetor",
            Mode::Vector => "Vector",
            Mode::Pixel if es => "Píxel",
            Mode::Pixel => "Pixel",
            Mode::Photo if pt || es => "Foto",
            Mode::Photo => "Photo",
        }
    }

    /// The PhotoCraft icon drawn beside the label.
    pub fn icon(self) -> &'static str {
        match self {
            Mode::Vector => "pen-tool",
            Mode::Pixel => "grid-2x2",
            Mode::Photo => "image",
        }
    }

    /// The Window › Workspace command that lays out this mode's panels.
    pub fn workspace_command(self) -> &'static str {
        match self {
            Mode::Vector => "window.workspace.graphicAndWeb",
            Mode::Pixel => "window.workspace.painting",
            Mode::Photo => "window.workspace.photography",
        }
    }

    /// Position in [`Mode::ALL`], which is also the Alt+digit shortcut minus one.
    pub fn index(self) -> usize {
        match self {
            Mode::Vector => 0,
            Mode::Pixel => 1,
            Mode::Photo => 2,
        }
    }

    /// The next mode, wrapping around (what a cycling shortcut steps to).
    pub fn next(self) -> Mode {
        match self {
            Mode::Vector => Mode::Pixel,
            Mode::Pixel => Mode::Photo,
            Mode::Photo => Mode::Vector,
        }
    }

    /// The tool picked the first time the mode is entered.
    pub fn default_tool(self) -> &'static str {
        match self {
            Mode::Vector => "Pen",
            Mode::Pixel => "Brush",
            Mode::Photo => "SpotHealing",
        }
    }

    /// The three tools the top bar keeps one tap away, like Procreate's Brush, Smudge and Eraser.
    pub fn quick_tools(self) -> [&'static str; 3] {
        match self {
            Mode::Vector => ["Pen", "Rectangle", "Type"],
            Mode::Pixel => ["Brush", "Smudge", "Eraser"],
            Mode::Photo => ["SpotHealing", "CloneStamp", "Dodge"],
        }
    }

    /// The selection tools this mode shows, in toolbar order (the top bar's Selection button).
    pub fn selection_tools(self) -> impl Iterator<Item = &'static str> {
        self.tools().filter(|t| SELECTION_TOOLS.contains(t))
    }

    /// The tools this mode adds to [`COMMON_TOOLS`].
    pub fn own_tools(self) -> &'static [&'static str] {
        match self {
            Mode::Vector => VECTOR_TOOLS,
            Mode::Pixel => PIXEL_TOOLS,
            Mode::Photo => PHOTO_TOOLS,
        }
    }

    /// Every tool the toolbar shows in this mode.
    pub fn tools(self) -> impl Iterator<Item = &'static str> {
        COMMON_TOOLS.iter().chain(self.own_tools()).copied()
    }

    /// Whether the toolbar shows `tool` in this mode.
    pub fn shows(self, tool: &str) -> bool {
        self.tools().any(|t| t == tool)
    }
}

/// What to apply to the editor to enter a mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Switch {
    pub mode: Mode,
    /// Run first: lays out the panels.
    pub workspace_command: &'static str,
    /// The `hidden` list for `edit.toolbar`: every tool the mode doesn't show.
    pub hidden: Vec<String>,
    /// The tool to make active.
    pub tool: String,
}

/// The mode in use and the tool last used in each mode. Saved between sessions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modes {
    #[serde(default)]
    pub current: Option<Mode>,
    #[serde(default)]
    pub last_tool: BTreeMap<Mode, String>,
}

impl Modes {
    /// The mode a fresh install starts in.
    pub const FIRST: Mode = Mode::Pixel;

    /// The current mode, or [`Modes::FIRST`] before any was chosen.
    pub fn mode(&self) -> Mode {
        self.current.unwrap_or(Self::FIRST)
    }

    /// Plans entering `to` and records it as the current mode.
    ///
    /// `current_tool` is the editor's active tool, remembered for the mode being left.
    /// `all_tools` is every tool the editor has; the ones `to` doesn't show become `hidden`.
    /// Re-entering the current mode keeps the active tool when the mode shows it, so applying
    /// the saved mode at start-up doesn't change tools.
    pub fn switch(&mut self, to: Mode, current_tool: &str, all_tools: &[&str]) -> Switch {
        if let Some(from) = self.current
            && from.shows(current_tool)
        {
            self.last_tool.insert(from, current_tool.to_string());
        }
        let staying = self.current == Some(to) && to.shows(current_tool);
        let tool = if staying {
            current_tool.to_string()
        } else {
            match self.last_tool.get(&to) {
                Some(t) if to.shows(t) => t.clone(),
                _ => to.default_tool().to_string(),
            }
        };
        self.current = Some(to);
        Switch { mode: to, workspace_command: to.workspace_command(), hidden: all_tools.iter().filter(|t| !to.shows(t)).map(|t| t.to_string()).collect(), tool }
    }
}

#[cfg(test)]
mod tests;
