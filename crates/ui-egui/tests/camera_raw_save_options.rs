//! Save Options is a modal draft, never a pretend DNG writer or a photo edit.
use egui::{accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use photocraft_ui_egui::{PhotocraftApp, camera_raw_ui, control, theme::ThemeKind};
use serde_json::{Value, json};
fn fixture() -> Harness<'static, PhotocraftApp> {
    fixture_size(vec2(1440.0, 1400.0))
}
fn fixture_size(size: egui::Vec2) -> Harness<'static, PhotocraftApp> {
    let mut h = Harness::builder().with_size(size).build_eframe(|cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, ThemeKind::ProMedium);
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":64,"height":48})).unwrap();
        app.run("layer.new.layer", json!({})).unwrap();
        camera_raw_ui::open(&mut app, &cc.egui_ctx).unwrap();
        app
    });
    h.run_steps(4);
    h
}
fn invoke(h: &mut Harness<'_, PhotocraftApp>, v: Value) -> Result<Value, String> {
    let ctx = h.ctx.clone();
    let r = camera_raw_ui::menu(h.state_mut(), &ctx, "filter.cameraRaw", &json!({"ui":v})).unwrap();
    h.run_steps(4);
    r
}
fn inspect(h: &Harness<'_, PhotocraftApp>) -> Value {
    control::inspect(h.state(), &h.ctx)
}
#[test]
fn menu_opens_native_save_options_and_cancel_discards_only_the_export_draft() {
    let mut h = fixture();
    invoke(&mut h, json!({"set":{"exposure":1.5}})).unwrap();
    let before = inspect(&h);
    let history = h.state().session.active().unwrap().history.past_len();
    invoke(&mut h, json!({"workspace":{"settingsMenu":true}})).unwrap();
    h.get_by_role_and_label(Role::Button, "Save Image").click();
    h.run_steps(4);
    let opened = inspect(&h);
    assert!(opened["cameraRaw"]["workspace"]["saveOptions"].as_bool().unwrap());
    assert!(opened["cameraRawSaveOptions"]["state"]["rect"].is_array());
    assert!(!opened["cameraRawSaveOptions"]["canSave"].as_bool().unwrap());
    let top = opened["cameraRawSaveOptions"]["state"]["rect"][1].as_f64().unwrap() as f32;
    assert!(h.query_all_by_label("Save").find(|n| n.rect().center().y > top).unwrap().accesskit_node().is_disabled());
    h.get_by_role_and_label(Role::CheckBox, "Embed Fast Load Data").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRawSaveOptions"]["state"]["draft"]["fastLoad"], false);
    h.key_press(egui::Key::Space);
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRawSaveOptions"]["state"]["draft"]["fastLoad"], true);
    h.get_by_role_and_label(Role::ComboBox, "File Extension").click();
    h.run_steps(4);
    h.get_by_label(".DNG").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRawSaveOptions"]["state"]["draft"]["uppercaseExtension"], true);
    h.get_by_role_and_label(Role::CheckBox, "Embed Original Raw File").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRawSaveOptions"]["state"]["draft"]["embedOriginal"], true);
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    h.query_all_by_label("Cancel").find(|n| !n.accesskit_node().is_disabled()).unwrap().click();
    h.run_steps(4);
    assert!(!inspect(&h)["cameraRaw"].is_null());
    assert_eq!(inspect(&h)["cameraRawSaveOptions"]["state"]["draft"], Value::Null);
    assert_eq!(inspect(&h)["cameraRawSaveOptions"]["state"]["settings"]["embedOriginal"], false);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}
#[test]
fn atomic_draft_validation_and_escape_do_not_reach_the_camera_raw_dialog() {
    let mut h = fixture();
    let before = inspect(&h);
    for bad in [json!(null), json!({"fastLoad":1}), json!({"naming":["documentName"]}), json!({"format":"png"}), json!({"save":true})] {
        assert!(invoke(&mut h, json!({"set":{"exposure":2},"saveOptions":bad})).is_err());
        assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
        assert_eq!(inspect(&h)["cameraRawSaveOptions"], before["cameraRawSaveOptions"]);
    }
    invoke(&mut h, json!({"workspace":{"saveOptions":true},"saveOptions":{"lossy":true,"uppercaseExtension":true}})).unwrap();
    assert_eq!(inspect(&h)["cameraRawSaveOptions"]["state"]["draft"]["uppercaseExtension"], true);
    h.event(egui::Event::Paste("{\"version\":1,\"cameraRaw\":{\"exposure\":3}}".into()));
    h.run_steps(4);
    h.key_press(egui::Key::Y);
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    assert_eq!(inspect(&h)["cameraRaw"]["before"], before["cameraRaw"]["before"]);
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!inspect(&h)["cameraRaw"].is_null());
    assert!(!inspect(&h)["cameraRaw"]["workspace"]["saveOptions"].as_bool().unwrap());
    assert!(inspect(&h)["cameraRawSaveOptions"]["state"]["rect"].is_null());
}

#[test]
fn small_viewport_remains_scrollable_and_escape_does_not_cancel_the_photo() {
    let mut h = fixture_size(vec2(720.0, 900.0));
    invoke(&mut h, json!({"workspace":{"saveOptions":true}})).unwrap();
    assert!(inspect(&h)["cameraRawSaveOptions"]["state"]["rect"].is_array());
    let before=inspect(&h)["cameraRawSaveOptions"]["state"]["rect"][1].as_f64().unwrap();
    h.event(egui::Event::PointerMoved(egui::pos2(360.0,450.0)));
    h.event(egui::Event::MouseWheel {unit:egui::MouseWheelUnit::Point,phase:egui::TouchPhase::Move,delta:vec2(0.0,-500.0),modifiers:egui::Modifiers::default()});
    h.run_steps(12);
    assert!(inspect(&h)["cameraRawSaveOptions"]["state"]["rect"][1].as_f64().unwrap()<before,"the tall content must scroll inside the small viewport");
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!inspect(&h)["cameraRaw"].is_null());
    assert!(!inspect(&h)["cameraRaw"]["workspace"]["saveOptions"].as_bool().unwrap());
}
