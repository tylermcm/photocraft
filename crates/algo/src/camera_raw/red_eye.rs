//! Replay click-local corrections with the main editor's unchanged red-eye kernel.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RedEyeCorrection {
    pub point: [f32; 2],
    pub pupil_size: f32,
    pub darken: f32,
}

pub fn validate(items: &[RedEyeCorrection]) -> Result<(), String> {
    if items.len() > crate::redeye::MAX_POINTS {
        return Err("at most 64 red-eye corrections".into());
    }
    for item in items {
        if item.point.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || !item.pupil_size.is_finite()
            || !(1.0..=100.0).contains(&item.pupil_size)
            || !item.darken.is_finite()
            || !(0.0..=100.0).contains(&item.darken)
        {
            return Err("invalid red-eye correction".into());
        }
    }
    Ok(())
}

fn sample(px: &[[f32; 4]], w: usize, h: usize, x: f32, y: f32) -> [f32; 4] {
    let x = x.clamp(0.0, w.saturating_sub(1) as f32);
    let y = y.clamp(0.0, h.saturating_sub(1) as f32);
    let (ix, iy) = (x.floor() as usize, y.floor() as usize);
    let (fx, fy) = (x - ix as f32, y - iy as f32);
    let at = |x: usize, y: usize| px.get(y * w + x).copied().unwrap_or([0.0; 4]);
    let (a, b, c, d) = (at(ix, iy), at((ix + 1).min(w - 1), iy), at(ix, (iy + 1).min(h - 1)), at((ix + 1).min(w - 1), (iy + 1).min(h - 1)));
    std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

pub fn apply(px: &mut [[f32; 4]], w: usize, h: usize, items: &[RedEyeCorrection], scale: f32) {
    if w == 0 || h == 0 || w.checked_mul(h) != Some(px.len()) || validate(items).is_err() || !scale.is_finite() {
        return;
    }
    let scale = scale.clamp(0.01, 1.0);
    for item in items {
        if item.darken == 0.0 {
            continue;
        }
        let cx = (item.point[0] * w as f32).floor().min((w - 1) as f32);
        let cy = (item.point[1] * h as f32).floor().min((h - 1) as f32);
        let radius = (crate::redeye::search_radius(item.pupil_size).ceil() as usize) + crate::redeye::halo_radius() as usize;
        let side = radius * 2 + 1;
        if side > crate::redeye::MAX_WINDOW as usize {
            continue;
        }
        // Bring only this bounded patch to full-resolution units. This keeps
        // search radius, erosion and the kernel's Gaussian consistent at proxy sizes.
        let mut patch = Vec::with_capacity(side * side);
        for y in 0..side {
            for x in 0..side {
                patch.push(sample(px, w, h, cx + (x as f32 - radius as f32) * scale, cy + (y as f32 - radius as f32) * scale));
            }
        }
        let original = patch.clone();
        if crate::redeye::apply(&mut patch, side, side, (radius as i32, radius as i32), (0, 0), item.pupil_size, item.darken) == 0 {
            continue;
        }
        for (p, before) in patch.iter_mut().zip(&original) {
            for c in 0..4 {
                p[c] -= before[c];
            }
        }
        let reach = radius as f32 * scale;
        let (x0, y0) = ((cx - reach).floor().max(0.0) as usize, (cy - reach).floor().max(0.0) as usize);
        let (x1, y1) = (((cx + reach).ceil() as usize + 1).min(w), ((cy + reach).ceil() as usize + 1).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let delta = sample(&patch, side, side, (x as f32 - cx) / scale + radius as f32, (y as f32 - cy) / scale + radius as f32);
                if let Some(p) = px.get_mut(y * w + x) {
                    // Add the correction delta, preserving untouched pixels and alpha exactly.
                    for c in 0..3 {
                        p[c] += delta[c];
                    }
                }
            }
        }
    }
}
