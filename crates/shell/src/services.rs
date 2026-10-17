//! Platform services every front-end shares: codecs, crash-safe writes, crash recovery, and
//! settings kept in one folder of PhotoCraft Mobile's own (never PhotoCraft's, since switching
//! modes rewrites the toolbar). A front-end adds what is platform-specific, such as file dialogs.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use photocraft_codecs::{ChannelLayout, EncodeOptions, Image};
use photocraft_doc::Document;
use photocraft_format::RecoveryStore;
use photocraft_mobile_modes::Modes;
use photocraft_ui_egui::{ExportSettings, Recoverable, Services};

const PREFS: &str = "preferences.json";
const MODES: &str = "modes.json";
const SHELL: &str = "shell.json";
/// The folder (in the settings folder) that autosaves for crash recovery go to.
pub const RECOVERY: &str = "recovery";

/// The shell's own settings (PhotoCraft keeps its preferences in `preferences.json`).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShellSettings {
    /// On a pen tablet, fingers on the canvas pan, pinch and tap undo/redo instead of painting.
    #[serde(default = "yes")]
    pub finger_navigates: bool,
    /// Keep PhotoCraft's options bar under the top bar for every tool. Off, it only shows for
    /// tools whose settings the side bar doesn't cover (selections, shapes, type, crop…).
    #[serde(default)]
    pub options_bar: bool,
    /// The colour popover's picker.
    #[serde(default)]
    pub color_selector: ColorSelector,
}

/// How the colour popover picks a colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorSelector {
    /// A hue ring around a saturation / brightness square.
    #[default]
    Square,
    /// A hue ring around a hue / white / black triangle, with hue, saturation and value bars
    /// below (Krita's Advanced Color Selector).
    Triangle,
}

fn yes() -> bool {
    true
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self { finger_navigates: true, options_bar: false, color_selector: ColorSelector::Square }
    }
}

/// The saved shell settings in `dir`; a missing or unreadable file starts from the defaults.
pub fn load_settings(dir: Option<&Path>) -> ShellSettings {
    dir.and_then(|d| std::fs::read_to_string(d.join(SHELL)).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Saves the shell settings into `dir`, logging (not failing) when that isn't possible.
pub fn save_settings(dir: Option<&Path>, settings: &ShellSettings) {
    let result =
        serde_json::to_vec_pretty(settings).map_err(|e| e.to_string()).and_then(|bytes| write_atomic(&dir.ok_or("no settings folder")?.join(SHELL), &bytes));
    if let Err(e) = result {
        log::warn!("couldn't save the shell settings: {e}");
    }
}

/// Whether PhotoCraft has saved preferences in `dir` yet (false on a first run).
pub fn has_preferences(dir: Option<&Path>) -> bool {
    dir.is_some_and(|d| d.join(PREFS).is_file())
}

/// Crash-safe write (temp file, fsync, rename), creating the folder first.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    photocraft_format::atomic_write(path, bytes).map_err(|e| e.to_string())
}

/// The saved modes in `dir`; a missing or unreadable file starts fresh.
pub fn load_modes(dir: Option<&Path>) -> Modes {
    dir.and_then(|d| std::fs::read_to_string(d.join(MODES)).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Saves the modes into `dir`, logging (not failing) when that isn't possible.
pub fn save_modes(dir: Option<&Path>, modes: &Modes) {
    let result =
        serde_json::to_vec_pretty(modes).map_err(|e| e.to_string()).and_then(|bytes| write_atomic(&dir.ok_or("no settings folder")?.join(MODES), &bytes));
    if let Err(e) = result {
        log::warn!("couldn't save the modes: {e}");
    }
}

pub fn export_options(settings: &ExportSettings) -> photocraft_io::ExportOptions {
    let mut opts = photocraft_io::ExportOptions::default();
    if let Some(q) = settings.jpeg_quality {
        opts.encode.jpeg_quality = q;
    }
    opts.encode.webp_lossless = settings.webp_lossless;
    if let Some(q) = settings.webp_quality {
        opts.encode.webp_quality = q;
    }
    opts.tiff_layers = settings.tiff_layers;
    opts.xmp = if settings.xmp_all { photocraft_io::XmpEmbed::All } else { photocraft_io::XmpEmbed::None };
    opts.tga_bits = settings.tga_bits;
    opts.image_previews = settings.image_previews;
    opts.psd_maximize_compatibility = settings.psd_maximize_compatibility;
    opts
}

type SharedRecovery = Rc<RefCell<Option<RecoveryStore>>>;

/// Runs `f` on the recovery store. Without a settings folder there is no store, and the result is
/// `Err`. The service closures never call one another, so the store is never borrowed twice.
fn with_store<R>(store: &SharedRecovery, f: impl FnOnce(&mut RecoveryStore) -> R) -> Result<R, String> {
    let mut slot = store.try_borrow_mut().map_err(|_| "crash recovery is busy".to_string())?;
    Ok(f(slot.as_mut().ok_or("no settings folder")?))
}

/// Crash recovery, as in PhotoCraft's desktop app. PhotoCraft autosaves documents with unsaved
/// changes (Preferences › File Handling) into `<config_dir>/recovery` in the background, and at the
/// next launch it offers back what a crash left there. This matters most on Android, which may end
/// an app in the background at any time to free memory.
pub fn recovery(config_dir: Option<&Path>) -> Services {
    let dir = config_dir.map(|d| d.join(RECOVERY));
    let recover_dir = dir.clone();
    let store: SharedRecovery = Rc::new(RefCell::new(dir.map(RecoveryStore::new)));
    let (s1, s2, s3) = (store.clone(), store.clone(), store.clone());
    Services {
        autosave: Some(Box::new(move |doc: &Arc<Document>, revision: u64, path: Option<&str>| {
            with_store(&s1, |s| s.autosave_checked(doc, revision, path.map(str::to_string)))?
        })),
        autosave_results: Some(Box::new(move || with_store(&s2, |s| s.take_completed()).unwrap_or_default())),
        discard_autosave: Some(Box::new(move |id: u64| {
            if let Err(e) = with_store(&s3, |s| s.discard(id).map_err(|e| e.to_string())).and_then(|r| r) {
                log::warn!("couldn't drop the recovery data of document {id}: {e}");
            }
        })),
        recover: Some(Box::new(move || {
            let Some(dir) = recover_dir.as_ref() else { return Vec::new() };
            photocraft_format::list_recovery(dir)
                .into_iter()
                .map(|e| Recoverable {
                    key: e.info.key.clone(),
                    name: e.info.document_name.clone(),
                    path: e.info.original_path.clone(),
                    load: Box::new(move || photocraft_format::recover(&e).map_err(|e| e.to_string())),
                })
                .collect()
        })),
        adopt_autosave: Some(Box::new(move |id: u64, key: &str| {
            let _ = with_store(&store, |s| s.adopt(id, key));
        })),
        ..Default::default()
    }
}

/// Where the documents waiting in `<config_dir>/recovery` were last saved (the paths a cleanup
/// must keep).
pub fn recovery_paths(config_dir: Option<&Path>) -> Vec<String> {
    config_dir.map(|d| photocraft_format::list_recovery(&d.join(RECOVERY)).into_iter().filter_map(|e| e.info.original_path).collect()).unwrap_or_default()
}

/// Open, export, write, PNG encoding, preferences and crash recovery (in `config_dir`, when there
/// is one).
pub fn base(config_dir: Option<PathBuf>) -> Services {
    let recovery = recovery(config_dir.as_deref());
    let (load_dir, save_dir) = (config_dir.clone(), config_dir);
    Services {
        import: Some(Box::new(|name: &str, bytes: &[u8], max_svg_group_depth: usize| {
            photocraft_io::import_with_svg_group_depth(name, bytes, max_svg_group_depth).map(|r| (r.document, r.warnings)).map_err(|e| e.to_string())
        })),
        export: Some(Box::new(|doc: &Document, path: &str, settings: &ExportSettings| {
            photocraft_io::export(doc, path, &export_options(settings)).map(|r| (r.bytes, r.warnings)).map_err(|e| e.to_string())
        })),
        write: Some(Box::new(|path: &str, bytes: &[u8]| write_atomic(Path::new(path), bytes))),
        encode_png: Some(Box::new(|w, h, rgba| {
            let img = Image::from_u8(w, h, ChannelLayout::Rgba, rgba.to_vec()).map_err(|e| e.to_string())?;
            photocraft_codecs::encode(&img, photocraft_codecs::Format::Png, &EncodeOptions::default()).map_err(|e| e.to_string())
        })),
        load_prefs: Some(Box::new(move || std::fs::read_to_string(load_dir.as_ref()?.join(PREFS)).ok())),
        save_prefs: Some(Box::new(move |text: &str| write_atomic(&save_dir.as_ref().ok_or("no settings folder")?.join(PREFS), text.as_bytes()))),
        ..recovery
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_mobile_modes::Mode;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("photocraft-mobile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn modes_round_trip_through_the_settings_folder() {
        let dir = temp_dir("modes");
        let mut modes = Modes::default();
        modes.switch(Mode::Photo, "Move", &["Move", "Crop"]);
        save_modes(Some(&dir), &modes);
        assert_eq!(load_modes(Some(&dir)), modes);
        std::fs::write(dir.join(MODES), "not json").unwrap();
        assert_eq!(load_modes(Some(&dir)), Modes::default());
        assert_eq!(load_modes(None), Modes::default());
        save_modes(None, &modes); // logs, doesn't panic
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shell_settings_round_trip_and_default_to_finger_navigation() {
        let dir = temp_dir("shell");
        assert_eq!(load_settings(Some(&dir)), ShellSettings { finger_navigates: true, options_bar: false, color_selector: ColorSelector::Square });
        let changed = ShellSettings { finger_navigates: false, options_bar: true, color_selector: ColorSelector::Triangle };
        save_settings(Some(&dir), &changed);
        assert_eq!(load_settings(Some(&dir)), changed);
        assert!(std::fs::read_to_string(dir.join(SHELL)).unwrap().contains("\"triangle\""));
        std::fs::write(dir.join(SHELL), "{\"finger_navigates\":false}").unwrap();
        let older = load_settings(Some(&dir));
        assert!(!older.finger_navigates && !older.options_bar, "a field missing from an older file takes its default");
        assert_eq!(older.color_selector, ColorSelector::Square);
        std::fs::write(dir.join(SHELL), "{}").unwrap();
        assert!(load_settings(Some(&dir)).finger_navigates);
        assert!(!has_preferences(Some(&dir)));
        std::fs::write(dir.join(PREFS), "{}").unwrap();
        assert!(has_preferences(Some(&dir)));
        assert!(!has_preferences(None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autosaves_go_to_the_recovery_folder_and_come_back() {
        let dir = temp_dir("recovery");
        let mut app = photocraft_ui_egui::PhotocraftApp::new(photocraft_engine::Session::new(), base(Some(dir.clone())));
        app.run("file.new", serde_json::json!({"width": 64, "height": 48})).unwrap();
        let doc = app.session.active().unwrap().doc.clone();
        let s = &mut app.services;
        (s.autosave.as_mut().unwrap())(&doc, 3, Some("/picked/a.psd")).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut done = Vec::new();
        while done.is_empty() && std::time::Instant::now() < deadline {
            done = (s.autosave_results.as_mut().unwrap())();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(matches!(done.as_slice(), [(id, 3, Ok(()))] if *id == doc.id.0), "{done:?}");
        let found = (s.recover.as_mut().unwrap())();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path.as_deref(), Some("/picked/a.psd"));
        assert_eq!(recovery_paths(Some(&dir)), ["/picked/a.psd"]);
        // Saved or closed: the recovery data goes.
        (s.discard_autosave.as_mut().unwrap())(doc.id.0);
        assert!((s.recover.as_mut().unwrap())().is_empty());
        // No settings folder: autosaves fail (PhotoCraft shows it) and nothing comes back.
        let mut none = recovery(None);
        assert!((none.autosave.as_mut().unwrap())(&doc, 1, None).is_err());
        assert!((none.recover.as_mut().unwrap())().is_empty());
        assert!(recovery_paths(None).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preferences_live_in_the_settings_folder() {
        let dir = temp_dir("prefs");
        let mut s = base(Some(dir.clone()));
        (s.save_prefs.as_mut().unwrap())("{\"a\":1}").unwrap();
        assert_eq!((s.load_prefs.as_mut().unwrap())().as_deref(), Some("{\"a\":1}"));
        let mut none = base(None);
        assert!((none.save_prefs.as_mut().unwrap())("{}").is_err());
        assert_eq!((none.load_prefs.as_mut().unwrap())(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
