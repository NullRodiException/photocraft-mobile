//! Open and save through Android's Storage Access Framework (the system file picker).
//!
//! PhotoCraft asks its `file_dialog` service for a file to open or a place to save, and writes
//! saves through its `write` service. On Android a picker hands out content URIs, not paths, so:
//!
//! - **Open:** the request waits here until `MainActivity` picks it up ([`next_request`],
//!   polled a few times a second), shows the picker, copies each chosen document into the app's
//!   cache and hands the paths back (`nativeOpenResult`); PhotoCraft opens them as files.
//! - **Save:** the picker creates the document; `MainActivity` answers with a cache path tied to
//!   that URI (`nativeSaveResult`), PhotoCraft writes the file there, and [`after_write`] asks
//!   `MainActivity` to copy it to the URI. Saving the same document again goes straight to its URI.
//!
//! Requests travel as tab-separated lines (`open\t<id>\t<multiple>`, `save\t<id>\t<name>`,
//! `export\t<path>`), so the JNI calls carry only strings and numbers.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, PoisonError};

use jni::EnvUnowned;
use jni::objects::{JClass, JString};
use photocraft_ui_egui::{FileDialogAnswer, FileDialogReply, FileDialogRequest};

/// Lines waiting for `MainActivity`.
static REQUESTS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// Dialogs waiting for the user, by request id.
static WAITING: Mutex<Vec<(i64, FileDialogReply)>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicI64 = AtomicI64::new(1);

/// The folder (under the app cache) that saves go through; only files in it are exported.
const SAVE_DIR: &str = "/saf-save/";

fn push(line: String) {
    REQUESTS.lock().unwrap_or_else(PoisonError::into_inner).push_back(line);
}

/// A field of a request line can't hold tabs or line breaks.
fn clean(s: &str) -> String {
    s.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect()
}

/// PhotoCraft's `file_dialog` service on Android.
pub fn file_dialog(request: FileDialogRequest, _frame: Option<&eframe::Frame>, reply: FileDialogReply) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let line = match request {
        FileDialogRequest::Open { multiple, .. } => format!("open\t{id}\t{}", u8::from(multiple)),
        FileDialogRequest::Save { suggested } => {
            let name = std::path::Path::new(&suggested).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled.psd".into());
            format!("save\t{id}\t{}", clean(&name))
        }
        FileDialogRequest::Folder { .. } => {
            reply.send(Some(FileDialogAnswer::Unavailable("Choosing a folder isn't available on Android yet.".into())));
            return;
        }
    };
    WAITING.lock().unwrap_or_else(PoisonError::into_inner).push((id, reply));
    push(line);
}

/// After PhotoCraft wrote `path`: copy it to the document the save picker chose for it.
pub fn after_write(path: &str) {
    if path.contains(SAVE_DIR) {
        push(format!("export\t{}", clean(path)));
    }
}

fn answer(id: i64, answer: Option<FileDialogAnswer>) {
    let reply = {
        let mut waiting = WAITING.lock().unwrap_or_else(PoisonError::into_inner);
        waiting.iter().position(|(i, _)| *i == id).map(|i| waiting.remove(i).1)
    };
    match reply {
        Some(reply) => reply.send(answer),
        None => log::warn!("file dialog {id}: no request waiting for this answer"),
    }
}

/// `private static native String nativeNextRequest()`: the next request line, or null.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeNextRequest<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    let next = REQUESTS.lock().unwrap_or_else(PoisonError::into_inner).pop_front();
    env.with_env(|env| -> Result<JString<'caller>, jni::errors::Error> {
        match next {
            Some(line) => JString::from_str(env, line),
            None => Ok(JString::default()),
        }
    })
    .resolve::<jni::errors::LogErrorAndDefault>()
}

/// `private static native void nativeOpenResult(long id, String paths)`: the opened copies,
/// one path per line; empty when the picker was cancelled.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeOpenResult<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    id: i64,
    paths: JString<'caller>,
) {
    env.with_env(|env| -> Result<(), jni::errors::Error> {
        let paths: Vec<String> =
            if paths.is_null() { Vec::new() } else { paths.try_to_string(env)?.lines().filter(|l| !l.is_empty()).map(str::to_string).collect() };
        answer(id, (!paths.is_empty()).then_some(FileDialogAnswer::Paths(paths)));
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>()
}

/// `private static native void nativeSaveResult(long id, String path)`: where PhotoCraft writes
/// the save (exported to the picked document afterwards); empty when the picker was cancelled.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeSaveResult<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    id: i64,
    path: JString<'caller>,
) {
    env.with_env(|env| -> Result<(), jni::errors::Error> {
        let path = if path.is_null() { String::new() } else { path.try_to_string(env)? };
        answer(id, (!path.is_empty()).then_some(FileDialogAnswer::SaveTo(path)));
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>()
}
