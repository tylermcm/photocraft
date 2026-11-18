//! AI-denoised raw files: the original raw file with denoised sensor data appended, or a small companion
//! file beside the raw that holds only the denoised data (see [`companion`] and [`expand`]).
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
    let neutral = s.color.xy_to_neutral([0.3127, 0.3290]).or_else(|| s.camera_wb.map(|m| m.map(|v| 1.0 / v))).unwrap_or([1.0, 1.0, 1.0]);
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

// ---- the companion file -------------------------------------------------------------------------------------------------
//
// The denoised samples kept beside a raw instead of in a copy of it, as Camera Raw keeps its AI results in a `.acr` file beside
// the photo: a host passes the companion where it would pass the raw, and the reader of the file (PhotoCraft's `app.render`, a
// preview worker) reads the original raw it names and decodes the two together ([`expand`]). Nothing else changes: the decoder
// sees the original raw with the trailer above, built in memory.
//
// The samples are kept on a square-root scale (how sensor noise grows with the signal), quantised to `bits`, split into the four
// CFA planes of the region, predicted from their neighbours in the plane (the median edge detector of lossless JPEG) and the
// differences zlib-compressed: about 22 MB for a 45 MP sensor at 10 bits, against 91 MB as half floats beside a copy of the raw.
//
// ```text
// magic          b"PCDNCMP1"
// version        u32 = 1
// amount         f32, 0 … 100, at byte 12 (a host changes it in place)
// x, y, w, h     u32: the region (even width and height, starting on the 2×2 pattern's first sample)
// sensor w, h    u32
// bits           u32, 8 … 14
// original       u32 length, then UTF-8: the raw's path relative to the companion's folder, `/`-separated
// residuals      u64 length, then zlib: one i8 per sample, plane by plane ((0,0), (0,1), (1,0), (1,1)), row-major;
//                -128 means "the next escape"
// escapes        u64 length, then zlib: i16 little-endian, in order
// ```

pub const COMPANION_MAGIC: &[u8; 8] = b"PCDNCMP1";
/// Where a companion's amount (f32) is, from the start of the file.
pub const COMPANION_AMOUNT_AT: usize = 12;
const ESCAPE: i8 = -128;

/// A companion file: the original raw it belongs to and the denoised samples.
#[derive(Debug, Clone, PartialEq)]
pub struct Companion {
    /// The original raw's path, relative to the companion's folder.
    pub original: String,
    pub trailer: Trailer,
}

/// `true` when `bytes` are a companion file (whether or not it is well-formed).
pub fn is_companion(bytes: &[u8]) -> bool {
    bytes.starts_with(COMPANION_MAGIC)
}

fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data).take(limit as u64 + 1).read_to_end(&mut out).map_err(|e| format!("the denoise companion is damaged ({e})"))?;
    if out.len() > limit {
        return Err("the denoise companion is damaged (too much data)".into());
    }
    Ok(out)
}

fn median_prediction(left: i32, up: i32, up_left: i32) -> i32 {
    let (hi, lo) = (left.max(up), left.min(up));
    if up_left >= hi {
        lo
    } else if up_left <= lo {
        hi
    } else {
        left + up - up_left
    }
}

fn section<'a>(bytes: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let len = usize::try_from(u64::from_le_bytes(bytes.get(*at..*at + 8)?.try_into().ok()?)).ok()?;
    let end = at.checked_add(8)?.checked_add(len)?;
    let data = bytes.get(*at + 8..end)?;
    *at = end;
    Some(data)
}

/// The companion in `bytes`; `Ok(None)` when they are not one, `Err` when they are one but cannot be read.
pub fn companion(bytes: &[u8]) -> Result<Option<Companion>, String> {
    if !is_companion(bytes) {
        return Ok(None);
    }
    let bad = || "the denoise companion is damaged".to_string();
    let u = |at: usize| u32_at(bytes, at).map(|v| v as usize).ok_or_else(bad);
    if u(8)? != 1 {
        return Err("the denoise companion was written by a newer version".into());
    }
    let amount = f32::from_le_bytes(bytes.get(12..16).ok_or_else(bad)?.try_into().map_err(|_| bad())?);
    let region = Rect::new(u(16)?, u(20)?, u(24)?, u(28)?);
    let (sensor_width, sensor_height, bits) = (u(32)?, u(36)?, u(40)?);
    let name_len = u(44)?;
    if !(8..=14).contains(&bits) || name_len > 4096 || !amount.is_finite() {
        return Err(bad());
    }
    let (w, h) = (region.width, region.height);
    let count = w.checked_mul(h).filter(|n| *n > 0 && *n <= 200_000_000 && w % 2 == 0 && h % 2 == 0).ok_or_else(bad)?;
    let mut at = 48;
    let original = std::str::from_utf8(bytes.get(at..at + name_len).ok_or_else(bad)?).map_err(|_| bad())?.to_string();
    at += name_len;
    let residuals = inflate(section(bytes, &mut at).ok_or_else(bad)?, count)?;
    let escapes = inflate(section(bytes, &mut at).ok_or_else(bad)?, count * 2)?;
    if residuals.len() != count || escapes.len() % 2 != 0 {
        return Err(bad());
    }
    let mut escapes = escapes.chunks_exact(2).map(|c| i32::from(i16::from_le_bytes([c[0], c[1]])));
    let top = ((1u32 << bits) - 1) as f32;
    let (pw, ph) = (w / 2, h / 2);
    let mut samples = vec![0f32; count];
    let mut codes = vec![0i32; pw * ph];
    let mut next = 0;
    for (sy, sx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        for py in 0..ph {
            for px in 0..pw {
                let left = if px > 0 { codes[py * pw + px - 1] } else { 0 };
                let up = if py > 0 { codes[(py - 1) * pw + px] } else { 0 };
                let up_left = if px > 0 && py > 0 { codes[(py - 1) * pw + px - 1] } else { 0 };
                let r = residuals[next] as i8;
                next += 1;
                let r = if r == ESCAPE { escapes.next().ok_or_else(bad)? } else { i32::from(r) };
                let code = median_prediction(left, up, up_left) + r;
                codes[py * pw + px] = code;
                let v = code as f32 / top;
                samples[(2 * py + sy) * w + 2 * px + sx] = v * v;
            }
        }
    }
    Ok(Some(Companion { original, trailer: Trailer { amount: amount.clamp(0.0, 100.0), region, sensor_width, sensor_height, samples } }))
}

/// The original raw's path for a companion at `companion_path`: its relative `original` taken from the companion's folder.
/// Both kinds of separator are accepted; the result uses `/`.
pub fn resolve_original(companion_path: &str, original: &str) -> String {
    let mut parts: Vec<&str> = companion_path.split(['/', '\\']).collect();
    parts.pop();
    for part in original.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| !p.is_empty() && *p != ".." && !p.ends_with(':')) => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The original raw's bytes with `t` appended: what [`crate::decode`] reads as the denoised photo.
pub fn expand(original: &[u8], t: &Trailer) -> Vec<u8> {
    let trailer = encode_trailer(t);
    let mut out = Vec::with_capacity(original.len() + trailer.len());
    out.extend_from_slice(original);
    out.extend_from_slice(&trailer);
    out
}

/// A companion of `t` for the raw at `original` (relative to the companion's folder), at `bits` (tests and other hosts).
pub fn encode_companion(original: &str, t: &Trailer, bits: u32) -> Vec<u8> {
    use std::io::Write;
    let (w, h) = (t.region.width, t.region.height);
    let top = ((1u32 << bits) - 1) as f32;
    let code = |v: f32| (v.clamp(0.0, 1.0).sqrt() * top).round() as i32;
    let (pw, ph) = (w / 2, h / 2);
    let mut residuals = Vec::with_capacity(w * h);
    let mut escapes = Vec::new();
    let mut codes = vec![0i32; pw * ph];
    for (sy, sx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        for py in 0..ph {
            for px in 0..pw {
                codes[py * pw + px] = code(t.samples[(2 * py + sy) * w + 2 * px + sx]);
            }
        }
        for py in 0..ph {
            for px in 0..pw {
                let left = if px > 0 { codes[py * pw + px - 1] } else { 0 };
                let up = if py > 0 { codes[(py - 1) * pw + px] } else { 0 };
                let up_left = if px > 0 && py > 0 { codes[(py - 1) * pw + px - 1] } else { 0 };
                let r = codes[py * pw + px] - median_prediction(left, up, up_left);
                if (-127..=127).contains(&r) {
                    residuals.push(r as i8 as u8);
                } else {
                    residuals.push(ESCAPE as u8);
                    escapes.extend_from_slice(&(r as i16).to_le_bytes());
                }
            }
        }
    }
    let zip = |data: &[u8]| {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
        e.write_all(data).expect("writing to memory");
        e.finish().expect("writing to memory")
    };
    let mut out = Vec::new();
    out.extend_from_slice(COMPANION_MAGIC);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&t.amount.to_le_bytes());
    for v in [t.region.x, t.region.y, w, h, t.sensor_width, t.sensor_height] {
        out.extend_from_slice(&(v as u32).to_le_bytes());
    }
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(&(original.len() as u32).to_le_bytes());
    out.extend_from_slice(original.as_bytes());
    for part in [zip(&residuals), zip(&escapes)] {
        out.extend_from_slice(&(part.len() as u64).to_le_bytes());
        out.extend_from_slice(&part);
    }
    out
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

    fn smooth_trailer(amount: f32) -> Trailer {
        let (w, h) = (64usize, 48usize);
        let samples = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32, (i / w) as f32);
                (0.02 + 0.5 * (x / w as f32) * (y / h as f32) + if i % 2 == 0 { 0.05 } else { 0.0 }).min(1.0)
            })
            .collect();
        Trailer { amount, region: Rect::new(2, 2, w, h), sensor_width: 80, sensor_height: 60, samples }
    }

    #[test]
    fn a_companion_round_trips_within_its_precision_and_finds_its_raw() {
        let t = smooth_trailer(65.0);
        let bytes = encode_companion("../../IMG_1.NEF", &t, 10);
        assert!(is_companion(&bytes) && bytes.len() < t.samples.len());
        let c = companion(&bytes).unwrap().unwrap();
        assert_eq!((c.original.as_str(), c.trailer.amount, c.trailer.region), ("../../IMG_1.NEF", 65.0, t.region));
        let worst = c.trailer.samples.iter().zip(&t.samples).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-3, "worst error {worst}");
        // the amount sits at a fixed place, so a host can change it in place
        let mut moved = bytes.clone();
        moved[COMPANION_AMOUNT_AT..COMPANION_AMOUNT_AT + 4].copy_from_slice(&30f32.to_le_bytes());
        assert_eq!(companion(&moved).unwrap().unwrap().trailer.amount, 30.0);
        assert_eq!(resolve_original("K:\\Photos\\.image_triage_edits\\denoised\\IMG_1.denoise", &c.original), "K:/Photos/IMG_1.NEF");
        assert_eq!(resolve_original(".image_triage_edits/denoised/IMG_1.denoise", &c.original), "IMG_1.NEF");
    }

    #[test]
    fn an_expanded_companion_decodes_like_the_appended_trailer() {
        let t = smooth_trailer(100.0);
        let c = companion(&encode_companion("x.NEF", &t, 12)).unwrap().unwrap();
        let raw = b"original raw bytes".to_vec();
        let expanded = expand(&raw, &c.trailer);
        assert_eq!(original(&expanded), &raw[..]);
        assert_eq!(trailer(&expanded).unwrap().region, t.region);
    }

    #[test]
    fn a_damaged_companion_is_refused_and_other_files_are_not_companions() {
        let bytes = encode_companion("x.NEF", &smooth_trailer(50.0), 10);
        assert!(companion(b"II*\0 a tiff").unwrap().is_none());
        assert!(companion(&bytes[..bytes.len() - 10]).is_err());
        let mut newer = bytes.clone();
        newer[8] = 2;
        assert!(companion(&newer).unwrap_err().contains("newer"));
    }
}
