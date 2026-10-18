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
fn masking_cards_are_disabled_and_people_disclosure_does_not_edit_the_photo() {
    let mut h = fixture();
    let before = inspect(&h);
    let history = h.state().session.active().unwrap().history.past_len();
    h.get_by_label("Masking").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["tab"], "masking");
    for label in ["Subject", "Sky", "Background", "Landscape", "Objects", "Brush", "Linear Gradient", "Radial Gradient", "Range"] {
        assert_eq!(h.query_all_by_label(label).filter(|n| n.accesskit_node().is_disabled()).count(), 1, "{label}");
    }
    h.get_by_label("People").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["maskingPeople"], false);
    h.key_press(egui::Key::Space);
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["maskingPeople"], true);
    for key in [egui::Key::K, egui::Key::G, egui::Key::J] {
        h.key_press(key);
        h.run_steps(4);
    }
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}
#[test]
fn help_owns_escape_and_parent_close_clears_it_and_invalid_views_are_atomic() {
    let mut h = fixture();
    let before = inspect(&h);
    for v in [json!({"maskingPeople":"yes"}), json!({"maskingHelp":null})] {
        assert!(invoke(&mut h, json!({"set":{"exposure":2},"workspace":v})).is_err());
        assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    }
    invoke(&mut h, json!({"workspace":{"tab":"masking"}})).unwrap();
    h.get_by_label("Click here to learn more about Masking and keyboard shortcuts.").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["maskingHelp"], true);
    h.get_by_label("Local masking is not available in Camera Raw yet.");
    h.key_press(egui::Key::Y);
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["before"], before["cameraRaw"]["before"]);
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!inspect(&h)["cameraRaw"].is_null());
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["maskingHelp"], false);
    invoke(&mut h, json!({"workspace":{"maskingHelp":true}})).unwrap();
    invoke(&mut h, json!({"cancel":true})).unwrap();
    assert!(inspect(&h)["cameraRaw"].is_null());
    let ctx = h.ctx.clone();
    camera_raw_ui::open(h.state_mut(), &ctx).unwrap();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["maskingHelp"], false);
}
