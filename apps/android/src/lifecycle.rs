//! The activity's lifecycle, handed to the shell as platform events.
//!
//! `MainActivity.onPause` calls `nativeBackground` when the app leaves the screen. Android may end
//! an app in the background to free memory, so the shell then autosaves the documents with
//! unsaved changes for crash recovery ([`PlatformEvent::Background`]).
//!
//! The autosave runs in a frame, and once the activity has paused, winit draws no more frames. So
//! `MainActivity` holds back its pause until a frame has taken the event
//! (`nativeBackgroundHandled`), for a short while at most.
//!
//! `MainActivity.onTrimMemory` passes Android's memory warnings on (`nativeTrimMemory`). What they
//! call for is in `photocraft_mobile_shell::memory`: the caches go in the call itself, since no
//! frame runs in the background, and a critical warning while the app is in front also autosaves
//! in the next frame ([`PlatformEvent::LowMemory`]).

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use photocraft_mobile_shell::PlatformEvent;
use photocraft_mobile_shell::memory::{self, Trim};

/// The activity is pausing, and no frame has taken that yet.
static BACKGROUND: AtomicBool = AtomicBool::new(false);
/// A frame took the last pause.
static HANDLED: AtomicBool = AtomicBool::new(false);
/// Memory is critically low, and no frame has autosaved for it yet.
static LOW_MEMORY: AtomicBool = AtomicBool::new(false);
static CTX: OnceLock<egui::Context> = OnceLock::new();

/// Repaint through `ctx` when the activity reports something, so a frame carries it out.
pub fn install(ctx: egui::Context) {
    let _ = CTX.set(ctx);
}

/// The shell's [`PlatformEvent`]s since the last frame.
pub fn events() -> Vec<PlatformEvent> {
    let mut events = Vec::new();
    if BACKGROUND.swap(false, Ordering::AcqRel) {
        HANDLED.store(true, Ordering::Release);
        events.push(PlatformEvent::Background);
    }
    if LOW_MEMORY.swap(false, Ordering::AcqRel) {
        events.push(PlatformEvent::LowMemory);
    }
    events
}

/// `private static native void nativeTrimMemory(int level)` in `MainActivity`: Android asked the
/// app to use less memory (`onTrimMemory`, with `ComponentCallbacks2`'s level).
// The unsafe part is only the unmangled export. No JNI environment use, and the caches are
// dropped under locks PhotoCraft recovers from poisoning, so nothing here can panic.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeTrimMemory(_env: *mut c_void, _class: *mut c_void, level: i32) {
    let trim = memory::trim_for(level);
    if trim == Trim::Nothing {
        log::info!("memory warning (level {level}): nothing to drop");
        return;
    }
    let freed = memory::purge_caches().saturating_add(crate::clipboard::release_memory());
    log::info!("memory warning (level {level}): dropped {:.1} MB of caches", freed as f64 / (1024.0 * 1024.0));
    if trim == Trim::CachesAndAutosave {
        LOW_MEMORY.store(true, Ordering::Release);
        if let Some(ctx) = CTX.get() {
            ctx.request_repaint();
        }
    }
}

/// `private static native void nativeBackground()` in `MainActivity`.
// The unsafe part is only the unmangled export the JVM looks up by name. No arguments, no JNI
// environment use, and nothing here can panic.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeBackground(_env: *mut c_void, _class: *mut c_void) {
    HANDLED.store(false, Ordering::Release);
    BACKGROUND.store(true, Ordering::Release);
    match CTX.get() {
        Some(ctx) => ctx.request_repaint(),
        // Before the app exists there is nothing to save: count it as handled.
        None => HANDLED.store(true, Ordering::Release),
    }
}

/// `private static native boolean nativeBackgroundHandled()` in `MainActivity`: a frame took the
/// last `nativeBackground`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeBackgroundHandled(_env: *mut c_void, _class: *mut c_void) -> u8 {
    u8::from(HANDLED.load(Ordering::Acquire))
}
