use super::*;

const ALL_TOOLS: &[&str] = &["Move", "Hand", "Brush", "Eraser", "Pen", "Rectangle", "SpotHealing", "Crop"];

#[test]
fn lists_have_no_duplicates_and_show_their_default_tool() {
    for mode in Mode::ALL {
        let tools: Vec<&str> = mode.tools().collect();
        let mut unique = tools.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), tools.len(), "{mode:?} lists a tool twice");
        assert!(mode.shows(mode.default_tool()), "{mode:?} hides its own default tool");
    }
}

#[test]
fn ids_indices_and_cycle_round_trip() {
    for (i, mode) in Mode::ALL.into_iter().enumerate() {
        assert_eq!(Mode::from_id(mode.id()), Some(mode));
        assert_eq!(Mode::from_id(&mode.id().to_uppercase()), Some(mode));
        assert_eq!(mode.index(), i);
        assert_eq!(mode.next(), Mode::ALL[(i + 1) % Mode::ALL.len()]);
    }
    assert_eq!(Mode::from_id("layout"), None);
    assert_eq!(Mode::from_id(""), None);
}

#[test]
fn labels_follow_the_ui_language() {
    assert_eq!(Mode::Vector.label("pt-BR"), "Vetor");
    assert_eq!(Mode::Photo.label("pt-br"), "Foto");
    assert_eq!(Mode::Pixel.label("es"), "Píxel");
    assert_eq!(Mode::Vector.label("en"), "Vector");
    assert_eq!(Mode::Photo.label(""), "Photo");
}

#[test]
fn switching_hides_the_other_modes_tools_and_picks_the_default() {
    let mut modes = Modes::default();
    let s = modes.switch(Mode::Vector, "Move", ALL_TOOLS);
    assert_eq!(s.workspace_command, "window.workspace.graphicAndWeb");
    assert_eq!(s.tool, "Pen");
    assert_eq!(s.hidden, ["Brush", "Eraser", "SpotHealing", "Crop"]);
    assert_eq!(modes.current, Some(Mode::Vector));
}

#[test]
fn each_mode_remembers_its_last_tool() {
    let mut modes = Modes::default();
    modes.switch(Mode::Pixel, "Move", ALL_TOOLS);
    // The user picks the Eraser in Pixel, then goes to Vector and picks Rectangle.
    let s = modes.switch(Mode::Vector, "Eraser", ALL_TOOLS);
    assert_eq!(s.tool, "Pen");
    let s = modes.switch(Mode::Pixel, "Rectangle", ALL_TOOLS);
    assert_eq!(s.tool, "Eraser");
    let s = modes.switch(Mode::Vector, "Eraser", ALL_TOOLS);
    assert_eq!(s.tool, "Rectangle");
}

#[test]
fn reentering_the_current_mode_keeps_the_active_tool() {
    let mut modes = Modes { current: Some(Mode::Pixel), ..Default::default() };
    assert_eq!(modes.switch(Mode::Pixel, "Eraser", ALL_TOOLS).tool, "Eraser");
    // A tool the mode doesn't show (picked through a shortcut or the control channel) gives way to
    // the mode's last tool, or its default before it has one.
    assert_eq!(modes.switch(Mode::Pixel, "Pen", ALL_TOOLS).tool, "Eraser");
    let mut fresh = Modes { current: Some(Mode::Pixel), ..Default::default() };
    assert_eq!(fresh.switch(Mode::Pixel, "Pen", ALL_TOOLS).tool, "Brush");
}

#[test]
fn a_saved_tool_the_mode_no_longer_shows_falls_back_to_the_default() {
    let mut modes = Modes::default();
    modes.last_tool.insert(Mode::Photo, "Pen".into());
    assert_eq!(modes.switch(Mode::Photo, "Move", ALL_TOOLS).tool, "SpotHealing");
}

#[test]
fn a_tool_the_mode_being_left_hides_is_not_remembered() {
    let mut modes = Modes { current: Some(Mode::Vector), ..Default::default() };
    modes.switch(Mode::Pixel, "Brush", ALL_TOOLS);
    assert_eq!(modes.last_tool.get(&Mode::Vector), None);
}

#[test]
fn saved_state_round_trips_and_tolerates_old_or_partial_files() {
    let mut modes = Modes::default();
    modes.switch(Mode::Pixel, "Move", ALL_TOOLS);
    modes.switch(Mode::Photo, "Eraser", ALL_TOOLS);
    let text = serde_json::to_string(&modes).unwrap();
    assert_eq!(serde_json::from_str::<Modes>(&text).unwrap(), modes);
    assert_eq!(serde_json::from_str::<Modes>("{}").unwrap(), Modes::default());
    assert_eq!(serde_json::from_str::<Modes>(r#"{"current":"pixel"}"#).unwrap().mode(), Mode::Pixel);
    assert!(serde_json::from_str::<Modes>(r#"{"current":"layout"}"#).is_err());
}

#[test]
fn fresh_state_starts_in_pixel() {
    assert_eq!(Modes::default().mode(), Mode::Pixel);
}

#[test]
fn quick_and_selection_tools_belong_to_their_mode() {
    for mode in Mode::ALL {
        for t in mode.quick_tools() {
            assert!(mode.shows(t), "{mode:?} has quick tool {t} that its toolbar hides");
        }
        let selection: Vec<&str> = mode.selection_tools().collect();
        assert!(!selection.is_empty(), "{mode:?} has no selection tool");
        assert!(selection.iter().all(|t| mode.shows(t) && SELECTION_TOOLS.contains(t)));
    }
    assert!(Mode::Vector.selection_tools().any(|t| t == "PathSelection"));
    assert!(!Mode::Pixel.selection_tools().any(|t| t == "Brush"));
}
