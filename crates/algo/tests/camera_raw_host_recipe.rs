use photocraft_algo::camera_raw::{CameraRaw, develop};

fn run(p: &CameraRaw, px: [f32; 3]) -> [f32; 3] {
    let mut v = [[px[0], px[1], px[2], 1.0]];
    develop(&mut v, 1, 1, p, false);
    [v[0][0], v[0][1], v[0][2]]
}

#[test]
fn super_resolution_is_a_recipe_flag_not_a_pixel_edit() {
    let p = CameraRaw { super_resolution: true, ..Default::default() };
    assert!(!p.is_identity(), "a recipe with only Super Resolution is still an edit");
    assert_eq!(run(&p, [0.3, 0.5, 0.7]), [0.3, 0.5, 0.7]);
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["superResolution"], true);
}
