//! A saved window too small to use is forgotten, so the app opens at its default size.

use dutchman_gui::forget_unusable_window;

const MIN: [f32; 2] = [520.0, 420.0];

fn saved(window: &str) -> String {
    format!(
        "{{\"egui\":\"(options:())\",\"window\":\"(inner_position_pixels:Some((x:9.0,y:38.0)),outer_position_pixels:Some((x:0.0,y:0.0)),fullscreen:false,maximized:false,{window})\"}}"
    )
}

#[test]
fn a_tiny_saved_window_is_forgotten_and_other_settings_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.ron");
    std::fs::write(&path, saved("inner_size_points:Some((x:64.0,y:64.0))")).unwrap();
    assert!(forget_unusable_window(&path, MIN));
    let state: std::collections::HashMap<String, String> =
        ron::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!state.contains_key("window"));
    assert_eq!(state.get("egui").map(String::as_str), Some("(options:())"));
}

#[test]
fn a_usable_saved_window_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.ron");
    let text = saved("inner_size_points:Some((x:968.0,y:897.0))");
    std::fs::write(&path, &text).unwrap();
    assert!(!forget_unusable_window(&path, MIN));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn one_side_too_small_is_unusable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.ron");
    std::fs::write(&path, saved("inner_size_points:Some((x:1200.0,y:100.0))")).unwrap();
    assert!(forget_unusable_window(&path, MIN));
}

#[test]
fn missing_or_unreadable_state_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!forget_unusable_window(&dir.path().join("absent.ron"), MIN));
    let path = dir.path().join("app.ron");
    std::fs::write(&path, "not ron").unwrap();
    assert!(!forget_unusable_window(&path, MIN));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not ron");
}
