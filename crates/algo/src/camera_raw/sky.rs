//! Sky replacement: a sky picture composited behind a sky matte, with the matte's edge refined,
//! the new sky matched to the scene, and the foreground relit by it.
//!
//! The recipe carries only a sky *id*, the matte (a raster mask, like any other) and the sliders.
//! The decoded sky picture reaches the stage through [`register`], which the host calls before
//! rendering (the algo crate decodes no files). A recipe whose sky is not registered leaves the
//! picture untouched, so an old recipe never fails on a machine without that sky.
//!
//! Sky pictures are expected in the same working encoding as the pixels `develop` is given.
use super::{MaskShape, gauss, guided_fast, luma, smoothstep};
use crate::photo_util::par_rows;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Longest side of a registered sky, and how many are kept (the oldest is dropped).
pub const MAX_SKY_SIDE: usize = 6000;
const MAX_REGISTERED: usize = 8;

/// A decoded sky picture: `w x h` pixels, row-major.
#[derive(Debug)]
pub struct SkyImage {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 4]>,
}

static REGISTRY: Mutex<VecDeque<(String, Arc<SkyImage>)>> = Mutex::new(VecDeque::new());

/// Makes `px` (`w x h`) available to recipes as sky `id`. Replaces an earlier sky of that id.
pub fn register(id: &str, w: usize, h: usize, px: Vec<[f32; 4]>) -> Result<(), String> {
    if id.is_empty() || id.len() > 256 {
        return Err("sky id must be 1 to 256 characters".into());
    }
    if w == 0 || h == 0 || w > MAX_SKY_SIDE || h > MAX_SKY_SIDE || w.checked_mul(h) != Some(px.len()) {
        return Err("sky picture has an unusable size".into());
    }
    let mut reg = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    reg.retain(|(k, _)| k != id);
    reg.push_back((id.to_string(), Arc::new(SkyImage { w, h, px })));
    while reg.len() > MAX_REGISTERED {
        reg.pop_front();
    }
    Ok(())
}

/// Forgets sky `id`.
pub fn unregister(id: &str) {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).retain(|(k, _)| k != id);
}

/// Whether sky `id` is registered.
pub fn is_registered(id: &str) -> bool {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(k, _)| k == id)
}

fn lookup(id: &str) -> Option<Arc<SkyImage>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| k == id).map(|(_, v)| v.clone())
}

/// Sky Replacement settings. Neutral (`amount` 0, no source or no matte) does nothing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SkyReplace {
    /// Which registered sky to use; empty for none.
    pub source: String,
    /// The sky matte (white = sky), a raster mask.
    pub mask: Option<MaskShape>,
    /// 0 … 100: how much of the new sky shows.
    pub amount: f32,
    /// −100 … 100: moves the matte's edge into (−) or out of (+) the sky.
    pub shift_edge: f32,
    /// 0 … 100: softness of the matte's edge.
    pub fade_edge: f32,
    /// −100 … 100: the new sky's brightness, ±2 stops.
    pub brightness: f32,
    /// −100 … 100: cooler (−) or warmer (+) new sky.
    pub warmth: f32,
    /// 0 … 100: how far the new sky's colour is pulled toward the old sky's.
    pub color_match: f32,
    /// 0 … 100: how much the new sky's light tints the foreground.
    pub relight: f32,
    /// 25 … 400 percent: size of the sky picture beyond covering the frame.
    pub scale: f32,
    /// −100 … 100 percent of the frame, each way.
    pub offset_x: f32,
    pub offset_y: f32,
    pub flip: bool,
}

impl Default for SkyReplace {
    fn default() -> Self {
        SkyReplace {
            source: String::new(),
            mask: None,
            amount: 0.0,
            shift_edge: 0.0,
            fade_edge: 0.0,
            brightness: 0.0,
            warmth: 0.0,
            color_match: 0.0,
            relight: 0.0,
            scale: 100.0,
            offset_x: 0.0,
            offset_y: 0.0,
            flip: false,
        }
    }
}

impl SkyReplace {
    /// Whether this would change a picture: a sky, a matte and some amount.
    pub fn active(&self) -> bool {
        self.amount > 0.0 && !self.source.is_empty() && self.mask.is_some()
    }
}

pub fn validate(s: &SkyReplace) -> Result<(), String> {
    let ranged = [
        ("amount", s.amount, 0.0, 100.0),
        ("shiftEdge", s.shift_edge, -100.0, 100.0),
        ("fadeEdge", s.fade_edge, 0.0, 100.0),
        ("brightness", s.brightness, -100.0, 100.0),
        ("warmth", s.warmth, -100.0, 100.0),
        ("colorMatch", s.color_match, 0.0, 100.0),
        ("relight", s.relight, 0.0, 100.0),
        ("scale", s.scale, 25.0, 400.0),
        ("offsetX", s.offset_x, -100.0, 100.0),
        ("offsetY", s.offset_y, -100.0, 100.0),
    ];
    for (name, v, lo, hi) in ranged {
        if !v.is_finite() || v < lo || v > hi {
            return Err(format!("sky {name} must be {lo} to {hi}"));
        }
    }
    if s.source.len() > 256 {
        return Err("sky source id is too long".into());
    }
    if let Some(mask) = &s.mask {
        match mask {
            MaskShape::Raster { .. } if mask.raster_bytes().is_some() => {}
            _ => return Err("sky matte must be a valid raster mask".into()),
        }
    }
    Ok(())
}

/// Bilinear sample of a single channel buffer at continuous pixel coordinates (edge clamped).
fn sample1(v: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let top = v[y0 * w + x0] * (1.0 - fx) + v[y0 * w + x1] * fx;
    let bot = v[y1 * w + x0] * (1.0 - fx) + v[y1 * w + x1] * fx;
    top * (1.0 - fy) + bot * fy
}

fn sample4(img: &SkyImage, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0.0, (img.w - 1) as f32);
    let y = y.clamp(0.0, (img.h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(img.w - 1), (y0 + 1).min(img.h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |xx: usize, yy: usize| img.px[yy * img.w + xx];
    let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

/// The matte at the picture's size, refined to follow the picture's edges and then shifted and softened.
fn refined_matte(s: &SkyReplace, px: &[[f32; 4]], w: usize, h: usize) -> Option<Vec<f32>> {
    let (mw, mh, bytes) = s.mask.as_ref()?.raster_bytes()?;
    let (mw, mh) = (mw as usize, mh as usize);
    let coarse: Vec<f32> = bytes.iter().map(|b| *b as f32 / 255.0).collect();
    let mut m = vec![0.0f32; w * h];
    let (sx, sy) = (mw as f32 / w as f32, mh as f32 / h as f32);
    par_rows(&mut m, w, 1, |y, row| {
        for (x, v) in row.iter_mut().enumerate() {
            *v = sample1(&coarse, mw, mh, (x as f32 + 0.5) * sx - 0.5, (y as f32 + 0.5) * sy - 0.5);
        }
    });
    // Snap the matte's soft, model-resolution edge onto the picture's own edges.
    let guide: Vec<f32> = px.iter().map(|q| luma([q[0].clamp(0.0, 1.0), q[1].clamp(0.0, 1.0), q[2].clamp(0.0, 1.0)])).collect();
    let r = ((w.max(h) as f32 * 0.004).round() as usize).clamp(2, 48);
    let mut m = guided_fast(&guide, &m, w, h, r, 1e-3);
    // Shift moves the threshold, fade widens the transition around it.
    let t = 0.5 - s.shift_edge / 200.0;
    for v in m.iter_mut() {
        *v = smoothstep(t - 0.03, t + 0.03, *v);
    }
    // Fade blends the edge over a few pixels (a share of the frame, so a preview matches the full size).
    let sigma = s.fade_edge / 100.0 * 0.03 * w.max(h) as f32;
    if sigma >= 0.3 {
        m = gauss(&m, w, h, sigma);
    }
    Some(m)
}

/// Mean colour of `px` weighted by `weight`; `None` when the weight is (nearly) empty.
fn weighted_mean(px: impl Iterator<Item = ([f32; 3], f32)>) -> Option<[f32; 3]> {
    let (mut acc, mut total) = ([0.0f64; 3], 0.0f64);
    for (c, wgt) in px {
        let wgt = wgt as f64;
        for k in 0..3 {
            acc[k] += c[k] as f64 * wgt;
        }
        total += wgt;
    }
    (total > 1e-6).then(|| [(acc[0] / total) as f32, (acc[1] / total) as f32, (acc[2] / total) as f32])
}

pub fn apply(px: &mut [[f32; 4]], w: usize, h: usize, s: &SkyReplace) {
    if w == 0 || h == 0 || w.checked_mul(h) != Some(px.len()) || !s.active() || validate(s).is_err() {
        return;
    }
    let Some(sky) = lookup(&s.source) else { return };
    let Some(matte) = refined_matte(s, px, w, h) else { return };
    let amount = s.amount / 100.0;

    // Lay the sky over the frame: cover the frame, then scale, flip and move it.
    let cover = (w as f32 / sky.w as f32).max(h as f32 / sky.h as f32) * s.scale / 100.0;
    let (cx, cy) = (sky.w as f32 / 2.0 + s.offset_x / 100.0 * sky.w as f32 / 2.0, sky.h as f32 / 2.0 + s.offset_y / 100.0 * sky.h as f32 / 2.0);
    let mut laid: Vec<[f32; 3]> = vec![[0.0; 3]; w * h];
    par_rows(&mut laid, w, 1, |y, row| {
        for (x, o) in row.iter_mut().enumerate() {
            let mut dx = (x as f32 + 0.5 - w as f32 / 2.0) / cover;
            if s.flip {
                dx = -dx;
            }
            let dy = (y as f32 + 0.5 - h as f32 / 2.0) / cover;
            *o = sample4(&sky, cx + dx - 0.5, cy + dy - 0.5);
        }
    });

    // Match the sky to the scene: exposure and warmth, then toward the old sky's colour.
    let gain = 2f32.powf(s.brightness / 100.0 * 2.0);
    let warm = s.warmth / 100.0 * 0.25;
    let (rg, bg) = (gain * (1.0 + warm), gain * (1.0 - warm));
    for c in laid.iter_mut() {
        *c = [c[0] * rg, c[1] * gain, c[2] * bg];
    }
    let old_sky = weighted_mean(px.iter().zip(&matte).map(|(q, m)| ([q[0], q[1], q[2]], *m)));
    let new_sky = weighted_mean(laid.iter().zip(&matte).map(|(c, m)| (*c, *m)));
    if let (Some(old), Some(new)) = (old_sky, new_sky) {
        let k = s.color_match / 100.0;
        if k > 0.0 {
            let g: [f32; 3] = std::array::from_fn(|i| 1.0 + ((old[i] / new[i].max(1e-4)).clamp(0.5, 2.0) - 1.0) * k);
            for c in laid.iter_mut() {
                *c = [c[0] * g[0], c[1] * g[1], c[2] * g[2]];
            }
        }
        // Relight the foreground by the change in sky colour, strongest beside the sky.
        let r = s.relight / 100.0;
        if r > 0.0 {
            let near = gauss(&matte, w, h, (w.max(h) as f32 * 0.05).max(2.0));
            let now = weighted_mean(laid.iter().zip(&matte).map(|(c, m)| (*c, *m))).unwrap_or(new);
            let shift: [f32; 3] = std::array::from_fn(|i| (now[i] / old[i].max(1e-4)).clamp(0.6, 1.6));
            for (i, q) in px.iter_mut().enumerate() {
                let f = (1.0 - matte[i]) * near[i].clamp(0.0, 1.0).sqrt() * r * amount;
                for k in 0..3 {
                    q[k] *= 1.0 + (shift[k] - 1.0) * f;
                }
            }
        }
    }

    for (i, q) in px.iter_mut().enumerate() {
        let a = matte[i] * amount;
        for k in 0..3 {
            q[k] = q[k] * (1.0 - a) + laid[i][k] * a;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 64;
    const H: usize = 48;

    /// Blue-ish sky above row 24, brown ground below.
    fn scene() -> Vec<[f32; 4]> {
        (0..W * H).map(|i| if i / W < 24 { [0.3, 0.5, 0.9, 1.0] } else { [0.4, 0.3, 0.2, 1.0] }).collect()
    }

    fn matte_top(rows: usize) -> MaskShape {
        let cov: Vec<f32> = (0..W * H).map(|i| if i / W < rows { 1.0 } else { 0.0 }).collect();
        MaskShape::raster(W as u32, H as u32, &cov)
    }

    fn settings(id: &str, mask: Option<MaskShape>) -> SkyReplace {
        SkyReplace { source: id.into(), mask, amount: 100.0, ..SkyReplace::default() }
    }

    fn register_flat(id: &str, c: [f32; 3]) {
        register(id, 16, 16, vec![[c[0], c[1], c[2], 1.0]; 256]).unwrap();
    }

    #[test]
    fn neutral_settings_do_nothing() {
        register_flat("t-neutral", [1.0, 0.0, 0.0]);
        let mut a = scene();
        apply(&mut a, W, H, &SkyReplace::default());
        assert_eq!(a, scene());
        let mut zero = settings("t-neutral", Some(matte_top(24)));
        zero.amount = 0.0;
        apply(&mut a, W, H, &zero);
        assert_eq!(a, scene());
        apply(&mut a, W, H, &settings("t-neutral", None));
        assert_eq!(a, scene());
    }

    #[test]
    fn missing_sky_leaves_the_picture() {
        let mut a = scene();
        apply(&mut a, W, H, &settings("t-never-registered", Some(matte_top(24))));
        assert_eq!(a, scene());
    }

    #[test]
    fn replaces_sky_and_keeps_ground() {
        register_flat("t-replace", [1.0, 0.8, 0.2]);
        let mut a = scene();
        apply(&mut a, W, H, &settings("t-replace", Some(matte_top(24))));
        let top = a[2 * W + 32];
        assert!((top[0] - 1.0).abs() < 0.02 && (top[2] - 0.2).abs() < 0.02, "sky not replaced: {top:?}");
        let ground = a[44 * W + 32];
        assert!((ground[0] - 0.4).abs() < 0.02 && (ground[2] - 0.2).abs() < 0.02, "ground changed: {ground:?}");
    }

    #[test]
    fn amount_blends_halfway() {
        register_flat("t-amount", [1.0, 1.0, 1.0]);
        let mut s = settings("t-amount", Some(matte_top(24)));
        s.amount = 50.0;
        let mut a = scene();
        apply(&mut a, W, H, &s);
        let top = a[2 * W + 32];
        assert!((top[0] - 0.65).abs() < 0.03, "{top:?}");
    }

    #[test]
    fn shift_edge_moves_the_boundary() {
        register_flat("t-shift", [1.0, 1.0, 1.0]);
        let count = |shift: f32| {
            let mut s = settings("t-shift", Some(matte_top(24)));
            s.shift_edge = shift;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            a.iter().filter(|q| q[0] > 0.9).count()
        };
        let (shrunk, plain, grown) = (count(-60.0), count(0.0), count(60.0));
        assert!(shrunk <= plain && plain <= grown, "{shrunk} {plain} {grown}");
    }

    #[test]
    fn fade_edge_softens_the_boundary() {
        register_flat("t-fade", [1.0, 1.0, 1.0]);
        let soft_rows = |fade: f32| {
            let mut s = settings("t-fade", Some(matte_top(24)));
            s.fade_edge = fade;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            // Rows whose red lies between the sky's and ground's pure values.
            (0..H).filter(|y| a[y * W + 32][0] > 0.45 && a[y * W + 32][0] < 0.95).count()
        };
        assert!(soft_rows(100.0) > soft_rows(0.0));
    }

    #[test]
    fn brightness_and_warmth_change_the_sky() {
        register_flat("t-tone", [0.4, 0.4, 0.4]);
        let sky = |f: &dyn Fn(&mut SkyReplace)| {
            let mut s = settings("t-tone", Some(matte_top(24)));
            f(&mut s);
            let mut a = scene();
            apply(&mut a, W, H, &s);
            a[2 * W + 32]
        };
        let base = sky(&|_| {});
        assert!(sky(&|s| s.brightness = 50.0)[1] > base[1]);
        assert!(sky(&|s| s.brightness = -50.0)[1] < base[1]);
        let warm = sky(&|s| s.warmth = 100.0);
        assert!(warm[0] > base[0] && warm[2] < base[2]);
    }

    #[test]
    fn color_match_pulls_toward_the_old_sky() {
        register_flat("t-match", [0.9, 0.5, 0.2]);
        let sky = |m: f32| {
            let mut s = settings("t-match", Some(matte_top(24)));
            s.color_match = m;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            a[2 * W + 32]
        };
        let (none, full) = (sky(0.0), sky(100.0));
        // The old sky was blue: matching moves blue up and red down.
        assert!(full[2] > none[2] && full[0] < none[0], "{none:?} {full:?}");
    }

    #[test]
    fn relight_tints_ground_beside_the_sky() {
        register_flat("t-relight", [1.0, 0.5, 0.2]);
        let ground = |r: f32| {
            let mut s = settings("t-relight", Some(matte_top(24)));
            s.relight = r;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            a[27 * W + 32]
        };
        let (off, on) = (ground(0.0), ground(100.0));
        assert!(on[0] > off[0] && on[2] < off[2], "{off:?} {on:?}");
        // Far from the sky the effect is weaker than at the horizon.
        let far_change = {
            let mut s = settings("t-relight", Some(matte_top(24)));
            s.relight = 100.0;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            (a[47 * W + 32][0] - 0.4).abs()
        };
        assert!(far_change < on[0] - off[0]);
    }

    #[test]
    fn flip_and_offset_move_the_sky_picture() {
        // Left half of the sky picture red, right half blue.
        let px: Vec<[f32; 4]> = (0..16 * 16).map(|i| if i % 16 < 8 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] }).collect();
        register("t-flip", 16, 16, px).unwrap();
        let left = |flip: bool| {
            let mut s = settings("t-flip", Some(matte_top(48)));
            s.flip = flip;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            a[10 * W + 4]
        };
        assert!(left(false)[0] > 0.9 && left(true)[2] > 0.9);
    }

    #[test]
    fn rejects_out_of_range_and_bad_mattes() {
        let mut s = settings("x", Some(matte_top(24)));
        assert!(validate(&s).is_ok());
        s.amount = 101.0;
        assert!(validate(&s).is_err());
        s.amount = f32::NAN;
        assert!(validate(&s).is_err());
        let bad = settings("x", Some(MaskShape::Raster { w: 4, h: 4, data: "!!".into() }));
        assert!(validate(&bad).is_err());
        let mut a = scene();
        apply(&mut a, W, H, &bad);
        assert_eq!(a, scene());
    }

    #[test]
    fn registry_rejects_bad_pictures_and_keeps_a_bounded_set() {
        assert!(register("", 1, 1, vec![[0.0; 4]]).is_err());
        assert!(register("bad", 2, 2, vec![[0.0; 4]]).is_err());
        for i in 0..MAX_REGISTERED + 3 {
            register(&format!("t-cap-{i}"), 1, 1, vec![[0.0; 4]]).unwrap();
        }
        assert!(!is_registered("t-cap-0"));
        assert!(is_registered(&format!("t-cap-{}", MAX_REGISTERED + 2)));
        unregister(&format!("t-cap-{}", MAX_REGISTERED + 2));
        assert!(!is_registered(&format!("t-cap-{}", MAX_REGISTERED + 2)));
    }

    #[test]
    fn mattes_of_another_size_are_resampled() {
        register_flat("t-resize", [1.0, 1.0, 1.0]);
        let cov: Vec<f32> = (0..8 * 6).map(|i| if i / 8 < 3 { 1.0 } else { 0.0 }).collect();
        let s = settings("t-resize", Some(MaskShape::raster(8, 6, &cov)));
        let mut a = scene();
        apply(&mut a, W, H, &s);
        assert!(a[2 * W + 32][0] > 0.95 && (a[44 * W + 32][0] - 0.4).abs() < 0.02);
    }
}
