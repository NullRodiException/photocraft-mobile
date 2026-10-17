//! Render PhotoCraft Mobile offscreen (no window) and save one PNG per mode.
//!
//! ```sh
//! cargo run --release -p photocraft-mobile-shell --example snapshot -- \
//!     --out shots --size 1280x800 --scale 1 --open photo.jpg
//! ```
//!
//! `--mode vector|pixel|photo` captures just that mode. `--new 1600x1200` starts from a new
//! document with a few strokes on it. With a document, `--popover actions|adjustments|selection|
//! tools|brushes|color|pressure` also captures that popover open, and `--layers` the Layers panel.
//! `--triangle` uses the triangle colour selector, `--harmony analogous|complementary|monochromatic`
//! the colour popover's harmony.

use photocraft_mobile_shell::photocraft_mobile_modes::{Mode, Modes};
use photocraft_mobile_shell::{ColorSelector, Harmony, ModeShell, Popover, SidePanel};
use photocraft_ui_egui::{PhotocraftApp, Services};
use serde_json::json;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn size(s: &str) -> Option<(f32, f32)> {
    s.split_once('x').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?)))
}

fn popover(name: &str) -> Option<Popover> {
    Some(match name {
        "actions" => Popover::Actions,
        "adjustments" => Popover::Adjustments,
        "selection" => Popover::Selection,
        "tools" => Popover::Tools,
        "brushes" => Popover::Brushes,
        "color" => Popover::Color,
        "pressure" => Popover::Pressure,
        _ => return None,
    })
}

/// A stroke: its colour, size and points (fractions of the document).
type Sketch = ([f32; 3], f32, &'static [(f32, f32)]);

/// A few colourful strokes, so the canvas isn't empty.
fn sketch(app: &mut PhotocraftApp, w: f32, h: f32) {
    let strokes: [Sketch; 4] = [
        ([0.93, 0.36, 0.22], 60.0, &[(0.15, 0.7), (0.3, 0.45), (0.45, 0.6), (0.6, 0.35), (0.8, 0.5)]),
        ([0.2, 0.45, 0.95], 40.0, &[(0.2, 0.3), (0.4, 0.25), (0.55, 0.45), (0.75, 0.3)]),
        ([0.55, 0.85, 0.3], 80.0, &[(0.25, 0.85), (0.5, 0.75), (0.7, 0.85), (0.85, 0.7)]),
        ([0.95, 0.8, 0.2], 30.0, &[(0.1, 0.15), (0.3, 0.2), (0.5, 0.12), (0.9, 0.2)]),
    ];
    for (rgb, size, pts) in strokes {
        let points: Vec<[f32; 3]> = pts.iter().map(|&(x, y)| [x * w, y * h, 1.0]).collect();
        let hex = format!("#{:02x}{:02x}{:02x}", (rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
        if let Err(e) = app.run("paint.stroke", json!({ "points": points, "size": size, "color": hex, "opacity": 0.8 })) {
            eprintln!("stroke: {e}");
        }
    }
    let _ = app.run("tools.setColors", json!({ "foreground": "#2f7bf5" }));
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(arg(&args, "--out").unwrap_or_else(|| "shots".into()));
    let (w, h) = arg(&args, "--size").and_then(|s| size(&s)).unwrap_or((1280.0, 800.0));
    let scale: f32 = arg(&args, "--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let open = arg(&args, "--open");
    let new_doc = arg(&args, "--new").and_then(|s| size(&s));
    let pop = match arg(&args, "--popover") {
        Some(name) => Some(popover(&name).ok_or_else(|| format!("unknown popover {name}"))?),
        None => None,
    };
    let layers = args.iter().any(|a| a == "--layers");
    let triangle = args.iter().any(|a| a == "--triangle");
    let harmony = match arg(&args, "--harmony") {
        Some(id) => Some(Harmony::ALL.into_iter().find(|h| h.names().0.eq_ignore_ascii_case(&id)).ok_or_else(|| format!("unknown harmony {id}"))?),
        None => None,
    };
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
            if let Some((dw, dh)) = new_doc {
                match app.run("file.new", json!({ "width": dw, "height": dh, "background": "white" })) {
                    Ok(_) => sketch(&mut app, dw, dh),
                    Err(e) => eprintln!("couldn't make a document: {e}"),
                }
            }
            ModeShell::new(app, Modes::default())
        });
    harness.run_steps(6);
    for mode in modes {
        let ctx = harness.ctx.clone();
        harness.state_mut().switch_to(&ctx, mode)?;
        harness.state_mut().studio.side = layers.then_some(SidePanel::Layers);
        if triangle {
            harness.state_mut().settings.color_selector = ColorSelector::Triangle;
        }
        if let Some(h) = harmony {
            harness.state_mut().settings.harmony = h;
        }
        harness.run_steps(6);
        let ctx = harness.ctx.clone();
        let _ = photocraft_ui_egui::menus::invoke(&mut harness.state_mut().app, &ctx, "view.fitOnScreen", json!({}));
        harness.run_steps(6);
        let path = out.join(format!("{}.png", mode.id()));
        harness.render()?.save(&path).map_err(|e| e.to_string())?;
        println!("{}", path.display());
        if let Some(p) = pop {
            harness.state_mut().toggle_popover(p);
            harness.run_steps(8);
            let path = out.join(format!("{}-{}.png", mode.id(), format!("{p:?}").to_lowercase()));
            harness.render()?.save(&path).map_err(|e| e.to_string())?;
            println!("{}", path.display());
            harness.state_mut().studio.popover = None;
        }
    }
    Ok(())
}
