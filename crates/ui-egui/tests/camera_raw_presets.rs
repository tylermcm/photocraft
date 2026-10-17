//! Preset-browser view operations must not imply or apply unavailable recipes.
use egui::{accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use photocraft_ui_egui::{PhotocraftApp, camera_raw_ui, control, theme::ThemeKind};
use serde_json::{Value, json};

fn fixture() -> Harness<'static, PhotocraftApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 1285.0)).build_eframe(|cc| {
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
fn rail_opens_presets_and_disclosure_changes_only_presentation() {
    let mut h = fixture();
    invoke(&mut h, json!({"set":{"exposure":1.25}})).unwrap();
    let before = inspect(&h);
    let history = h.state().session.active().unwrap().history.past_len();
    h.get_by_label("Presets").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["tab"], "presets");
    h.get_by_label("Adaptive: Portrait").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["presetGroups"], json!(["Adaptive: Portrait"]));
    assert!(h.get_by_role_and_label(Role::Button, "Create Preset…").accesskit_node().is_disabled());
    assert!(h.get_by_role_and_label(Role::Slider, "Amount").accesskit_node().is_disabled());
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
    h.key_press(egui::Key::Space);
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["presetGroups"], json!([]));
}

#[test]
fn invalid_preset_view_is_atomic_and_menu_uses_existing_workspace_popup() {
    let mut h = fixture();
    invoke(&mut h, json!({"workspace":{"tab":"presets"}})).unwrap();
    let before = inspect(&h);
    for bad in [json!(["unknown"]), json!(["Color", "Color"]), json!(null), json!([0])] {
        assert!(invoke(&mut h, json!({"set":{"exposure":2},"workspace":{"presetGroups":bad}})).is_err());
        assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
        assert_eq!(inspect(&h)["cameraRaw"]["workspace"], before["cameraRaw"]["workspace"]);
    }
    // Preset management has no implementation yet; the rail's global menu remains usable.
    let buttons: Vec<_> = h.query_all_by_label("Settings").collect();
    assert_eq!(buttons.len(), 2);
    assert_eq!(buttons.iter().filter(|b| b.accesskit_node().is_disabled()).count(), 1);
    buttons.into_iter().find(|b| !b.accesskit_node().is_disabled()).unwrap().click();
    h.run_steps(4);
    assert!(inspect(&h)["cameraRaw"]["workspace"]["settingsMenu"].as_bool().unwrap());
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["tab"], "presets");
}
