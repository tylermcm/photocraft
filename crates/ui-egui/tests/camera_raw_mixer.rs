use egui::{accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use photocraft_ui_egui::{PhotocraftApp, camera_raw_ui, control, theme::ThemeKind};
use serde_json::{Value, json};
fn fixture() -> Harness<'static, PhotocraftApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 1400.0)).build_eframe(|cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, ThemeKind::ProMedium);
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":64,"height":48})).unwrap();
        app.run("layer.new.layer", json!({})).unwrap();
        app.run("edit.fill", json!({"color":"#3366cc"})).unwrap();
        camera_raw_ui::open(&mut app, &cc.egui_ctx).unwrap();
        app
    });
    let ctx = h.ctx.clone();
    camera_raw_ui::menu(h.state_mut(), &ctx, "filter.cameraRaw", &json!({"ui":{"workspace":{"panels":{"open":["Color Mixer"]},"mixer":"color"}}}))
        .unwrap()
        .unwrap();
    h.run_steps(5);
    h
}
fn inspect(h: &Harness<'_, PhotocraftApp>) -> Value {
    control::inspect(h.state(), &h.ctx)
}
#[test]
fn measured_mixer_keeps_band_selection_as_view_state_and_exposes_point_and_all_color_views() {
    let mut h = fixture();
    let before = inspect(&h);
    let history = h.state().session.active().unwrap().history.past_len();
    assert!(!h.get_by_label("Point Color").accesskit_node().is_disabled());
    assert!(!h.get_by_label("All Colors").accesskit_node().is_disabled());
    h.get_by_label("Mixer");
    h.get_by_role_and_label(Role::ComboBox, "Adjust");
    h.get_by_label("Blue").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["band"], 5);
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}
#[test]
fn color_slider_edits_only_its_band_then_cancel_restores_the_document() {
    let mut h = fixture();
    let history = h.state().session.active().unwrap().history.past_len();
    h.get_by_label("Blue").click();
    h.run_steps(4);
    let before_pixels = inspect(&h)["cameraRaw"]["histogram"].clone();
    let rect = h.get_by_role_and_label(Role::Slider, "Hue").rect();
    let at = egui::pos2(rect.left() + rect.width() * 0.75, rect.center().y);
    h.event(egui::Event::PointerMoved(at));
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::default() });
    h.run_steps(2);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() });
    h.run_steps(4);
    let v = inspect(&h);
    let values = v["cameraRaw"]["params"]["hslHue"].as_array().unwrap();
    assert!(values[5].as_f64().unwrap() > 40.0);
    assert!(
        ["red", "green", "blue"].iter().any(|key| v["cameraRaw"]["histogram"][key] != before_pixels[key]),
        "the blue-band change must affect actual preview bins"
    );
    assert!(values.iter().enumerate().all(|(i, v)| i == 5 || v.as_f64() == Some(0.0)));
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
    let ctx = h.ctx.clone();
    camera_raw_ui::menu(h.state_mut(), &ctx, "filter.cameraRaw", &json!({"ui":{"cancel":true}})).unwrap().unwrap();
    h.run_steps(4);
    assert!(inspect(&h)["cameraRaw"].is_null());
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}
#[test]
fn adjust_mode_popup_preserves_the_existing_hsl_rows_without_editing_parameters() {
    let mut h = fixture();
    let before = inspect(&h);
    h.get_by_role_and_label(Role::ComboBox, "Adjust").click();
    h.run_steps(4);
    h.get_by_label("HSL").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["mixer"], "hsl");
    h.get_by_label("Red");
    h.get_by_label("Magenta");
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
}

#[test]
fn all_colors_is_an_overview_of_the_existing_eight_bands() {
    let mut h = fixture();
    let before = inspect(&h);
    let history = h.state().session.active().unwrap().history.past_len();
    h.get_by_label("All Colors").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["mixerTools"]["allColors"], true);
    assert_eq!(inspect(&h)["cameraRaw"]["params"], before["cameraRaw"]["params"]);
    let rect = h.query_all_by_label("Hue").find(|n| n.accesskit_node().role() == Role::Slider).unwrap().rect();
    let at = egui::pos2(rect.left() + rect.width() * 0.75, rect.center().y);
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.run_steps(3);
    }
    let edited = inspect(&h);
    assert!(edited["cameraRaw"]["params"]["hslHue"][0].as_f64().unwrap() > 20.0);
    assert!(edited["cameraRaw"]["params"]["hslHue"].as_array().unwrap()[1..].iter().all(|v| v.as_f64() == Some(0.0)));
    h.get_by_role_and_label(Role::Button, "Blue").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["band"], 5);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["mixerTools"]["allColors"], false);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}
#[test]
fn point_color_samples_the_photo_adjusts_pixels_and_cancel_leaves_document_untouched() {
    let mut h = fixture();
    let history = h.state().session.active().unwrap().history.past_len();
    h.get_by_label("Point Color").click();
    h.run_steps(4);
    h.get_by_label("Sample Color").click();
    h.run_steps(3);
    let state = inspect(&h);
    let rect = state["cameraRaw"]["previewRect"].as_array().unwrap();
    let at = egui::pos2(
        ((rect[0].as_f64().unwrap() + rect[2].as_f64().unwrap()) / 2.0) as f32,
        ((rect[1].as_f64().unwrap() + rect[3].as_f64().unwrap()) / 2.0) as f32,
    );
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.run_steps(3);
    }
    let sampled = inspect(&h);
    assert_eq!(sampled["cameraRaw"]["params"]["pointColors"].as_array().unwrap().len(), 1);
    let before = sampled["cameraRaw"]["histogram"].clone();
    let mut points = sampled["cameraRaw"]["params"]["pointColors"].clone();
    points[0]["hue"] = json!(80.0);
    let ctx = h.ctx.clone();
    camera_raw_ui::menu(h.state_mut(), &ctx, "filter.cameraRaw", &json!({"ui":{"set":{"pointColors":points}}})).unwrap().unwrap();
    h.run_steps(4);
    assert_ne!(inspect(&h)["cameraRaw"]["histogram"], before);
    let params = inspect(&h)["cameraRaw"]["params"].clone();
    h.get_by_label("Visualize Range").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["cameraRaw"]["params"], params);
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
    let ctx = h.ctx.clone();
    camera_raw_ui::menu(h.state_mut(), &ctx, "filter.cameraRaw", &json!({"ui":{"cancel":true}})).unwrap().unwrap();
    assert_eq!(h.state().session.active().unwrap().history.past_len(), history);
}

#[test]
fn point_picker_releases_preview_input_when_another_tool_is_selected() {
    let mut h = fixture();
    h.get_by_label("Point Color").click();
    h.run_steps(3);
    h.get_by_label("Sample Color").click();
    h.run_steps(3);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["mixerTools"]["picker"], true);
    let params = inspect(&h)["cameraRaw"]["params"].clone();
    let ctx = h.ctx.clone();
    camera_raw_ui::menu(h.state_mut(), &ctx, "filter.cameraRaw", &json!({"ui":{"workspace":{"tab":"remove"}}})).unwrap().unwrap();
    h.run_steps(3);
    assert_eq!(inspect(&h)["cameraRaw"]["workspace"]["mixerTools"]["picker"], false);
    assert_eq!(inspect(&h)["cameraRaw"]["params"], params);
}
