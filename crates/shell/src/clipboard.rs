//! The plain part of the system clipboard bridge on Android (`apps/android/src/clipboard.rs`),
//! tested on any platform.
//!
//! Android's clipboard carries an image as a content URI, so a copy is written as a PNG file for
//! other apps to read ([`write_copy`]), and a paste reads a file: the app's own copy, or the image
//! another app put on the clipboard, copied into the cache ([`read_image`]).

use std::path::{Path, PathBuf};

use photocraft_codecs::{ChannelLayout, EncodeOptions, Format, Image};

/// The file name of copy `id`.
pub fn copy_name(id: u64) -> String {
    format!("copy-{id}.png")
}

/// Writes a copied RGBA8 image as a PNG into `dir`, as [`copy_name`]`(id)`; returns the file.
pub fn write_copy(dir: &Path, id: u64, width: u32, height: u32, rgba: &[u8]) -> Result<PathBuf, String> {
    let image = Image::from_u8(width, height, ChannelLayout::Rgba, rgba.to_vec()).map_err(|e| e.to_string())?;
    let png = photocraft_codecs::encode(&image, Format::Png, &EncodeOptions::default()).map_err(|e| e.to_string())?;
    let path = dir.join(copy_name(id));
    crate::services::write_atomic(&path, &png)?;
    Ok(path)
}

/// Deletes the copies in `dir` other than the file `keep`: only the one on the clipboard is still
/// read.
pub fn remove_other_copies(dir: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name != keep
            && name.starts_with("copy-")
            && let Err(e) = std::fs::remove_file(entry.path())
        {
            log::warn!("couldn't delete the old clipboard copy {name}: {e}");
        }
    }
}

/// The image in the file at `path` as RGBA8: its width, height and pixels. `None` when the file
/// isn't an image PhotoCraft decodes. As with files copied on the desktop, a TGA (which has no
/// magic number, so any file may look like one) counts only when its extension says so.
pub fn read_image(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = std::fs::read(path).ok()?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if photocraft_codecs::detect(&bytes)? == Format::Tga && photocraft_codecs::from_extension(ext) != Some(Format::Tga) {
        return None;
    }
    let image = photocraft_codecs::decode(&bytes).ok()?;
    Some((image.width(), image.height(), image.to_rgba8()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("photocraft-mobile-clipboard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_copy_pastes_back_the_same_pixels() {
        let dir = temp_dir("round-trip");
        // Opaque red, half-transparent green, and a transparent pixel that still has a colour.
        let rgba = [255, 0, 0, 255, 0, 255, 0, 128, 10, 20, 30, 0];
        let path = write_copy(&dir, 7, 3, 1, &rgba).unwrap();
        assert_eq!(path, dir.join("copy-7.png"));
        assert_eq!(read_image(&path), Some((3, 1, rgba.to_vec())));
        assert!(write_copy(&dir, 8, 2, 2, &rgba).is_err(), "fewer pixels than the size says");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_copy_on_the_clipboard_stays() {
        let dir = temp_dir("older");
        let px = [0, 0, 0, 255];
        for id in 1..=3 {
            write_copy(&dir, id, 1, 1, &px).unwrap();
        }
        std::fs::write(dir.join("paste.png"), b"kept").unwrap();
        remove_other_copies(&dir, &copy_name(2));
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["copy-2.png", "paste.png"]);
        remove_other_copies(&dir.join("missing"), "x"); // no folder: nothing to do
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_images_paste() {
        let dir = temp_dir("not-images");
        std::fs::create_dir_all(&dir).unwrap();
        let text = dir.join("paste.txt");
        std::fs::write(&text, "not an image").unwrap();
        assert_eq!(read_image(&text), None);
        assert_eq!(read_image(&dir.join("missing.png")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
