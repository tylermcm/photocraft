use photocraft_algo::camera_raw::{CameraRaw, GlowStyle, PointColor, develop};
#[test]
fn neutral_samples_are_exact_identity_and_selected_colors_change_only_their_range() {
    let original = vec![[0.9, 0.1, 0.1, 1.0], [0.1, 0.1, 0.9, 1.0], [0.5, 0.5, 0.5, 1.0]];
    let mut px = original.clone();
    let mut p = CameraRaw::default();
    p.point_colors.push(PointColor { color: [0.9, 0.1, 0.1], ..Default::default() });
    develop(&mut px, 3, 1, &p, false);
    assert_eq!(px, original);
    p.point_colors[0].hue = 50.0;
    develop(&mut px, 3, 1, &p, false);
    assert_ne!(px[0], original[0]);
    assert_eq!(px[1..], original[1..]);
    assert_eq!(serde_json::from_str::<CameraRaw>(&serde_json::to_string(&p).unwrap()).unwrap(), p);
}
#[test]
fn glow_spreads_bright_light_preserves_alpha_and_rejects_bad_controls() {
    for style in [GlowStyle::Diffusion, GlowStyle::Bloom, GlowStyle::Halation] {
        let mut p = CameraRaw::default();
        p.glow.amount = 80.0;
        p.glow.style = style;
        let mut px = vec![[0.0, 0.0, 0.0, 1.0]; 81];
        px[40] = [1.0; 4];
        px[0] = [1.0, 0.0, 0.0, 0.0];
        let alpha: Vec<f32> = px.iter().map(|p| p[3]).collect();
        develop(&mut px, 9, 9, &p, false);
        assert!(px[39][0] > 0.0);
        assert_eq!(px.iter().map(|p| p[3]).collect::<Vec<_>>(), alpha);
        assert!(px.iter().flatten().all(|v| v.is_finite()));
    }
    let mut p = CameraRaw::default();
    p.glow.spread = f32::NAN;
    assert!(p.validate().is_err());
    p = CameraRaw::default();
    p.point_colors = vec![PointColor::default(); 9];
    assert!(p.validate().is_err());
}

#[test]
fn finishing_detail_controls_have_real_effects_and_zero_amount_is_identity() {
    let mut source = vec![[0.6, 0.6, 0.6, 1.0]; 129];
    source[64] = [1.0; 4];
    let mut p = CameraRaw::default();
    p.glow.amount = 80.0;
    p.glow.style = GlowStyle::Bloom;
    let render = |p: &CameraRaw| {
        let mut out = source.clone();
        develop(&mut out, 129, 1, p, false);
        out
    };
    let base = render(&p);
    p.glow.spread = 100.0;
    assert_ne!(render(&p), base);
    p.glow.spread = 50.0;
    p.glow.range = 100.0;
    assert_ne!(render(&p), base);
    p.glow.range = 50.0;
    p.glow.warmth = 100.0;
    assert_ne!(render(&p), base);
    p.glow.amount = 0.0;
    assert_eq!(render(&p), source);
    let target = [0.9, 0.1, 0.1];
    let input = vec![[0.7, 0.1, 0.1, 1.0]];
    let mut p = CameraRaw::default();
    p.point_colors.push(PointColor { color: target, variance: -50.0, range: 100.0, ..Default::default() });
    let mut out = input.clone();
    develop(&mut out, 1, 1, &p, false);
    assert!((out[0][0] - target[0]).abs() < (input[0][0] - target[0]).abs());
    assert_eq!(serde_json::from_str::<CameraRaw>("{}").unwrap(), CameraRaw::default());
}

#[test]
fn glow_does_not_introduce_nonfinite_channels_at_extreme_hdr_values() {
    let mut p = CameraRaw::default();
    p.glow.amount = 100.0;
    let mut px = vec![[f32::MAX, 0.0, 0.0, 1.0]; 81];
    develop(&mut px, 9, 9, &p, true);
    assert!(px.iter().flatten().all(|v| v.is_finite()));
}

#[test]
fn point_color_selection_uses_the_same_sdr_values_as_the_picker() {
    let p = CameraRaw { point_colors: vec![PointColor { color: [1.0, 0.0, 0.0], hue: 50.0, ..Default::default() }], ..Default::default() };
    let mut bright = vec![[2.0, 0.0, 0.0, 1.0]];
    let mut clipped = vec![[1.0, 0.0, 0.0, 1.0]];
    develop(&mut bright, 1, 1, &p, false);
    develop(&mut clipped, 1, 1, &p, false);
    assert_eq!(bright, clipped);
}
