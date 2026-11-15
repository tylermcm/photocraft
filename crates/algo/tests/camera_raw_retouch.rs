use photocraft_algo::camera_raw::{CameraRaw, RetouchStroke, develop, per_pixel};
fn stroke(kind: &str) -> RetouchStroke {
    RetouchStroke {
        kind: kind.into(),
        points: vec![[0.5, 0.5]],
        size: 0.2,
        feather: 50.0,
        opacity: 100.0,
        source: if ["clone", "heal"].contains(&kind) { Some([0.2, 0.2]) } else { None },
        patch: None,
    }
}
#[test]
fn validation_roundtrip_and_cpu_fallback() {
    for kind in ["remove", "spot", "heal", "clone"] {
        let p = CameraRaw { retouch: vec![stroke(kind)], ..Default::default() };
        p.validate().unwrap();
        assert!(!p.is_identity());
        assert!(per_pixel(&p, 32, 32, false, None).is_none());
        let q: CameraRaw = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(p, q);
    }
    for value in [
        serde_json::json!({"kind":"clone","points":[[0.5,0.5]],"size":0.1,"feather":50,"opacity":100}),
        serde_json::json!({"kind":"oops","points":[[0.5,0.5]],"size":0.1,"feather":50,"opacity":100}),
    ] {
        let s: RetouchStroke = serde_json::from_value(value).unwrap();
        assert!(CameraRaw { retouch: vec![s], ..Default::default() }.validate().is_err());
    }
}
#[test]
fn clone_replaces_only_covered_pixels_and_preserves_alpha() {
    let mut px = vec![[0.2, 0.3, 0.4, 0.7]; 64 * 64];
    for y in 27..37 {
        for x in 27..37 {
            px[y * 64 + x] = [0.9, 0.1, 0.1, 0.7];
        }
    }
    let before = px.clone();
    let p = CameraRaw { retouch: vec![stroke("clone")], ..Default::default() };
    develop(&mut px, 64, 64, &p, true);
    assert!(px[32 * 64 + 32].iter().zip(before[12 * 64 + 12]).all(|(a, b)| (a - b).abs() < 1e-6));
    assert_eq!(px[0], before[0]);
    assert!(px.iter().all(|p| p[3] == 0.7));
}
#[test]
fn completion_tools_remove_small_defect_without_touching_outside() {
    for kind in ["remove", "spot", "heal"] {
        let mut px = vec![[0.3, 0.3, 0.3, 1.0]; 48 * 48];
        for y in 22..26 {
            for x in 22..26 {
                px[y * 48 + x] = [0.9, 0.9, 0.9, 1.0];
            }
        }
        let p = CameraRaw { retouch: vec![stroke(kind)], ..Default::default() };
        develop(&mut px, 48, 48, &p, false);
        assert!(px[24 * 48 + 24][0] < 0.6, "{kind}");
        assert_eq!(px[0], [0.3, 0.3, 0.3, 1.0]);
    }
}

#[test]
fn dense_domain_budget_is_checked_before_rendering() {
    let mut s = stroke("remove");
    s.points = vec![[0.0, 0.0], [1.0, 1.0]];
    s.size = 0.5;
    let p = CameraRaw { retouch: vec![s], ..Default::default() };
    assert!(p.validate_retouch_size(8000, 6000).is_err());
    assert!(p.validate_retouch_size(800, 600).is_ok());
}
#[test]
fn source_outside_canvas_does_not_destroy_edge_pixels() {
    let mut s = stroke("clone");
    s.points = vec![[0.0, 0.0]];
    s.source = Some([1.0, 1.0]);
    let p = CameraRaw { retouch: vec![s], ..Default::default() };
    let mut px = vec![[0.2, 0.3, 0.4, 0.7]; 64 * 64];
    let before = px.clone();
    develop(&mut px, 64, 64, &p, true);
    assert_eq!(px, before);
}

#[test]
fn remove_respects_feather_at_the_painted_edge() {
    let mut base = vec![[0.3, 0.3, 0.3, 1.0]; 64 * 64];
    for y in 26..38 {
        for x in 26..38 {
            base[y * 64 + x] = [0.9, 0.9, 0.9, 1.0];
        }
    }
    let mut s = stroke("remove");
    s.feather = 0.0;
    let mut hard = base.clone();
    develop(&mut hard, 64, 64, &CameraRaw { retouch: vec![s.clone()], ..Default::default() }, false);
    s.feather = 100.0;
    let mut soft = base;
    develop(&mut soft, 64, 64, &CameraRaw { retouch: vec![s], ..Default::default() }, false);
    assert!(soft[32 * 64 + 37][0] > hard[32 * 64 + 37][0] + 0.1);
}
