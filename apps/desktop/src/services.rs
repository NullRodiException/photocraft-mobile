//! Desktop platform services: the shared ones (`photocraft_mobile_shell::services`) plus file
//! dialogs (rfd), with settings in the per-user settings folder.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use photocraft_mobile_shell::ShellSettings;
use photocraft_mobile_shell::photocraft_mobile_modes::Modes;
use photocraft_mobile_shell::services;
use photocraft_ui_egui::{FileDialogAnswer, FileDialogReply, FileDialogRequest, Services};

/// What the Open dialog lists. Everything else can still be opened by choosing "All files".
const OPEN_EXTS: &[&str] = &[
    "pcraft", "psd", "psb", "ora", "kra", "pdn", "png", "jpg", "jpeg", "tif", "tiff", "webp", "gif", "bmp", "tga", "heic", "heif", "dng", "cr2", "cr3", "nef",
    "arw", "raf", "svg", "exr", "hdr", "af", "afdesign", "afphoto",
];

/// `PHOTOCRAFT_MOBILE_CONFIG_DIR`, else the platform's per-user settings folder.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PHOTOCRAFT_MOBILE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(dir.into());
    }
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))
    };
    base.map(|b| b.join("PhotoCraftMobile"))
}

pub fn load_modes() -> Modes {
    services::load_modes(config_dir().as_deref())
}

pub fn save_modes(modes: &Modes) {
    services::save_modes(config_dir().as_deref(), modes);
}

pub fn load_settings() -> ShellSettings {
    services::load_settings(config_dir().as_deref())
}

pub fn save_settings(settings: &ShellSettings) {
    services::save_settings(config_dir().as_deref(), settings);
}

/// Shows an Open, Folder or Save dialog without blocking the event loop: a helper thread waits
/// for rfd's async dialog and hands the answer to the app.
fn show_file_dialog(request: FileDialogRequest, parent: Option<&eframe::Frame>, reply: FileDialogReply) {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(parent) = parent {
        dialog = dialog.set_parent(parent);
    }
    let answer: Pin<Box<dyn Future<Output = Option<FileDialogAnswer>> + Send>> = match request {
        FileDialogRequest::Open { multiple, initial_dir, extensions } => {
            if let Some(dir) = initial_dir {
                dialog = dialog.set_directory(dir);
            }
            dialog = match extensions {
                Some(exts) => dialog.add_filter("Supported Files", &exts),
                None => dialog.add_filter("Images and documents", OPEN_EXTS),
            }
            .add_filter("All files", &["*"]);
            if multiple {
                let picked = dialog.pick_files();
                Box::pin(async move { picked.await.map(|files| FileDialogAnswer::Paths(files.iter().map(path_of).collect())) })
            } else {
                let picked = dialog.pick_file();
                Box::pin(async move { picked.await.map(|file| FileDialogAnswer::Paths(vec![path_of(&file)])) })
            }
        }
        FileDialogRequest::Folder { initial_dir } => {
            if let Some(dir) = initial_dir {
                dialog = dialog.set_directory(dir);
            }
            let picked = dialog.pick_folder();
            Box::pin(async move { picked.await.map(|folder| FileDialogAnswer::FolderTo(path_of(&folder))) })
        }
        FileDialogRequest::Save { suggested } => {
            let path = Path::new(&suggested);
            if let Some(name) = path.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
            if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                dialog = dialog.set_directory(dir);
            }
            let picked = dialog.save_file();
            Box::pin(async move { picked.await.map(|file| FileDialogAnswer::SaveTo(path_of(&file))) })
        }
    };
    // Without the thread the reply is dropped unanswered, which the app takes as Cancel.
    if let Err(e) = std::thread::Builder::new().name("file dialog".into()).spawn(move || reply.send(block_on(answer))) {
        log::error!("couldn't wait for the file dialog: {e}");
    }
}

fn path_of(file: &rfd::FileHandle) -> String {
    file.path().to_string_lossy().into_owned()
}

/// Runs `future` to completion on this thread, sleeping until it is woken.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    struct Unpark(std::thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        std::thread::park();
    }
}

pub fn native() -> Services {
    Services { file_dialog: Some(Box::new(show_file_dialog)), ..services::base(config_dir()) }
}
