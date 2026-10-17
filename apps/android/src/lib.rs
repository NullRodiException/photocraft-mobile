//! PhotoCraft Mobile for Android.
//!
//! A `cdylib` loaded by `MainActivity` (a small AndroidX `GameActivity` subclass that adds immersive
//! full screen and palm rejection): `android_main` runs eframe on wgpu (Vulkan, or GLES where there
//! is no Vulkan) with the same [`ModeShell`] as the desktop app.
//! Build and install with `cargo xtask android --install --run` (see the README).
//!
//! Pen pressure: GameActivity copies only X and Y from each motion event unless more axes are
//! enabled, so without [`AndroidApp::enable_motion_axis`] every contact reaches winit (and from
//! there PhotoCraft's stylus input, as `egui::Event::Touch { force }`) at pressure 0.
//!
//! Fingers: `MainActivity` reports each contact's tool type through `nativeTouchTool`
//! (`touch_tools`), and the shell turns fingers on the canvas into pan, pinch and undo/redo taps.
//!
//! The S Pen: `MainActivity` sends every stylus event (`pen`): pressure, tilt and the side button
//! (held: the Eraser) go into PhotoCraft's stylus feed, and hovering moves the brush outline.
//!
//! Files: open and save go through the system file picker (`files`, the Storage Access Framework).
//! Other apps can send documents ("Open with", "Share"), and the image can go to their share sheet
//! or the gallery.
//!
//! The clipboard: images copied in the app paste in other apps, and the other way round
//! (`clipboard`). Links open in the browser.
//!
//! The on-screen keyboard: its text reaches egui through `ime`, since winit 0.30 drops
//! GameActivity's text input events.
//!
//! On any other target this crate is empty, so the workspace still builds everywhere.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "android")]
mod clipboard;
#[cfg(target_os = "android")]
mod files;
#[cfg(target_os = "android")]
mod ime;
#[cfg(target_os = "android")]
mod lifecycle;
#[cfg(target_os = "android")]
mod pen;
#[cfg(target_os = "android")]
mod touch_tools;

#[cfg(target_os = "android")]
mod android {
    use photocraft_engine::Session;
    use photocraft_mobile_shell::pen::{mem_total_mb, memory_budget_mb};
    use photocraft_mobile_shell::{ModeShell, services, ui_scale_for};
    use photocraft_ui_egui::{PhotocraftApp, Services};
    use serde_json::{Map, Value, json};
    use winit::platform::android::activity::AndroidApp;
    use winit::platform::android::activity::input::Axis;

    /// The platform services: the shared ones, the system file picker, saves that go on to the
    /// document the picker chose, documents other apps send, the clipboard and links.
    fn android_services(config_dir: Option<std::path::PathBuf>) -> Services {
        Services {
            file_dialog: Some(Box::new(crate::files::file_dialog)),
            os_events: Some(Box::new(crate::files::os_events)),
            write: Some(Box::new(|path: &str, bytes: &[u8]| {
                // The cache copy of a picked document is written, then copied to the document. A copy
                // that can't reach its document refuses the write rather than look saved.
                let document = crate::files::write_target(path)?;
                services::write_atomic(std::path::Path::new(path), bytes)?;
                if let Some(uri) = document {
                    crate::files::export(path, &uri);
                }
                Ok(())
            })),
            clipboard_set_image: Some(Box::new(crate::clipboard::set_image)),
            clipboard_get_image: Some(Box::new(crate::clipboard::get_image)),
            open_url: Some(Box::new(crate::files::open_url)),
            ..services::base(config_dir)
        }
    }

    /// First-run preferences for a touch device: a larger UI, and memory limits that leave room
    /// for the system and other apps (PhotoCraft's desktop defaults assume a computer's RAM).
    fn first_run_preferences(native_scale: Option<f32>) -> Value {
        let mut values = Map::new();
        if let Some(scale) = native_scale.and_then(ui_scale_for) {
            values.insert("interface.uiScale".into(), json!(scale));
        }
        if let Some(total) = std::fs::read_to_string("/proc/meminfo").ok().as_deref().and_then(mem_total_mb) {
            values.insert("performance.memoryUsageMb".into(), json!(memory_budget_mb(total)));
        }
        values.insert("performance.historyStates".into(), json!(30));
        values.insert("performance.effectCacheMb".into(), json!(256));
        // Crash recovery: autosave more often than the desktop's 10 minutes, since Android may end
        // the app in the background at any time.
        values.insert("fileHandling.autosaveMinutes".into(), json!(2));
        json!({ "values": values })
    }

    /// The GameActivity entry point (called by android-activity's glue on its own thread).
    // The unsafe part is only the unmangled export: the glue finds the function by this name.
    // No unsafe operation happens here.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    fn android_main(app: AndroidApp) {
        android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("photocraft-mobile"));
        // App-private storage: preferences and the saved mode.
        let config_dir = app.internal_data_path();
        // MainActivity names the build (versionName); a panic is written to crash.log with it.
        let version = std::env::var("PHOTOCRAFT_MOBILE_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
        log::info!("starting {version}; settings in {config_dir:?}");
        photocraft_mobile_shell::crash::install(config_dir.clone(), version);
        let crash_report = photocraft_mobile_shell::crash::take_report(config_dir.as_deref());
        // MainActivity reads the device's RAM for PhotoCraft's GPU memory budgets.
        match std::env::var("PHOTOCRAFT_RAM_MB") {
            Ok(mb) => log::info!("RAM for the GPU budgets: {mb} MB"),
            Err(_) => log::warn!("RAM unknown: PhotoCraft assumes 16 GB for its GPU budgets"),
        }
        // S Pen (and any stylus) pressure; see the crate docs.
        app.enable_motion_axis(Axis::Pressure);
        // The system scale (dpi / 160), for the first run's larger UI.
        let native_scale = app.config().density().map(|dpi| dpi as f32 / 160.0);
        let first_run = !services::has_preferences(config_dir.as_deref());
        // The picked documents' cache copies, from earlier launches too.
        crate::files::init(config_dir.clone());
        // The on-screen keyboard's text field, which winit doesn't read (see `ime`).
        let ime_app = app.clone();
        let mut options = eframe::NativeOptions { android_app: Some(app), renderer: eframe::Renderer::Wgpu, ..Default::default() };
        // The GPU's own texture and buffer size limits, as PhotoCraft's desktop and web apps ask
        // for. egui's default caps textures at 8192 px, which sends any larger image (a 200 MP
        // photo, a panorama) to the much slower CPU compositor.
        photocraft_ui_egui::gpu_canvas::use_adapter_limits(&mut options.wgpu_options.wgpu_setup);
        // One frame queued, not egui-wgpu's default two: the stroke follows the pen a frame sooner
        // (a drawing app's trade of a little throughput for latency).
        options.wgpu_options.surface = eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY;
        let result = eframe::run_native(
            "PhotoCraft Mobile",
            options,
            Box::new(move |cc| {
                PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
                let mut app = PhotocraftApp::new(Session::new(), android_services(config_dir.clone()));
                app.background_jobs = true;
                // The saved preferences (with the recent files) are loaded: drop the cache copies
                // that neither a recent file nor crash recovery will open again.
                let mut keep = app.session.prefs().file_handling.recent_files.clone();
                keep.extend(services::recovery_paths(config_dir.as_deref()));
                crate::files::clean_cache(&keep);
                if let Some(rs) = cc.wgpu_render_state.as_ref() {
                    app.set_wgpu(rs.clone());
                }
                if first_run {
                    let prefs = first_run_preferences(native_scale);
                    log::info!("first run: {prefs}");
                    if let Err(e) = app.run("prefs.set", prefs) {
                        log::warn!("couldn't set the first-run preferences: {e}");
                    }
                }
                // The dock's icon rail repeats the panel tabs: on a tablet the canvas gets that width.
                app.ui.panels.rail = false;
                crate::pen::install(app.stylus.feed.clone(), cc.egui_ctx.clone());
                crate::lifecycle::install(cc.egui_ctx.clone());
                crate::files::install(cc.egui_ctx.clone());
                crate::ime::install(ime_app, cc.egui_ctx.clone());
                let modes = services::load_modes(config_dir.as_deref());
                let settings = services::load_settings(config_dir.as_deref());
                let (modes_dir, settings_dir) = (config_dir.clone(), config_dir.clone());
                let mut shell = ModeShell::new(app, modes)
                    .on_change(Box::new(move |m| services::save_modes(modes_dir.as_deref(), m)))
                    .with_settings(settings, Box::new(move |s| services::save_settings(settings_dir.as_deref(), s)))
                    .with_touch_kind(Box::new(crate::touch_tools::tool_of))
                    .with_input_hook(Box::new(|raw, pixels_per_point| {
                        crate::pen::input_hook(raw, pixels_per_point);
                        crate::ime::input_hook(raw);
                    }))
                    .with_platform_events(Box::new(crate::lifecycle::events));
                if let Some(dir) = crate::files::share_dir() {
                    shell = shell.with_share(dir, crate::files::share_image);
                }
                if let Some(report) = crash_report {
                    log::warn!("the last run crashed: report in {}", report.display());
                    shell = shell.with_crash_report(report, Some(Box::new(crate::files::send_report)));
                }
                Ok(Box::new(shell))
            }),
        );
        if let Err(e) = result {
            log::error!("eframe stopped: {e}");
        }
    }
}
