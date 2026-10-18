use egui::{accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use photocraft_ui_egui::{PhotocraftApp, camera_raw_ui, control, theme::ThemeKind};
use serde_json::{Value, json};
fn fixture() -> Harness<'static, PhotocraftApp> {
    fixture_with_profile(None)
}
fn fixture_with_profile(profile: Option<Value>) -> Harness<'static, PhotocraftApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 1400.0)).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, ThemeKind::ProMedium);
        let mut session = photocraft_engine::Session::new();
        if let Some(profile) = profile {
            session.prefs.edit(|p| p.dialogs.insert("filter.cameraRaw.preferences".into(), profile));
        }
        let mut app = PhotocraftApp::new(session, Default::default());
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
    let out = camera_raw_ui::menu(h.state_mut(), &ctx, "filter.cameraRaw", &json!({"ui":v})).unwrap();
    h.run_steps(4);
    out
}
fn inspect(h: &Harness<'_, PhotocraftApp>) -> Value {
    control::inspect(h.state(), &h.ctx)
}
#[test]
fn gear_opens_preferences_and_cancel_does_not_change_views_or_photo() {
    let mut h = fixture();
    let before = inspect(&h);
    h.get_by_label("Camera Raw Settings").click();
    h.run_steps(4);
    h.get_by_label("Camera Raw Preferences…").click();
    h.run_steps(4);
    assert!(inspect(&h)["cameraRaw"]["workspace"]["preferences"].as_bool().unwrap());
    assert!(inspect(&h)["cameraRawPreferences"]["rect"].is_array());
    assert!(!h.get_by_role_and_label(Role::CheckBox, "Show filenames").accesskit_node().is_disabled());
    h.get_by_role_and_label(Role::CheckBox, "Show filenames").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRawPreferences"]["draft"]["showFilenames"], true);
    assert_eq!(inspect(&h)["filmstrip"]["display"]["showFilenames"], false);
    h.query_all_by_label("Cancel").find(|n| !n.accesskit_node().is_disabled()).unwrap().click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["filmstrip"]["display"]["showFilenames"], false);
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    assert!(!h.state().session.prefs().dialogs.contains_key("filter.cameraRaw.preferences"));
}
#[test]
fn ok_applies_existing_panel_filmstrip_and_click_zoom_behavior_and_persists_it() {
    let mut h = fixture();
    let history = h.state().session.active().unwrap().history.past_len();
    invoke(&mut h, json!({"workspace":{"panels":{"open":["Light","Color","Effects"]},"preferences":true}})).unwrap();
    h.get_by_role_and_label(Role::ComboBox, "Edit panel behavior").click();
    h.run_steps(4);
    h.get_by_label("Single").click();
    h.run_steps(4);
    h.get_by_role_and_label(Role::CheckBox, "Show filenames").click();
    h.run_steps(4);
    h.get_by_role_and_label(Role::ComboBox, "Default image click zooms to").click();
    h.run_steps(4);
    h.get_by_label("200%").click();
    h.run_steps(4);
    h.query_all_by_label("OK").find(|n| !n.accesskit_node().is_disabled()).unwrap().click();
    h.run_steps(4);
    let after = inspect(&h);
    assert_eq!(after["cameraRaw"]["workspace"]["panels"]["single"], true);
    assert_eq!(after["cameraRaw"]["workspace"]["panels"]["open"].as_array().unwrap().len(), 1);
    assert_eq!(after["filmstrip"]["display"]["showFilenames"], true);
    assert_eq!(h.state().session.prefs().dialogs["filter.cameraRaw.preferences"]["clickZoom"], 2.0);
    let r = after["cameraRaw"]["viewportRect"].as_array().unwrap();
    let at = egui::pos2(((r[0].as_f64().unwrap() + r[2].as_f64().unwrap()) / 2.0) as f32, ((r[1].as_f64().unwrap() + r[3].as_f64().unwrap()) / 2.0) as f32);
    h.event(egui::Event::PointerMoved(at));
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::default() });
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() });
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["view"]["zoom"], 2.0);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}
#[test]
fn invalid_preferences_are_atomic_and_escape_closes_only_this_modal() {
    let mut h = fixture();
    let before = inspect(&h);
    for bad in [json!({"clickZoom":0}), json!({"showFilenames":"yes"}), json!({"colorLabels":true}), Value::Null] {
        assert!(invoke(&mut h, json!({"set":{"exposure":2},"preferences":bad})).is_err());
        assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    }
    assert!(invoke(&mut h, json!({"workspace":{"preferences":true,"saveOptions":true}})).is_err());
    invoke(&mut h, json!({"workspace":{"preferences":true}})).unwrap();
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!inspect(&h)["cameraRaw"].is_null());
    assert_eq!(inspect(&h)["cameraRawPreferences"]["draft"], Value::Null);
}

#[test]
fn saved_profile_is_applied_on_startup_and_invalid_profile_is_reported() {
    let h = fixture_with_profile(Some(json!({"singlePanel":true,"showFilenames":true,"clickZoom":0.5})));
    let v = inspect(&h);
    assert_eq!(v["cameraRaw"]["workspace"]["panels"]["single"], true);
    assert_eq!(v["filmstrip"]["display"]["showFilenames"], true);
    assert_eq!(v["cameraRawPreferences"]["settings"]["clickZoom"], 0.5);
    let h = fixture_with_profile(Some(json!({"clickZoom":0})));
    assert!(h.state().ui.status_error);
    assert!(h.state().ui.status.contains("Invalid Camera Raw preferences"));
    assert_eq!(inspect(&h)["cameraRawPreferences"]["applied"], false);
    assert_eq!(inspect(&h)["filmstrip"]["display"]["showFilenames"], false);
}
