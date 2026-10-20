use photocraft_algo::camera_raw::{CameraRaw, LocalAdjust, LocalMask, MaskShape, develop};

const W: usize = 64;
const H: usize = 48;

fn grey(v: f32) -> Vec<[f32; 4]> {
    vec![[v, v, v, 1.0]; W * H]
}

fn run(p: &CameraRaw, base: f32) -> Vec<[f32; 4]> {
    let mut px = grey(base);
    develop(&mut px, W, H, p, false);
    px
}

fn at(px: &[[f32; 4]], x: usize, y: usize) -> f32 {
    px[y * W + x][0]
}

/// What +1 stop does to this grey everywhere (the global control): what a fully covering mask must give.
fn lifted(base: f32) -> f32 {
    at(&run(&CameraRaw { exposure: 1.0, ..Default::default() }, base), 0, 0)
}

/// A gradient covering the whole picture: its full end lies beyond the bottom edge, so every pixel is on the covered side.
const EVERYWHERE: MaskShape = MaskShape::Linear { x0: 0.5, y0: 2.5, x1: 0.5, y1: 3.0 };

fn brighten(shape: MaskShape) -> CameraRaw {
    CameraRaw { masks: vec![LocalMask { shape, adjust: LocalAdjust { exposure: 1.0, ..Default::default() }, ..Default::default() }], ..Default::default() }
}

#[test]
fn no_masks_or_neutral_masks_change_nothing() {
    let p = CameraRaw::default();
    assert!(p.is_identity());
    let neutral = CameraRaw { masks: vec![LocalMask::default()], ..Default::default() };
    assert!(neutral.is_identity(), "a mask with no adjustments does nothing");
    assert_eq!(run(&neutral, 0.4), grey(0.4));
    let off = CameraRaw { masks: vec![LocalMask { enabled: false, adjust: LocalAdjust { exposure: 2.0, ..Default::default() }, ..Default::default() }], ..Default::default() };
    assert!(off.is_identity() && run(&off, 0.4) == grey(0.4), "a switched-off mask changes nothing");
    let zero = CameraRaw { masks: vec![LocalMask { amount: 0.0, adjust: LocalAdjust { exposure: 2.0, ..Default::default() }, ..Default::default() }], ..Default::default() };
    assert!(zero.is_identity());
    assert!(!brighten(MaskShape::default()).is_identity());
}

#[test]
fn a_linear_gradient_is_full_at_its_start_none_at_its_end_and_about_half_between() {
    // Top (y = 0) full, bottom (y = 1) none.
    let p = brighten(MaskShape::Linear { x0: 0.5, y0: 0.0, x1: 0.5, y1: 1.0 });
    let out = run(&p, 0.3);
    let (top, middle, bottom) = (at(&out, 10, 0), at(&out, 10, H / 2), at(&out, 10, H - 1));
    assert!(top > middle && middle > bottom, "{top} > {middle} > {bottom}");
    assert!(bottom - 0.3 < 0.02, "the far end is barely touched: {bottom}");
    let full = at(&run(&brighten(EVERYWHERE), 0.3), 10, 10);
    assert!((full - lifted(0.3)).abs() < 1e-5, "a covering mask is the global control: {full} vs {}", lifted(0.3));
    assert!(top <= full && full - top < 0.01, "the start row is (almost) fully covered: {top} vs {full}");
    assert!((at(&out, 3, 20) - at(&out, 60, 20)).abs() < 1e-6, "a vertical gradient does not vary across a row");
    let inverted = CameraRaw { masks: vec![LocalMask { invert: true, ..p.masks[0].clone() }], ..Default::default() };
    let flipped = run(&inverted, 0.3);
    assert!(at(&flipped, 10, 0) < at(&flipped, 10, H - 1), "invert swaps which end is covered");
}

#[test]
fn a_radial_mask_is_full_inside_none_outside_and_feathers_between() {
    let hard = brighten(MaskShape::Radial { cx: 0.5, cy: 0.5, rx: 0.25, ry: 0.25, angle: 0.0, feather: 0.0 });
    let out = run(&hard, 0.3);
    let (inside, outside) = (at(&out, W / 2, H / 2), at(&out, 1, 1));
    assert!((inside - lifted(0.3)).abs() < 1e-5 && (outside - 0.3).abs() < 1e-6, "inside {inside} outside {outside}");
    let soft = brighten(MaskShape::Radial { cx: 0.5, cy: 0.5, rx: 0.4, ry: 0.4, angle: 0.0, feather: 100.0 });
    let out = run(&soft, 0.3);
    let centre = at(&out, W / 2, H / 2);
    let edge = at(&out, W / 2 + (0.2 * W as f32) as usize, H / 2);
    assert!(centre > edge && edge > 0.3, "{centre} > {edge} > 0.3");
    // An ellipse wider than tall covers more along x than along y at the same distance.
    let wide = brighten(MaskShape::Radial { cx: 0.5, cy: 0.5, rx: 0.45, ry: 0.15, angle: 0.0, feather: 0.0 });
    let out = run(&wide, 0.3);
    assert!(at(&out, W / 2 + 20, H / 2) > at(&out, W / 2, H / 2 + 14), "wider along x than along y");
    // Turned a quarter, it is the other way.
    let turned = brighten(MaskShape::Radial { cx: 0.5, cy: 0.5, rx: 0.45, ry: 0.15, angle: 90.0, feather: 0.0 });
    let out = run(&turned, 0.3);
    assert!(at(&out, W / 2 + 20, H / 2) < at(&out, W / 2, H / 2 + 14) + 1e-6);
}

#[test]
fn a_raster_mask_covers_where_the_map_does_at_any_picture_size() {
    // Left half covered in a 4 x 2 map.
    let coverage = [1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0];
    let p = brighten(MaskShape::raster(4, 2, &coverage));
    let out = run(&p, 0.3);
    assert!((at(&out, 2, 10) - lifted(0.3)).abs() < 1e-4 && (at(&out, W - 3, 10) - 0.3).abs() < 1e-6, "left covered, right not");
    // The same map on a bigger picture covers the same fraction of it.
    let mut big = vec![[0.3, 0.3, 0.3, 1.0]; 256 * 192];
    develop(&mut big, 256, 192, &p, false);
    assert!((big[96 * 256 + 8][0] - lifted(0.3)).abs() < 1e-4 && (big[96 * 256 + 250][0] - 0.3).abs() < 1e-6);
    // The picture is soft where the map changes (bilinear), not stepped.
    let row: Vec<f32> = (0..256).map(|x| big[96 * 256 + x][0]).collect();
    let steps = row.windows(2).filter(|w| (w[1] - w[0]).abs() > 0.05).count();
    assert!(steps <= 2, "{steps} abrupt steps in a row");
}

#[test]
fn local_adjustments_use_the_same_controls_as_global_ones() {
    // A mask covering everything with Saturation equals the global slider.
    let local = CameraRaw {
        masks: vec![LocalMask { shape: EVERYWHERE, adjust: LocalAdjust { saturation: 40.0, ..Default::default() }, ..Default::default() }],
        ..Default::default()
    };
    let global = CameraRaw { saturation: 40.0, ..Default::default() };
    let colour = vec![[0.6, 0.3, 0.2, 1.0]; W * H];
    let (mut a, mut b) = (colour.clone(), colour);
    develop(&mut a, W, H, &local, false);
    develop(&mut b, W, H, &global, false);
    assert!(a.iter().zip(&b).all(|(x, y)| (0..3).all(|c| (x[c] - y[c]).abs() < 1e-5)), "a fully covering mask is the global control");
    // Amount scales the effect.
    let half = CameraRaw { masks: vec![LocalMask { amount: 50.0, ..local.masks[0].clone() }], ..Default::default() };
    let mut c = vec![[0.6, 0.3, 0.2, 1.0]; W * H];
    develop(&mut c, W, H, &half, false);
    let (g, m, u) = (b[0][0] - 0.6, c[0][0] - 0.6, a[0][0] - 0.6);
    assert!((m - g * 0.5).abs() < 1e-4 && (u - g).abs() < 1e-4, "half amount is half the change");
    // Masks stack: a second mask adds its own change.
    let both = CameraRaw { masks: vec![brighten(MaskShape::default()).masks[0].clone(), local.masks[0].clone()], ..Default::default() };
    assert!(run(&both, 0.3)[0][0] > run(&brighten(MaskShape::default()), 0.3)[0][0] - 1e-6);
}

#[test]
fn masks_that_could_hurt_are_rejected() {
    let mask = |shape: MaskShape| CameraRaw { masks: vec![LocalMask { shape, ..Default::default() }], ..Default::default() };
    assert!(mask(MaskShape::default()).validate().is_ok());
    assert!(mask(MaskShape::Linear { x0: f32::NAN, y0: 0.0, x1: 1.0, y1: 1.0 }).validate().is_err());
    assert!(mask(MaskShape::Radial { cx: 0.5, cy: 0.5, rx: -1.0, ry: 0.2, angle: 0.0, feather: 0.0 }).validate().is_err());
    assert!(mask(MaskShape::Radial { cx: 0.5, cy: 0.5, rx: 0.2, ry: 0.2, angle: 0.0, feather: 140.0 }).validate().is_err());
    assert!(mask(MaskShape::Raster { w: 0, h: 4, data: String::new() }).validate().is_err());
    assert!(mask(MaskShape::Raster { w: 100_000, h: 100_000, data: "AAAA".into() }).validate().is_err(), "absurd size");
    assert!(mask(MaskShape::Raster { w: 4, h: 4, data: "not a map".into() }).validate().is_err(), "damaged data");
    assert!(mask(MaskShape::raster(4, 4, &[0.5; 16])).validate().is_ok());
    let mut bad = brighten(MaskShape::default());
    bad.masks[0].amount = 140.0;
    assert!(bad.validate().is_err());
    let mut bad = brighten(MaskShape::default());
    bad.masks[0].adjust.exposure = 9.0;
    assert!(bad.validate().is_err());
    let many = CameraRaw { masks: vec![LocalMask::default(); 17], ..Default::default() };
    assert!(many.validate().is_err(), "too many masks");
    // A damaged mask is skipped by develop, never a crash.
    let mut px = grey(0.3);
    develop(&mut px, W, H, &mask(MaskShape::Raster { w: 4, h: 4, data: "garbage!".into() }), false);
    assert_eq!(px, grey(0.3));
}

#[test]
fn masks_are_saved_and_read_back_exactly() {
    let mut p = brighten(MaskShape::raster(3, 2, &[0.0, 0.25, 0.5, 0.75, 1.0, 1.0]));
    p.masks[0].name = "Subject".into();
    p.masks[0].invert = true;
    p.masks.push(LocalMask { shape: MaskShape::Radial { cx: 0.3, cy: 0.6, rx: 0.2, ry: 0.1, angle: 20.0, feather: 35.0 }, ..Default::default() });
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["masks"][0]["shape"]["type"], "raster");
    assert_eq!(json["masks"][1]["shape"]["type"], "radial");
    let back: CameraRaw = serde_json::from_value(json).unwrap();
    assert_eq!(back, p);
    // Settings from before masks existed still open.
    let old: CameraRaw = serde_json::from_str(r#"{"exposure":0.5}"#).unwrap();
    assert!(old.masks.is_empty());
    // An unknown field in a mask is an error, not silently dropped.
    assert!(serde_json::from_str::<LocalMask>(r#"{"shape":{"type":"linear","x0":0,"y0":0,"x1":1,"y1":1},"bogus":1}"#).is_err());
    assert!(serde_json::from_str::<LocalMask>(r#"{"shape":{"type":"mystery"}}"#).is_err());
}

/// Left half dark, right half bright, with a red patch top-left: for the Range masks.
fn two_tone() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if x < 8 && y < 8 {
                [0.8, 0.1, 0.1, 1.0]
            } else if x < W / 2 {
                [0.1, 0.1, 0.1, 1.0]
            } else {
                [0.8, 0.8, 0.8, 1.0]
            }
        })
        .collect()
}

fn develop_two_tone(shape: MaskShape) -> Vec<[f32; 4]> {
    let mut px = two_tone();
    develop(&mut px, W, H, &brighten(shape), false);
    px
}

#[test]
fn a_luminance_range_lifts_only_the_brightnesses_inside_it() {
    let before = two_tone();
    // Only the dark half: the bright half stays as it was.
    let out = develop_two_tone(MaskShape::LuminanceRange { low: 0.0, high: 0.3, smooth: 0.0 });
    assert!(out[20 * W + 20][0] > before[20 * W + 20][0] + 0.05, "dark pixels are lifted");
    assert!((out[20 * W + 50][0] - before[20 * W + 50][0]).abs() < 1e-6, "bright pixels are untouched");
    // Only the bright half.
    let out = develop_two_tone(MaskShape::LuminanceRange { low: 0.5, high: 1.0, smooth: 0.0 });
    assert!(out[20 * W + 50][0] > before[20 * W + 50][0] + 0.02 && (out[20 * W + 20][0] - before[20 * W + 20][0]).abs() < 1e-6);
    // Inverting swaps them.
    let mut p = brighten(MaskShape::LuminanceRange { low: 0.5, high: 1.0, smooth: 0.0 });
    p.masks[0].invert = true;
    let mut px = two_tone();
    develop(&mut px, W, H, &p, false);
    assert!(px[20 * W + 20][0] > before[20 * W + 20][0] + 0.05 && (px[20 * W + 50][0] - before[20 * W + 50][0]).abs() < 1e-6);
    // Smoothness makes the edge gradual: a pixel just outside the range is partly lifted.
    let hard = develop_two_tone(MaskShape::LuminanceRange { low: 0.0, high: 0.05, smooth: 0.0 });
    let soft = develop_two_tone(MaskShape::LuminanceRange { low: 0.0, high: 0.05, smooth: 100.0 });
    assert!((hard[20 * W + 20][0] - before[20 * W + 20][0]).abs() < 1e-6, "0.1 is outside 0..0.05 with no smoothing");
    assert!(soft[20 * W + 20][0] > before[20 * W + 20][0] + 0.01, "but inside the softened edge");
}

#[test]
fn a_colour_range_follows_the_colour_and_its_range() {
    let before = two_tone();
    let red = MaskShape::ColorRange { color: [0.8, 0.1, 0.1], range: 10.0, smooth: 0.0 };
    let out = develop_two_tone(red);
    assert!(out[2 * W + 2][0] > before[2 * W + 2][0] + 0.02, "the red patch is lifted");
    assert!((out[20 * W + 20][0] - before[20 * W + 20][0]).abs() < 1e-6 && (out[20 * W + 50][0] - before[20 * W + 50][0]).abs() < 1e-6, "greys are not");
    // A wide range reaches the greys too.
    let wide = develop_two_tone(MaskShape::ColorRange { color: [0.45, 0.1, 0.1], range: 100.0, smooth: 0.0 });
    assert!(wide[20 * W + 20][0] > before[20 * W + 20][0] + 0.01);
}

#[test]
fn range_masks_are_validated_and_survive_serde() {
    use photocraft_algo::camera_raw::CameraRaw;
    for bad in [
        MaskShape::ColorRange { color: [f32::NAN, 0.0, 0.0], range: 10.0, smooth: 0.0 },
        MaskShape::ColorRange { color: [0.1, 0.1, 0.1], range: 101.0, smooth: 0.0 },
        MaskShape::LuminanceRange { low: 0.6, high: 0.4, smooth: 0.0 },
        MaskShape::LuminanceRange { low: -0.1, high: 0.4, smooth: 0.0 },
        MaskShape::LuminanceRange { low: 0.1, high: 0.4, smooth: 200.0 },
    ] {
        let p = brighten(bad.clone());
        assert!(p.validate().is_err(), "{bad:?}");
    }
    for good in [MaskShape::ColorRange { color: [0.2, 0.3, 0.4], range: 40.0, smooth: 50.0 }, MaskShape::LuminanceRange { low: 0.2, high: 0.8, smooth: 30.0 }] {
        let p = brighten(good);
        assert!(p.validate().is_ok());
        let back: CameraRaw = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back.masks, p.masks);
    }
}

#[test]
fn a_raster_mask_can_be_read_back_and_rewritten() {
    let shape = MaskShape::raster_from_bytes(4, 2, &[0, 255, 0, 255, 10, 20, 30, 40]);
    assert_eq!(shape.raster_bytes(), Some((4, 2, vec![0, 255, 0, 255, 10, 20, 30, 40])));
    assert_eq!(MaskShape::default().raster_bytes(), None);
}
