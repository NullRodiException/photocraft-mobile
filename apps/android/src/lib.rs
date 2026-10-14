//! PhotoCraft Mobile for Android.
//!
//! A `cdylib` loaded by AndroidX `GameActivity` (no Java code of our own): `android_main` runs eframe on
//! wgpu (Vulkan, or GLES where there is no Vulkan) with the same [`ModeShell`] as the desktop app.
//! Build and install with `cargo xtask android --install --run` (see the README).
//!
//! On any other target this crate is empty, so the workspace still builds everywhere.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "android")]
mod android {
    use photocraft_engine::Session;
    use photocraft_mobile_shell::{ModeShell, services};
    use photocraft_ui_egui::PhotocraftApp;
    use winit::platform::android::activity::AndroidApp;

    /// The GameActivity entry point (called by android-activity's glue on its own thread).
    // The unsafe part is only the unmangled export: the glue finds the function by this name.
    // No unsafe operation happens here.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    fn android_main(app: AndroidApp) {
        android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("photocraft-mobile"));
        // App-private storage: preferences and the saved mode.
        let config_dir = app.internal_data_path();
        log::info!("starting; settings in {config_dir:?}");
        let options = eframe::NativeOptions { android_app: Some(app), renderer: eframe::Renderer::Wgpu, ..Default::default() };
        let result = eframe::run_native(
            "PhotoCraft Mobile",
            options,
            Box::new(move |cc| {
                PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
                let mut app = PhotocraftApp::new(Session::new(), services::base(config_dir.clone()));
                app.background_jobs = true;
                if let Some(rs) = cc.wgpu_render_state.as_ref() {
                    app.set_wgpu(rs.clone());
                }
                let modes = services::load_modes(config_dir.as_deref());
                let save_dir = config_dir.clone();
                Ok(Box::new(ModeShell::new(app, modes).on_change(Box::new(move |m| services::save_modes(save_dir.as_deref(), m)))))
            }),
        );
        if let Err(e) = result {
            log::error!("eframe stopped: {e}");
        }
    }
}
