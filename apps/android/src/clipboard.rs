//! The system clipboard, for PhotoCraft's OS clipboard services: images copied in the app paste in
//! other apps, and screenshots and images copied in other apps paste here.
//!
//! - **Copy:** PhotoCraft hands over the copied pixels. A worker thread writes them as a PNG into
//!   the cache folder `clipboard/` and has `MainActivity.setClipboardFile` put the file on the
//!   clipboard (a FileProvider content URI). Until that is done, a paste gets the pixels themselves.
//! - **Paste:** `MainActivity.clipboardFile` answers with a file holding the clipboard's image: our
//!   own copy, or the image another app put there, copied into the cache. Our latest copy comes
//!   back from memory; anything else is decoded with PhotoCraft's codecs.
//!
//! Unlike the file picker's requests, which `MainActivity` polls, these calls go straight into Java,
//! since a paste needs its answer at once. `MainActivity` registers its class for them
//! (`nativeClipboardInit`): a thread the Rust side started can't look up the app's classes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, mpsc};

use jni::objects::{JClass, JString, JValue};
use jni::refs::Global;
use jni::{Env, EnvUnowned, JavaVM, jni_sig, jni_str};
use photocraft_mobile_shell::clipboard;

/// The cache folder copies and pastes go through (`res/xml/file_paths.xml` names it too).
const DIR: &str = "clipboard";

/// RGBA8: width, height, pixels.
type Pixels = Arc<(u32, u32, Vec<u8>)>;

/// `MainActivity`, from `nativeClipboardInit`.
static CLASS: OnceLock<Global<JClass<'static>>> = OnceLock::new();
/// The latest copy, while it is (or is on its way to) the clipboard.
static LATEST: Mutex<Option<Latest>> = Mutex::new(None);

#[derive(Clone)]
struct Latest {
    id: u64,
    pixels: Pixels,
    /// On the clipboard (rather than still being written).
    landed: bool,
}

/// A copy for the worker to put on the clipboard.
struct CopyJob {
    id: u64,
    pixels: Pixels,
    dir: PathBuf,
}

/// A new copy id, past the time so that no earlier launch's copy has it.
fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().and_then(|d| u64::try_from(d.as_millis()).ok()).unwrap_or(0);
    NEXT.fetch_max(now, Ordering::Relaxed);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn latest() -> std::sync::MutexGuard<'static, Option<Latest>> {
    LATEST.lock().unwrap_or_else(PoisonError::into_inner)
}

/// PhotoCraft's `clipboard_set_image` service: hands the copy to the worker and returns.
pub fn set_image(width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if CLASS.get().is_none() {
        return Err("the clipboard isn't ready".into());
    }
    let dir = crate::files::cache_dir().ok_or("no cache folder")?.join(DIR);
    let sender = worker().ok_or("the clipboard worker didn't start")?;
    let (id, pixels) = (next_id(), Arc::new((width, height, rgba.to_vec())));
    *latest() = Some(Latest { id, pixels: pixels.clone(), landed: false });
    sender.send(CopyJob { id, pixels, dir }).map_err(|_| "the clipboard worker stopped".to_string())
}

/// PhotoCraft's `clipboard_get_image` service, called on an explicit paste.
pub fn get_image() -> Option<(u32, u32, Vec<u8>)> {
    let ours = latest().clone();
    if let Some(copy) = ours.as_ref().filter(|c| !c.landed) {
        return Some((*copy.pixels).clone());
    }
    let path = match clipboard_file() {
        Ok(path) => path.map(PathBuf::from),
        Err(e) => {
            log::warn!("couldn't read the clipboard: {e}");
            return ours.map(|c| (*c.pixels).clone());
        }
    };
    if let Some(copy) = ours {
        if path.as_deref().and_then(Path::file_name).is_some_and(|n| *n == *clipboard::copy_name(copy.id)) {
            return Some((*copy.pixels).clone());
        }
        // Something else was copied since: our copy is gone from the clipboard for good.
        let mut latest = latest();
        if latest.as_ref().is_some_and(|l| l.id == copy.id) {
            *latest = None;
        }
    }
    clipboard::read_image(&path?)
}

/// A memory warning: drops the latest copy's pixels once it is on the clipboard (a paste decodes
/// its PNG then). A copy still on its way keeps them, since only they hold it yet. Returns the
/// bytes released.
pub fn release_memory() -> usize {
    let mut latest = latest();
    match latest.as_ref() {
        Some(copy) if copy.landed => {
            let bytes = copy.pixels.2.len();
            *latest = None;
            bytes
        }
        _ => 0,
    }
}

/// The worker's queue, started on the first copy.
fn worker() -> Option<&'static mpsc::Sender<CopyJob>> {
    static SENDER: OnceLock<Option<mpsc::Sender<CopyJob>>> = OnceLock::new();
    SENDER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel();
            match std::thread::Builder::new().name("clipboard".into()).spawn(move || run(&receiver)) {
                Ok(_) => Some(sender),
                Err(e) => {
                    log::warn!("couldn't start the clipboard worker: {e}");
                    None
                }
            }
        })
        .as_ref()
}

fn run(receiver: &mpsc::Receiver<CopyJob>) {
    while let Ok(first) = receiver.recv() {
        // Copies made while the last one was being written: only the newest still matters.
        let copy = receiver.try_iter().last().unwrap_or(first);
        let result = put(&copy);
        let mut latest = latest();
        if latest.as_ref().is_some_and(|l| l.id == copy.id) {
            match result {
                Ok(()) => {
                    if let Some(l) = latest.as_mut() {
                        l.landed = true;
                    }
                }
                Err(e) => {
                    log::warn!("couldn't put the copy on the clipboard: {e}");
                    *latest = None;
                }
            }
        }
    }
}

fn put(copy: &CopyJob) -> Result<(), String> {
    let (width, height, rgba) = &*copy.pixels;
    let path = clipboard::write_copy(&copy.dir, copy.id, *width, *height, rgba)?;
    if !set_clipboard_file(&path)? {
        return Err("MainActivity couldn't set the clipboard".into());
    }
    clipboard::remove_other_copies(&copy.dir, &clipboard::copy_name(copy.id));
    Ok(())
}

/// Runs `f` with `MainActivity`'s class, on any thread. A Java exception is logged and cleared.
fn with_class<T>(f: impl FnOnce(&mut Env, &Global<JClass<'static>>) -> jni::errors::Result<T>) -> Result<T, String> {
    let class = CLASS.get().ok_or("MainActivity didn't register its clipboard methods")?;
    let vm = JavaVM::singleton().map_err(|e| e.to_string())?;
    vm.attach_current_thread(|env| -> jni::errors::Result<T> {
        let result = f(env, class);
        if env.exception_check() {
            env.exception_describe();
            env.exception_clear();
        }
        result
    })
    .map_err(|e| e.to_string())
}

/// `static String clipboardFile()`: the file holding the clipboard's image, or null.
fn clipboard_file() -> Result<Option<String>, String> {
    with_class(|env, class| {
        let value = env.call_static_method(class, jni_str!("clipboardFile"), jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
        let path = env.cast_local::<JString>(value)?;
        if path.is_null() { Ok(None) } else { path.try_to_string(env).map(Some) }
    })
}

/// `static boolean setClipboardFile(String path)`: puts the PNG at `path` on the clipboard.
fn set_clipboard_file(path: &Path) -> Result<bool, String> {
    with_class(|env, class| {
        let path = JString::from_str(env, path.to_string_lossy())?;
        env.call_static_method(class, jni_str!("setClipboardFile"), jni_sig!("(Ljava/lang/String;)Z"), &[JValue::Object(&path)])?.z()
    })
}

/// `private static native void nativeClipboardInit()`: keeps `MainActivity`'s class for the calls
/// above.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
extern "system" fn Java_dev_photocraftmobile_app_MainActivity_nativeClipboardInit<'caller>(mut env: EnvUnowned<'caller>, class: JClass<'caller>) {
    env.with_env(|env| -> Result<(), jni::errors::Error> {
        if CLASS.get().is_none() {
            let _ = CLASS.set(env.new_global_ref(class)?);
        }
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>()
}
