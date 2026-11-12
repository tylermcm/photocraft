use photocraft_algo::camera_raw::{CameraRaw, RedEyeCorrection, develop, per_pixel};

fn correction(point: [f32; 2]) -> RedEyeCorrection {
    RedEyeCorrection { point, pupil_size: 25.0, darken: 100.0 }
}

fn image(w: usize, h: usize, cx: usize, cy: usize, radius: usize) -> Vec<[f32; 4]> {
    let mut px = vec![[0.2, 0.25, 0.3, 0.7]; w * h];
    for y in 0..h {
        for x in 0..w {
            if (x as f32 - cx as f32).hypot(y as f32 - cy as f32) < radius as f32 {
                px[y * w + x] = [0.9, 0.1, 0.1, 0.7];
            }
        }
    }
    px
}

#[test]
fn correction_roundtrips_and_rejects_hostile_settings() {
    let mut p = CameraRaw { red_eye: vec![correction([0.5, 0.5])], ..Default::default() };
    p.validate().unwrap();
    assert!(!p.is_identity());
    assert!(per_pixel(&p, 256, 256, false, None).is_none());
    assert_eq!(p, serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap());
    p.red_eye[0].point[0] = f32::NAN;
    assert!(p.validate().is_err());
    p.red_eye = vec![correction([0.5, 0.5]); 65];
    assert!(p.validate().is_err());
    p.red_eye = vec![correction([0.5, 0.5])];
    p.red_eye[0].darken = 101.0;
    assert!(p.validate().is_err());
    p.red_eye[0].darken = 0.0;
    assert!(p.is_identity());
    p.red_eye[0].pupil_size = 0.0;
    assert!(p.validate().is_err());
}

#[test]
fn local_correction_preserves_other_eye_surroundings_and_alpha() {
    let mut px = image(256, 128, 64, 64, 12);
    for y in 54..74 {
        for x in 182..202 {
            px[y * 256 + x] = [0.9, 0.1, 0.1, 0.7];
        }
    }
    let before = px.clone();
    let p = CameraRaw { red_eye: vec![correction([0.25, 0.5])], ..Default::default() };
    develop(&mut px, 256, 128, &p, true);
    assert!(px[64 * 256 + 64][0] < 0.3);
    assert_eq!(px[64 * 256 + 192], before[64 * 256 + 192]);
    assert_eq!(px[0], before[0]);
    assert!(px.iter().all(|p| p[3] == 0.7));
}

#[test]
fn proxy_and_full_size_correct_the_same_pupil() {
    let mut full = image(256, 256, 128, 128, 12);
    let mut proxy = image(64, 64, 32, 32, 3);
    let mut p = CameraRaw { red_eye: vec![correction([0.5, 0.5])], ..Default::default() };
    develop(&mut full, 256, 256, &p, true);
    p.pixel_scale = 0.25;
    develop(&mut proxy, 64, 64, &p, true);
    assert!(full[128 * 256 + 128][0] < 0.3);
    assert!(proxy[32 * 64 + 32][0] < 0.3);
    assert_eq!(proxy[0], [0.2, 0.25, 0.3, 0.7]);
}

#[test]
fn non_red_pupils_and_zero_darken_are_exact_noops() {
    let mut px = vec![[0.2, 0.25, 0.3, 0.7]; 64 * 64];
    let before = px.clone();
    let mut p = CameraRaw { red_eye: vec![correction([0.0, 0.0]), correction([1.0, 1.0])], ..Default::default() };
    develop(&mut px, 64, 64, &p, true);
    assert_eq!(px, before);
    px = image(64, 64, 32, 32, 8);
    let before = px.clone();
    p.red_eye = vec![correction([0.5, 0.5])];
    p.red_eye[0].darken = 0.0;
    develop(&mut px, 64, 64, &p, true);
    assert_eq!(px, before);
}

#[test]
fn full_size_matches_the_existing_paint_tool_kernel() {
    let mut expected = image(256, 128, 64, 64, 12);
    let mut actual = expected.clone();
    photocraft_algo::redeye::apply(&mut expected, 256, 128, (64, 64), (0, 0), 25.0, 100.0);
    let p = CameraRaw { red_eye: vec![correction([0.25, 0.5])], ..Default::default() };
    develop(&mut actual, 256, 128, &p, true);
    let maximum = actual.iter().flatten().zip(expected.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(maximum < 1e-6, "maximum kernel difference: {maximum}");
}
