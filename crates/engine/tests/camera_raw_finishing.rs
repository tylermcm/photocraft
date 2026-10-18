use photocraft_engine::{Session, lens_cmds::raw_params};
use serde_json::json;

#[test]
fn new_controls_validate_without_rejecting_an_unchanged_legacy_curve() {
    let p = raw_params("filter.cameraRaw", &json!({"pointCurve":[[0,0],[0,10]],"glow":{"amount":40,"style":"halation"}})).unwrap();
    assert_eq!(p.glow.amount, 40.0);
    for bad in [
        json!({"glow":{"amount":101}}),
        json!({"glow":{"spread":-1}}),
        json!({"glow":{"style":"invalid"}}),
        json!({"pointColors":[{"color":[2,0,0]}]}),
        json!({"pointColors":[{"variance":101}]}),
    ] {
        assert!(raw_params("filter.cameraRaw", &bad).is_err());
    }
}
#[test]
fn finishing_commits_as_one_reversible_edit_and_bad_settings_leave_history_alone() {
    for depth in [8, 16, 32] {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":16,"height":16,"depth":depth})).unwrap();
        s.execute("layer.new.layer", json!({})).unwrap();
        s.execute("edit.fill", json!({"color":"#e03020"})).unwrap();
        let history = s.active().unwrap().history.past_len();
        assert!(s.execute("filter.cameraRaw", json!({"glow":{"amount":999}})).is_err());
        assert_eq!(s.active().unwrap().history.past_len(), history);
        s.execute("filter.cameraRaw", json!({"glow":{"amount":40,"style":"bloom"},"pointColors":[{"color":[0.88,0.19,0.13],"hue":40}]})).unwrap();
        assert_eq!(s.active().unwrap().history.past_len(), history + 1);
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.active().unwrap().history.past_len(), history);
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(s.active().unwrap().history.past_len(), history + 1);
    }
}
