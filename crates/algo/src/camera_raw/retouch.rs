//! Non-destructive retouch replay using the same completion and Poisson kernels as the paint tools.
use photocraft_raster::Interrupt;
use serde::{Deserialize, Serialize};

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
}

pub fn validate(strokes: &[RetouchStroke]) -> Result<(), String> {
    if strokes.len() > 128 {
        return Err("at most 128 retouch strokes".into());
    }
    let mut total = 0;
    for s in strokes {
        if !["remove", "heal", "spot", "clone"].contains(&s.kind.as_str()) {
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
    }
    Ok(())
}

pub fn validate_size(strokes: &[RetouchStroke], w: usize, h: usize) -> Result<(), String> {
    validate(strokes)?;
    for s in strokes {
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
