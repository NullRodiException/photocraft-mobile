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

/// The on-screen angle (degrees) of the document's x axis.
fn screen_angle_of_x(s: &ModeShell) -> f32 {
    let v = photocraft_ui_egui::canvas::ViewXform::active(&s.app).unwrap().map_vec(egui::vec2(1.0, 0.0));
    v.y.atan2(v.x).to_degrees()
}

#[test]
fn two_fingers_turn_the_view_around_the_point_between_them() {
    use photocraft_ui_egui::canvas::ViewXform;
    let (mut s, idx) = shell_with_document();
    s.app.last_canvas_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0));
    let anchor = egui::pos2(300.0, 200.0);
    for flip in [false, true] {
        s.app.ui.view.flip_horizontal = flip;
        let doc = ViewXform::active(&s.app).unwrap().to_doc(anchor);
        let before = screen_angle_of_x(&s);
        s.rotate_view(30.0, anchor);
        let after = ViewXform::active(&s.app).unwrap().to_screen(doc[0] as f32, doc[1] as f32);
        assert!(after.distance(anchor) < 0.01, "flip {flip}: the spot under the fingers stays: {after:?}");
        let turned = (screen_angle_of_x(&s) - before + 540.0).rem_euclid(360.0) - 180.0;
        assert!((turned - 30.0).abs() < 0.01, "flip {flip}: the image turns clockwise with the fingers: {turned}");
    }
    // Lifting near a right angle snaps there; farther off, the rotation stays.
    s.app.ui.view.flip_horizontal = false;
    s.app.ui.views[idx].rotation = 87.0;
    s.settle_rotation(anchor);
    assert!((s.app.ui.views[idx].rotation - 90.0).abs() < 1e-3);
    s.app.ui.views[idx].rotation = 80.0;
    s.settle_rotation(anchor);
    assert_eq!(s.app.ui.views[idx].rotation, 80.0);
}

#[test]
fn holding_a_finger_picks_a_colour_and_gives_the_tool_back() {
    use eframe::App;
    let (s, _) = shell_with_document();
    let mut s = s.with_touch_kind(Box::new(|_| Some(ToolKind::Finger)));
    s.app.last_canvas_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0));
    s.app.ui.tool = Tool::Brush;
    let ctx = egui::Context::default();
    let feed = |s: &mut ModeShell, events: Vec<egui::Event>, time: f64| {
        let mut raw = egui::RawInput { events, time: Some(time), ..Default::default() };
        s.raw_input_hook(&ctx, &mut raw);
        s.apply_gesture(&ctx);
        raw.events
    };
    feed(&mut s, vec![touch_event(1, egui::TouchPhase::Start, 300.0, 300.0)], 0.0);
    assert_eq!(s.gesture_wake, Some(touch::HOLD_SECONDS), "a frame is asked for when the hold is due");
    let start = feed(&mut s, Vec::new(), 0.6);
    assert_eq!(s.app.ui.tool, Tool::Eyedropper);
    assert!(start.iter().any(|e| matches!(e, egui::Event::PointerButton { pressed: true, pos, .. } if *pos == egui::pos2(300.0, 300.0))), "{start:?}");
    let end = feed(&mut s, vec![touch_event(1, egui::TouchPhase::End, 320.0, 300.0)], 0.8);
    let release = end.iter().position(|e| matches!(e, egui::Event::PointerButton { pressed: false, .. }));
    let gone = end.iter().position(|e| matches!(e, egui::Event::PointerGone));
    assert!(release.is_some() && release < gone, "the release comes before the pointer goes: {end:?}");
    // The eyedropper stays for the frame that takes the release; then the brush comes back.
    assert_eq!(s.app.ui.tool, Tool::Eyedropper);
    assert!(s.restore_after_frame);
    assert_eq!(s.hold_return, Some(Tool::Brush));
}

#[test]
fn a_four_finger_tap_toggles_full_screen() {
    let mut s = shell();
    s.pending_tap = Some(4);
    s.apply_gesture(&egui::Context::default());
    assert!(s.focused());
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
fn going_to_the_background_autosaves_unsaved_documents() {
    autosaves_on(PlatformEvent::Background, "background");
}

#[test]
fn critically_low_memory_autosaves_unsaved_documents() {
    autosaves_on(PlatformEvent::LowMemory, "low-memory");
}

/// `event` makes the next frame autosave a document with unsaved changes for crash recovery.
fn autosaves_on(event: PlatformEvent, name: &str) {
    let dir = std::env::temp_dir().join(format!("photocraft-mobile-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pending = Rc::new(std::cell::Cell::new(false));
    let flag = pending.clone();
    let config = dir.clone();
    let mut harness = egui_kittest::Harness::builder().build_eframe(move |_cc| {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services::base(Some(config)));
        app.run("file.new", json!({"width": 64, "height": 48})).unwrap();
        app.run("layer.new.layer", json!({})).unwrap();
        ModeShell::new(app, Modes::default()).with_platform_events(Box::new(move || if flag.replace(false) { vec![event.clone()] } else { Vec::new() }))
    });
    let recovery = dir.join(services::RECOVERY);
    harness.step();
    harness.step();
    assert!(photocraft_format::list_recovery(&recovery).is_empty(), "the autosave interval hasn't passed");
    pending.set(true);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while photocraft_format::list_recovery(&recovery).is_empty() && std::time::Instant::now() < deadline {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(photocraft_format::list_recovery(&recovery).len(), 1, "the unsaved document was autosaved");
    drop(harness);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_report_is_announced_and_sent_from_actions() {
    let sent: Rc<RefCell<Vec<std::path::PathBuf>>> = Rc::default();
    let sink = sent.clone();
    let path = std::path::PathBuf::from("/data/files/crash-last.log");
    let mut s = shell().with_crash_report(path.clone(), Some(Box::new(move |p| sink.borrow_mut().push(p.to_path_buf()))));
    let before = s.app.ui.notices.len();
    s.post_crash_notice();
    assert_eq!(s.app.ui.notices.len(), before + 1);
    let notice = s.app.ui.notices.last().unwrap();
    assert!(notice.error && notice.lines.iter().any(|l| l.contains("crash-last.log")), "{:?}", notice.lines);
    assert!(s.can_send_report());
    s.apply_act(&egui::Context::default(), Act::SendCrashReport);
    assert_eq!(*sent.borrow(), [path]);
    // Without a crash, nothing is said or offered.
    let mut quiet = shell();
    let before = quiet.app.ui.notices.len();
    quiet.post_crash_notice();
    assert_eq!(quiet.app.ui.notices.len(), before);
    assert!(!quiet.can_send_report());
}

static SHARED: std::sync::Mutex<Vec<(share::ShareTarget, std::path::PathBuf)>> = std::sync::Mutex::new(Vec::new());

fn record_share(target: share::ShareTarget, path: &std::path::Path) {
    SHARED.lock().unwrap().push((target, path.to_path_buf()));
}

#[test]
fn sharing_exports_the_image_as_a_png_for_the_platform() {
    use share::ShareTarget;
    let dir = std::env::temp_dir().join(format!("photocraft-mobile-share-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let ctx = egui::Context::default();
    assert_eq!(shell().sharing_busy(), None, "nothing to offer without the platform's share");
    let mut s = shell().with_share(dir.clone(), record_share);
    // No document: nothing to share.
    s.apply_act(&ctx, Act::Share(ShareTarget::Send));
    assert_eq!(s.sharing_busy(), Some(false));
    s.app.run("file.new", json!({"width": 32, "height": 24, "name": "Sketch"})).unwrap();
    let notices = s.app.ui.notices.len();
    s.apply_act(&ctx, Act::Share(ShareTarget::Gallery));
    assert_eq!(s.sharing_busy(), Some(true));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while s.sharing_busy() == Some(true) {
        assert!(std::time::Instant::now() < deadline, "the export never finished");
        std::thread::sleep(std::time::Duration::from_millis(10));
        s.shared();
    }
    assert_eq!(s.app.ui.notices.len(), notices, "a successful export says nothing");
    let shared = SHARED.lock().unwrap().clone();
    let [(target, path)] = shared.as_slice() else { panic!("{shared:?}") };
    assert_eq!(*target, ShareTarget::Gallery);
    assert!(path.starts_with(&dir) && path.file_name().is_some_and(|n| n == "Sketch.png"), "{}", path.display());
    assert!(std::fs::read(path).unwrap().starts_with(b"\x89PNG"));
    let _ = std::fs::remove_dir_all(&dir);
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
fn tools_are_found_by_name_as_photocraft_finds_them() {
    for name in tool_names() {
        let tool = tool_named(&name);
        assert_eq!(tool, Tool::from_name(&name), "{name}");
        assert_eq!(tool.map(tool_name), Some(name.as_str()));
    }
    // Any other spelling goes to PhotoCraft's own lookup.
    assert_eq!(tool_named("brush"), Tool::from_name("brush"));
    assert_eq!(tool_named("no such tool"), None);
}

/// One frame drawing the open popover, with `events`.
fn popover_frame(ctx: &egui::Context, s: &mut ModeShell, events: Vec<egui::Event>) {
    let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| s.popover(ui.ctx()));
    out.textures_delta.clear();
}

#[test]
fn menu_popovers_keep_their_items_until_something_changes() {
    let ctx = egui::Context::default();
    PhotocraftApp::setup_context(&ctx, Default::default());
    let (mut s, _) = shell_with_document();
    let same = |a: &Option<std::rc::Rc<[photocraft_ui_egui::menus::MenuItem]>>, b: &Option<std::rc::Rc<[photocraft_ui_egui::menus::MenuItem]>>| matches!((a, b), (Some(a), Some(b)) if std::rc::Rc::ptr_eq(a, b));
    s.toggle_popover(Popover::Actions);
    popover_frame(&ctx, &mut s, vec![]);
    let first = s.studio.kept_menu_items();
    assert!(first.is_some());
    popover_frame(&ctx, &mut s, vec![]);
    assert!(same(&first, &s.studio.kept_menu_items()), "nothing changed: the items are kept");

    // A command changes the document: the rows are built again, and match a fresh build.
    s.app.run("layer.new.layer", json!({})).unwrap();
    popover_frame(&ctx, &mut s, vec![]);
    let after = s.studio.kept_menu_items();
    assert!(!same(&first, &after), "the document changed");
    let fresh: Vec<(String, bool, Option<bool>)> = studio::live_items(&s.app).into_iter().map(|i| (i.id, i.enabled, i.checked)).collect();
    let kept: Vec<(String, bool, Option<bool>)> = after.iter().flat_map(|items| items.iter()).map(|i| (i.id.clone(), i.enabled, i.checked)).collect();
    assert_eq!(kept, fresh);

    // A tap rebuilds them on its frame and the next (its command runs after the rows are drawn).
    popover_frame(&ctx, &mut s, vec![press_at(5000.0, 5000.0, false)]);
    let tapped = s.studio.kept_menu_items();
    assert!(!same(&after, &tapped));
    popover_frame(&ctx, &mut s, vec![]);
    let next = s.studio.kept_menu_items();
    assert!(!same(&tapped, &next));
    popover_frame(&ctx, &mut s, vec![]);
    assert!(same(&next, &s.studio.kept_menu_items()));

    // The search finds what PhotoCraft's search finds, and is kept with the items.
    let items = s.studio.kept_menu_items().unwrap();
    let hits = s.search_hits(&items, "layer");
    assert!(std::rc::Rc::ptr_eq(&hits, &s.search_hits(&items, "layer")));
    let expected: Vec<&str> =
        photocraft_ui_egui::menus::search_items(&items, "layer", photocraft_ui_egui::i18n::current()).iter().map(|i| i.id.as_str()).collect();
    let found: Vec<&str> = hits.iter().map(|&i| items[i].id.as_str()).collect();
    assert!(!found.is_empty());
    assert_eq!(found, expected);
    assert!(!std::rc::Rc::ptr_eq(&hits, &s.search_hits(&items, "layers")), "another query searches again");

    // Closed, nothing is kept.
    s.studio.popover = None;
    popover_frame(&ctx, &mut s, vec![]);
    assert!(s.studio.kept_menu_items().is_none());
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
fn recent_colors_are_saved_newest_first() {
    let saved: Rc<RefCell<Vec<Vec<[f32; 3]>>>> = Rc::default();
    let sink = saved.clone();
    let start = ShellSettings { recent_colors: vec![[0.0, 0.0, 1.0]], ..ShellSettings::default() };
    let mut s = shell().with_settings(start, Box::new(move |st| sink.borrow_mut().push(st.recent_colors.clone())));
    s.remember_color([1.0, 0.0, 0.0]);
    s.remember_color([1.0, 0.0, 0.0]);
    assert_eq!(saved.borrow().len(), 1, "picking the newest colour again saves nothing");
    s.remember_color([0.0, 0.0, 1.0]);
    assert_eq!(saved.borrow().last().unwrap(), &vec![[0.0, 0.0, 1.0], [1.0, 0.0, 0.0]], "an older colour moves to the front, once");
    for i in 0..20 {
        s.remember_color([i as f32 / 20.0, 1.0, 0.0]);
    }
    assert_eq!(s.settings.recent_colors.len(), studio::RECENT_COLORS);
    assert_eq!(s.settings.recent_colors[0], [19.0 / 20.0, 1.0, 0.0]);
}

#[test]
fn the_harmony_choice_is_saved() {
    let ctx = egui::Context::default();
    let saved: Rc<RefCell<Vec<Harmony>>> = Rc::default();
    let sink = saved.clone();
    let mut s = shell().with_settings(ShellSettings::default(), Box::new(move |st| sink.borrow_mut().push(st.harmony)));
    assert_eq!(s.settings.harmony, Harmony::Analogous, "analogous stays the default");
    s.apply_act(&ctx, Act::Harmony(Harmony::Monochromatic));
    assert_eq!(*saved.borrow(), [Harmony::Monochromatic]);
}

#[test]
fn the_colour_lists_go_beside_the_picker_only_on_a_short_wide_screen() {
    use studio::color_side_by_side;
    let screen = |w: f32, h: f32| egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
    // The Tab S9 FE+ in landscape at the 2.5× UI scale: a picker over the lists would shrink.
    assert!(color_side_by_side(screen(1024.0, 640.0), ColorSelector::Square));
    // Tall enough for the square, not for the triangle and its bars.
    assert!(!color_side_by_side(screen(1280.0, 800.0), ColorSelector::Square));
    assert!(color_side_by_side(screen(1280.0, 800.0), ColorSelector::Triangle));
    // Portrait: one column, however short (it scrolls).
    assert!(!color_side_by_side(screen(640.0, 1024.0), ColorSelector::Triangle));
    assert!(!color_side_by_side(screen(400.0, 700.0), ColorSelector::Square));
}

#[test]
fn a_colour_taken_from_the_palette_keeps_the_palette() {
    use photocraft_ui_egui::color_picker_ui::hsv_to_rgb;
    let ctx = egui::Context::default();
    PhotocraftApp::setup_context(&ctx, Default::default());
    let mut s = shell();
    s.apply_act(&ctx, Act::Color([1.0, 0.0, 0.0]));
    s.toggle_popover(Popover::Color);
    popover_frame(&ctx, &mut s, vec![]);
    let base = s.studio.palette_base;
    assert!(base[0].abs() < 0.5 && (base[1] - 1.0).abs() < 1e-3 && (base[2] - 1.0).abs() < 1e-3, "made from the foreground: {base:?}");
    // A neighbour becomes the foreground (and a recent colour); the palette doesn't move.
    let [h, sat, v] = Harmony::Analogous.palette(base).colors[3];
    let neighbour = hsv_to_rgb(h, sat, v);
    s.apply_act(&ctx, Act::PaletteColor(neighbour));
    popover_frame(&ctx, &mut s, vec![]);
    let fg = s.app.session.tools.foreground;
    assert!(fg.iter().zip(neighbour).all(|(a, b)| (a - b).abs() < 1e-3), "{fg:?}");
    assert_eq!(s.settings.recent_colors.first(), Some(&neighbour));
    assert_eq!(s.studio.palette_base, base, "the palette stays");
    // A colour from anywhere else makes a new palette.
    s.apply_act(&ctx, Act::Color([0.0, 0.0, 1.0]));
    popover_frame(&ctx, &mut s, vec![]);
    assert!((s.studio.palette_base[0] - 240.0).abs() < 0.5, "{:?}", s.studio.palette_base);
}

#[test]
fn a_saved_palette_becomes_a_swatch_group_listed_first() {
    let ctx = egui::Context::default();
    let mut s = shell();
    let before = s.app.session.presets.swatches.len();
    let colors = vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    s.apply_act(&ctx, Act::SavePalette(Harmony::Complementary, colors.clone()));
    s.apply_act(&ctx, Act::SavePalette(Harmony::Complementary, colors));
    let name = studio::tr("Complementary", "Complementares");
    let groups = &s.app.session.presets.swatches;
    let names: Vec<&str> = groups[before..].iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, [format!("{name} 1"), format!("{name} 2")], "a new group each time");
    let swatches: Vec<&str> = groups[before].items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(swatches, ["#FF0000", "#00FF00", "#0000FF"]);
    // Palettes shows the newest first, and a swatch tapped there becomes the foreground.
    s.toggle_popover(Popover::Color);
    let rows = s.swatch_rows();
    assert_eq!(rows[0].group, format!("{name} 2"));
    assert_eq!(rows[1].swatches[1].1, [0.0, 1.0, 0.0]);
    s.apply_act(&ctx, Act::UseSwatch { group: format!("{name} 1"), name: "#00FF00".into() });
    assert_eq!(s.app.session.tools.foreground, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(s.settings.recent_colors.first(), Some(&[0.0, 1.0, 0.0]));
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
