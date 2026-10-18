use egui::vec2;
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
fn remove_disclosures_are_views_and_processing_and_feedback_are_disabled() {
    let mut h = fixture();
    let before = inspect(&h);
    let history = h.state().session.active().unwrap().history.past_len();
    h.get_by_label("Remove").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["tab"], "remove");
    for label in ["Remove", "Heal", "Clone", "Use Generative AI", "Detect Objects", "Size", "Show Overlay on Hover", "Visualize Spots", "Provide feedback"] {
        assert_eq!(h.query_all_by_label(label).filter(|n| n.accesskit_node().is_disabled()).count(), 1, "{label}");
    }
    h.get_by_label("Reflections").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["removeGroups"], json!(["Reflections"]));
    h.key_press(egui::Key::Space);
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["removeGroups"], json!([]));
    h.get_by_label("People").click();
    h.run_steps(4);
    h.get_by_label("Dust").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["removeGroups"], json!(["People", "Dust"]));
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}
#[test]
fn invalid_remove_views_are_atomic_and_help_owns_escape_and_clears_on_close() {
    let mut h = fixture();
    let before = inspect(&h);
    for v in [json!({"removeGroups":["Unknown"]}), json!({"removeGroups":["Dust","Dust"]}), json!({"removeGroups":null}), json!({"removeHelp":"yes"})] {
        assert!(invoke(&mut h, json!({"set":{"exposure":2},"workspace":v})).is_err());
        assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
        assert_eq!(inspect(&h)["cameraRaw"]["workspace"], before["cameraRaw"]["workspace"]);
    }
    invoke(&mut h, json!({"workspace":{"tab":"remove"}})).unwrap();
    h.get_by_label("About Remove").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["removeHelp"], true);
    h.get_by_label("Remove, Heal and Clone are not available in Camera Raw yet.");
    h.key_press(egui::Key::Y);
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["before"], before["cameraRaw"]["before"]);
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!inspect(&h)["cameraRaw"].is_null());
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["removeHelp"], false);
    invoke(&mut h, json!({"workspace":{"removeHelp":true}})).unwrap();
    invoke(&mut h, json!({"cancel":true})).unwrap();
    let ctx = h.ctx.clone();
    camera_raw_ui::open(h.state_mut(), &ctx).unwrap();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["removeHelp"], false);
}
