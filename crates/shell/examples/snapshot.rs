//! Render PhotoCraft Mobile offscreen (no window) and save one PNG per mode.
//!
//! ```sh
//! cargo run --release -p photocraft-mobile-shell --example snapshot -- \
//!     --out shots --size 1280x800 --scale 1 --open photo.jpg
//! ```
//!
//! `--mode vector|pixel|photo` captures just that mode.

use photocraft_mobile_shell::ModeShell;
use photocraft_mobile_shell::photocraft_mobile_modes::{Mode, Modes};
use photocraft_ui_egui::{PhotocraftApp, Services};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(arg(&args, "--out").unwrap_or_else(|| "shots".into()));
    let (w, h) = arg(&args, "--size")
        .and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?))))
        .unwrap_or((1280.0, 800.0));
    let scale: f32 = arg(&args, "--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let open = arg(&args, "--open");
    let modes: Vec<Mode> = match arg(&args, "--mode") {
        Some(id) => vec![Mode::from_id(&id).ok_or_else(|| format!("unknown mode {id}"))?],
        None => Mode::ALL.to_vec(),
    };
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;

    let services = Services {
        import: Some(Box::new(|name: &str, bytes: &[u8], depth: usize| {
            photocraft_io::import_with_svg_group_depth(name, bytes, depth).map(|r| (r.document, r.warnings)).map_err(|e| e.to_string())
        })),
        ..Default::default()
    };
    let mut harness =
        egui_kittest::Harness::builder().with_size(egui::vec2(w, h)).with_pixels_per_point(scale).with_max_steps(64).wgpu().build_eframe(move |cc| {
            PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
            let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
            if let Some(rs) = cc.wgpu_render_state.as_ref() {
                app.set_wgpu(rs.clone());
            }
            if let Some(path) = &open
                && let Err(e) = app.open_path(path)
            {
                eprintln!("couldn't open {path}: {e}");
            }
            ModeShell::new(app, Modes::default())
        });
    harness.run_steps(6);
    for mode in modes {
        let ctx = harness.ctx.clone();
        harness.state_mut().switch_to(&ctx, mode)?;
        harness.run_steps(6);
        let path = out.join(format!("{}.png", mode.id()));
        harness.render()?.save(&path).map_err(|e| e.to_string())?;
        println!("{}", path.display());
    }
    Ok(())
}
