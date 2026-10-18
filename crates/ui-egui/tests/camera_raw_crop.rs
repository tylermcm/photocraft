use egui::{accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use photocraft_ui_egui::{PhotocraftApp, camera_raw_ui, control, theme::ThemeKind};
use serde_json::{Value, json};
fn fixture() -> Harness<'static, PhotocraftApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 1100.0)).build_eframe(|cc| {
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
fn inspect(h: &Harness<'_, PhotocraftApp>) -> Value {
    control::inspect(h.state(), &h.ctx)
}
fn invoke(h: &mut Harness<'_, PhotocraftApp>, v: Value) -> Result<Value, String> {
    let ctx = h.ctx.clone();
    let out = camera_raw_ui::menu(h.state_mut(), &ctx, "filter.cameraRaw", &json!({"ui":v})).unwrap();
    h.run_steps(4);
    out
}
#[test]
fn crop_tab_disclosures_are_real_view_controls_and_do_not_edit_the_photo() {
    let mut h = fixture();
    let before = inspect(&h);
    let history = h.state().session.active().unwrap().history.past_len();
    h.get_by_label("Crop & Expand").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["tab"], "crop");
    for (role, label) in [(Role::ComboBox, "Preset"), (Role::CheckBox, "Enable Expand"), (Role::Button, "Rotate Left"), (Role::Button, "Flip Horizontal")] {
        assert!(h.get_by_role_and_label(role, label).accesskit_node().is_disabled(), "{label}");
    }
    h.get_by_label("Geometry").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["cropGroups"], json!([]));
    h.key_press(egui::Key::Space);
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["cropGroups"], json!(["Geometry"]));
    h.get_by_label("Trim").click();
    h.run_steps(4);
    assert!(inspect(&h)["cameraRaw"]["workspace"]["cropGroups"].as_array().unwrap().contains(&json!("Trim")));
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}
#[test]
fn invalid_crop_disclosures_reject_the_entire_mixed_request() {
    let mut h = fixture();
    let before = inspect(&h);
    for groups in [json!(["Unknown"]), json!(["Geometry", "Geometry"]), Value::Null] {
        assert!(invoke(&mut h, json!({"set":{"exposure":2},"workspace":{"tab":"crop","cropGroups":groups}})).is_err());
        assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
        assert_eq!(inspect(&h)["cameraRaw"]["workspace"], before["cameraRaw"]["workspace"]);
    }
    invoke(&mut h, json!({"workspace":{"tab":"crop","cropGroups":["Anamorphic"]}})).unwrap();
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["cropGroups"], json!(["Anamorphic"]));
}
