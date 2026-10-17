//! Documents picked with Android's system file picker (the Storage Access Framework).
//!
//! The picker hands out content URIs, which PhotoCraft can neither open nor write. So an opened
//! document is copied into the app cache (`saf-open/<id>/`), and a save goes to a cache file
//! (`saf-save/<id>/`) that is then copied to the picked document. [`Links`] remembers which content
//! URI each of those cache files stands for, and whether the app may write to it. The links are
//! kept in `saf-links.json`, so a later save still reaches the picked document after Android
//! restarted the app.
//!
//! This is plain bookkeeping, with no Android API, so it is tested on any platform. The Android app
//! (`apps/android/src/files.rs`) calls it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::studio::tr;

/// The cache folder for the copies of opened documents (one folder per Open).
pub const OPEN_DIR: &str = "saf-open";
/// The cache folder for saves on their way to a picked document (one folder per Save As).
pub const SAVE_DIR: &str = "saf-save";
const FILE: &str = "saf-links.json";

/// A cache file and the picked document it stands for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// The cache file PhotoCraft opens and writes.
    pub path: String,
    /// The picked document's content URI.
    pub uri: String,
    /// The app may write the document; otherwise it can only read it.
    pub writable: bool,
}

/// Every cache file that stands for a picked document.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Links {
    #[serde(default)]
    links: Vec<Link>,
}

/// What a write to a path involves.
#[derive(Debug, PartialEq, Eq)]
pub enum WriteTarget<'a> {
    /// An ordinary file: just write it.
    File,
    /// The cache file of a picked document: write it, then copy it to this content URI.
    Document(&'a str),
}

/// The file name in `path`, for messages.
fn name_of(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// `path` lies in one of the picker's cache folders.
fn in_picker_cache(path: &str) -> bool {
    Path::new(path).components().any(|c| c.as_os_str() == OPEN_DIR || c.as_os_str() == SAVE_DIR)
}

impl Links {
    /// The links saved in `dir`; none when the file is missing or unreadable.
    pub fn load(dir: Option<&Path>) -> Links {
        dir.and_then(|d| std::fs::read_to_string(d.join(FILE)).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// Saves the links into `dir`, logging (not failing) when that isn't possible.
    pub fn save(&self, dir: Option<&Path>) {
        let result = serde_json::to_vec_pretty(self)
            .map_err(|e| e.to_string())
            .and_then(|bytes| crate::services::write_atomic(&dir.ok_or("no settings folder")?.join(FILE), &bytes));
        if let Err(e) = result {
            log::warn!("couldn't save the picked documents' links: {e}");
        }
    }

    /// The cache file `path` now stands for the document at `uri`.
    pub fn link(&mut self, path: &str, uri: &str, writable: bool) {
        self.links.retain(|l| l.path != path);
        self.links.push(Link { path: path.to_string(), uri: uri.to_string(), writable });
    }

    pub fn get(&self, path: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.path == path)
    }

    pub fn len(&self) -> usize {
        self.links.len()
    }

    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// What writing `path` involves. A picked document the app may only read refuses the write, and
    /// so does a copy in the picker's cache that no longer stands for any document. Writing the
    /// copy alone would look like a save while the document stayed as it was.
    pub fn write_target(&self, path: &str) -> Result<WriteTarget<'_>, String> {
        match self.get(path) {
            Some(link) if link.writable => Ok(WriteTarget::Document(&link.uri)),
            Some(_) => Err(format!(
                "{}: {}",
                name_of(path),
                tr(
                    "this file can only be read from here. Use Save As to keep your changes in a new file.",
                    "este arquivo só pode ser lido daqui. Use Salvar Como para guardar as alterações num arquivo novo."
                )
            )),
            None if in_picker_cache(path) => Err(format!(
                "{}: {}",
                name_of(path),
                tr(
                    "the app no longer reaches the original file. Use Save As to keep your changes.",
                    "o app não alcança mais o arquivo original. Use Salvar Como para guardar as alterações."
                )
            )),
            None => Ok(WriteTarget::File),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("photocraft-mobile-saf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const OPENED: &str = "/data/cache/saf-open/7/photo.psd";
    const SAVED: &str = "/data/cache/saf-save/9/new.png";

    #[test]
    fn a_picked_document_is_written_through_to_its_uri() {
        let mut links = Links::default();
        links.link(OPENED, "content://docs/photo", true);
        assert_eq!(links.write_target(OPENED), Ok(WriteTarget::Document("content://docs/photo")));
        // Linking the same file again replaces its document.
        links.link(OPENED, "content://docs/other", true);
        assert_eq!(links.len(), 1);
        assert_eq!(links.write_target(OPENED), Ok(WriteTarget::Document("content://docs/other")));
    }

    #[test]
    fn writes_that_wouldnt_reach_the_document_are_refused() {
        let mut links = Links::default();
        links.link(OPENED, "content://docs/photo", false);
        let read_only = links.write_target(OPENED).unwrap_err();
        assert!(read_only.starts_with("photo.psd:"), "{read_only}");
        // A copy in the picker's cache that lost its link: writing it would save nothing.
        assert!(links.write_target(SAVED).is_err());
        // Anything else is an ordinary file.
        assert_eq!(links.write_target("/data/files/recovery/x.pcraft"), Ok(WriteTarget::File));
        assert_eq!(links.write_target("/data/cache/elsewhere/saf-openings.png"), Ok(WriteTarget::File));
    }

    #[test]
    fn links_survive_a_restart() {
        let dir = temp_dir("links");
        let mut links = Links::default();
        links.link(OPENED, "content://docs/photo", true);
        links.link(SAVED, "content://docs/new", true);
        links.save(Some(&dir));
        assert_eq!(Links::load(Some(&dir)), links);
        std::fs::write(dir.join(FILE), "not json").unwrap();
        assert!(Links::load(Some(&dir)).is_empty());
        assert!(Links::load(None).is_empty());
        links.save(None); // logs, doesn't panic
        let _ = std::fs::remove_dir_all(&dir);
    }
}
