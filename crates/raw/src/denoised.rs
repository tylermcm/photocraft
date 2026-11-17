//! AI-denoised raw files: the original raw file with denoised sensor data appended.
//!
//! A denoiser outside PhotoCraft (Image Triage's AI Denoise) reads the sensor through
//! [`denoise_input`], denoises the CFA samples and appends them to a copy of the original file as a
//! trailer. Every TIFF-style raw reader ignores bytes past the structures it points at, so the copy
//! still identifies, previews and reads its metadata as the original raw does; only [`decode`]
//! (through [`apply_trailer`]) looks at the end of the file and replaces the samples of the
//! denoised region, blended with the originals by the trailer's amount. Nothing else about the
//! raw changes: the development, colour and look are the original's.
//!
//! Trailer, little-endian, at the very end of the file:
//!
//! ```text
//! payload  w*h half floats: denoised samples of the region, normalised as Scale::value
//!          (0 = black, 1 = white), row-major
//! version  u32 = 1
//! amount   f32, 0 … 100 (how much of the denoised samples to use)
//! x, y, w, h                u32: the region, in sensor data coordinates
//! sensor width, height      u32: the sensor the samples were made for
//! payload length            u64 (bytes)
//! magic    b"PCDENOI1"
//! ```
//!
//! [`decode`]: crate::decode
use crate::sensor::{Rect, Sensor};

pub const MAGIC: &[u8; 8] = b"PCDENOI1";
/// Bytes after the payload: the fixed header and the magic.
pub const TRAILER_LEN: usize = 4 + 4 + 6 * 4 + 8 + 8;
/// Offset of the amount (f32) counted back from the end of the file, so a host can change it in place.
pub const AMOUNT_FROM_END: usize = TRAILER_LEN - 4;

/// The denoised samples a file carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Trailer {
    pub amount: f32,
    pub region: Rect,
    pub sensor_width: usize,
    pub sensor_height: usize,
    /// Normalised samples of the region, row-major.
    pub samples: Vec<f32>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The trailer of `bytes`, if it ends with a well-formed one. Bytes that merely end with the magic
/// but do not add up are treated as no trailer (the raw is then read as it is).
pub fn trailer(bytes: &[u8]) -> Option<Trailer> {
    let n = bytes.len();
    if n < TRAILER_LEN || &bytes[n - 8..] != MAGIC {
        return None;
    }
    let h = n - TRAILER_LEN;
    if u32_at(bytes, h)? != 1 {
        return None;
    }
    let amount = f32::from_le_bytes(bytes.get(h + 4..h + 8)?.try_into().ok()?);
    let field = |i: usize| u32_at(bytes, h + 8 + i * 4).map(|v| v as usize);
    let region = Rect::new(field(0)?, field(1)?, field(2)?, field(3)?);
    let (sensor_width, sensor_height) = (field(4)?, field(5)?);
    let payload_len = usize::try_from(u64::from_le_bytes(bytes.get(h + 32..h + 40)?.try_into().ok()?)).ok()?;
    let expected = region.width.checked_mul(region.height)?.checked_mul(2)?;
    if payload_len != expected || payload_len > h || !amount.is_finite() {
        return None;
    }
    let payload = &bytes[h - payload_len..h];
    let samples = payload.chunks_exact(2).map(|c| f16_to_f32(u16::from_le_bytes([c[0], c[1]]))).collect();
    Some(Trailer { amount: amount.clamp(0.0, 100.0), region, sensor_width, sensor_height, samples })
}

/// The original raw file inside `bytes`: everything before the trailer (all of `bytes` without one).
pub fn original(bytes: &[u8]) -> &[u8] {
    match trailer(bytes) {
        Some(t) => &bytes[..bytes.len() - TRAILER_LEN - t.samples.len() * 2],
        None => bytes,
    }
}

/// The trailer for `t`, to append to a copy of the original raw file.
pub fn encode_trailer(t: &Trailer) -> Vec<u8> {
    let mut out = Vec::with_capacity(t.samples.len() * 2 + TRAILER_LEN);
    for v in &t.samples {
        out.extend_from_slice(&f32_to_f16(*v).to_le_bytes());
    }
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&t.amount.to_le_bytes());
    for v in [t.region.x, t.region.y, t.region.width, t.region.height, t.sensor_width, t.sensor_height] {
        out.extend_from_slice(&(v as u32).to_le_bytes());
    }
    out.extend_from_slice(&((t.samples.len() * 2) as u64).to_le_bytes());
    out.extend_from_slice(MAGIC);
    out
}

/// Why a trailer cannot be applied to `s`, or `None` when it can.
fn unusable(s: &Sensor, t: &Trailer) -> Option<&'static str> {
    let r = t.region;
    if s.width != t.sensor_width || s.height != t.sensor_height {
        Some("the denoised data was made for a different sensor size")
    } else if s.samples != 1 || !s.cfa.as_ref().is_some_and(|c| c.is_bayer()) {
        Some("AI Denoise needs a Bayer sensor")
    } else if s.linearization.is_some() {
        Some("AI Denoise does not support sensors with a linearization table yet")
    } else if r.is_empty() || r.x.checked_add(r.width).is_none_or(|v| v > s.width) || r.y.checked_add(r.height).is_none_or(|v| v > s.height) {
        Some("the denoised region lies outside the sensor")
    } else {
        None
    }
}

/// Replaces the denoised region of `s` (decoded from the original raw) by the trailer's samples,
/// blended with the original samples by `amount`. A trailer that does not fit leaves `s` as it is
/// and adds a warning, so the photo still develops.
pub fn apply_trailer(s: &mut Sensor, t: &Trailer) {
    if let Some(why) = unusable(s, t) {
        s.warnings.push(format!("AI Denoise not applied: {why}"));
        return;
    }
    let a = t.amount / 100.0;
    if a <= 0.0 {
        return;
    }
    let r = t.region;
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            let i = y * s.width + x;
            let black = s.black.at(x.saturating_sub(s.active.x), y.saturating_sub(s.active.y), 0, 1);
            let range = s.white[0] - black;
            if !(range >= 1.0) {
                continue;
            }
            let orig = (f32::from(s.data[i]) - black) / range;
            let den = t.samples[(y - r.y) * r.width + (x - r.x)];
            if !den.is_finite() {
                continue;
            }
            let v = black + range * (orig + a * (den - orig));
            s.data[i] = v.round().clamp(0.0, 65535.0) as u16;
        }
    }
}

/// What a denoiser needs from a sensor: its CFA samples normalised to 0 (black) … 1 (white) over
/// the crop, the 2×2 pattern at the crop's top-left, and the white-balance multipliers of daylight
/// (green = 1) the denoiser was trained with.
#[derive(Debug, Clone)]
pub struct DenoiseInput {
    pub region: Rect,
    /// `"RGGB"`, `"BGGR"`, `"GRBG"` or `"GBRG"` from the region's top-left sample.
    pub pattern: String,
    pub daylight_multipliers: [f32; 3],
    pub samples: Vec<f32>,
}

/// The input [`denoise_input`] gives, or why this sensor cannot be denoised.
pub fn denoise_input(s: &Sensor) -> Result<DenoiseInput, String> {
    let probe = Trailer { amount: 0.0, region: s.crop, sensor_width: s.width, sensor_height: s.height, samples: Vec::new() };
    if let Some(why) = unusable(s, &probe) {
        return Err(why.to_string());
    }
    let r = s.crop;
    let cfa = s.cfa.as_ref().expect("checked: Bayer");
    let pattern: String = cfa.phase(r.x, r.y).iter().map(|c| ['R', 'G', 'B'][usize::from(*c).min(2)]).collect();
    let neutral = s
        .color
        .xy_to_neutral([0.3127, 0.3290])
        .or_else(|| s.camera_wb.map(|m| m.map(|v| 1.0 / v)))
        .unwrap_or([1.0, 1.0, 1.0]);
    let g = neutral[1].max(1e-9);
    let daylight_multipliers = neutral.map(|n| (g / n.max(1e-9)) as f32);
    let mut samples = Vec::with_capacity(r.width * r.height);
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            let black = s.black.at(x.saturating_sub(s.active.x), y.saturating_sub(s.active.y), 0, 1);
            let range = s.white[0] - black;
            samples.push(if range >= 1.0 { (f32::from(s.data[y * s.width + x]) - black) / range } else { 0.0 });
        }
    }
    Ok(DenoiseInput { region: r, pattern, daylight_multipliers, samples })
}

/// IEEE 754 half to single precision.
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (h >> 10) & 0x1f;
    let frac = f32::from(h & 0x3ff);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => (1.0 + frac / 1024.0) * 2f32.powi(i32::from(e) - 15),
    }
}

/// Single to half precision, rounding to nearest; out-of-range values become ±65504.
pub fn f32_to_f16(v: f32) -> u16 {
    if v.is_nan() {
        return 0x7e00;
    }
    let sign: u16 = if v.is_sign_negative() { 0x8000 } else { 0 };
    let a = v.abs().min(65504.0);
    if a < 2f32.powi(-24) {
        return sign;
    }
    if a < 2f32.powi(-14) {
        return sign | (a / 2f32.powi(-24)).round() as u16;
    }
    let e = a.log2().floor() as i32;
    let mut m = ((a / 2f32.powi(e) - 1.0) * 1024.0).round() as u32;
    let mut e = e + 15;
    if m == 1024 {
        m = 0;
        e += 1;
    }
    if e >= 31 {
        return sign | 0x7bff;
    }
    sign | ((e as u16) << 10) | m as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensor::{BlackLevels, Cfa};

    fn sensor() -> Sensor {
        let (w, h) = (8, 6);
        Sensor {
            format: crate::RawFormat::Dng,
            make: None,
            model: None,
            width: w,
            height: h,
            samples: 1,
            data: (0..w * h).map(|i| 1000 + (i as u16 * 37) % 500).collect(),
            cfa: Cfa::bayer("GRBG"),
            linearization: None,
            black: BlackLevels::uniform(100.0),
            white: [4100.0; 3],
            active: Rect::new(0, 0, w, h),
            crop: Rect::new(1, 1, 6, 4),
            color: crate::ColorInfo::default(),
            camera_wb: Some([2.0, 1.0, 1.5]),
            orientation: 1,
            baseline_exposure: 0.0,
            gain_maps: Vec::new(),
            tone_curve: Vec::new(),
            black_point: 0.0,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn half_floats_round_trip_closely() {
        for v in [0.0f32, 1e-5, 0.0123, 0.25, 0.5, 0.99, 1.0, 1.5, -0.01, 123.0] {
            let back = f16_to_f32(f32_to_f16(v));
            assert!((back - v).abs() <= v.abs() / 1024.0 + 1e-7, "{v} -> {back}");
        }
    }

    #[test]
    fn input_is_the_crop_normalised_with_its_pattern() {
        let s = sensor();
        let input = denoise_input(&s).unwrap();
        assert_eq!(input.region, s.crop);
        assert_eq!(input.pattern, "GBRG"); // GRBG seen from (1, 1)
        assert_eq!(input.samples.len(), 24);
        let first = (f32::from(s.data[s.width + 1]) - 100.0) / 4000.0;
        assert!((input.samples[0] - first).abs() < 1e-6);
    }

    #[test]
    fn a_trailer_replaces_the_region_by_its_amount() {
        let s = sensor();
        let input = denoise_input(&s).unwrap();
        let flat = vec![0.5f32; input.samples.len()];
        let mut file = b"original raw bytes".to_vec();
        let t = Trailer { amount: 100.0, region: input.region, sensor_width: s.width, sensor_height: s.height, samples: flat.clone() };
        file.extend(encode_trailer(&t));
        let read = trailer(&file).unwrap();
        assert_eq!(read.region, t.region);
        assert_eq!(original(&file), b"original raw bytes");

        let mut full = s.clone();
        apply_trailer(&mut full, &read);
        assert_eq!(full.data[s.width + 1], 2100); // black + 0.5 * range
        assert_eq!(full.data[0], s.data[0]); // outside the region: untouched

        let mut half = s.clone();
        apply_trailer(&mut half, &Trailer { amount: 50.0, ..read.clone() });
        let orig = f32::from(s.data[s.width + 1]);
        assert_eq!(half.data[s.width + 1], ((orig + 2100.0) / 2.0).round() as u16);

        let mut none = s.clone();
        apply_trailer(&mut none, &Trailer { amount: 0.0, ..read });
        assert_eq!(none.data, s.data);
    }

    #[test]
    fn the_amount_can_be_changed_in_place() {
        let s = sensor();
        let input = denoise_input(&s).unwrap();
        let mut file = b"raw".to_vec();
        file.extend(encode_trailer(&Trailer { amount: 100.0, region: input.region, sensor_width: s.width, sensor_height: s.height, samples: input.samples }));
        let at = file.len() - AMOUNT_FROM_END;
        file[at..at + 4].copy_from_slice(&25f32.to_le_bytes());
        assert_eq!(trailer(&file).unwrap().amount, 25.0);
    }

    #[test]
    fn a_damaged_or_foreign_trailer_is_ignored_or_refused() {
        assert!(trailer(b"no trailer here").is_none());
        let mut bad = vec![0u8; 10];
        bad.extend_from_slice(MAGIC);
        assert!(trailer(&bad).is_none());

        let s = sensor();
        let mut other = s.clone();
        let t = Trailer { amount: 100.0, region: Rect::new(0, 0, 2, 2), sensor_width: 99, sensor_height: 6, samples: vec![0.5; 4] };
        apply_trailer(&mut other, &t);
        assert_eq!(other.data, s.data);
        assert!(other.warnings.iter().any(|w| w.contains("different sensor size")));
    }

    #[test]
    fn non_bayer_sensors_are_refused() {
        let mut s = sensor();
        s.cfa = None;
        s.samples = 1;
        assert!(denoise_input(&s).is_err());
    }
}
