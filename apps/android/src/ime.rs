//! The on-screen keyboard's text, which winit 0.30 drops on Android.
//!
//! With GameActivity, the keyboard doesn't send keys: it edits a text field of the activity's own
//! (GameTextInput), and android-activity reports each edit as `InputEvent::TextEvent`, which winit
//! 0.30 ignores. So nothing typed on the keyboard ever reached egui. Here:
//!
//! - `MainActivity.stateChanged` (the field changed) calls `nativeImeChanged`, which asks for a
//!   frame;
//! - each frame, [`input_hook`] reads the field (`AndroidApp::text_input_state`), and the shell's
//!   [`ImeMirror`] turns what changed into egui's text and Backspace events;
//! - when another egui widget takes the focus, the field is cleared (`set_text_input_state`), so
//!   each text field starts from an empty one;
//! - the keyboard's action key (Done, Go, Search…), which `MainActivity.onEditorAction` reports and
//!   winit drops too, becomes Enter (`nativeImeAction`).

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use egui::{Event, Key, Modifiers};
use photocraft_mobile_shell::ime::ImeMirror;
use winit::platform::android::activity::AndroidApp;
use winit::platform::android::activity::input::{TextInputState, TextSpan};

static APP: OnceLock<AndroidApp> = OnceLock::new();
static CTX: OnceLock<egui::Context> = OnceLock::new();
static MIRROR: Mutex<Option<ImeMirror>> = Mutex::new(None);
/// The keyboard's action key was pressed since the last frame.
static ACTION: AtomicBool = AtomicBool::new(false);

/// Read the activity's text field through `app`, and repaint through `ctx` when it changes.
pub fn install(app: AndroidApp, ctx: egui::Context) {
    let _ = APP.set(app);
    let _ = CTX.set(ctx);
}

fn enter() -> [Event; 2] {
    let event = |pressed| Event::Key { key: Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    [event(true), event(false)]
}

/// Adds the keyboard's edits since the last frame to a frame's input.
pub fn input_hook(raw: &mut egui::RawInput) {
    let (Some(app), Some(ctx)) = (APP.get(), CTX.get()) else { return };
    let focus = ctx.memory(|m| m.focused());
    let mut events = Vec::new();
    {
        let mut mirror = MIRROR.lock().unwrap_or_else(PoisonError::into_inner);
        let mirror = mirror.get_or_insert_with(ImeMirror::default);
        if mirror.focus(focus) {
            app.set_text_input_state(TextInputState { text: String::new(), selection: TextSpan { start: 0, end: 0 }, compose_region: None });
        } else if focus.is_some() {
            events = mirror.update(&app.text_input_state().text);
        }
    }
    let typed_enter = events.iter().any(|e| matches!(e, Event::Key { key: Key::Enter, .. }));
    if ACTION.swap(false, Ordering::Relaxed) && focus.is_some() && !typed_enter {
        events.extend(enter());
    }
    if !events.is_empty() {
        raw.events.extend(events);
    }
}

fn wake() {
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

/// `private static native void nativeImeChanged()` in `MainActivity`: the keyboard edited the field.
// The unsafe part is only the unmangled export the JVM looks up by name. No arguments, no JNI
// environment use, and nothing here can panic.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeImeChanged(_env: *mut c_void, _class: *mut c_void) {
    wake();
}

/// `private static native void nativeImeAction()` in `MainActivity`: the keyboard's action key.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeImeAction(_env: *mut c_void, _class: *mut c_void) {
    ACTION.store(true, Ordering::Relaxed);
    wake();
}
