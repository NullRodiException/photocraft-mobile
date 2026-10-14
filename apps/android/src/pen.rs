//! The S Pen (any stylus) beyond what winit delivers on Android.
//!
//! winit 0.30 passes a stylus contact on as a plain touch with pressure, and drops hover, tilt
//! and the buttons. `MainActivity` sends every stylus event here (`nativePenSample`), and:
//!
//! - each sample (pressure, tilt, side button) goes into PhotoCraft's [`StylusFeed`], the same
//!   input the desktop's tablet readers fill. The side button reports the eraser end, so holding
//!   it switches to the Eraser and letting go switches back (PhotoCraft's own pen-eraser handling);
//! - hovering moves egui's pointer ([`input_hook`]), so the brush outline follows the pen before
//!   it touches;
//! - pen contacts lose winit's force in [`input_hook`], so pressure comes from the feed only and
//!   each sample keeps its tilt.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use photocraft_mobile_shell::ToolKind;
use photocraft_mobile_shell::pen::tilt_from_android;
use photocraft_ui_egui::stylus::{PenSample, StylusFeed};

/// `MotionEvent.ACTION_*`.
const ACTION_DOWN: i32 = 0;
const ACTION_UP: i32 = 1;
const ACTION_MOVE: i32 = 2;
const ACTION_CANCEL: i32 = 3;
const ACTION_POINTER_DOWN: i32 = 5;
const ACTION_POINTER_UP: i32 = 6;
const ACTION_HOVER_MOVE: i32 = 7;
const ACTION_HOVER_ENTER: i32 = 9;
const ACTION_HOVER_EXIT: i32 = 10;
/// `MotionEvent.BUTTON_STYLUS_PRIMARY` (the S Pen's side button).
const BUTTON_STYLUS_PRIMARY: i32 = 0x20;
/// `MotionEvent.TOOL_TYPE_ERASER`.
const TOOL_TYPE_ERASER: i32 = 4;

static FEED: OnceLock<StylusFeed> = OnceLock::new();
static CTX: OnceLock<egui::Context> = OnceLock::new();
/// A stylus has sent samples: from then on, pen pressure comes from the feed.
static PEN_SEEN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug)]
enum Hover {
    /// Physical pixels.
    At(f32, f32),
    Gone,
}

/// Hover changes since the last frame, oldest first.
static HOVER: Mutex<Vec<Hover>> = Mutex::new(Vec::new());

/// Connect to the app's stylus input and repaint; call once the app exists.
pub fn install(feed: StylusFeed, ctx: egui::Context) {
    let _ = FEED.set(feed);
    let _ = CTX.set(ctx);
}

#[allow(clippy::too_many_arguments)] // one per MotionEvent field, as Java passes them
fn sample(action: i32, x: f32, y: f32, pressure: f32, tilt: f32, orientation: f32, buttons: i32, tool: i32) {
    PEN_SEEN.store(true, Ordering::Relaxed);
    let eraser = tool == TOOL_TYPE_ERASER || buttons & BUTTON_STYLUS_PRIMARY != 0;
    let (tilt_x, tilt_y) = tilt_from_android(tilt, orientation);
    let touching = matches!(action, ACTION_DOWN | ACTION_MOVE | ACTION_POINTER_DOWN);
    let pen = |pressure: f32| PenSample { pressure, tilt_x, tilt_y, rotation: 0.0, eraser }.sanitized();
    if let Some(feed) = FEED.get() {
        match action {
            _ if touching => feed.set(Some(pen(pressure))),
            // Lifted or hovering: pressure 0, but tilt and the side button still count (the
            // Eraser takes over while the button is held over the screen).
            ACTION_UP | ACTION_POINTER_UP | ACTION_HOVER_ENTER | ACTION_HOVER_MOVE => feed.set(Some(pen(0.0))),
            // Out of range: no pen, as with a mouse.
            ACTION_HOVER_EXIT | ACTION_CANCEL => feed.set(None),
            _ => {}
        }
    }
    let hover = match action {
        ACTION_HOVER_ENTER | ACTION_HOVER_MOVE if x.is_finite() && y.is_finite() => Some(Hover::At(x, y)),
        ACTION_HOVER_EXIT => Some(Hover::Gone),
        _ => None,
    };
    if let Some(h) = hover {
        let mut queue = HOVER.lock().unwrap_or_else(PoisonError::into_inner);
        // Only the latest position matters; keep a short history so a Gone isn't lost.
        if queue.len() >= 32 {
            queue.remove(0);
        }
        queue.push(h);
    }
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

/// Adds the pen's hover to a frame's input and leaves pen pressure to the feed.
pub fn input_hook(raw: &mut egui::RawInput, pixels_per_point: f32) {
    if PEN_SEEN.load(Ordering::Relaxed) {
        for e in &mut raw.events {
            if let egui::Event::Touch { id, force, .. } = e
                && crate::touch_tools::tool_of(id.0) == Some(ToolKind::Pen)
            {
                *force = None;
            }
        }
    }
    let hovers = std::mem::take(&mut *HOVER.lock().unwrap_or_else(PoisonError::into_inner));
    // A frame where something touches belongs to that contact: hover moves and the pen leaving
    // its hover just before touching must not disturb the press.
    let touching = raw.events.iter().any(|e| matches!(e, egui::Event::Touch { .. } | egui::Event::PointerButton { .. }));
    if touching || !(pixels_per_point.is_finite() && pixels_per_point > 0.0) {
        return;
    }
    match hovers.last() {
        Some(Hover::At(x, y)) => raw.events.push(egui::Event::PointerMoved(egui::pos2(x / pixels_per_point, y / pixels_per_point))),
        Some(Hover::Gone) => raw.events.push(egui::Event::PointerGone),
        None => {}
    }
}

/// `private static native void nativePenSample(int action, float x, float y, float pressure,
/// float tilt, float orientation, int buttons, int toolType)` in `MainActivity`.
// The unsafe part is only the unmangled export the JVM looks up by name. Plain numbers, no JNI
// environment use, and nothing here can panic.
#[allow(unsafe_code, clippy::too_many_arguments)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativePenSample(
    _env: *mut c_void,
    _class: *mut c_void,
    action: i32,
    x: f32,
    y: f32,
    pressure: f32,
    tilt: f32,
    orientation: f32,
    buttons: i32,
    tool: i32,
) {
    sample(action, x, y, pressure, tilt, orientation, buttons, tool);
}
