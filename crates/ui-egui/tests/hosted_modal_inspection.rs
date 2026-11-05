//! Host document inspection must remain read-only while Camera Raw owns the photo.
use photocraft_ui_egui::{PhotocraftApp, Services, control};
use serde_json::{Value, json};

fn fixture() -> (PhotocraftApp, egui::Context) {
    let ctx = egui::Context::default();
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    app.run("file.new", json!({"width": 32, "height": 24})).unwrap();
    app.run("layer.new.layer", json!({})).unwrap();
    app.raw_workspace.enabled = true;
    photocraft_ui_egui::raw_workspace::control(&mut app, &ctx, &json!({"open": true})).unwrap();
    (app, ctx)
}

fn execute(app: &mut PhotocraftApp, ctx: &egui::Context, command: &str) -> Value {
    let (request, _) = control::ControlRequest::new("engine.execute", json!({"command": command}));
    match control::handle(app, ctx, &request) {
        control::Outcome::Done(value) => value,
        _ => panic!("expected immediate reply"),
    }
}

#[test]
fn hosted_inspection_does_not_unlock_document_mutations() {
    let (mut app, ctx) = fixture();
    let history = app.session.active().unwrap().history.past_len();
    let inspected = execute(&mut app, &ctx, "session.inspect");
    assert_eq!(inspected["ok"], true, "{inspected}");
    assert_eq!(inspected["result"]["documents"].as_array().unwrap().len(), 1);
    let mutation = execute(&mut app, &ctx, "image.adjustments.invert");
    assert_eq!(mutation["ok"], false);
    assert!(mutation["error"].as_str().unwrap().contains("modal"));
    assert_eq!(app.session.active().unwrap().history.past_len(), history);
}

#[test]
fn hosted_inspection_keeps_automation_authorization() {
    let (mut app, ctx) = fixture();
    app.services.automation_command = Some(Box::new(|_, _| Err("denied by test authority".into())));
    let reply = execute(&mut app, &ctx, "session.inspect");
    assert_eq!(reply["ok"], false);
    assert!(reply["error"].as_str().unwrap().contains("denied by test authority"));
}
