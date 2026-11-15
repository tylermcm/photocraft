use super::*;
use crate::camera_raw::CameraRaw;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("retouch-patch-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Writes an 8-bit RGBA PNG from f32 RGBA samples.
fn write_png(path: &std::path::Path, w: usize, h: usize, rgba: &[f32]) {
    let mut img = photocraft_codecs::Image::new(w as u32, h as u32, photocraft_codecs::ChannelLayout::Rgba, photocraft_codecs::SampleType::U8).unwrap();
    for (o, v) in img.data_mut().iter_mut().zip(rgba) {
        *o = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    std::fs::write(path, photocraft_codecs::encode(&img, photocraft_codecs::Format::Png, &Default::default()).unwrap()).unwrap();
}

fn patch_stroke(path: &std::path::Path, region: [f32; 4], opacity: f32) -> RetouchStroke {
    RetouchStroke {
        kind: "patch".into(),
        points: vec![[0.5, 0.5]],
        size: 0.01,
        feather: 0.0,
        opacity,
        source: None,
        patch: Some(RetouchPatch { path: path.to_string_lossy().replace('\\', "/"), region }),
    }
}

fn gradient(w: usize, h: usize) -> Vec<[f32; 4]> {
    (0..w * h).map(|i| [0.1 + 0.8 * (i % w) as f32 / w as f32, 0.1 + 0.8 * (i / w) as f32 / h as f32, 0.5, 1.0]).collect()
}

const REGION: [f32; 4] = [0.25, 0.25, 0.75, 0.75];

#[test]
fn patch_stroke_round_trips_and_unknown_fields_are_rejected() {
    let json = r#"{"kind":"patch","points":[[0.5,0.5]],"size":0.25,"feather":0.0,"opacity":100.0,"source":null,"patch":{"path":"C:/x/p.png","region":[0.25,0.25,0.75,0.75]}}"#;
    let s: RetouchStroke = serde_json::from_str(json).unwrap();
    assert_eq!(s.patch.as_ref().unwrap().region, REGION);
    assert_eq!(serde_json::to_value(&s).unwrap(), serde_json::from_str::<serde_json::Value>(json).unwrap());
    let p = CameraRaw { retouch: vec![s], ..Default::default() };
    p.validate().unwrap();
    let q: CameraRaw = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(p, q);
    assert!(!p.is_identity());
    // A patch stroke at zero opacity is not an edit.
    let mut zero = p.clone();
    zero.retouch[0].opacity = 0.0;
    assert!(zero.is_identity());
    assert!(serde_json::from_str::<RetouchStroke>(&json.replace(r#""feather""#, r#""bogus":1,"feather""#)).is_err());
    assert!(serde_json::from_str::<RetouchStroke>(&json.replace(r#""region""#, r#""bogus":1,"region""#)).is_err());
    // Strokes without a patch serialise exactly as before.
    let old = r#"{"kind":"clone","points":[[0.5,0.5]],"size":0.25,"feather":50.0,"opacity":100.0,"source":[0.25,0.5]}"#;
    let o: RetouchStroke = serde_json::from_str(old).unwrap();
    assert_eq!(serde_json::to_value(&o).unwrap(), serde_json::from_str::<serde_json::Value>(old).unwrap());
}

#[test]
fn invalid_patch_shapes_are_rejected() {
    let good = patch_stroke(&std::env::temp_dir().join("p.png"), REGION, 100.0);
    validate(std::slice::from_ref(&good)).unwrap();
    let bad = |f: &dyn Fn(&mut RetouchStroke)| {
        let mut s = good.clone();
        f(&mut s);
        assert!(validate(&[s]).is_err());
    };
    bad(&|s| s.patch = None);
    bad(&|s| s.kind = "clone".into());
    bad(&|s| {
        s.kind = "heal".into();
        s.source = Some([0.1, 0.1]);
    });
    bad(&|s| s.patch.as_mut().unwrap().path = "relative/p.png".into());
    bad(&|s| s.patch.as_mut().unwrap().path = String::new());
    bad(&|s| s.patch.as_mut().unwrap().path = format!("{}/{}", std::env::temp_dir().to_string_lossy(), "a".repeat(600)));
    bad(&|s| s.patch.as_mut().unwrap().region = [0.5, 0.2, 0.5, 0.8]);
    bad(&|s| s.patch.as_mut().unwrap().region = [0.6, 0.2, 0.4, 0.8]);
    bad(&|s| s.patch.as_mut().unwrap().region = [0.2, 0.8, 0.8, 0.2]);
    bad(&|s| s.patch.as_mut().unwrap().region = [-0.1, 0.0, 0.5, 0.5]);
    bad(&|s| s.patch.as_mut().unwrap().region = [0.0, 0.0, 1.5, 0.5]);
    bad(&|s| s.patch.as_mut().unwrap().region = [f32::NAN, 0.0, 0.5, 0.5]);
    bad(&|s| s.patch.as_mut().unwrap().region = [0.0, 0.0, f32::INFINITY, 0.5]);
    bad(&|s| s.size = 0.0);
    bad(&|s| s.opacity = 101.0);
    bad(&|s| s.points.clear());
    // Too large a region for the area bound.
    let full = patch_stroke(&std::env::temp_dir().join("p.png"), [0.0, 0.0, 1.0, 1.0], 100.0);
    assert!(validate_size(std::slice::from_ref(&full), 4000, 4000).is_ok());
    assert!(validate_size(&[full], 5000, 5000).is_err());
}

#[test]
fn a_picture_cropped_from_the_frame_leaves_it_unchanged() {
    let d = dir("identity");
    let (w, h) = (64, 48);
    let frame = gradient(w, h);
    let (x0, y0, x1, y1) = patch_rect(REGION, w, h);
    let (rw, rh) = (x1 - x0, y1 - y0);
    let mut rgba = Vec::new();
    for y in 0..rh {
        for x in 0..rw {
            let inside = (4..rw - 4).contains(&x) && (4..rh - 4).contains(&y);
            let p = frame[(y + y0) * w + x + x0];
            rgba.extend_from_slice(&[p[0], p[1], p[2], if inside { 1.0 } else { 0.0 }]);
        }
    }
    let file = d.join("crop.png");
    write_png(&file, rw, rh, &rgba);
    let mut px = frame.clone();
    apply(&mut px, w, h, &[patch_stroke(&file, REGION, 100.0)]);
    let worst = px.iter().zip(&frame).flat_map(|(a, b)| (0..4).map(move |c| (a[c] - b[c]).abs())).fold(0.0, f32::max);
    assert!(worst < 0.01, "worst error {worst}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_filled_picture_replaces_the_middle_and_nothing_outside_the_region() {
    let d = dir("replace");
    let (w, h) = (64, 48);
    let (pw, ph) = (32, 24);
    // The surroundings as they are (0.2), a new colour (0.8) in the middle, a feathered edge around it.
    let mut rgba = Vec::new();
    for y in 0..ph {
        for x in 0..pw {
            let core = (8..24).contains(&x) && (6..18).contains(&y);
            let ring = (6..26).contains(&x) && (4..20).contains(&y);
            let (v, a) = if core {
                (0.8, 1.0)
            } else if ring {
                (0.8, 0.5)
            } else {
                (0.2, 0.0)
            };
            rgba.extend_from_slice(&[v, v, v, a]);
        }
    }
    let file = d.join("fill.png");
    write_png(&file, pw, ph, &rgba);
    let frame = vec![[0.2, 0.2, 0.2, 1.0]; w * h];
    let mut px = frame.clone();
    apply(&mut px, w, h, &[patch_stroke(&file, REGION, 100.0)]);
    let c = px[24 * w + 32];
    assert!(c[0] > 0.7 && c[1] > 0.7 && c[2] > 0.7, "centre {c:?}");
    assert_eq!(c[3], 1.0);
    let (x0, y0, x1, y1) = patch_rect(REGION, w, h);
    for y in 0..h {
        for x in 0..w {
            if !(x0..x1).contains(&x) || !(y0..y1).contains(&y) {
                assert_eq!(px[y * w + x], frame[y * w + x], "({x},{y})");
            }
        }
    }
    // Half opacity lands about halfway.
    let mut half = frame.clone();
    apply(&mut half, w, h, &[patch_stroke(&file, REGION, 50.0)]);
    assert!((half[24 * w + 32][0] - 0.5).abs() < 0.08, "{:?}", half[24 * w + 32]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_missing_or_unreadable_file_does_nothing() {
    let d = dir("missing");
    let (w, h) = (32, 32);
    let frame = gradient(w, h);
    let mut px = frame.clone();
    apply(&mut px, w, h, &[patch_stroke(&d.join("nope.png"), REGION, 100.0)]);
    assert_eq!(px, frame);
    let junk = d.join("junk.png");
    std::fs::write(&junk, b"not a picture").unwrap();
    apply(&mut px, w, h, &[patch_stroke(&junk, REGION, 100.0)]);
    assert_eq!(px, frame);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_changed_file_is_picked_up() {
    let d = dir("cache");
    let (w, h) = (32, 32);
    let file = d.join("flat.png");
    let flat = |v: f32| -> Vec<f32> { (0..8 * 8).flat_map(|_| [v, v, v, 1.0]).collect() };
    // The region covers the whole frame, so the picture has no unmasked ring to carry a mismatch inward.
    let whole = [0.0, 0.0, 1.0, 1.0];
    write_png(&file, 8, 8, &flat(0.9));
    let mut a = vec![[0.1, 0.1, 0.1, 1.0]; w * h];
    apply(&mut a, w, h, &[patch_stroke(&file, whole, 100.0)]);
    assert!(a[16 * w + 16][0] > 0.85, "{:?}", a[16 * w + 16]);
    write_png(&file, 8, 8, &flat(0.3));
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(later).unwrap();
    let mut b = vec![[0.1, 0.1, 0.1, 1.0]; w * h];
    apply(&mut b, w, h, &[patch_stroke(&file, whole, 100.0)]);
    assert!((b[16 * w + 16][0] - 0.3).abs() < 0.05, "{:?}", b[16 * w + 16]);
    let _ = std::fs::remove_dir_all(&d);
}
