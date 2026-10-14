use super::*;
use photocraft_ui_egui::Services;
use std::cell::RefCell;
use std::rc::Rc;

fn shell() -> ModeShell {
    ModeShell::new(PhotocraftApp::new(photocraft_engine::Session::new(), Services::default()), Modes::default())
}

#[test]
fn every_mode_tool_is_a_photocraft_tool() {
    for mode in Mode::ALL {
        for name in mode.tools() {
            let tool = Tool::from_name(name).unwrap_or_else(|| panic!("{mode:?} lists {name}, which PhotoCraft doesn't have"));
            assert_eq!(format!("{tool:?}"), name, "{name} resolves to another tool");
        }
    }
}

#[test]
fn every_photocraft_tool_belongs_to_some_mode() {
    for name in tool_names() {
        assert!(Mode::ALL.into_iter().any(|m| m.shows(&name)), "{name} is in no mode: it could never be reached");
    }
}

#[test]
fn switching_sets_workspace_toolbar_and_tool() {
    let ctx = egui::Context::default();
    let mut s = shell();
    s.switch_to(&ctx, Mode::Vector).unwrap();
    assert_eq!(s.app.ui.tool, Tool::Pen);
    assert_eq!(s.app.ui.workspace, "Graphic and Web");
    let hidden = &s.app.session.prefs().toolbar.hidden;
    assert!(hidden.iter().any(|t| t == "Brush"));
    assert!(!hidden.iter().any(|t| t == "Pen" || t == "Move"));

    s.switch_to(&ctx, Mode::Pixel).unwrap();
    assert_eq!(s.app.ui.tool, Tool::Brush);
    assert_eq!(s.app.ui.workspace, "Painting");
    assert!(s.app.session.prefs().toolbar.hidden.iter().any(|t| t == "Pen"));

    s.switch_to(&ctx, Mode::Photo).unwrap();
    assert_eq!(s.app.ui.tool, Tool::SpotHealing);
    assert_eq!(s.app.ui.workspace, "Photography");
}

#[test]
fn the_last_tool_comes_back_with_its_mode() {
    let ctx = egui::Context::default();
    let mut s = shell();
    s.switch_to(&ctx, Mode::Vector).unwrap();
    s.app.ui.tool = Tool::Rectangle;
    s.switch_to(&ctx, Mode::Pixel).unwrap();
    s.app.ui.tool = Tool::Eraser;
    s.switch_to(&ctx, Mode::Vector).unwrap();
    assert_eq!(s.app.ui.tool, Tool::Rectangle);
    s.switch_to(&ctx, Mode::Pixel).unwrap();
    assert_eq!(s.app.ui.tool, Tool::Eraser);
}

#[test]
fn each_switch_is_saved() {
    let ctx = egui::Context::default();
    let saved: Rc<RefCell<Vec<Option<Mode>>>> = Rc::default();
    let sink = saved.clone();
    let mut s = shell().on_change(Box::new(move |m| sink.borrow_mut().push(m.current)));
    s.switch_to(&ctx, Mode::Photo).unwrap();
    s.switch_to(&ctx, Mode::Vector).unwrap();
    assert_eq!(*saved.borrow(), [Some(Mode::Photo), Some(Mode::Vector)]);
}

fn press(ctx: &egui::Context, s: &mut ModeShell, key: Key, modifiers: Modifiers) {
    let event = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let raw = egui::RawInput { events: vec![event], ..Default::default() };
    let mut output = ctx.run_ui(raw, |ui| s.shortcuts(ui.ctx()));
    output.textures_delta.clear();
}

#[test]
fn alt_digits_switch_modes() {
    let ctx = egui::Context::default();
    let mut s = shell();
    press(&ctx, &mut s, Key::Num1, Modifiers::ALT);
    assert_eq!(s.modes.current, Some(Mode::Vector));
    press(&ctx, &mut s, Key::Num3, Modifiers::ALT);
    assert_eq!(s.modes.current, Some(Mode::Photo));
    // A plain digit is PhotoCraft's (opacity), not a mode switch.
    press(&ctx, &mut s, Key::Num2, Modifiers::NONE);
    assert_eq!(s.modes.current, Some(Mode::Photo));
}

#[test]
fn ink_reads_on_light_and_dark_fills() {
    assert_eq!(ink_on(Color32::from_rgb(0, 200, 230)), Color32::from_gray(16));
    assert_eq!(ink_on(Color32::from_rgb(30, 30, 30)), Color32::WHITE);
}

#[test]
fn ui_scale_is_about_a_quarter_larger_than_the_system() {
    assert_eq!(ui_scale_for(2.0), Some("250"));
    assert_eq!(ui_scale_for(2.125), Some("250"));
    assert_eq!(ui_scale_for(1.0), Some("125"));
    assert_eq!(ui_scale_for(3.5), Some("300"), "capped at PhotoCraft's largest");
    assert_eq!(ui_scale_for(f32::NAN), None);
    assert_eq!(ui_scale_for(0.0), None);
}

#[test]
fn focus_hides_the_panels_and_brings_back_what_was_shown() {
    let mut s = shell();
    s.app.ui.panels.options_bar = false;
    s.toggle_focus();
    assert!(s.focused());
    let p = &s.app.ui.panels;
    assert!(!p.toolbar && !p.options_bar && !p.dock);
    s.toggle_focus();
    assert!(!s.focused());
    let p = &s.app.ui.panels;
    assert!(p.toolbar && !p.options_bar && p.dock, "the options bar was hidden before Focus and stays hidden");
}

/// A shell with a 400 × 300 document whose view is at 200 % on a 2× display.
fn shell_with_document() -> (ModeShell, usize) {
    let mut s = shell();
    s.app.run("file.new", json!({"width": 400, "height": 300})).unwrap();
    let idx = s.app.session.active_index().unwrap();
    while s.app.ui.views.len() <= idx {
        s.app.ui.views.push(Default::default());
    }
    let view = &mut s.app.ui.views[idx];
    view.zoom = 4.0;
    view.center = [200.0, 150.0];
    view.rotation = 0.0;
    (s, idx)
}

#[test]
fn a_finger_drag_moves_the_image_with_the_finger() {
    let (mut s, idx) = shell_with_document();
    // 2 document pixels per point: 30 points right is 15 pixels, so the centre moves 15 left.
    s.pan_view(2.0, vec2(30.0, -10.0));
    assert_eq!(s.app.ui.views[idx].center, [185.0, 155.0]);
    // A view turned 90° clockwise: dragging right moves the image up in document space.
    s.app.ui.views[idx].rotation = 90.0;
    s.app.ui.views[idx].center = [200.0, 150.0];
    s.pan_view(2.0, vec2(30.0, 0.0));
    let c = s.app.ui.views[idx].center;
    assert!((c[0] - 200.0).abs() < 1e-3 && (c[1] - 165.0).abs() < 1e-3, "{c:?}");
    // Nothing happens for nonsense input.
    s.pan_view(0.0, vec2(30.0, 0.0));
    s.pan_view(2.0, vec2(f32::NAN, 0.0));
    let c2 = s.app.ui.views[idx].center;
    assert_eq!(c, c2);
}

fn touch_event(id: u64, phase: egui::TouchPhase, x: f32, y: f32) -> egui::Event {
    egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase, pos: egui::pos2(x, y), force: Some(1.0) }
}

#[test]
fn fingers_on_the_canvas_pan_through_the_input_hook_and_the_pen_does_not() {
    use eframe::App;
    let (s, idx) = shell_with_document();
    let mut s = s.with_touch_kind(Box::new(|id| Some(if id == 7 { ToolKind::Pen } else { ToolKind::Finger })));
    s.app.last_canvas_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0));
    let ctx = egui::Context::default();
    let feed = |s: &mut ModeShell, events: Vec<egui::Event>| {
        let mut raw = egui::RawInput { events, time: Some(0.0), ..Default::default() };
        s.raw_input_hook(&ctx, &mut raw);
        s.apply_gesture(&ctx);
        raw.events
    };
    let left = feed(&mut s, vec![touch_event(1, egui::TouchPhase::Start, 100.0, 100.0), egui::Event::PointerMoved(egui::pos2(100.0, 100.0))]);
    assert!(!left.iter().any(|e| matches!(e, egui::Event::Touch { .. } | egui::Event::PointerMoved(_))), "{left:?}");
    feed(&mut s, vec![touch_event(1, egui::TouchPhase::Move, 140.0, 100.0)]);
    // egui::Context::default() is 1 point per pixel: 40 points at zoom 4 is 10 document pixels.
    assert_eq!(s.app.ui.views[idx].center, [190.0, 150.0]);
    feed(&mut s, vec![touch_event(1, egui::TouchPhase::End, 140.0, 100.0)]);
    let pen = feed(&mut s, vec![touch_event(7, egui::TouchPhase::Start, 100.0, 100.0)]);
    assert_eq!(pen.len(), 1, "the pen's touch reaches PhotoCraft");
    assert_eq!(s.app.ui.views[idx].center, [190.0, 150.0]);
}

#[test]
fn turning_finger_navigation_off_is_saved() {
    let saved: Rc<RefCell<Vec<bool>>> = Rc::default();
    let sink = saved.clone();
    let mut s = shell().with_settings(ShellSettings::default(), Box::new(move |st| sink.borrow_mut().push(st.finger_navigates)));
    s.set_finger_navigates(false);
    assert!(!s.settings.finger_navigates);
    assert_eq!(*saved.borrow(), [false]);
}

#[test]
fn without_a_document_a_finger_taps_the_start_screen() {
    use eframe::App;
    let mut s = shell().with_touch_kind(Box::new(|_| Some(ToolKind::Finger)));
    s.app.last_canvas_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0));
    let ctx = egui::Context::default();
    let mut raw = egui::RawInput { events: vec![touch_event(1, egui::TouchPhase::Start, 300.0, 300.0)], time: Some(0.0), ..Default::default() };
    s.raw_input_hook(&ctx, &mut raw);
    assert_eq!(raw.events.len(), 1, "the touch reaches PhotoCraft's start screen buttons");
}

/// One frame of a real PhotoCraft value field with `events`, returning its widget id.
fn value_field_frame(ctx: &egui::Context, value: &mut f32, events: Vec<egui::Event>) -> egui::Id {
    let mut id = egui::Id::NULL;
    let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        id = photocraft_ui_egui::widgets::value_field(ui, value, 0.0..=100.0, "%", 60.0).id;
    });
    out.textures_delta.clear();
    id
}

#[test]
fn one_arrow_key_is_one_step_in_a_photocraft_value_field() {
    let ctx = egui::Context::default();
    PhotocraftApp::setup_context(&ctx, Default::default());
    let mut v = 100.0;
    let id = value_field_frame(&ctx, &mut v, vec![]);
    let id = {
        let _ = value_field_frame(&ctx, &mut v, vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        value_field_frame(&ctx, &mut v, vec![])
    };
    ctx.memory_mut(|m| m.request_focus(id));
    let key = |pressed| egui::Event::Key { key: Key::ArrowDown, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    value_field_frame(&ctx, &mut v, vec![key(true), key(false)]);
    assert_eq!(v, 99.0, "focused: {:?}", ctx.memory(|m| m.focused()));
}
