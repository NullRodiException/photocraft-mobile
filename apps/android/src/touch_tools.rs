//! What made each touch contact: finger or pen.
//!
//! winit drops Android's tool type, so `MainActivity.processMotionEvent` reports it here for every
//! new pointer (`nativeTouchTool`), just before the event goes on to winit. The shell asks
//! [`tool_of`] with the touch id egui sees, which is the same Android pointer id.

use std::ffi::c_void;
use std::sync::{Mutex, PoisonError};

use photocraft_mobile_shell::ToolKind;

/// `MotionEvent.TOOL_TYPE_*`.
const TOOL_TYPE_FINGER: i32 = 1;
const TOOL_TYPE_STYLUS: i32 = 2;
const TOOL_TYPE_ERASER: i32 = 4;

/// Recent pointers and their tool, newest last. Android reuses small pointer ids, so a few are enough.
static TOOLS: Mutex<Vec<(u64, ToolKind)>> = Mutex::new(Vec::new());
const KEEP: usize = 16;

fn record(pointer_id: i32, tool_type: i32) {
    let Ok(id) = u64::try_from(pointer_id) else { return };
    let kind = match tool_type {
        TOOL_TYPE_FINGER => Some(ToolKind::Finger),
        TOOL_TYPE_STYLUS | TOOL_TYPE_ERASER => Some(ToolKind::Pen),
        // A mouse or an unknown tool: no gesture handling, it reaches PhotoCraft as is.
        _ => None,
    };
    let mut tools = TOOLS.lock().unwrap_or_else(PoisonError::into_inner);
    tools.retain(|(i, _)| *i != id);
    if let Some(kind) = kind {
        tools.push((id, kind));
    }
    if tools.len() > KEEP {
        tools.remove(0);
    }
}

/// The tool of the contact with this touch id, when `MainActivity` reported it.
pub fn tool_of(id: u64) -> Option<ToolKind> {
    TOOLS.lock().unwrap_or_else(PoisonError::into_inner).iter().rev().find(|(i, _)| *i == id).map(|(_, k)| *k)
}

/// `private static native void nativeTouchTool(int pointerId, int toolType)` in `MainActivity`.
// Like `android_main`, the unsafe part is only the unmangled export the JVM looks up by name. It
// takes plain integers, never touches the JNI environment, and can't panic.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeTouchTool(_env: *mut c_void, _class: *mut c_void, pointer_id: i32, tool_type: i32) {
    record(pointer_id, tool_type);
}
