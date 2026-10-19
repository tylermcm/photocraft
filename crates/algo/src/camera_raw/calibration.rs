//! Calibration: Shadows Tint and per-primary Hue / Saturation. Independently implemented;
//! Adobe's documented controls are a behavioral reference, not a pixel oracle.
use super::{CameraRaw, Encoding, luma, smoothstep};

/// Largest primary rotation (degrees) at a Hue of ±100.
const HUE_SPAN: f32 = 40.0;

impl CameraRaw {
    /// True when no Calibration control moves anything.
    pub fn calibration_neutral(&self) -> bool {
        self.calib_shadow_tint == 0.0
            && self.calib_hue.iter().all(|v| *v == 0.0)
            && self.calib_sat.iter().all(|v| *v == 0.0)
    }
}

pub fn validate(p: &CameraRaw) -> Result<(), String> {
    let all = std::iter::once(("calibration.shadowTint", p.calib_shadow_tint))
        .chain(p.calib_hue.iter().map(|v| ("calibration.hue", *v)))
        .chain(p.calib_sat.iter().map(|v| ("calibration.saturation", *v)));
    for (name, v) in all {
        if !v.is_finite() || !(-100.0..=100.0).contains(&v) {
            return Err(format!("{name} must be finite and within -100..100"));
        }
    }
    Ok(())
}

/// Rotates `v` about the grey axis (1,1,1) by `deg` degrees (Rodrigues' formula).
fn rotate_hue(v: [f32; 3], deg: f32) -> [f32; 3] {
    let (s, c) = deg.to_radians().sin_cos();
    let k = 1.0 / 3.0f32.sqrt();
    let axis = [k, k, k];
    let dot = v[0] * axis[0] + v[1] * axis[1] + v[2] * axis[2];
    let cross = [axis[1] * v[2] - axis[2] * v[1], axis[2] * v[0] - axis[0] * v[2], axis[0] * v[1] - axis[1] * v[0]];
    [0, 1, 2].map(|i| v[i] * c + cross[i] * s + axis[i] * dot * (1.0 - c))
}

/// Linear-light matrix whose columns are the re-calibrated red, green and blue primaries.
/// Rows sum to one, so neutrals stay neutral.
pub(super) fn matrix(p: &CameraRaw) -> [[f32; 3]; 3] {
    let mut m = [[0.0f32; 3]; 3];
    for ch in 0..3 {
        let mut e = [0.0f32; 3];
        e[ch] = 1.0;
        let mut v = rotate_hue(e, p.calib_hue[ch] / 100.0 * HUE_SPAN);
        let mean = (v[0] + v[1] + v[2]) / 3.0;
        let gain = (1.0 + p.calib_sat[ch] / 100.0).max(0.0);
        v = v.map(|x| mean + (x - mean) * gain);
        for row in 0..3 {
            m[row][ch] = v[row];
        }
    }
    for row in &mut m {
        let sum: f32 = row.iter().sum();
        if sum.abs() > 1e-6 {
            for x in row.iter_mut() {
                *x /= sum;
            }
        }
    }
    m
}

/// Applied first, in linear light, as camera calibration precedes every other adjustment.
pub(super) fn apply(px: &mut [[f32; 4]], p: &CameraRaw) {
    if p.calibration_neutral() {
        return;
    }
    let enc = Encoding::of(p.encoding_gamma);
    let m = matrix(p);
    let tint = p.calib_shadow_tint / 100.0 * 0.15;
    for q in px.iter_mut() {
        if !(q[3] > 0.0) || q[..3].iter().any(|v| !v.is_finite()) {
            continue;
        }
        let c = [q[0], q[1], q[2]].map(|v| enc.decode(v).max(0.0));
        let mut o = [0, 1, 2].map(|r| m[r][0] * c[0] + m[r][1] * c[1] + m[r][2] * c[2]);
        if tint != 0.0 {
            // Positive tint leans shadows magenta (less green), negative green.
            o[1] *= 1.0 - tint * (1.0 - smoothstep(0.0, 0.25, luma(o)));
        }
        for ch in 0..3 {
            let v = enc.encode(o[ch].max(0.0));
            if v.is_finite() {
                q[ch] = v;
            }
        }
    }
}
