use super::*;
use egui::Color32;
use photocraft_ui_egui::Services;
use std::cell::RefCell;
use std::rc::Rc;
use studio::{Act, needs_options_bar};

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
    use studio::ink_on;
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
fn photocraft_chrome_is_hidden_and_focus_hides_the_rest() {
    let mut s = shell();
    s.apply_chrome();
    let p = &s.app.ui.panels;
    assert!(!p.menu_bar && !p.toolbar && !p.status_bar && !p.rail && !p.dock, "PhotoCraft's own chrome is hidden");
    let v = &s.app.ui.view;
    assert!(v.hides_tabs() && !v.shows_scrollbars() && !v.hides_chrome(), "no tabs or scroll bars, but PhotoCraft's panels still draw");
    s.studio.side = Some(SidePanel::Layers);
    s.apply_chrome();
    let p = &s.app.ui.panels;
    assert!(p.dock && p.layers && !p.color && !p.history, "Layers opens the dock with just the Layers panel");
    s.app.ui.tool = Tool::RectMarquee;
    s.toggle_focus();
    assert!(s.focused());
    let p = &s.app.ui.panels;
    assert!(!p.dock && !p.options_bar);
    s.toggle_focus();
    assert!(!s.focused());
    let p = &s.app.ui.panels;
    assert!(p.dock && p.layers && p.options_bar, "leaving Focus brings back the Layers panel and the options bar");
}

#[test]
fn the_options_bar_shows_only_where_the_side_bar_falls_short() {
    let mut s = shell();
    s.app.ui.tool = Tool::Brush;
    assert!(!needs_options_bar(&s.app));
    s.apply_chrome();
    assert!(!s.app.ui.panels.options_bar);
    for tool in [Tool::RectMarquee, Tool::Type, Tool::Crop, Tool::Rectangle, Tool::Gradient, Tool::CloneStamp] {
        s.app.ui.tool = tool;
        assert!(needs_options_bar(&s.app), "{tool:?} needs its options");
    }
    s.app.ui.tool = Tool::Eraser;
    s.settings.options_bar = true;
    s.apply_chrome();
    assert!(s.app.ui.panels.options_bar, "the setting keeps it for every tool");
}

#[test]
fn a_panel_photocraft_opens_shows_in_the_dock_but_a_mode_switch_does_not() {
    let ctx = egui::Context::default();
    let mut s = shell();
    s.apply_chrome();
    // Window › History.
    s.app.ui.panels.history = true;
    s.apply_chrome();
    assert_eq!(s.studio.side, Some(SidePanel::Window));
    assert!(s.app.ui.panels.dock && s.app.ui.panels.history);
    // A mode lays out its workspace (which opens panels) without opening the dock.
    s.studio.side = None;
    s.apply_chrome();
    s.switch_to(&ctx, Mode::Photo).unwrap();
    s.apply_chrome();
    assert_eq!(s.studio.side, None);
    assert!(!s.app.ui.panels.dock);
}

fn press_at(x: f32, y: f32, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos: egui::pos2(x, y), button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

#[test]
fn a_press_outside_the_popover_closes_it_without_reaching_the_canvas() {
    let ctx = egui::Context::default();
    PhotocraftApp::setup_context(&ctx, Default::default());
    let mut s = shell();
    // Draw once so the top bar has its place.
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| s.top_bar(ui));
    out.textures_delta.clear();
    s.toggle_popover(Popover::Tools);
    assert_eq!(s.studio.popover, Some(Popover::Tools));
    let mut events = vec![press_at(400.0, 400.0, true), egui::Event::PointerMoved(egui::pos2(410.0, 400.0)), press_at(410.0, 400.0, false)];
    s.studio.filter(&mut events);
    assert!(events.is_empty(), "the closing press never reaches the canvas: {events:?}");
    assert_eq!(s.studio.popover, None);
    // On the top bar the press goes through (it may open another popover), but the button of
    // the popover it closed doesn't open it again.
    s.toggle_popover(Popover::Color);
    let mut events = vec![press_at(20.0, 10.0, true)];
    s.studio.filter(&mut events);
    assert_eq!(events.len(), 1);
    assert_eq!(s.studio.popover, None);
    s.toggle_popover(Popover::Color);
    assert_eq!(s.studio.popover, None, "the click that closed it doesn't reopen it");
    s.toggle_popover(Popover::Actions);
    assert_eq!(s.studio.popover, Some(Popover::Actions), "another button opens its popover");
}

#[test]
fn modify_borrows_the_eyedropper_and_gives_the_tool_back() {
    let ctx = egui::Context::default();
    let mut s = shell();
    s.app.ui.tool = Tool::Smudge;
    s.apply_act(&ctx, Act::Modify);
    assert_eq!(s.app.ui.tool, Tool::Eyedropper);
    s.apply_act(&ctx, Act::Modify);
    assert_eq!(s.app.ui.tool, Tool::Smudge);
}

#[test]
fn colors_from_the_disc_set_the_foreground() {
    let ctx = egui::Context::default();
    let mut s = shell();
    s.apply_act(&ctx, Act::Color([1.0, 0.0, 0.0]));
    let fg = s.app.session.tools.foreground;
    assert!((fg[0] - 1.0).abs() < 1e-3 && fg[1].abs() < 1e-3 && fg[2].abs() < 1e-3, "{fg:?}");
    s.toggle_popover(Popover::Color);
    s.apply_act(&ctx, Act::SwapColors);
    assert_ne!(s.app.session.tools.foreground, fg);
}

#[test]
fn layers_toggles_the_dock() {
    let ctx = egui::Context::default();
    let mut s = shell();
    s.apply_act(&ctx, Act::Layers);
    assert_eq!(s.studio.side, Some(SidePanel::Layers));
    s.apply_chrome();
    assert!(s.app.ui.panels.dock && s.app.ui.panels.layers);
    s.apply_act(&ctx, Act::Layers);
    s.apply_chrome();
    assert_eq!(s.studio.side, None);
    assert!(!s.app.ui.panels.dock);
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

#[test]
fn tool_grids_drop_the_word_tool() {
    use studio::short_tool_name;
    assert_eq!(short_tool_name("Move Tool"), "Move");
    assert_eq!(short_tool_name("Ferramenta Laço poligonal"), "Laço poligonal");
    assert_eq!(short_tool_name("Herramienta pincel"), "Pincel");
    assert_eq!(short_tool_name("Brush"), "Brush");
    assert_eq!(short_tool_name("Tool"), "Tool");
}

#[test]
fn the_triangle_maps_its_corners_to_hue_white_and_black() {
    use studio::{triangle_sv, triangle_weights};
    let (hue, white, black) = (egui::pos2(100.0, 60.0), egui::pos2(0.0, 60.0), egui::pos2(50.0, -27.0));
    let sv = |p| triangle_sv(triangle_weights(p, hue, white, black), 0.5);
    let close = |(s, v): (f32, f32), want: (f32, f32)| (s - want.0).abs() < 1e-3 && (v - want.1).abs() < 1e-3;
    assert!(close(sv(hue), (1.0, 1.0)), "{:?}", sv(hue));
    assert!(close(sv(white), (0.0, 1.0)), "{:?}", sv(white));
    assert_eq!(sv(black).1, 0.0);
    assert_eq!(sv(black).0, 0.5, "black keeps the saturation it had");
    assert!(close(sv(egui::pos2(50.0, 60.0)), (0.5, 1.0)), "halfway from white to the hue");
    // Outside the triangle it clamps onto it; nonsense stays in range.
    let (s, v) = sv(egui::pos2(500.0, 500.0));
    assert!((0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&v));
    let (s, v) = sv(egui::pos2(f32::NAN, 0.0));
    assert!((0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&v));
}

#[test]
fn the_color_selector_choice_is_saved() {
    let ctx = egui::Context::default();
    let saved: Rc<RefCell<Vec<ColorSelector>>> = Rc::default();
    let sink = saved.clone();
    let mut s = shell().with_settings(ShellSettings::default(), Box::new(move |st| sink.borrow_mut().push(st.color_selector)));
    assert_eq!(s.settings.color_selector, ColorSelector::Square, "the square stays the default");
    s.apply_act(&ctx, Act::ColorSelector(ColorSelector::Triangle));
    assert_eq!(s.settings.color_selector, ColorSelector::Triangle);
    assert_eq!(*saved.borrow(), [ColorSelector::Triangle]);
}

#[test]
fn the_sensitivity_slider_moves_one_middle_point_and_reads_back() {
    use studio::{curve_for_sensitivity, sensitivity_of};
    assert_eq!(curve_for_sensitivity(0.0, [0.0, 1.0]), vec![[0.0, 0.0], [1.0, 1.0]], "the middle is linear");
    let soft = curve_for_sensitivity(1.0, [0.0, 1.0]);
    assert_eq!(soft.len(), 3);
    assert!(soft[1][1] > soft[1][0], "soft gives more than it gets: {soft:?}");
    let hard = curve_for_sensitivity(-0.5, [0.0, 1.0]);
    assert!(hard[1][1] < hard[1][0], "{hard:?}");
    for s in [-1.0, -0.5, 0.0, 0.4, 1.0] {
        assert!((sensitivity_of(&curve_for_sensitivity(s, [0.0, 1.0])) - s).abs() < 1e-4, "{s}");
    }
    // The ends' outputs stay, and the middle never leaves them.
    let lifted = curve_for_sensitivity(1.0, [0.2, 0.7]);
    assert_eq!((lifted[0], lifted[2]), ([0.0, 0.2], [1.0, 0.7]));
    assert!((0.2..=0.7).contains(&lifted[1][1]));
    assert_eq!(curve_for_sensitivity(f32::NAN, [0.0, 1.0]).len(), 2);
    // PhotoCraft's presets read as soft and hard.
    assert!(sensitivity_of(&[[0.0, 0.0], [0.3, 0.6], [1.0, 1.0]]) > 0.3);
    assert!(sensitivity_of(&[[0.0, 0.0], [0.6, 0.3], [1.0, 1.0]]) < -0.3);
}

#[test]
fn stored_curves_are_cleaned_before_editing() {
    use studio::sanitize_curve;
    assert_eq!(sanitize_curve(vec![]), vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(sanitize_curve(vec![[1.0, 1.0], [f32::NAN, 0.5], [0.0, -2.0]]), vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(sanitize_curve(vec![[0.5, 0.5]; 40]).len(), 16);
}

#[test]
fn pen_sensitivity_is_saved_in_photocraft_preferences_and_the_brush() {
    let ctx = egui::Context::default();
    let mut s = shell();
    let soft = vec![[0.0, 0.0], [0.3, 0.6], [1.0, 1.0]];
    s.apply_act(&ctx, Act::PressureCurve(soft.clone()));
    assert_eq!(s.app.session.prefs().tools.pressure_curve, soft);
    s.apply_act(&ctx, Act::UseTabletPressure(false));
    assert!(!s.app.session.prefs().tools.use_tablet_pressure);
    s.app.ui.tool = Tool::Brush;
    let was = s.app.session.tools.brush.pressure_size;
    s.apply_act(&ctx, Act::PressureSize(!was));
    assert_eq!(s.app.session.tools.brush.pressure_size, !was);
    let was = s.app.session.tools.brush.pressure_opacity;
    s.apply_act(&ctx, Act::PressureOpacity(!was));
    assert_eq!(s.app.session.tools.brush.pressure_opacity, !was);
}
