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
