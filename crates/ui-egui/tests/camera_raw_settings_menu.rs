//! Real settings-menu interactions, through the same native dialog and recipe transaction path.
use egui::{accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use photocraft_ui_egui::{PhotocraftApp, camera_raw_ui, control, theme::ThemeKind};
use serde_json::{Value, json};
fn fixture() -> Harness<'static, PhotocraftApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 1200.0)).build_eframe(|cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, ThemeKind::ProMedium);
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":64,"height":48})).unwrap();
        app.run("layer.new.layer", json!({})).unwrap();
        app.run("edit.fill", json!({"color":"#808080"})).unwrap();
        camera_raw_ui::open(&mut app, &cc.egui_ctx).unwrap();
        app
    });
    h.run_steps(4);
    h
}
fn invoke(h: &mut Harness<'_, PhotocraftApp>, v: Value) {
    let ctx = h.ctx.clone();
    camera_raw_ui::menu(h.state_mut(), &ctx, "filter.cameraRaw", &json!({"ui":v})).unwrap().unwrap();
    h.run_steps(4);
}
fn inspect(h: &Harness<'_, PhotocraftApp>) -> Value {
    control::inspect(h.state(), &h.ctx)
}
fn menu(h: &mut Harness<'_, PhotocraftApp>) {
    invoke(h, json!({"workspace":{"settingsMenu":true}}));
}
#[test]
fn opening_dismissing_and_unavailable_rows_preserve_recipe_and_document() {
    let mut h = fixture();
    let before = inspect(&h);
    let history = h.state().session.active().unwrap().history.past_len();
    menu(&mut h);
    let during = inspect(&h);
    assert!(during["cameraRawMenu"]["rect"].is_array());
    assert_eq!(during["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    for label in [
        "Reset to Open",
        "Reset to Default",
        "Apply Previous Settings",
        "Paste Edit Settings",
        "Create Preset…",
        "Render to DNG",
        "Update AI Settings",
    ] {
        assert!(h.get_by_role_and_label(Role::Button, label).accesskit_node().is_disabled(), "{label}");
    }
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!inspect(&h)["cameraRaw"].is_null());
    assert!(!h.state().ui.camera_raw_workspace.settings_menu);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}
#[test]
fn committed_previous_settings_apply_with_undo_and_no_document_commit() {
    let mut h = fixture();
    invoke(&mut h, json!({"set":{"exposure":1.25,"tint":17.0},"commit":true}));
    assert_eq!(h.state().ui.camera_raw_menu.previous.as_ref().unwrap().exposure, 1.25);
    let ctx = h.ctx.clone();
    camera_raw_ui::open(h.state_mut(), &ctx).unwrap();
    h.run_steps(4);
    let history = h.state().session.active().unwrap().history.past_len();
    menu(&mut h);
    h.get_by_role_and_label(Role::Button, "Apply Previous Settings").click();
    h.run_steps(6);
    assert_eq!(inspect(&h)["cameraRaw"]["params"]["exposure"], 1.25);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
    invoke(&mut h, json!({"undo":true}));
    assert_eq!(inspect(&h)["cameraRaw"]["params"]["exposure"], 0.0);
}
#[test]
fn copy_and_paste_request_use_clipboard_events_and_validate_before_editing() {
    let mut h = fixture();
    invoke(&mut h, json!({"set":{"exposure":1.0}}));
    menu(&mut h);
    h.get_by_role_and_label(Role::Button, "Copy Edit Settings").click();
    h.run_steps(3);
    assert!(h.state().ui.camera_raw_menu.clipboard_ready);
    menu(&mut h);
    h.get_by_role_and_label(Role::Button, "Paste Edit Settings").click();
    h.run_steps(3);
    assert!(h.state().ui.camera_raw_menu.paste_pending);
    let before = inspect(&h)["cameraRaw"]["params"].clone();
    h.event(egui::Event::Paste("not a recipe".into()));
    h.run_steps(3);
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before);
    h.event(egui::Event::Paste(r#"{"version":1,"cameraRaw":{"exposure":2.0,"tint":9.0}}"#.into()));
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["params"]["exposure"], 2.0);
    assert_eq!(inspect(&h)["cameraRaw"]["params"]["tint"], 9.0);
    invoke(&mut h, json!({"undo":true}));
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before);
}
#[test]
fn snapshot_created_from_menu_can_restore_the_captured_recipe() {
    let mut h = fixture();
    invoke(&mut h, json!({"set":{"exposure":1.0}}));
    menu(&mut h);
    h.get_by_role_and_label(Role::Button, "Create Snapshot…").click();
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, "Create").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["snapshots"], json!(["Snapshot 1"]));
    invoke(&mut h, json!({"set":{"exposure":2.0}}));
    menu(&mut h);
    h.get_by_role_and_label(Role::Button, "Apply Snapshot").click();
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, "Snapshot 1").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["params"]["exposure"], 1.0);
}
#[test]
fn menu_keyboard_skips_disabled_rows_and_does_not_edit_underlying_sliders() {
    let mut h = fixture();
    menu(&mut h);
    let before = inspect(&h)["cameraRaw"]["params"].clone();
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(3);
    assert_eq!(h.state().ui.camera_raw_menu.focus, Some(5));
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Create").is_some());
    assert!(!inspect(&h)["cameraRaw"].is_null());
}

#[test]
fn settings_transfer_keeps_destination_color_and_proxy_context() {
    let mut h = fixture();
    invoke(&mut h, json!({"set":{"encodingGamma":2.2,"pixelScale":0.5}}));
    h.state_mut().ui.camera_raw_menu.previous = Some(photocraft_algo::camera_raw::CameraRaw { exposure: 1.0, ..Default::default() });
    menu(&mut h);
    assert!(h.get_by_role_and_label(Role::Button, "Reset to Default").accesskit_node().is_disabled());
    h.get_by_role_and_label(Role::Button, "Apply Previous Settings").click();
    h.run_steps(4);
    let p = inspect(&h)["cameraRaw"]["params"].clone();
    assert_eq!(p["exposure"], 1.0);
    assert!((p["encodingGamma"].as_f64().unwrap() - 2.2).abs() < 1e-5);
    assert_eq!(p["pixelScale"], 0.5);
}

#[test]
fn closing_or_replacing_the_dialog_clears_only_transient_menu_state() {
    let mut h = fixture();
    menu(&mut h);
    invoke(&mut h, json!({"cancel":true}));
    assert!(!h.state().ui.camera_raw_workspace.settings_menu);
    assert!(h.state().ui.camera_raw_menu.rect.is_none());
    let ctx = h.ctx.clone();
    camera_raw_ui::open(h.state_mut(), &ctx).unwrap();
    h.run_steps(3);
    assert!(!h.state().ui.camera_raw_workspace.settings_menu);
    menu(&mut h);
    invoke(&mut h, json!({"set":{"exposure":0.5},"commit":true}));
    assert!(!h.state().ui.camera_raw_workspace.settings_menu);
    assert!(h.state().ui.camera_raw_menu.rect.is_none());
    assert_eq!(h.state().ui.camera_raw_menu.previous.as_ref().unwrap().exposure, 0.5);
}

#[test]
fn more_button_opens_the_real_menu_without_the_opening_click_dismissing_it() {
    let mut h = fixture();
    h.get_by_role_and_label(Role::Button, "Settings").click();
    h.run_steps(4);
    assert!(h.state().ui.camera_raw_workspace.settings_menu);
    assert!(h.query_by_role_and_label(Role::Button, "Copy Edit Settings").is_some());
    h.get_by_role_and_label(Role::Button, "Settings").click();
    h.run_steps(4);
    assert!(!h.state().ui.camera_raw_workspace.settings_menu);
    assert!(!inspect(&h)["cameraRaw"].is_null());
}
