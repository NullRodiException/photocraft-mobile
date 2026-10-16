//! Measure the CPU cost of PhotoCraft Mobile's frames offscreen (no window): the shell and
//! PhotoCraft's `logic` + `ui`, egui's pass, and the tessellation, in the situations a tablet
//! spends its time in (idle, pen hovering, painting, a finger pan, each popover).
//!
//! ```sh
//! cargo run --release -p photocraft-mobile-shell --example frame_bench -- --frames 120
//! ```
//!
//! `--size 920x576 --scale 2.5` (the default) is a Galaxy Tab S9 FE in landscape at the first
//! run's UI scale. `--new 2400x1600` is the document painted on. Numbers are per frame: the mean
//! and the 95th percentile, in milliseconds. A phone or tablet core is several times slower than a
//! desktop one, so compare runs on the same machine.

use std::time::Instant;

use eframe::App as _;
use egui::{Event, PointerButton, Pos2, TouchDeviceId, TouchId, TouchPhase, pos2, vec2};
use photocraft_mobile_shell::photocraft_mobile_modes::{Mode, Modes};
use photocraft_mobile_shell::{ModeShell, Popover, SidePanel, ToolKind};
use photocraft_ui_egui::{PhotocraftApp, Services};
use serde_json::json;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn size(s: &str) -> Option<(f32, f32)> {
    s.split_once('x').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?)))
}

/// The shell, timing its `logic` + `ui`.
struct Timed {
    shell: ModeShell,
    last_ms: f64,
}

impl eframe::App for Timed {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let t = Instant::now();
        self.shell.logic(ctx, frame);
        self.last_ms = t.elapsed().as_secs_f64() * 1000.0;
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let t = Instant::now();
        self.shell.ui(ui, frame);
        self.last_ms += t.elapsed().as_secs_f64() * 1000.0;
    }
}

type Harness = egui_kittest::Harness<'static, Timed>;

/// Touch ids at or above this are fingers; below, the pen.
const FINGER: u64 = 100;

#[derive(Default)]
struct Stats {
    app: Vec<f64>,
    pass: Vec<f64>,
    tess: Vec<f64>,
    vertices: Vec<usize>,
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
}

fn p95(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s.get((s.len() as f64 * 0.95) as usize).or(s.last()).copied().unwrap_or(0.0)
}

/// One frame with `events`, through the shell's raw input hook as eframe would.
fn frame(h: &mut Harness, events: Vec<Event>, stats: &mut Stats) {
    let ctx = h.ctx.clone();
    let mut raw = std::mem::take(h.input_mut());
    raw.events.extend(events);
    h.state_mut().shell.raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    let t = Instant::now();
    h.step();
    let pass = t.elapsed().as_secs_f64() * 1000.0;
    let shapes = h.output().shapes.clone();
    let ppp = h.output().pixels_per_point;
    let t = Instant::now();
    let prims = ctx.tessellate(shapes, ppp);
    let tess = t.elapsed().as_secs_f64() * 1000.0;
    let verts = prims
        .iter()
        .map(|p| match &p.primitive {
            egui::epaint::Primitive::Mesh(m) => m.vertices.len(),
            egui::epaint::Primitive::Callback(_) => 0,
        })
        .sum();
    stats.app.push(h.state().last_ms);
    stats.pass.push(pass);
    stats.tess.push(tess);
    stats.vertices.push(verts);
}

fn report(name: &str, s: &Stats) {
    let verts = s.vertices.iter().sum::<usize>() / s.vertices.len().max(1);
    println!(
        "{name:<22} app {:>6.2} ms (p95 {:>6.2})   egui pass {:>6.2} ms (p95 {:>6.2})   tessellate {:>5.2} ms   {verts:>6} vertices",
        mean(&s.app),
        p95(&s.app),
        mean(&s.pass),
        p95(&s.pass),
        mean(&s.tess),
    );
}

fn canvas_center(h: &Harness) -> Pos2 {
    h.state().shell.app.last_canvas_rect.center()
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let (w, h) = arg(&args, "--size").and_then(|s| size(&s)).unwrap_or((920.0, 576.0));
    let scale: f32 = arg(&args, "--scale").and_then(|s| s.parse().ok()).unwrap_or(2.5);
    let (dw, dh) = arg(&args, "--new").and_then(|s| size(&s)).unwrap_or((2400.0, 1600.0));
    let frames: usize = arg(&args, "--frames").and_then(|s| s.parse().ok()).unwrap_or(120);
    let only = arg(&args, "--only");

    let mut harness: Harness =
        egui_kittest::Harness::builder().with_size(vec2(w, h)).with_pixels_per_point(scale).with_max_steps(64).with_step_dt(1.0 / 60.0).wgpu().build_eframe(
            move |cc| {
                PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
                let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
                if let Some(rs) = cc.wgpu_render_state.as_ref() {
                    app.set_wgpu(rs.clone());
                }
                if let Err(e) = app.run("file.new", json!({ "width": dw, "height": dh, "background": "white" })) {
                    eprintln!("couldn't make a document: {e}");
                }
                app.ui.panels.rail = false;
                let shell =
                    ModeShell::new(app, Modes::default()).with_touch_kind(Box::new(|id| Some(if id >= FINGER { ToolKind::Finger } else { ToolKind::Pen })));
                Timed { shell, last_ms: 0.0 }
            },
        );
    harness.run_steps(6);
    let ctx = harness.ctx.clone();
    harness.state_mut().shell.switch_to(&ctx, Mode::Pixel)?;
    harness.run_steps(6);
    let _ = photocraft_ui_egui::menus::invoke(&mut harness.state_mut().shell.app, &ctx, "view.fitOnScreen", json!({}));
    harness.run_steps(6);

    let run = |name: &str| only.as_deref().is_none_or(|o| name.contains(o));
    let mut time = Instant::now();
    let mut clock = || {
        let t = time.elapsed().as_secs_f64();
        time = Instant::now();
        t
    };
    let _ = clock();

    // Nothing happens: what a repaint costs when something asks for one.
    if run("idle") {
        let mut s = Stats::default();
        for _ in 0..frames {
            frame(&mut harness, Vec::new(), &mut s);
        }
        report("idle", &s);
        // When nothing happens, nothing should ask for the next frame.
        match harness.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay) {
            Some(delay) if delay < std::time::Duration::MAX => println!("{:<22} asks for another frame in {delay:?}", "idle"),
            _ => println!("{:<22} asks for no other frame", "idle"),
        }
    }

    // The pen hovering over the canvas: the brush outline follows it.
    if run("hover") {
        let mut s = Stats::default();
        let c = canvas_center(&harness);
        for i in 0..frames {
            let a = i as f32 / 12.0;
            frame(&mut harness, vec![Event::PointerMoved(c + vec2(a.cos() * 120.0, a.sin() * 80.0))], &mut s);
        }
        frame(&mut harness, vec![Event::PointerGone], &mut Stats::default());
        report("pen hover", &s);
    }

    // A brush stroke: the pen down on the canvas, moving, then lifted.
    if run("stroke") {
        let mut s = Stats::default();
        let c = canvas_center(&harness);
        let at = |i: usize| c + vec2((i as f32 / frames as f32 - 0.5) * 500.0, (i as f32 / 7.0).sin() * 120.0);
        let pen = |pos: Pos2, phase: TouchPhase| Event::Touch { device_id: TouchDeviceId(1), id: TouchId(1), phase, pos, force: Some(0.6) };
        frame(
            &mut harness,
            vec![
                Event::PointerMoved(at(0)),
                pen(at(0), TouchPhase::Start),
                Event::PointerButton { pos: at(0), button: PointerButton::Primary, pressed: true, modifiers: Default::default() },
            ],
            &mut Stats::default(),
        );
        for i in 1..frames {
            frame(&mut harness, vec![pen(at(i), TouchPhase::Move), Event::PointerMoved(at(i))], &mut s);
        }
        let end = at(frames - 1);
        frame(
            &mut harness,
            vec![
                pen(end, TouchPhase::End),
                Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Default::default() },
                Event::PointerGone,
            ],
            &mut Stats::default(),
        );
        report("brush stroke", &s);
        harness.run_steps(4);
    }

    // One finger panning the canvas.
    if run("pan") {
        let mut s = Stats::default();
        let c = canvas_center(&harness);
        let finger = |pos: Pos2, phase: TouchPhase| Event::Touch { device_id: TouchDeviceId(1), id: TouchId(FINGER), phase, pos, force: None };
        frame(&mut harness, vec![finger(c, TouchPhase::Start)], &mut Stats::default());
        for i in 1..frames {
            let p = c + vec2((i as f32 * 0.3).sin() * 60.0, (i as f32 * 0.2).cos() * 40.0);
            frame(&mut harness, vec![finger(p, TouchPhase::Move)], &mut s);
        }
        frame(&mut harness, vec![finger(c, TouchPhase::End)], &mut Stats::default());
        report("finger pan", &s);
        let _ = photocraft_ui_egui::menus::invoke(&mut harness.state_mut().shell.app, &ctx, "view.fitOnScreen", json!({}));
        harness.run_steps(4);
    }

    // Each popover open, the pointer moving over it (rows highlight under it).
    let popovers = [
        ("actions", Popover::Actions),
        ("adjustments", Popover::Adjustments),
        ("selection", Popover::Selection),
        ("tools", Popover::Tools),
        ("brushes", Popover::Brushes),
        ("color", Popover::Color),
        ("pressure", Popover::Pressure),
    ];
    for (name, p) in popovers {
        if !run(name) {
            continue;
        }
        harness.state_mut().shell.toggle_popover(p);
        harness.run_steps(4);
        let mut s = Stats::default();
        let r = harness.state().shell.app.last_canvas_rect;
        for i in 0..frames {
            let f = (i % 40) as f32 / 40.0;
            frame(&mut harness, vec![Event::PointerMoved(pos2(r.center().x + 100.0 * f, r.top() + 80.0 + 300.0 * f))], &mut s);
        }
        frame(&mut harness, vec![Event::PointerGone], &mut Stats::default());
        report(&format!("popover {name}"), &s);
        harness.state_mut().shell.studio.popover = None;
        harness.run_steps(2);
    }

    // The Layers panel open beside the canvas.
    if run("layers") {
        harness.state_mut().shell.studio.side = Some(SidePanel::Layers);
        harness.run_steps(4);
        let mut s = Stats::default();
        for _ in 0..frames {
            frame(&mut harness, Vec::new(), &mut s);
        }
        report("layers panel", &s);
        harness.state_mut().shell.studio.side = None;
    }
    // The Actions popover's search, which runs on every frame while a query is typed.
    if run("search") {
        use photocraft_ui_egui::{i18n, menus};
        let items = menus::menu_items(&harness.state().shell.app);
        let t = Instant::now();
        for _ in 0..frames {
            std::hint::black_box(menus::search_items(&items, "camada", i18n::current()));
        }
        println!("{:<22} {:>6.2} ms per search", "actions search", t.elapsed().as_secs_f64() * 1000.0 / frames as f64);
    }
    println!("total {:.1} s", clock());
    Ok(())
}
