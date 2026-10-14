//! PhotoCraft Mobile on the desktop: PhotoCraft's editor with the Vector / Pixel / Photo
//! switcher. The quick way to develop and try the mobile shell before it runs on Android.
//!
//! ```sh
//! cargo run --release -p photocraft-mobile
//! cargo run --release -p photocraft-mobile -- photo.jpg
//! ```
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod services;

use photocraft_engine::Session;
use photocraft_mobile_shell::ModeShell;
use photocraft_ui_egui::PhotocraftApp;

fn main() -> eframe::Result {
    // The system font scan can take seconds: start it off the UI thread.
    photocraft_ui_egui::type_tool::warm_fonts();
    let files: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_title("PhotoCraft Mobile").with_inner_size([1280.0, 800.0]).with_min_inner_size([360.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "PhotoCraft Mobile",
        options,
        Box::new(move |cc| {
            PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
            let mut app = PhotocraftApp::new(Session::new(), services::native());
            app.background_jobs = true;
            if let Some(rs) = cc.wgpu_render_state.as_ref() {
                app.set_wgpu(rs.clone());
            }
            for file in &files {
                if let Err(e) = app.open_path(file) {
                    log::error!("couldn't open {file}: {e}");
                }
            }
            Ok(Box::new(ModeShell::new(app, services::load_modes()).on_change(Box::new(services::save_modes))))
        }),
    )
}
