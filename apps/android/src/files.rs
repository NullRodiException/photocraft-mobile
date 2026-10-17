//! Open and save through Android's Storage Access Framework (the system file picker).
//!
//! PhotoCraft asks its `file_dialog` service for a file to open or a place to save, and writes
//! saves through its `write` service. On Android a picker hands out content URIs, not paths, so:
//!
//! - **Open:** the request waits here until `MainActivity` picks it up (polled a few times a
//!   second). It shows the picker, copies each chosen document into the app's cache, links each
//!   copy to its document (`nativeLink`) and hands the paths back (`nativeOpenResult`). PhotoCraft
//!   opens them as files.
//! - **Save:** the picker creates the document. `MainActivity` links a cache path to it and answers
//!   with that path (`nativeSaveResult`), and PhotoCraft writes the file there.
//! - **Writing a linked copy** (Save As, and Save on an opened or saved document) goes on to its
//!   document: the `write` service checks [`write_target`] first, then asks `MainActivity` to copy
//!   the file to the document ([`export`]). A copy the app may only read, or one that lost its
//!   link, refuses the write, so it never looks saved while the document stays as it was.
//!
//! The links ([`photocraft_mobile_shell::saf::Links`]) are kept in the settings folder, so saving
//! again still reaches the document after Android restarted the app. `MainActivity` keeps the URI
//! permissions across restarts too.
//!
//! At start-up, [`clean_cache`] deletes the cache copies that nothing will open again.
//!
//! Other apps: "Open with" and "Share" reach `MainActivity`, which copies the documents into the
//! cache the same way and hands the paths over (`nativeOpenFiles`); PhotoCraft opens them as OS
//! requests ([`os_events`]). The other way, the shell's exported images ([`share_image`]) go to
//! the share sheet or the gallery.
//!
//! Requests travel as tab-separated lines (`open\t<id>\t<multiple>`, `save\t<id>\t<name>`,
//! `export\t<path>\t<uri>`, `release\t<uri>`, `share\t<path>`, `image\t<send|gallery>\t<path>`),
//! so the JNI calls carry only strings and numbers.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use jni::EnvUnowned;
use jni::objects::{JClass, JString};
use photocraft_mobile_shell::saf::{Links, WriteTarget};
use photocraft_mobile_shell::share::ShareTarget;
use photocraft_ui_egui::{FileDialogAnswer, FileDialogReply, FileDialogRequest, OsEvent};

/// Lines waiting for `MainActivity`.
static REQUESTS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// Dialogs waiting for the user, by request id.
static WAITING: Mutex<Vec<(i64, FileDialogReply)>> = Mutex::new(Vec::new());
/// Request ids also name the cache folders of opened and saved documents, which outlive a launch
/// (recent files, links): [`next_id`] keeps them at or past the time, so a new launch never
/// reuses one.
static NEXT_ID: AtomicI64 = AtomicI64::new(1);
/// The cache copies of picked documents (loaded by [`init`]).
static LINKS: Mutex<Option<Links>> = Mutex::new(None);
/// Where the links are saved (the settings folder).
static CONFIG_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
/// Documents other apps sent, waiting for the next frame.
static INCOMING: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CTX: OnceLock<egui::Context> = OnceLock::new();

/// Load the saved links from `config_dir`; call once before the app starts.
pub fn init(config_dir: Option<PathBuf>) {
    launch();
    let links = Links::load(config_dir.as_deref());
    log::info!("{} picked document(s) linked", links.len());
    *LINKS.lock().unwrap_or_else(PoisonError::into_inner) = Some(links);
    let _ = CONFIG_DIR.set(config_dir);
}

/// Repaint through `ctx` when another app sends documents, so a frame opens them.
pub fn install(ctx: egui::Context) {
    let _ = CTX.set(ctx);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().and_then(|d| i64::try_from(d.as_millis()).ok()).unwrap_or(1)
}

/// When this launch began (ms): its request ids are at or past it, an earlier launch's before it.
fn launch() -> i64 {
    static LAUNCH: OnceLock<i64> = OnceLock::new();
    *LAUNCH.get_or_init(now_ms)
}

/// A new request id, never one an earlier launch used.
fn next_id() -> i64 {
    NEXT_ID.fetch_max(launch().max(now_ms()), Ordering::Relaxed);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// PhotoCraft's `os_events` service: the documents other apps sent since the last frame.
pub fn os_events() -> Vec<OsEvent> {
    let paths = std::mem::take(&mut *INCOMING.lock().unwrap_or_else(PoisonError::into_inner));
    if paths.is_empty() { Vec::new() } else { vec![OsEvent::Open(paths)] }
}

fn with_links<R>(f: impl FnOnce(&mut Links) -> R) -> R {
    let mut links = LINKS.lock().unwrap_or_else(PoisonError::into_inner);
    f(links.get_or_insert_with(Links::default))
}

/// What writing `path` involves: `Some(uri)` for the copy of a picked document (write it, then
/// [`export`] it there), `None` for an ordinary file, `Err` when the write must not happen.
pub fn write_target(path: &str) -> Result<Option<String>, String> {
    with_links(|links| {
        links.write_target(path).map(|target| match target {
            WriteTarget::File => None,
            WriteTarget::Document(uri) => Some(uri.to_string()),
        })
    })
}

/// After PhotoCraft wrote `path`: copy it to its picked document at `uri`.
pub fn export(path: &str, uri: &str) {
    push(format!("export\t{}\t{}", clean(path), clean(uri)));
}

/// Offers the text file at `path` (a crash report) to the system's share sheet.
pub fn send_report(path: &Path) {
    push(format!("share\t{}", clean(&path.to_string_lossy())));
}

/// The shell's `ShareFn`: the exported image at `path` goes to the share sheet or the gallery.
pub fn share_image(target: ShareTarget, path: &Path) {
    let target = match target {
        ShareTarget::Send => "send",
        ShareTarget::Gallery => "gallery",
    };
    push(format!("image\t{target}\t{}", clean(&path.to_string_lossy())));
}

/// `MainActivity` puts the app's cache folder here before the library starts.
const CACHE_ENV: &str = "PHOTOCRAFT_MOBILE_CACHE";
/// The cache folder for images exported to share (`res/xml/file_paths.xml` names it too).
const SHARE_DIR: &str = "share";

/// The app's cache folder, when `MainActivity` said where it is.
fn cache_dir() -> Option<PathBuf> {
    std::env::var_os(CACHE_ENV).filter(|c| !c.is_empty()).map(PathBuf::from)
}

/// Where the shell exports images to share (see [`share_image`]).
pub fn share_dir() -> Option<PathBuf> {
    cache_dir().map(|c| c.join(SHARE_DIR))
}

/// Deletes the picked documents' cache copies that nothing will open again: every folder under
/// `saf-open/` and `saf-save/` that holds none of `keep` (the recent files, and the documents crash
/// recovery may reopen). Their links go too, and `MainActivity` gives back the URI permissions no
/// other link uses. Images exported to share last time go too.
pub fn clean_cache(keep: &[String]) {
    let Some(cache) = cache_dir() else {
        log::warn!("{CACHE_ENV} isn't set: the picked documents' cache copies stay");
        return;
    };
    let shared = cache.join(SHARE_DIR);
    if shared.exists()
        && let Err(e) = std::fs::remove_dir_all(&shared)
    {
        log::warn!("couldn't delete {}: {e}", shared.display());
    }
    let stale = photocraft_mobile_shell::saf::stale_dirs(&cache, keep, launch());
    if stale.is_empty() {
        return;
    }
    let mut removed = 0;
    for dir in &stale {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => removed += 1,
            Err(e) => log::warn!("couldn't delete {}: {e}", dir.display()),
        }
    }
    let released = with_links(|links| {
        let released: Vec<String> = stale.iter().flat_map(|dir| links.forget_under(dir)).collect();
        links.save(CONFIG_DIR.get().and_then(|d| d.as_deref()));
        released
    });
    for uri in &released {
        push(format!("release\t{}", clean(uri)));
    }
    log::info!("cache: deleted {removed} picked-document folder(s) nothing opens again, released {} URI permission(s)", released.len());
}

fn push(line: String) {
    REQUESTS.lock().unwrap_or_else(PoisonError::into_inner).push_back(line);
}

/// A field of a request line can't hold tabs or line breaks.
fn clean(s: &str) -> String {
    s.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect()
}

/// PhotoCraft's `file_dialog` service on Android.
pub fn file_dialog(request: FileDialogRequest, _frame: Option<&eframe::Frame>, reply: FileDialogReply) {
    let id = next_id();
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

/// `private static native void nativeLink(String path, String uri, int writable)`: the cache file
/// `path` stands for the picked document `uri`, which the app may write when `writable` isn't 0.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeLink<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    path: JString<'caller>,
    uri: JString<'caller>,
    writable: i32,
) {
    env.with_env(|env| -> Result<(), jni::errors::Error> {
        if path.is_null() || uri.is_null() {
            return Ok(());
        }
        let (path, uri) = (path.try_to_string(env)?, uri.try_to_string(env)?);
        with_links(|links| {
            links.link(&path, &uri, writable != 0);
            links.save(CONFIG_DIR.get().and_then(|d| d.as_deref()));
        });
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>()
}

/// `private static native long nativeNextId()`: a new cache folder id for documents another app
/// sent (see [`next_id`]).
// No arguments and no JNI environment use: only the unmangled export is unsafe.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeNextId(_env: *mut std::ffi::c_void, _class: *mut std::ffi::c_void) -> i64 {
    next_id()
}

/// `private static native void nativeOpenFiles(String paths)`: the copies of the documents another
/// app sent ("Open with", "Share"), one path per line, for PhotoCraft to open.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeOpenFiles<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    paths: JString<'caller>,
) {
    env.with_env(|env| -> Result<(), jni::errors::Error> {
        if paths.is_null() {
            return Ok(());
        }
        let text = paths.try_to_string(env)?;
        let opened: Vec<String> = text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
        log::info!("another app sent {} document(s)", opened.len());
        INCOMING.lock().unwrap_or_else(PoisonError::into_inner).extend(opened);
        if let Some(ctx) = CTX.get() {
            ctx.request_repaint();
        }
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>()
}
