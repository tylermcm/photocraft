use photocraft_algo::camera_raw::{CameraRaw, develop};

fn run(p: &CameraRaw, px: [f32; 3]) -> [f32; 3] {
    let mut v = [[px[0], px[1], px[2], 1.0]];
    develop(&mut v, 1, 1, p, false);
    [v[0][0], v[0][1], v[0][2]]
}

#[test]
fn neutral_calibration_is_identity() {
    let p = CameraRaw::default();
    assert!(p.calibration_neutral() && p.is_identity());
    assert_eq!(run(&p, [0.3, 0.5, 0.7]), [0.3, 0.5, 0.7]);
}

#[test]
fn greys_stay_grey() {
    let p = CameraRaw { calib_hue: [50.0, -30.0, 80.0], calib_sat: [40.0, -60.0, 100.0], ..Default::default() };
    let o = run(&p, [0.4, 0.4, 0.4]);
    assert!((o[0] - o[1]).abs() < 1e-3 && (o[1] - o[2]).abs() < 1e-3, "{o:?}");
}

#[test]
fn red_hue_moves_toward_orange_and_saturation_scales() {
    let hue = CameraRaw { calib_hue: [100.0, 0.0, 0.0], ..Default::default() };
    let o = run(&hue, [0.8, 0.1, 0.1]);
    assert!(o[1] > 0.1 + 0.01, "positive Red Hue should add green: {o:?}");
    let desat = CameraRaw { calib_sat: [-100.0, 0.0, 0.0], ..Default::default() };
    let o = run(&desat, [0.8, 0.1, 0.1]);
    assert!(o[0] - o[1] < 0.7 - 0.05 && o[1] == o[2], "Red Saturation -100 should pull red toward grey: {o:?}");
}

#[test]
fn shadow_tint_only_affects_shadows() {
    let p = CameraRaw { calib_shadow_tint: 100.0, ..Default::default() };
    let dark = run(&p, [0.05, 0.05, 0.05]);
    let bright = run(&p, [0.9, 0.9, 0.9]);
    assert!(dark[1] < dark[0], "positive tint reduces shadow green: {dark:?}");
    assert!((bright[1] - bright[0]).abs() < 1e-3, "{bright:?}");
}

#[test]
fn out_of_range_calibration_is_rejected() {
    let p = CameraRaw { calib_hue: [0.0, 101.0, 0.0], ..Default::default() };
    assert!(p.validate_finishing().is_err());
}

#[test]
fn super_resolution_is_a_recipe_flag_not_a_pixel_edit() {
    let p = CameraRaw { super_resolution: true, ..Default::default() };
    assert!(!p.is_identity(), "a recipe with only Super Resolution is still an edit");
    assert_eq!(run(&p, [0.3, 0.5, 0.7]), [0.3, 0.5, 0.7]);
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["superResolution"], true);
}
