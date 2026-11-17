//! Sky replacement: a sky picture composited behind a sky matte, with the matte's edge refined,
//! the new sky matched to the scene, and the foreground relit by it.
//!
//! A sky id that is an absolute file path is decoded on first use, so the worker, the hosted editor and a full render all
//! find it without being told. The recipe carries only a sky *id*, the matte (a raster mask, like any other) and the sliders.
//! The decoded sky picture reaches the stage through [`register`], which the host calls before
//! rendering (the algo crate decodes no files). A recipe whose sky is not registered leaves the
//! picture untouched, so an old recipe never fails on a machine without that sky.
//!
//! Sky pictures are expected in the same working encoding as the pixels `develop` is given.
//!
//! Beyond the blend (Amount, Shift and Fade Edge, Brightness, Warmth, Match Colors, Relight) the stage follows Luminar's Sky
//! AI: the sky picture's horizon is laid on the photo's own horizon (found from the matte) and moved by Horizon Position, panned
//! by Vertical and Horizontal Position (enlarged so no edge shows) and flipped; the matte is refined by Global (more new sky in
//! partly-sky areas), Close Gaps (old sky showing through near the sky becomes new sky) and Fix Details (a tighter edge, no
//! specks); the relight is split into its brightness (Relight Strength) and colour (Relight Saturation), with people (a raster
//! mask from the host) left out of it and relit by Relight Human; the new sky is reflected in water (a raster mask) in place of
//! the old sky's reflection, blurred by Water Blur; and the new sky itself can be defocused, hazed toward its horizon and given
//! grain.
use super::{MaskShape, gauss, guided_fast, luma, smoothstep};
use crate::photo_util::par_rows;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Longest side of a registered sky, and how many are kept (the oldest is dropped).
pub const MAX_SKY_SIDE: usize = 6000;
// Tests register many small skies in parallel; a real session keeps a few large ones.
const MAX_REGISTERED: usize = if cfg!(test) { 256 } else { 8 };

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
    insert(&mut REGISTRY.lock().unwrap_or_else(|e| e.into_inner()), id, SkyImage { w, h, px });
    Ok(())
}

fn insert(reg: &mut VecDeque<(String, Arc<SkyImage>)>, id: &str, img: SkyImage) {
    reg.retain(|(k, _)| k != id);
    reg.push_back((id.to_string(), Arc::new(img)));
    while reg.len() > MAX_REGISTERED {
        reg.pop_front();
    }
}

/// Forgets sky `id`.
pub fn unregister(id: &str) {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).retain(|(k, _)| k != id);
}

/// Whether sky `id` is registered.
pub fn is_registered(id: &str) -> bool {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(k, _)| k == id)
}

/// Longest side a sky loaded from a file is reduced to: skies fill a frame softly, so more is wasted memory.
const LOADED_SIDE: usize = 3000;

/// Decodes the picture at `path` (any format the codecs read) as a sky and registers it under `path`.
pub fn load_file(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("a sky file needs an absolute path".into());
    }
    if std::fs::metadata(p).map_err(|e| e.to_string())?.len() > 256 << 20 {
        return Err("sky file is too large".into());
    }
    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
    let img = photocraft_codecs::decode(&bytes).map_err(|e| e.to_string())?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let flat = img.to_rgba_f32();
    let (w, h, flat) = if w.max(h) > LOADED_SIDE {
        let k = LOADED_SIDE as f64 / w.max(h) as f64;
        let (nw, nh) = (((w as f64 * k).round() as usize).max(1), ((h as f64 * k).round() as usize).max(1));
        (nw, nh, crate::seam::resize_bilinear(w, h, 4, &flat, nw, nh))
    } else {
        (w, h, flat)
    };
    let px: Vec<[f32; 4]> = flat.chunks_exact(4).map(|c| [c[0], c[1], c[2], 1.0]).collect();
    register(path, w, h, px)
}

fn lookup(id: &str) -> Option<Arc<SkyImage>> {
    let find = || REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| k == id).map(|(_, v)| v.clone());
    // A sky that is a file path loads itself the first time a render asks for it.
    find().or_else(|| std::path::Path::new(id).is_absolute().then(|| load_file(id).ok()).flatten().and_then(|_| find()))
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
    /// 0 … 100: how much the new sky's light relights the scene (its exposure).
    pub relight: f32,
    /// 25 … 400 percent: size of the sky picture beyond covering the sky.
    pub scale: f32,
    /// −100 … 100: pans the sky picture sideways (it is enlarged so no edge shows).
    pub offset_x: f32,
    /// −100 … 100: pans the sky picture up (+, a higher part of it shows) or lowers it below the horizon (−).
    pub offset_y: f32,
    pub flip: bool,
    /// −100 … 100: moves the new sky's horizon (the bottom of the sky picture) below (−) or above (+) the photo's horizon.
    pub horizon: f32,
    /// 0 … 100: more of the new sky where the matte is only partly sky (haze, thin branches, the horizon).
    pub global: f32,
    /// 0 … 100: fills the gaps the matte left where the old sky still shows through (branches, wires, arches).
    pub close_gaps: f32,
    /// 0 … 100: fits the matte's edge more tightly to fine detail and clears small specks.
    pub fix_details: f32,
    /// 0 … 100: how much of the new sky's colour the relight carries (50 is its own colour, 0 only its brightness).
    pub relight_saturation: f32,
    /// 0 … 100: relight on people. People are left out of the scene's relight; this relights them on their own.
    pub relight_human: f32,
    /// The people in the picture (white = person), a raster mask; used by `relight_human`.
    pub people: Option<MaskShape>,
    /// 0 … 100: the new sky reflected in the water (in place of the old sky's reflection).
    pub reflection: f32,
    /// 0 … 100: blur of that reflection, for rippled water.
    pub water_blur: f32,
    /// The water in the picture (white = water), a raster mask; used by `reflection`.
    pub water: Option<MaskShape>,
    /// 0 … 100: blur of the new sky (to match a shallow depth of field).
    pub defocus: f32,
    /// 0 … 100: grain added to the new sky (to match the photo's).
    pub grain: f32,
    /// 0 … 100: haze over the new sky, thickest at its horizon.
    pub haze: f32,
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
            horizon: 0.0,
            global: 0.0,
            close_gaps: 0.0,
            fix_details: 0.0,
            relight_saturation: 50.0,
            relight_human: 0.0,
            people: None,
            reflection: 0.0,
            water_blur: 0.0,
            water: None,
            defocus: 0.0,
            grain: 0.0,
            haze: 0.0,
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
        ("horizon", s.horizon, -100.0, 100.0),
        ("global", s.global, 0.0, 100.0),
        ("closeGaps", s.close_gaps, 0.0, 100.0),
        ("fixDetails", s.fix_details, 0.0, 100.0),
        ("relightSaturation", s.relight_saturation, 0.0, 100.0),
        ("relightHuman", s.relight_human, 0.0, 100.0),
        ("reflection", s.reflection, 0.0, 100.0),
        ("waterBlur", s.water_blur, 0.0, 100.0),
        ("defocus", s.defocus, 0.0, 100.0),
        ("grain", s.grain, 0.0, 100.0),
        ("haze", s.haze, 0.0, 100.0),
    ];
    for (name, v, lo, hi) in ranged {
        if !v.is_finite() || v < lo || v > hi {
            return Err(format!("sky {name} must be {lo} to {hi}"));
        }
    }
    if s.source.len() > 256 {
        return Err("sky source id is too long".into());
    }
    for (name, mask) in [("matte", &s.mask), ("people mask", &s.people), ("water mask", &s.water)] {
        if let Some(mask) = mask {
            match mask {
                MaskShape::Raster { .. } if mask.raster_bytes().is_some() => {}
                _ => return Err(format!("sky {name} must be a valid raster mask")),
            }
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

/// Bilinear sample of an RGB buffer (`w x h`) at continuous pixel coordinates (edge clamped).
fn sample3(v: &[[f32; 3]], w: usize, h: usize, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (a, b, c, d) = (v[y0 * w + x0], v[y0 * w + x1], v[y1 * w + x0], v[y1 * w + x1]);
    std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

/// A raster mask at the picture's size (`w x h`, bilinear), 0 … 1.
fn raster_at(shape: &MaskShape, w: usize, h: usize) -> Option<Vec<f32>> {
    let (mw, mh, bytes) = shape.raster_bytes()?;
    let (mw, mh) = (mw as usize, mh as usize);
    if mw == 0 || mh == 0 {
        return None;
    }
    let coarse: Vec<f32> = bytes.iter().map(|b| *b as f32 / 255.0).collect();
    let mut m = vec![0.0f32; w * h];
    let (sx, sy) = (mw as f32 / w as f32, mh as f32 / h as f32);
    par_rows(&mut m, w, 1, |y, row| {
        for (x, v) in row.iter_mut().enumerate() {
            *v = sample1(&coarse, mw, mh, (x as f32 + 0.5) * sx - 0.5, (y as f32 + 0.5) * sy - 0.5);
        }
    });
    Some(m)
}

/// Each channel of `v` blurred (approximate Gaussian of `sigma` pixels).
fn gauss3(v: &[[f32; 3]], w: usize, h: usize, sigma: f32) -> Vec<[f32; 3]> {
    let channels: Vec<Vec<f32>> = (0..3).map(|k| gauss(&v.iter().map(|c| c[k]).collect::<Vec<_>>(), w, h, sigma)).collect();
    (0..v.len()).map(|i| [channels[0][i], channels[1][i], channels[2][i]]).collect()
}

/// Close Gaps: where the old sky still shows through near the sky (between branches, through wires and arches) the matte is
/// raised: pixels close to the sky that have the old sky's colour and brightness become sky.
fn close_gaps(m: &mut [f32], px: &[[f32; 4]], w: usize, h: usize, k: f32) {
    let Some(sky) = weighted_mean(px.iter().zip(m.iter()).map(|(q, v)| ([q[0], q[1], q[2]], if *v > 0.9 { 1.0 } else { 0.0 }))) else {
        return;
    };
    let sky_l = luma(sky).max(1e-4);
    let reach = (w.max(h) as f32 * (0.01 + 0.06 * k)).max(1.0);
    let near = gauss(m, w, h, reach);
    let tolerance = 0.18 + 0.32 * k;
    for (i, v) in m.iter_mut().enumerate() {
        let c = [px[i][0], px[i][1], px[i][2]];
        let l = luma(c);
        let distance = (0..3).map(|j| ((c[j] - sky[j]) / sky_l).powi(2)).sum::<f32>().sqrt();
        let like = (1.0 - distance / tolerance).clamp(0.0, 1.0) * smoothstep(0.6, 0.9, l / sky_l);
        let fill = like * smoothstep(0.02, 0.2, near[i]) * smoothstep(0.0, 0.25, k);
        *v = v.max(fill);
    }
}

/// The matte at the picture's size, refined to follow the picture's edges and then shifted and softened.
fn refined_matte(s: &SkyReplace, px: &[[f32; 4]], w: usize, h: usize) -> Option<Vec<f32>> {
    let m = raster_at(s.mask.as_ref()?, w, h)?;
    // Snap the matte's soft, model-resolution edge onto the picture's own edges.
    let guide: Vec<f32> = px.iter().map(|q| luma([q[0].clamp(0.0, 1.0), q[1].clamp(0.0, 1.0), q[2].clamp(0.0, 1.0)])).collect();
    let r = ((w.max(h) as f32 * 0.004).round() as usize).clamp(2, 48);
    let mut m = guided_fast(&guide, &m, w, h, r, 1e-3);
    // Fix Details: a second, tighter pass that follows fine detail (twigs, wires, hair).
    let fix = s.fix_details / 100.0;
    if fix > 0.0 {
        let rs = ((w.max(h) as f32 * 0.0012).round() as usize).clamp(1, 16);
        let tight = guided_fast(&guide, &m, w, h, rs, 1e-4);
        for (v, t) in m.iter_mut().zip(&tight) {
            *v += (t - *v) * fix;
        }
    }
    // Global: more of the new sky where the matte is only partly sky.
    let global = s.global / 100.0;
    if global > 0.0 {
        for v in m.iter_mut() {
            *v = 1.0 - (1.0 - v.clamp(0.0, 1.0)).powf(1.0 + 3.0 * global);
        }
    }
    // Shift moves the threshold; the transition around it is narrow (wider with Global, so partial sky blends).
    let t = 0.5 - s.shift_edge / 200.0;
    let band = 0.03 + 0.12 * global;
    for v in m.iter_mut() {
        *v = smoothstep(t - band, t + band, *v);
    }
    // Fix Details also clears specks: a small blur settled again keeps shapes and drops isolated dots.
    if fix > 0.0 {
        let blurred = gauss(&m, w, h, (w.max(h) as f32 * 0.0008).max(0.6));
        for (v, b) in m.iter_mut().zip(&blurred) {
            *v += (smoothstep(0.3, 0.7, *b) - *v) * fix * 0.6;
        }
    }
    let gaps = s.close_gaps / 100.0;
    if gaps > 0.0 {
        close_gaps(&mut m, px, w, h, gaps);
    }
    // Fade blends the edge over a few pixels (a share of the frame, so a preview matches the full size).
    let sigma = s.fade_edge / 100.0 * 0.03 * w.max(h) as f32;
    if sigma >= 0.3 {
        m = gauss(&m, w, h, sigma);
    }
    Some(m)
}

/// The photo's horizon: the row (0 top … 1 bottom) below which the matte holds almost no sky.
fn horizon_of(matte: &[f32], w: usize, h: usize) -> f32 {
    let rows: Vec<f32> = (0..h).map(|y| matte[y * w..(y + 1) * w].iter().sum::<f32>()).collect();
    let total: f32 = rows.iter().sum();
    if total <= 1e-3 {
        return 0.5;
    }
    let mut acc = 0.0;
    for (y, r) in rows.iter().enumerate() {
        acc += r;
        if acc >= total * 0.98 {
            return (y as f32 + 1.0) / h as f32;
        }
    }
    1.0
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

/// The sky picture laid over the frame: its bottom (its horizon) on the photo's horizon, covering the sky, then sized, panned
/// and flipped. Rows below the horizon repeat the sky's bottom row (the matte hides them).
fn lay_sky(sky: &SkyImage, s: &SkyReplace, w: usize, h: usize, horizon: f32) -> (Vec<[f32; 3]>, f32) {
    let (fw, fh) = (w as f32, h as f32);
    let bottom = (horizon + 0.03 - s.horizon / 100.0 * 0.5).clamp(0.15, 1.25) * fh;
    let push = (-s.offset_y).max(0.0) / 100.0 * 0.3 * bottom;     // lowered below the horizon
    let grow = 1.0 + s.offset_x.abs().max(s.offset_y.abs()) / 100.0 * 0.5;
    let cover = (fw / sky.w as f32).max((bottom + push) / sky.h as f32) * s.scale / 100.0 * grow;   // frame px per sky px
    let spare_x = (sky.w as f32 - fw / cover).max(0.0);
    let spare_y = (sky.h as f32 - (bottom + push) / cover).max(0.0);
    let pan_x = s.offset_x / 100.0 * spare_x / 2.0;
    let pan_y = s.offset_y.max(0.0) / 100.0 * spare_y;               // a higher part of the sky picture
    let mut laid: Vec<[f32; 3]> = vec![[0.0; 3]; w * h];
    par_rows(&mut laid, w, 1, |y, row| {
        for (x, o) in row.iter_mut().enumerate() {
            let mut dx = (x as f32 + 0.5 - fw / 2.0) / cover;
            if s.flip {
                dx = -dx;
            }
            let sx = sky.w as f32 / 2.0 + dx + pan_x;
            let sy = sky.h as f32 - (bottom + push - (y as f32 + 0.5)) / cover - pan_y;
            *o = sample4(sky, sx - 0.5, sy - 0.5);
        }
    });
    (laid, bottom)
}

/// Sky Adjustments on the laid sky: Defocus, Atmospheric Haze (thickest at its horizon) and Grain.
fn adjust_sky(laid: &mut Vec<[f32; 3]>, matte: &[f32], s: &SkyReplace, w: usize, h: usize, bottom: f32) {
    let side = w.max(h) as f32;
    let defocus = s.defocus / 100.0;
    if defocus > 0.0 {
        let sigma = defocus * 0.012 * side;
        if sigma >= 0.3 {
            *laid = gauss3(laid, w, h, sigma);
        }
    }
    let haze = s.haze / 100.0;
    if haze > 0.0 {
        if let Some(mean) = weighted_mean(laid.iter().zip(matte).map(|(c, m)| (*c, *m))) {
            let l = luma(mean);
            let tone: [f32; 3] = std::array::from_fn(|k| (l + (mean[k] - l) * 0.3) * 1.15 + 0.03);
            let depth = (0.35 * bottom).max(1.0);
            par_rows(laid, w, 1, |y, row| {
                let f = haze * 0.85 * (-(bottom - y as f32 - 0.5).max(0.0) / depth).exp();
                for c in row.iter_mut() {
                    for k in 0..3 {
                        c[k] += (tone[k] - c[k]) * f;
                    }
                }
            });
        }
    }
    let grain = s.grain / 100.0;
    if grain > 0.0 {
        // A share of the frame per grain, so a preview looks as the full size does.
        let cell = (side / 2400.0).max(0.35);
        par_rows(laid, w, 1, |y, row| {
            for (x, c) in row.iter_mut().enumerate() {
                let (fx, fy) = (x as f32 / cell, y as f32 / cell);
                let n = (super::value_noise(fx, fy, 0x5ca1) * 0.7 + super::value_noise(fx * 2.3, fy * 2.3, 0x5ca2) * 0.3) - 0.5;
                let l = luma(*c).clamp(0.0, 1.0);
                let g = n * 2.0 * grain * 0.06 * (0.3 + 0.7 * l.sqrt());
                for k in 0..3 {
                    c[k] += g;
                }
            }
        });
    }
}

/// The new sky reflected in the water: for each column the water is mirrored about its shoreline (where the water starts), and
/// what the mirror sees of the sky is changed from the old sky to the new one. The change is a ratio (new sky over old sky, both
/// as the mirror sees them): water reflects only part of the light and has its own colour and ripples, and all of that stays.
fn reflect(px: &mut [[f32; 4]], original: &[[f32; 3]], laid: &[[f32; 3]], matte: &[f32], water: &[f32], s: &SkyReplace, w: usize, h: usize) {
    let strength = s.reflection / 100.0 * s.amount / 100.0 * 0.85;
    let mut shore: Vec<f32> = (0..w).map(|x| (0..h).find(|&y| water[y * w + x] > 0.5).map(|y| y as f32).unwrap_or(f32::NAN)).collect();
    let known: Vec<f32> = shore.iter().copied().filter(|v| v.is_finite()).collect();
    if known.is_empty() {
        return;
    }
    let fallback = known.iter().sum::<f32>() / known.len() as f32;
    for v in shore.iter_mut() {
        if !v.is_finite() {
            *v = fallback;
        }
    }
    let shore = gauss(&shore, w, 1, (w as f32 * 0.01).max(1.0));
    let mut delta: Vec<[f32; 3]> = vec![[0.0; 3]; w * h];
    par_rows(&mut delta, w, 1, |y, row| {
        for (x, d) in row.iter_mut().enumerate() {
            if water[y * w + x] <= 0.02 || (y as f32) < shore[x] {
                continue;
            }
            let ym = (2.0 * shore[x] - y as f32).max(0.0);
            let sky_there = sample1(matte, w, h, x as f32, ym);
            if sky_there <= 0.01 {
                continue;
            }
            let new = sample3(laid, w, h, x as f32, ym);
            let old = sample3(original, w, h, x as f32, ym);
            *d = std::array::from_fn(|k| (((new[k].max(0.0) + 0.02) / (old[k].max(0.0) + 0.02)).clamp(0.2, 5.0) - 1.0) * sky_there);
        }
    });
    // Even calm water breaks a reflection up a little; Water Blur is for rippled water.
    let sigma = (0.004 + s.water_blur / 100.0 * 0.02) * w.max(h) as f32;
    let delta = if sigma >= 0.3 { gauss3(&delta, w, h, sigma) } else { delta };
    for (i, q) in px.iter_mut().enumerate() {
        let f = water[i] * strength;
        if f > 0.0 {
            for k in 0..3 {
                q[k] *= 1.0 + delta[i][k] * f;
            }
        }
    }
}

pub fn apply(px: &mut [[f32; 4]], w: usize, h: usize, s: &SkyReplace) {
    if w == 0 || h == 0 || w.checked_mul(h) != Some(px.len()) || !s.active() || validate(s).is_err() {
        return;
    }
    let Some(sky) = lookup(&s.source) else { return };
    let Some(matte) = refined_matte(s, px, w, h) else { return };
    let amount = s.amount / 100.0;
    let horizon = horizon_of(&matte, w, h);
    let (mut laid, bottom) = lay_sky(&sky, s, w, h, horizon);

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
    }
    adjust_sky(&mut laid, &matte, s, w, h, bottom);
    let original: Vec<[f32; 3]> = px.iter().map(|q| [q[0], q[1], q[2]]).collect();

    // Relight the scene by the change in the sky's light, strongest beside the sky: its brightness (Relight Strength) and its
    // colour (Relight Saturation). People are left out of it and relit by Relight Human instead.
    if let Some(old) = old_sky {
        let now = weighted_mean(laid.iter().zip(&matte).map(|(c, m)| (*c, *m))).unwrap_or(old);
        let shift: [f32; 3] = std::array::from_fn(|i| (now[i] / old[i].max(1e-4)).clamp(0.6, 1.6));
        let bright = (luma(now) / luma(old).max(1e-4)).clamp(0.6, 1.6);
        let colour: [f32; 3] = std::array::from_fn(|i| shift[i] / bright);
        let saturation = s.relight_saturation / 50.0;
        let scene = s.relight / 100.0;
        let human = s.relight_human / 100.0;
        let people = if human > 0.0 || s.people.is_some() { s.people.as_ref().and_then(|p| raster_at(p, w, h)) } else { None };
        if scene > 0.0 || (human > 0.0 && people.is_some()) {
            let near = gauss(&matte, w, h, (w.max(h) as f32 * 0.05).max(2.0));
            for (i, q) in px.iter_mut().enumerate() {
                let person = people.as_ref().map(|p| p[i].clamp(0.0, 1.0)).unwrap_or(0.0);
                let f = ((1.0 - matte[i]) * near[i].clamp(0.0, 1.0).sqrt() * scene * (1.0 - person) + person * human) * amount;
                if f <= 0.0 {
                    continue;
                }
                for k in 0..3 {
                    q[k] *= (1.0 + (bright - 1.0) * f) * (1.0 + (colour[k] - 1.0) * f * saturation);
                }
            }
        }
    }

    if s.reflection > 0.0 {
        if let Some(water) = s.water.as_ref().and_then(|m| raster_at(m, w, h)) {
            reflect(px, &original, &laid, &matte, &water, s, w, h);
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
        let mut reg = VecDeque::new();
        for i in 0..MAX_REGISTERED + 3 {
            insert(&mut reg, &format!("c{i}"), SkyImage { w: 1, h: 1, px: vec![[0.0; 4]] });
        }
        assert_eq!(reg.len(), MAX_REGISTERED);
        assert_eq!(reg.front().unwrap().0, "c3");
        insert(&mut reg, "c3", SkyImage { w: 1, h: 1, px: vec![[0.0; 4]] });
        assert_eq!(reg.back().unwrap().0, "c3");
        register("t-unreg", 1, 1, vec![[0.0; 4]]).unwrap();
        unregister("t-unreg");
        assert!(!is_registered("t-unreg"));
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

    /// A wide sky picture: its top half red, its bottom half blue.
    fn register_wide(id: &str) {
        let px: Vec<[f32; 4]> = (0..64 * 8).map(|i| if i / 64 < 4 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] }).collect();
        register(id, 64, 8, px).unwrap();
    }

    #[test]
    fn the_sky_pictures_horizon_sits_on_the_photos_and_horizon_position_moves_it() {
        register_wide("t-horizon");
        let at = |horizon: f32, row: usize| {
            let mut s = settings("t-horizon", Some(matte_top(24)));
            s.horizon = horizon;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            a[row * W + 32]
        };
        assert!(at(0.0, 22)[2] > 0.9, "{:?}", at(0.0, 22));      // the bottom of the sky picture at the photo's horizon
        assert!(at(0.0, 1)[0] > 0.9, "{:?}", at(0.0, 1));        // and its top at the top of the frame
        assert!(at(-100.0, 16)[0] > at(0.0, 16)[0] + 0.5);       // its horizon lowered: more of its upper part shows
    }

    #[test]
    fn panning_moves_the_sky_and_never_shows_its_edge() {
        let px: Vec<[f32; 4]> = (0..64 * 8).map(|i| { let x = (i % 64) as f32 / 63.0; [x, 0.0, 1.0 - x, 1.0] }).collect();
        register("t-pan", 64, 8, px).unwrap();
        let at = |dx: f32, x: usize| {
            let mut s = settings("t-pan", Some(matte_top(24)));
            s.offset_x = dx;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            a[10 * W + x][0]
        };
        assert!(at(100.0, 32) > at(0.0, 32) + 0.05 && at(-100.0, 32) < at(0.0, 32) - 0.05);
        assert!(at(100.0, W - 1) > at(100.0, W - 4) && at(-100.0, 0) < at(-100.0, 3));   // no smeared edge: the picture goes on to the frame's edges
    }

    #[test]
    fn global_lets_the_new_sky_into_partly_sky_areas() {
        register_flat("t-global", [1.0, 1.0, 1.0]);
        let cov: Vec<f32> = (0..W * H).map(|i| { let y = i / W; if y < 24 { 1.0 } else if y < 32 { 0.42 } else { 0.0 } }).collect();
        let at = |g: f32| {
            let mut s = settings("t-global", Some(MaskShape::raster(W as u32, H as u32, &cov)));
            s.global = g;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            a[28 * W + 32][0]
        };
        assert!(at(100.0) > at(0.0) + 0.2, "{} {}", at(0.0), at(100.0));
    }

    #[test]
    fn close_gaps_turns_old_sky_showing_through_near_the_sky_into_new_sky() {
        register_flat("t-gaps", [1.0, 0.2, 0.2]);
        let mut base = scene();
        for y in 27..30 {
            for x in 30..35 {
                base[y * W + x] = [0.3, 0.5, 0.9, 1.0];            // the old sky seen through a gap below the matte's edge
            }
        }
        let at = |k: f32, x: usize| {
            let mut s = settings("t-gaps", Some(matte_top(24)));
            s.close_gaps = k;
            let mut a = base.clone();
            apply(&mut a, W, H, &s);
            a[28 * W + x]
        };
        assert!(at(0.0, 32)[0] < 0.5 && at(100.0, 32)[0] > 0.8, "{:?} {:?}", at(0.0, 32), at(100.0, 32));
        assert!((at(100.0, 10)[0] - 0.4).abs() < 0.05);            // ground of another colour is not taken
    }

    #[test]
    fn fix_details_keeps_what_is_far_from_the_edge() {
        register_flat("t-fix", [1.0, 1.0, 1.0]);
        let mut s = settings("t-fix", Some(matte_top(24)));
        s.fix_details = 100.0;
        let mut a = scene();
        apply(&mut a, W, H, &s);
        assert!(a[4 * W + 32][0] > 0.95 && (a[44 * W + 32][0] - 0.4).abs() < 0.02);
    }

    #[test]
    fn relight_saturation_carries_the_skys_colour_and_zero_carries_only_its_brightness() {
        register_flat("t-relsat", [1.0, 0.5, 0.2]);
        let ground = |sat: f32| {
            let mut s = settings("t-relsat", Some(matte_top(24)));
            s.relight = 100.0;
            s.relight_saturation = sat;
            let mut a = scene();
            apply(&mut a, W, H, &s);
            a[27 * W + 32]
        };
        let tint = |c: [f32; 4]| c[0] / c[2];
        let (grey, vivid) = (ground(0.0), ground(100.0));
        assert!(tint(vivid) > tint(grey) + 0.1, "{grey:?} {vivid:?}");
        assert!((tint(grey) - 2.0).abs() < 0.1, "{grey:?}");       // the ground (0.4 / 0.2) keeps its colour
    }

    #[test]
    fn people_are_left_out_of_the_scene_relight_and_relit_by_relight_human() {
        register_flat("t-human", [1.0, 0.5, 0.2]);
        let people: Vec<f32> = (0..W * H)
            .map(|i| { let (x, y) = (i % W, i / W); if (26..30).contains(&y) && (28..36).contains(&x) { 1.0 } else { 0.0 } })
            .collect();
        let at = |human: f32| {
            let mut s = settings("t-human", Some(matte_top(24)));
            s.relight = 100.0;
            s.relight_human = human;
            s.people = Some(MaskShape::raster(W as u32, H as u32, &people));
            let mut a = scene();
            apply(&mut a, W, H, &s);
            (a[27 * W + 32], a[27 * W + 10])
        };
        let (person, beside) = at(0.0);
        assert!((person[0] - 0.4).abs() < 0.02, "{person:?}");
        assert!(beside[0] > 0.42, "{beside:?}");
        assert!(at(100.0).0[0] > 0.42);
    }

    #[test]
    fn the_new_sky_is_reflected_in_the_water_and_water_blur_softens_it() {
        let px: Vec<[f32; 4]> = (0..64 * 8).map(|i| if i % 64 < 32 { [1.0, 0.6, 0.2, 1.0] } else { [0.2, 0.6, 1.0, 1.0] }).collect();
        register("t-reflect", 64, 8, px).unwrap();
        let mut base = scene();
        for y in 24..H {
            for x in 0..W {
                base[y * W + x] = [0.2, 0.3, 0.55, 1.0];
            }
        }
        let water: Vec<f32> = (0..W * H).map(|i| if i / W >= 24 { 1.0 } else { 0.0 }).collect();
        let at = |r: f32, blur: f32| {
            let mut s = settings("t-reflect", Some(matte_top(24)));
            s.reflection = r;
            s.water_blur = blur;
            s.water = Some(MaskShape::raster(W as u32, H as u32, &water));
            let mut a = base.clone();
            apply(&mut a, W, H, &s);
            a
        };
        let (off, on) = (at(0.0, 0.0), at(100.0, 0.0));
        assert!(on[30 * W + 10][0] > off[30 * W + 10][0] + 0.2, "{:?} {:?}", off[30 * W + 10], on[30 * W + 10]);
        let step = |a: &[[f32; 4]]| (a[30 * W + 28][0] - a[30 * W + 36][0]).abs();
        assert!(step(&at(100.0, 100.0)) < step(&on));
    }

    #[test]
    fn defocus_blurs_the_new_sky_haze_is_thickest_at_its_horizon_and_grain_adds_texture() {
        let (w, h) = (256usize, 192usize);
        let scene: Vec<[f32; 4]> = (0..w * h).map(|i| if i / w < 96 { [0.3, 0.5, 0.9, 1.0] } else { [0.4, 0.3, 0.2, 1.0] }).collect();
        let cov: Vec<f32> = (0..w * h).map(|i| if i / w < 96 { 1.0 } else { 0.0 }).collect();
        let px: Vec<[f32; 4]> = (0..128 * 32).map(|i| if (i % 128 / 2) % 2 == 0 { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] }).collect();
        register("t-adjust", 128, 32, px).unwrap();
        let run = |f: &dyn Fn(&mut SkyReplace)| {
            let mut s = SkyReplace { source: "t-adjust".into(), mask: Some(MaskShape::raster(w as u32, h as u32, &cov)), amount: 100.0, ..SkyReplace::default() };
            f(&mut s);
            let mut a = scene.clone();
            apply(&mut a, w, h, &s);
            a
        };
        let spread = |a: &[[f32; 4]], row: usize| {
            let v: Vec<f32> = (40..216).map(|x| a[row * w + x][0]).collect();
            v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min)
        };
        let plain = run(&|_| {});
        assert!(spread(&run(&|s| s.defocus = 100.0), 40) < spread(&plain, 40) * 0.6);
        let hazy = run(&|s| s.haze = 100.0);
        assert!(spread(&hazy, 92) < spread(&hazy, 8) * 0.8, "{} {}", spread(&hazy, 92), spread(&hazy, 8));
        register_flat("t-grain", [0.5, 0.5, 0.5]);
        let grainy = |g: f32| {
            let s = SkyReplace { source: "t-grain".into(), mask: Some(MaskShape::raster(w as u32, h as u32, &cov)), amount: 100.0, grain: g, ..SkyReplace::default() };
            let mut a = scene.clone();
            apply(&mut a, w, h, &s);
            spread(&a, 40)
        };
        assert!(grainy(100.0) > grainy(0.0) + 0.02);
    }

    #[test]
    fn the_new_settings_are_range_checked_and_their_masks_must_be_rasters() {
        let mut s = settings("x", Some(matte_top(24)));
        s.reflection = 101.0;
        assert!(validate(&s).is_err());
        let mut s = settings("x", Some(matte_top(24)));
        s.horizon = -100.0;
        s.relight_saturation = 0.0;
        assert!(validate(&s).is_ok());
        s.water = Some(MaskShape::Raster { w: 4, h: 4, data: "!!".into() });
        assert!(validate(&s).is_err());
        let json = serde_json::json!({"source": "x", "amount": 50.0, "closeGaps": 20.0, "relightHuman": 10.0, "waterBlur": 5.0});
        let parsed: SkyReplace = serde_json::from_value(json).unwrap();
        assert_eq!((parsed.close_gaps, parsed.relight_human, parsed.water_blur, parsed.relight_saturation), (20.0, 10.0, 5.0, 50.0));
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn a_path_id_loads_itself_and_renders() {
        let dir = std::env::temp_dir().join(format!("sky-file-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("flat.png");
        let mut img = photocraft_codecs::Image::new(8, 8, photocraft_codecs::ChannelLayout::Rgba, photocraft_codecs::SampleType::U8).unwrap();
        for p in img.data_mut().chunks_exact_mut(4) {
            p.copy_from_slice(&[255, 0, 0, 255]);
        }
        std::fs::write(&file, photocraft_codecs::encode(&img, photocraft_codecs::Format::Png, &Default::default()).unwrap()).unwrap();
        let id = file.to_string_lossy().to_string();
        assert!(!is_registered(&id));
        let cov = vec![1.0f32; 16 * 16];
        let s = SkyReplace { source: id.clone(), mask: Some(MaskShape::raster(16, 16, &cov)), amount: 100.0, ..SkyReplace::default() };
        let mut a = vec![[0.0, 0.0, 1.0, 1.0]; 16 * 16];
        apply(&mut a, 16, 16, &s);
        assert!(is_registered(&id));
        assert!(a[100][0] > 0.95 && a[100][2] < 0.05, "{:?}", a[100]);
        assert!(load_file("relative.png").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
