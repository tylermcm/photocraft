//! Non-destructive retouch replay using the same completion and Poisson kernels as the paint tools.
use photocraft_raster::Interrupt;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetouchStroke {
    pub kind: String,
    pub points: Vec<[f32; 2]>,
    /// Diameter relative to the shorter image side.
    pub size: f32,
    pub feather: f32,
    pub opacity: f32,
    #[serde(default)]
    pub source: Option<[f32; 2]>,
    /// The picture a `patch` stroke drops into the frame; required for that kind and invalid for every other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<RetouchPatch>,
}

/// An externally computed picture (for example an AI fill) laid into the frame by a `patch` stroke.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetouchPatch {
    /// Absolute path of a PNG (8 or 16 bit, RGBA). RGB is the filled picture in the engine's working encoding at the
    /// retouch stage (the numbers `px[..3]` holds there; the engine converts nothing). A is coverage 0..1: 1 inside the
    /// area to replace, falling to 0 across a feathered edge.
    pub path: String,
    /// The part of the frame the picture covers, as fractions of the frame: `[x0, y0, x1, y1]`.
    pub region: [f32; 4],
}

const MAX_PATCH_PATH: usize = 512;
const MAX_PATCH_FILE: u64 = 64 << 20;
const MAX_PATCHES_KEPT: usize = 8;
const MAX_AREA: usize = 16 * 1024 * 1024;

fn validate_patch(s: &RetouchStroke) -> Result<(), String> {
    match (s.kind == "patch", &s.patch) {
        (false, None) => Ok(()),
        (false, Some(_)) => Err("only a patch stroke carries a patch".into()),
        (true, None) => Err("a patch stroke needs a patch".into()),
        (true, Some(p)) => {
            if p.path.is_empty() || p.path.len() > MAX_PATCH_PATH || !std::path::Path::new(&p.path).is_absolute() {
                return Err("a retouch patch needs an absolute path of at most 512 characters".into());
            }
            let [x0, y0, x1, y1] = p.region;
            if p.region.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) || x0 >= x1 || y0 >= y1 {
                return Err("invalid retouch patch region".into());
            }
            Ok(())
        }
    }
}

/// The patch region in whole pixels of a `w x h` frame: `(x0, y0, x1, y1)`, at least one pixel each way.
fn patch_rect(region: [f32; 4], w: usize, h: usize) -> (usize, usize, usize, usize) {
    let px = |f: f32, n: usize| ((f * n as f32).round().max(0.0) as usize).min(n);
    let x0 = px(region[0], w).min(w.saturating_sub(1));
    let y0 = px(region[1], h).min(h.saturating_sub(1));
    let x1 = px(region[2], w).max(x0 + 1).min(w);
    let y1 = px(region[3], h).max(y0 + 1).min(h);
    (x0, y0, x1, y1)
}

/// A decoded patch picture: RGBA as f32 0..1, `w x h`.
struct PatchPicture {
    w: usize,
    h: usize,
    rgba: Vec<f32>,
}

type PatchKey = (String, u64, Option<std::time::SystemTime>);

static PATCHES: Mutex<VecDeque<(PatchKey, Arc<PatchPicture>)>> = Mutex::new(VecDeque::new());

/// The picture at `path`, decoded on first use and kept (the oldest of eight is dropped) while the file's size and
/// modified time stay the same. `None` for a file that is missing, unreadable, too large or not a picture.
fn load_patch(path: &str) -> Option<Arc<PatchPicture>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_PATCH_FILE {
        return None;
    }
    let key: PatchKey = (path.to_string(), meta.len(), meta.modified().ok());
    let find = |reg: &VecDeque<(PatchKey, Arc<PatchPicture>)>| reg.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
    if let Some(hit) = find(&PATCHES.lock().unwrap_or_else(|e| e.into_inner())) {
        return Some(hit);
    }
    let bytes = std::fs::read(path).ok()?;
    let img = photocraft_codecs::decode(&bytes).ok()?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let rgba = img.to_rgba_f32();
    if w == 0 || h == 0 || w.checked_mul(h)?.checked_mul(4)? != rgba.len() {
        return None;
    }
    let pic = Arc::new(PatchPicture { w, h, rgba });
    let mut reg = PATCHES.lock().unwrap_or_else(|e| e.into_inner());
    reg.retain(|(k, _)| k.0 != key.0);
    reg.push_back((key, pic.clone()));
    while reg.len() > MAX_PATCHES_KEPT {
        reg.pop_front();
    }
    Some(pic)
}

/// Lays a patch picture into `px`: resampled to its region, adapted to the surroundings by seamless cloning, then
/// blended by its coverage and the stroke's opacity. Does nothing when the picture cannot be had.
fn apply_patch(px: &mut [[f32; 4]], w: usize, h: usize, s: &RetouchStroke) {
    let Some(patch) = &s.patch else {
        return;
    };
    let (x0, y0, x1, y1) = patch_rect(patch.region, w, h);
    let (rw, rh) = (x1 - x0, y1 - y0);
    let Some(n) = rw.checked_mul(rh) else {
        return;
    };
    if n == 0 || n > MAX_AREA {
        return;
    }
    let Some(pic) = load_patch(&patch.path) else {
        return;
    };
    let rgba = crate::seam::resize_bilinear(pic.w, pic.h, 4, &pic.rgba, rw, rh);
    if rgba.len() != n * 4 {
        return;
    }
    let mut src = Vec::with_capacity(n * 3);
    let mut alpha = Vec::with_capacity(n);
    for c in rgba.as_chunks::<4>().0 {
        src.extend_from_slice(&c[..3]);
        alpha.push(c[3].clamp(0.0, 1.0));
    }
    let mask: Vec<bool> = alpha.iter().map(|a| *a > 0.02).collect();
    if !mask.contains(&true) {
        return;
    }
    let mut dst = Vec::with_capacity(n * 3);
    for y in y0..y1 {
        for x in x0..x1 {
            dst.extend_from_slice(&px[y * w + x][..3]);
        }
    }
    let filled = crate::poisson::seamless_clone(rw, rh, 3, &src, &dst, &mask);
    for y in 0..rh {
        for x in 0..rw {
            let i = y * rw + x;
            let k = alpha[i] * s.opacity / 100.0;
            if k > 0.0 {
                for c in 0..3 {
                    let out = &mut px[(y + y0) * w + x + x0][c];
                    *out += (filled[i * 3 + c] - *out) * k;
                }
            }
        }
    }
}

pub fn validate(strokes: &[RetouchStroke]) -> Result<(), String> {
    if strokes.len() > 128 {
        return Err("at most 128 retouch strokes".into());
    }
    let mut total = 0;
    for s in strokes {
        if !["remove", "heal", "spot", "clone", "patch"].contains(&s.kind.as_str()) {
            return Err("unknown retouch tool".into());
        }
        total += s.points.len();
        if s.points.is_empty() || s.points.len() > 4096 || total > 32768 {
            return Err("retouch point budget exceeded".into());
        }
        if s.points.iter().flatten().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err("invalid retouch coordinates".into());
        }
        if !s.size.is_finite()
            || !(0.001..=0.5).contains(&s.size)
            || !s.feather.is_finite()
            || !(0.0..=100.0).contains(&s.feather)
            || !s.opacity.is_finite()
            || !(0.0..=100.0).contains(&s.opacity)
        {
            return Err("invalid retouch brush".into());
        }
        if let Some(p) = s.source
            && p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("invalid retouch source".into());
        }
        if ["clone", "heal"].contains(&s.kind.as_str()) && s.source.is_none() {
            return Err("choose a retouch source first".into());
        }
        validate_patch(s)?;
    }
    Ok(())
}

pub fn validate_size(strokes: &[RetouchStroke], w: usize, h: usize) -> Result<(), String> {
    validate(strokes)?;
    for s in strokes {
        if let Some(patch) = s.patch.as_ref().filter(|_| s.kind == "patch") {
            let (x0, y0, x1, y1) = patch_rect(patch.region, w, h);
            if w == 0 || h == 0 || (x1 - x0).checked_mul(y1 - y0).is_none_or(|n| n > MAX_AREA) {
                return Err("Retouch area too large; use smaller separate strokes".into());
            }
            continue;
        }
        let radius = s.size * w.min(h) as f32 / 2.0;
        let margin = if ["remove", "spot"].contains(&s.kind.as_str()) { (radius * 4.0).max(32.0) } else { 3.0 };
        let minx = s.points.iter().map(|p| p[0] * w as f32).fold(f32::INFINITY, f32::min);
        let maxx = s.points.iter().map(|p| p[0] * w as f32).fold(0.0, f32::max);
        let miny = s.points.iter().map(|p| p[1] * h as f32).fold(f32::INFINITY, f32::min);
        let maxy = s.points.iter().map(|p| p[1] * h as f32).fold(0.0, f32::max);
        let rw = ((maxx + radius + margin).ceil().min(w as f32) - (minx - radius - margin).floor().max(0.0)).max(0.0) as usize;
        let rh = ((maxy + radius + margin).ceil().min(h as f32) - (miny - radius - margin).floor().max(0.0)).max(0.0) as usize;
        if rw.checked_mul(rh).is_none_or(|n| n > 16 * 1024 * 1024) {
            return Err("Retouch area too large; use smaller separate strokes".into());
        }
    }
    Ok(())
}

pub fn apply(px: &mut [[f32; 4]], w: usize, h: usize, strokes: &[RetouchStroke]) {
    if w == 0 || h == 0 || w.checked_mul(h) != Some(px.len()) || validate(strokes).is_err() {
        return;
    }
    for s in strokes {
        if s.opacity == 0.0 {
            continue;
        }
        if s.kind == "patch" {
            apply_patch(px, w, h, s);
            continue;
        }
        let radius = (s.size * w.min(h) as f32 / 2.0).max(0.5);
        let minx = s.points.iter().map(|p| p[0] * w as f32).fold(f32::INFINITY, f32::min);
        let maxx = s.points.iter().map(|p| p[0] * w as f32).fold(0.0, f32::max);
        let miny = s.points.iter().map(|p| p[1] * h as f32).fold(f32::INFINITY, f32::min);
        let maxy = s.points.iter().map(|p| p[1] * h as f32).fold(0.0, f32::max);
        let margin = if ["remove", "spot"].contains(&s.kind.as_str()) { (radius * 4.0).max(32.0) } else { 3.0 };
        let x0 = ((minx - radius - margin).floor().max(0.0) as usize).min(w - 1);
        let y0 = ((miny - radius - margin).floor().max(0.0) as usize).min(h - 1);
        let x1 = ((maxx + radius + margin).ceil() as usize).min(w);
        let y1 = ((maxy + radius + margin).ceil() as usize).min(h);
        let (rw, rh) = (x1 - x0, y1 - y0);
        let Some(n) = rw.checked_mul(rh) else {
            continue;
        };
        // Dense domains are bounded independently of the full-resolution canvas.
        if n == 0 || n > 16 * 1024 * 1024 {
            continue;
        }
        let mut cov = vec![0.0f32; n];
        for pair in s.points.windows(2).chain(std::iter::once(&s.points[..1])) {
            let a = pair[0];
            let b = *pair.last().unwrap_or(&a);
            let (ax, ay, bx, by) = (a[0] * w as f32, a[1] * h as f32, b[0] * w as f32, b[1] * h as f32);
            let (dx, dy) = (bx - ax, by - ay);
            let length = dx * dx + dy * dy;
            let lx = ((ax.min(bx) - radius).floor().max(x0 as f32) as usize).min(x1);
            let rx = ((ax.max(bx) + radius).ceil() as usize).min(x1);
            let ty = ((ay.min(by) - radius).floor().max(y0 as f32) as usize).min(y1);
            let byy = ((ay.max(by) + radius).ceil() as usize).min(y1);
            for y in ty..byy {
                for x in lx..rx {
                    let t = if length > 1e-8 { (((x as f32 + 0.5 - ax) * dx + (y as f32 + 0.5 - ay) * dy) / length).clamp(0.0, 1.0) } else { 0.0 };
                    let d = ((x as f32 + 0.5 - ax - t * dx).hypot(y as f32 + 0.5 - ay - t * dy)) / radius;
                    let f = (s.feather / 100.0).max(0.001);
                    let v = ((1.0 - d) / f).clamp(0.0, 1.0);
                    let v = v * v * (3.0 - 2.0 * v);
                    let i = (y - y0) * rw + x - x0;
                    cov[i] = cov[i].max(v);
                }
            }
        }
        let mut dst = Vec::with_capacity(n * 3);
        for y in y0..y1 {
            for x in x0..x1 {
                dst.extend_from_slice(&px[y * w + x][..3]);
            }
        }
        let hole: Vec<bool> = cov.iter().map(|c| *c > 0.0).collect();
        let mut filled = dst.clone();
        if let Some(source) = s.source {
            let first = s.points[0];
            let (ox, oy) = ((source[0] - first[0]) * w as f32, (source[1] - first[1]) * h as f32);
            for y in 0..rh {
                for x in 0..rw {
                    let sx = x as f32 + x0 as f32 + ox;
                    let sy = y as f32 + y0 as f32 + oy;
                    if sx < 0.0 || sy < 0.0 || sx > w.saturating_sub(1) as f32 || sy > h.saturating_sub(1) as f32 {
                        cov[y * rw + x] = 0.0;
                        continue;
                    }
                    let (ix, iy) = (sx.floor() as usize, sy.floor() as usize);
                    let (fx, fy) = (sx - ix as f32, sy - iy as f32);
                    for c in 0..3 {
                        filled[(y * rw + x) * 3 + c] = (px[iy * w + ix][c] * (1.0 - fx) + px[iy * w + (ix + 1).min(w - 1)][c] * fx) * (1.0 - fy)
                            + (px[(iy + 1).min(h - 1) * w + ix][c] * (1.0 - fx) + px[(iy + 1).min(h - 1) * w + (ix + 1).min(w - 1)][c] * fx) * fy;
                    }
                }
            }
            if s.kind == "heal" {
                let mask: Vec<bool> = cov.iter().map(|c| *c > 0.0).collect();
                filled = crate::poisson::seamless_clone(rw, rh, 3, &filled, &dst, &mask);
            }
        } else if s.kind == "remove" {
            let closed = crate::remove::close_loops(rw, rh, &hole);
            let grown = crate::remove::dilate(rw, rh, &closed, 2);
            filled = crate::remove::remove_with(rw, rh, 3, &dst, &grown, 0x0052_e30e, &Interrupt::NONE).unwrap_or_else(|_| dst.clone());
            // Remove fills enclosed areas as the main editor does.
            for ((c, painted), inside) in cov.iter_mut().zip(&hole).zip(closed) {
                if inside && !painted {
                    *c = 1.0;
                }
            }
        } else {
            let domain = crate::remove::dilate(rw, rh, &hole, 2);
            filled = crate::inpaint::complete(rw, rh, 3, &dst, &domain, &crate::inpaint::CompleteParams::default())
                .unwrap_or_else(|| crate::poisson::membrane_fill(rw, rh, 3, &dst, &domain));
            let heal = crate::remove::dilate(rw, rh, &hole, 1);
            filled = crate::poisson::seamless_clone(rw, rh, 3, &filled, &dst, &heal);
        }
        for y in 0..rh {
            for x in 0..rw {
                let i = y * rw + x;
                let k = cov[i] * s.opacity / 100.0;
                if k > 0.0 {
                    for c in 0..3 {
                        let out = &mut px[(y + y0) * w + x + x0][c];
                        *out += (filled[i * 3 + c] - *out) * k;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod patch_tests;
