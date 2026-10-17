//! Sharing the open image with other apps: the system's share sheet, or the photo gallery.
//!
//! Actions › Quick › Share image… and Save to Gallery export the active document as a PNG
//! (flattened, as File › Export does) on a worker thread, so a large image doesn't stop the app,
//! into a new folder under the sharing folder. The platform ([`ShareFn`]) then takes the file.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use photocraft_doc::Document;

/// Where a shared image goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareTarget {
    /// The system's share sheet: another app, a message, an e-mail.
    Send,
    /// The device's photo gallery.
    Gallery,
}

/// Hands an exported image file to the platform. Called on the worker thread.
pub type ShareFn = fn(ShareTarget, &Path);

/// The PNG file name for a document called `name`: its name without the extension, and nothing a
/// file name can't hold.
pub fn file_name(name: &str) -> String {
    // By hand rather than with `Path`, which reads `a:b` as a drive on Windows.
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = match base.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => base,
    };
    let clean: String = stem.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '\t' | '\r' | '\n') { '_' } else { c }).collect();
    let clean = clean.trim();
    format!("{}.png", if clean.is_empty() { "Image" } else { clean })
}

/// Writes `doc`, flattened, as a PNG into `folder`; returns the file.
pub fn export_png(doc: &Document, folder: &Path) -> Result<PathBuf, String> {
    let path = folder.join(file_name(&doc.name));
    let exported = photocraft_io::export(doc, "png", &photocraft_io::ExportOptions::default()).map_err(|e| e.to_string())?;
    crate::services::write_atomic(&path, &exported.bytes)?;
    Ok(path)
}

/// One export at a time, on its way to the platform.
pub struct Sharing {
    dir: PathBuf,
    share: ShareFn,
    running: Option<Receiver<Result<(), String>>>,
}

impl Sharing {
    /// Exports go into folders under `dir` (one per export, so an app still reading an earlier
    /// file keeps it), and then to `share`.
    pub fn new(dir: PathBuf, share: ShareFn) -> Self {
        Self { dir, share, running: None }
    }

    /// An export is still running.
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Exports `doc` and hands it to the platform for `target`, calling `wake` when done. Refused
    /// while another export runs.
    pub fn start(&mut self, doc: Arc<Document>, target: ShareTarget, wake: impl Fn() + Send + 'static) -> Result<(), String> {
        if self.busy() {
            return Err("an image is already being exported".into());
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or_default();
        let folder = self.dir.join(stamp.to_string());
        let share = self.share;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("photocraft-mobile-share".into())
            .spawn(move || {
                let result = export_png(&doc, &folder).map(|path| share(target, &path));
                let _ = tx.send(result);
                wake();
            })
            .map_err(|e| e.to_string())?;
        self.running = Some(rx);
        Ok(())
    }

    /// How the export ended, once it has.
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        let rx = self.running.as_ref()?;
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("the export stopped".into()),
        };
        self.running = None;
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_named_after_the_document() {
        assert_eq!(file_name("Sunset.psd"), "Sunset.png");
        assert_eq!(file_name("Untitled-1"), "Untitled-1.png");
        assert_eq!(file_name("a:b\tc.pcraft"), "a_b_c.png");
        assert_eq!(file_name(""), "Image.png");
        assert_eq!(file_name(" .psd"), "Image.png");
    }
}
