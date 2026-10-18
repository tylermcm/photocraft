//! Native Glow and sampled Point Color processing. Independently implemented;
//! Adobe's documented controls are a behavioral reference, not a pixel oracle.
use super::{CameraRaw, gauss, hsv_to_rgb, luma, rgb_to_hsv};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GlowStyle {
    #[default]
    Diffusion,
    Bloom,
    Halation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Glow {
    pub amount: f32,
    pub style: GlowStyle,
    pub range: f32,
    pub spread: f32,
    pub warmth: f32,
}
impl Default for Glow {
    fn default() -> Self {
        Self { amount: 0.0, style: GlowStyle::Diffusion, range: 50.0, spread: 50.0, warmth: 0.0 }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PointColor {
    /// Encoded RGB at the Point Color input stage (not display-managed RGB).
    pub color: [f32; 3],
    pub hue: f32,
    pub saturation: f32,
    pub luminance: f32,
    pub variance: f32,
    pub range: f32,
}
impl Default for PointColor {
    fn default() -> Self {
        Self { color: [0.5; 3], hue: 0.0, saturation: 0.0, luminance: 0.0, variance: 0.0, range: 50.0 }
    }
}
impl PointColor {
    pub fn neutral(&self) -> bool {
        self.hue == 0.0 && self.saturation == 0.0 && self.luminance == 0.0 && self.variance == 0.0
    }
    /// Soft selection in circular hue, saturation and value; neutral samples select neutrals.
    pub fn weight(&self, rgb: [f32; 3]) -> f32 {
        let a = rgb_to_hsv(self.color);
        let b = rgb_to_hsv(rgb.map(|v| v.max(0.0)));
        let r = self.range.clamp(0.0, 100.0) / 100.0;
        let hue = if a[1] < 0.02 {
            1.0
        } else {
            let delta = ((b[0] - a[0] + 180.0).rem_euclid(360.0) - 180.0).abs();
            (1.0 - delta / (3.0 + 117.0 * r)).clamp(0.0, 1.0) * (b[1] / a[1].clamp(0.001, 0.1)).min(1.0)
        };
        let sat = (1.0 - (b[1] - a[1]).abs() / (0.03 + 0.97 * r)).clamp(0.0, 1.0);
        let val = (1.0 - (b[2] - a[2]).abs() / (0.03 + 1.97 * r)).clamp(0.0, 1.0);
        let x = hue * sat * val;
        x * x * (3.0 - 2.0 * x)
    }
}
pub fn validate(p: &CameraRaw) -> Result<(), String> {
    for (name, v, lo, hi) in [
        ("glow.amount", p.glow.amount, 0.0, 100.0),
        ("glow.range", p.glow.range, 0.0, 100.0),
        ("glow.spread", p.glow.spread, 0.0, 100.0),
        ("glow.warmth", p.glow.warmth, -100.0, 100.0),
    ] {
        if !v.is_finite() || !(lo..=hi).contains(&v) {
            return Err(format!("{name} must be finite and within {lo}..{hi}"));
        }
    }
    if p.point_colors.len() > 8 {
        return Err("Point Color supports at most eight samples".into());
    }
    for sample in &p.point_colors {
        if sample.color.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err("Point Color sample must be RGB 0..1".into());
        }
        for v in [sample.hue, sample.saturation, sample.luminance, sample.variance] {
            if !v.is_finite() || !(-100.0..=100.0).contains(&v) {
                return Err("Point Color adjustments must be finite and within -100..100".into());
            }
        }
        if !sample.range.is_finite() || !(0.0..=100.0).contains(&sample.range) {
            return Err("Point Color range must be finite and within 0..100".into());
        }
    }
    Ok(())
}

/// Highlights are diffused in linear light; transparent hidden RGB cannot emit light.
pub(super) fn glow(px: &mut [[f32; 4]], w: usize, h: usize, p: &CameraRaw) {
    if p.glow.amount == 0.0 || w.checked_mul(h) != Some(px.len()) || w == 0 || h == 0 {
        return;
    }
    let enc = super::Encoding::of(p.encoding_gamma);
    let threshold = 0.95 - 0.9 * p.glow.range / 100.0;
    let alpha = |a: f32| if a.is_finite() { a.clamp(0.0, 1.0) } else { 0.0 };
    let emit: Vec<[f32; 3]> = px
        .iter()
        .map(|q| {
            let c = [q[0], q[1], q[2]].map(|v| if v.is_finite() { enc.decode(v).clamp(0.0, 1.0e12) } else { 0.0 });
            let k = super::smoothstep(threshold, threshold + 0.25, luma(c)) * alpha(q[3]);
            c.map(|v| v * k)
        })
        .collect();
    let sigma = (w.max(h) as f32 * (0.001 + 0.0003 * p.glow.spread)).max(0.5);
    let mut blurred = vec![[0.0; 3]; px.len()];
    for ch in 0..3 {
        let plane: Vec<f32> = emit.iter().map(|v| v[ch]).collect();
        for (q, v) in blurred.iter_mut().zip(gauss(&plane, w, h, sigma)) {
            q[ch] = v;
        }
    }
    let amount = p.glow.amount / 100.0;
    let warmth = p.glow.warmth / 100.0;
    for ((q, b), e) in px.iter_mut().zip(blurred).zip(emit) {
        if alpha(q[3]) == 0.0 {
            continue;
        }
        let c = [q[0], q[1], q[2]].map(|v| enc.decode(v).max(0.0));
        let tint = if p.glow.style == GlowStyle::Halation { [1.0, 0.45 - 0.4 * warmth, 0.08] } else { [1.0 + 0.4 * warmth, 1.0, 1.0 - 0.4 * warmth] };
        for ch in 0..3 {
            let light = if p.glow.style == GlowStyle::Halation { (b[ch] - e[ch]).max(0.0) } else { b[ch] };
            let base = if p.glow.style == GlowStyle::Diffusion { c[ch] * (1.0 - 0.1 * amount * b[ch].clamp(0.0, 1.0)) } else { c[ch] };
            let value = enc.encode(base + amount * light * tint[ch]);
            if value.is_finite() {
                q[ch] = value;
            }
        }
    }
}
/// Selections are evaluated against the same input, so earlier samples do not move later selections.
pub(super) fn points(px: &mut [[f32; 4]], samples: &[PointColor], float: bool) {
    if samples.iter().all(PointColor::neutral) {
        return;
    }
    for q in px {
        if q[3] <= 0.0 {
            continue;
        }
        // Match the picker's pre-Point-Color preview, including SDR output clipping.
        let input = [q[0], q[1], q[2]].map(|v| if float { v } else { v.clamp(0.0, 1.0) });
        let mut out = input;
        for s in samples.iter().filter(|s| !s.neutral()) {
            let weight = s.weight(input);
            if weight <= 0.0 {
                continue;
            }
            let [h, sat, v] = rgb_to_hsv(input.map(|v| v.max(0.0)));
            let a = rgb_to_hsv(s.color);
            let delta = (h - a[0] + 180.0).rem_euclid(360.0) - 180.0;
            let variance = s.variance / 100.0;
            let nh = h + 1.8 * s.hue + delta * variance;
            let ns = (sat * (1.0 + s.saturation / 100.0) + (sat - a[1]) * variance).clamp(0.0, 1.0);
            let nv = (v * 2f32.powf(s.luminance / 100.0) + (v - a[2]) * variance).max(0.0);
            let adjusted = hsv_to_rgb(nh, ns, nv);
            for ch in 0..3 {
                out[ch] += (adjusted[ch] - input[ch]) * weight;
            }
        }
        q[..3].copy_from_slice(&out);
    }
}
