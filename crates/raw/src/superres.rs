//! Super Resolution's base, as Camera Raw makes it: the photo upscaled once, before any adjustment, and every
//! development after that made from the upscaled base, so an edit never upscales again.
//!
//! The base is the photo's demosaiced linear camera RGB over its crop ([`linear_rgb`]), upscaled 2× by a host
//! (Image Triage runs Real-ESRGAN on it), and developed as a LinearRaw frame with the original raw's metadata: its
//! colour calibration, white balance, tone curve and baseline exposure are the original's, so white balance and every
//! other slider still apply on top.
use crate::color;
use crate::demosaic::{Demosaic, PAD, Padded, demosaic};
use crate::develop::{DevelopOptions, Scale, gain_row, resolve_neutral};
use crate::par;
use crate::sensor::Sensor;

/// A sensor's demosaiced linear camera RGB over its crop.
#[derive(Debug, Clone)]
pub struct LinearRgb {
    pub width: usize,
    pub height: usize,
    /// `width * height * 3`, row-major, interleaved: camera RGB normalized to black 0 … white 1, with the white balance
    /// it was demosaiced under divided out again (so a white balance chosen later applies as on the raw).
    pub data: Vec<f32>,
    /// The white balance it was demosaiced under (camera neutral, as-shot).
    pub neutral: [f64; 3],
}

/// The demosaiced linear camera RGB of `s`'s crop, demosaiced as [`crate::develop_sensor`] does (normalized, the
/// lens-shading gain maps applied, white-balanced as shot and clipped, then demosaiced with `method`), with that
/// white balance divided out. Bayer sensors only.
pub fn linear_rgb(s: &Sensor, method: Demosaic) -> Result<LinearRgb, String> {
    let cfa = match &s.cfa {
        Some(cfa) if s.samples == 1 && cfa.is_bayer() => cfa,
        _ => return Err("Super Resolution needs a Bayer sensor".into()),
    };
    let c = s.crop;
    if c.is_empty() || c.x + c.width > s.width || c.y + c.height > s.height || s.data.len() < s.width * s.height {
        return Err("the crop lies outside the sensor data".into());
    }
    let (w, h) = (c.width, c.height);
    let sc = Scale::new(s);
    let mut warnings = Vec::new();
    let neutral = resolve_neutral(s, &DevelopOptions::default(), &sc, &mut warnings).map_err(|e| e.to_string())?;
    let mult = neutral.map(|v| (1.0 / v) as f32);
    let to_xyz = s.color.balanced_to_xyz_d50(neutral).unwrap_or_else(color::srgb_to_xyz_d50);
    let mut plane = Padded::new(w, h);
    let stride = plane.stride;
    plane.fill_rows(par::band_rows(w), |y0, chunk| {
        for (r, prow) in chunk.chunks_exact_mut(stride).enumerate() {
            let y = c.y + y0 + r;
            let row = &mut prow[PAD..PAD + w];
            for (i, v) in row.iter_mut().enumerate() {
                let x = c.x + i;
                *v = (sc.value(x, y, 0) * mult[usize::from(cfa.color(x, y)).min(2)]).clamp(0.0, 1.0);
            }
            if let Some(g) = gain_row(s, y, c.x, w, 0) {
                for (v, g) in row.iter_mut().zip(g) {
                    *v = (*v * g).clamp(0.0, 1.0);
                }
            }
        }
    });
    plane.fill_borders();
    let mut data = demosaic(&plane, cfa.phase(c.x, c.y), method, &to_xyz, par::workers());
    for px in data.as_chunks_mut::<3>().0 {
        for k in 0..3 {
            px[k] /= mult[k];
        }
    }
    Ok(LinearRgb { width: w, height: h, data, neutral })
}

// ---- the base companion -------------------------------------------------------------------------------------------------
//
// The upscaled base lives beside the photo in a companion file, as AI Denoise's samples do (`denoised`): a host passes the
// companion where it would pass the raw, reads the original raw it names, and hands the decoder the two together
// ([`expand`]); [`crate::decode`] then gives the base as a LinearRaw sensor with the original's metadata ([`apply`]).
//
// Each channel is kept on a square-root scale at `bits` (the codes are the sensor's samples, a LinearizationTable maps them
// to linear values), predicted from its neighbours with lossless JPEG's median edge detector, and the differences zlib-
// compressed, channel by channel (so a channel is read row by row without holding it whole).
//
// ```text
// magic           b"PCSRBAS1"
// version         u32 = 1
// scale           u32 (2): the base is `scale` times the original crop's size
// width, height   u32: the base's size
// crop x, y, w, h u32: the original sensor's crop it was made from
// bits            u32, 8 … 14
// original        u32 length, then UTF-8: the raw's path relative to the companion's folder, `/`-separated
// 3 × channel     u64 length, then zlib: one i8 per sample, row-major, -128 = "the next escape";
//                 u64 length, then zlib: i16 little-endian escapes, in order
// ```
// [`expand`] appends the companion to the original raw, then its length (u64) and b"PCSRTRL1".

pub const BASE_MAGIC: &[u8; 8] = b"PCSRBAS1";
const TRAILER_MAGIC: &[u8; 8] = b"PCSRTRL1";
const ESCAPE: i8 = -128;

/// A base companion's header (its channels stay compressed until [`apply`]).
#[derive(Debug, Clone, PartialEq)]
pub struct BaseHeader {
    pub original: String,
    pub scale: usize,
    pub width: usize,
    pub height: usize,
    pub crop: crate::sensor::Rect,
    pub bits: u32,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn section<'a>(bytes: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let len = usize::try_from(u64::from_le_bytes(bytes.get(*at..*at + 8)?.try_into().ok()?)).ok()?;
    let end = at.checked_add(8)?.checked_add(len)?;
    let data = bytes.get(*at + 8..end)?;
    *at = end;
    Some(data)
}

type Sections<'a> = Vec<(&'a [u8], &'a [u8])>;

fn header_and_sections(bytes: &[u8]) -> Result<(BaseHeader, Sections<'_>), String> {
    let bad = || "the Super Resolution base is damaged".to_string();
    if !bytes.starts_with(BASE_MAGIC) {
        return Err(bad());
    }
    let u = |at: usize| u32_at(bytes, at).map(|v| v as usize).ok_or_else(bad);
    if u(8)? != 1 {
        return Err("the Super Resolution base was written by a newer version".into());
    }
    let (scale, width, height) = (u(12)?, u(16)?, u(20)?);
    let crop = crate::sensor::Rect::new(u(24)?, u(28)?, u(32)?, u(36)?);
    let bits = u(40)? as u32;
    let name_len = u(44)?;
    if !(8..=14).contains(&bits)
        || name_len > 4096
        || scale == 0
        || width == 0
        || height == 0
        || width.checked_mul(height).is_none_or(|n| n > 400_000_000)
    {
        return Err(bad());
    }
    let mut at = 48;
    let original = std::str::from_utf8(bytes.get(at..at + name_len).ok_or_else(bad)?).map_err(|_| bad())?.to_string();
    at += name_len;
    let mut channels = Vec::with_capacity(3);
    for _ in 0..3 {
        let residuals = section(bytes, &mut at).ok_or_else(bad)?;
        let escapes = section(bytes, &mut at).ok_or_else(bad)?;
        channels.push((residuals, escapes));
    }
    Ok((BaseHeader { original, scale, width, height, crop, bits }, channels))
}

/// The header of the base companion in `bytes`; `Ok(None)` when they are not one.
pub fn base(bytes: &[u8]) -> Result<Option<BaseHeader>, String> {
    if !bytes.starts_with(BASE_MAGIC) {
        return Ok(None);
    }
    header_and_sections(bytes).map(|(h, _)| Some(h))
}

/// The original raw's bytes with the base companion appended: what [`crate::decode`] reads as the upscaled photo.
pub fn expand(original: &[u8], companion: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(original.len() + companion.len() + 16);
    out.extend_from_slice(original);
    out.extend_from_slice(companion);
    out.extend_from_slice(&(companion.len() as u64).to_le_bytes());
    out.extend_from_slice(TRAILER_MAGIC);
    out
}

/// `(original raw, base companion)` of bytes [`expand`] made; `None` for any other file.
pub fn split(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let n = bytes.len();
    if n < 16 || &bytes[n - 8..] != TRAILER_MAGIC {
        return None;
    }
    let len = usize::try_from(u64::from_le_bytes(bytes[n - 16..n - 8].try_into().ok()?)).ok()?;
    let start = (n - 16).checked_sub(len)?;
    let companion = &bytes[start..n - 16];
    companion.starts_with(BASE_MAGIC).then_some((&bytes[..start], companion))
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

/// Turn `s` (the original raw, decoded) into the upscaled base the companion holds: a LinearRaw frame of the base's size with
/// the original's colour calibration, white balance, tone curve and baseline exposure. Errors when the base was made from a
/// different crop of the sensor.
pub fn apply(s: &mut Sensor, companion: &[u8]) -> Result<(), String> {
    use std::io::Read;
    let (h, channels) = header_and_sections(companion)?;
    if h.crop != s.crop || h.width != s.crop.width * h.scale || h.height != s.crop.height * h.scale {
        return Err("the Super Resolution base was made from a different crop of the photo".into());
    }
    let (w, ht) = (h.width, h.height);
    let top = (1u32 << h.bits) - 1;
    let bad = || "the Super Resolution base is damaged".to_string();
    let mut residuals: Vec<_> = channels.iter().map(|(r, _)| flate2::read::ZlibDecoder::new(*r)).collect();
    let mut escapes: Vec<Vec<i32>> = Vec::with_capacity(3);
    for (_, e) in &channels {
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(*e).take((w * ht * 2) as u64 + 1).read_to_end(&mut raw).map_err(|_| bad())?;
        escapes.push(raw.chunks_exact(2).map(|c| i32::from(i16::from_le_bytes([c[0], c[1]]))).collect());
    }
    let mut next_escape = [0usize; 3];
    let mut data = vec![0u16; w * ht * 3];
    let mut prev = vec![vec![0i32; w]; 3];
    let mut cur = vec![vec![0i32; w]; 3];
    let mut row = vec![0u8; w];
    for y in 0..ht {
        for k in 0..3 {
            residuals[k].read_exact(&mut row).map_err(|_| bad())?;
            for x in 0..w {
                let left = if x > 0 { cur[k][x - 1] } else { 0 };
                let up = if y > 0 { prev[k][x] } else { 0 };
                let up_left = if x > 0 && y > 0 { prev[k][x - 1] } else { 0 };
                let r = row[x] as i8;
                let r = if r == ESCAPE {
                    let v = *escapes[k].get(next_escape[k]).ok_or_else(bad)?;
                    next_escape[k] += 1;
                    v
                } else {
                    i32::from(r)
                };
                let code = median_prediction(left, up, up_left) + r;
                if !(0..=top as i32).contains(&code) {
                    return Err(bad());
                }
                cur[k][x] = code;
                data[(y * w + x) * 3 + k] = code as u16;
            }
            std::mem::swap(&mut prev[k], &mut cur[k]);
        }
    }
    s.linearization = Some((0..=top).map(|c| ((f64::from(c) / f64::from(top)).powi(2) * 65535.0).round() as u16).collect());
    s.width = w;
    s.height = ht;
    s.samples = 3;
    s.data = data;
    s.cfa = None;
    s.black = crate::sensor::BlackLevels::uniform(0.0);
    s.white = [65535.0; 3];
    s.active = crate::sensor::Rect::new(0, 0, w, ht);
    s.crop = crate::sensor::Rect::new(0, 0, w, ht);
    s.gain_maps.clear(); // applied before the base was demosaiced
    Ok(())
}

/// A base companion of `channels` (the base's linear camera RGB, `width * height * 3` interleaved, 0 … 1) for the raw at
/// `original`, made from `crop` at `scale` (tests and other hosts; Image Triage writes its own from a stream).
#[allow(clippy::too_many_arguments)]
pub fn encode_base(original: &str, scale: usize, width: usize, height: usize, crop: crate::sensor::Rect, bits: u32, channels: &[f32]) -> Vec<u8> {
    use std::io::Write;
    let top = ((1u32 << bits) - 1) as f32;
    let mut out = Vec::new();
    out.extend_from_slice(BASE_MAGIC);
    for v in [1u32, scale as u32, width as u32, height as u32, crop.x as u32, crop.y as u32, crop.width as u32, crop.height as u32, bits, original.len() as u32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(original.as_bytes());
    for k in 0..3 {
        let codes: Vec<i32> = (0..width * height).map(|i| (channels[i * 3 + k].clamp(0.0, 1.0).sqrt() * top).round() as i32).collect();
        let (mut residuals, mut escapes) = (Vec::with_capacity(width * height), Vec::new());
        for y in 0..height {
            for x in 0..width {
                let left = if x > 0 { codes[y * width + x - 1] } else { 0 };
                let up = if y > 0 { codes[(y - 1) * width + x] } else { 0 };
                let up_left = if x > 0 && y > 0 { codes[(y - 1) * width + x - 1] } else { 0 };
                let r = codes[y * width + x] - median_prediction(left, up, up_left);
                if (-127..=127).contains(&r) {
                    residuals.push(r as i8 as u8);
                } else {
                    residuals.push(ESCAPE as u8);
                    escapes.extend_from_slice(&(r as i16).to_le_bytes());
                }
            }
        }
        for part in [&residuals, &escapes] {
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
            e.write_all(part).expect("writing to memory");
            let z = e.finish().expect("writing to memory");
            out.extend_from_slice(&(z.len() as u64).to_le_bytes());
            out.extend_from_slice(&z);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorInfo;
    use crate::sensor::{BlackLevels, Cfa, Rect};
    use crate::RawFormat;

    fn flat_bayer(w: usize, h: usize, values: [u16; 3]) -> Sensor {
        let cfa = Cfa { width: 2, height: 2, colors: vec![0, 1, 1, 2], origin_x: 0, origin_y: 0 };
        let data = (0..w * h).map(|i| values[usize::from(cfa.color(i % w, i / w))]).collect();
        Sensor {
            format: RawFormat::Dng, make: None, model: None, width: w, height: h, samples: 1, data, cfa: Some(cfa),
            linearization: None, black: BlackLevels::uniform(0.0), white: [1000.0; 3], active: Rect::new(0, 0, w, h),
            crop: Rect::new(0, 0, w, h), color: ColorInfo::default(), camera_wb: Some([2.0, 1.0, 1.5]), orientation: 1,
            baseline_exposure: 0.0, gain_maps: Vec::new(), tone_curve: Vec::new(), black_point: 0.0, warnings: Vec::new(),
        }
    }

    #[test]
    fn a_flat_bayer_frame_gives_its_unbalanced_camera_colour_everywhere() {
        let s = flat_bayer(32, 24, [200, 400, 300]);
        let rgb = linear_rgb(&s, Demosaic::Bilinear).unwrap();
        assert_eq!((rgb.width, rgb.height, rgb.data.len()), (32, 24, 32 * 24 * 3));
        for px in rgb.data.as_chunks::<3>().0 {
            assert!((px[0] - 0.2).abs() < 1e-4 && (px[1] - 0.4).abs() < 1e-4 && (px[2] - 0.3).abs() < 1e-4, "{px:?}");
        }
    }

    #[test]
    fn a_base_round_trips_and_decodes_as_a_linear_raw_frame_with_the_originals_metadata() {
        let mut s = flat_bayer(32, 24, [200, 400, 300]);
        let crop = s.crop;
        let (w, h) = (64, 48);
        let channels: Vec<f32> = (0..w * h * 3).map(|i| [0.1, 0.5, 0.3][i % 3] + 0.001 * ((i / 3) % 7) as f32).collect();
        let companion = encode_base("../../IMG.NEF", 2, w, h, crop, 12, &channels);
        let head = base(&companion).unwrap().unwrap();
        assert_eq!((head.original.as_str(), head.scale, head.width, head.height, head.bits), ("../../IMG.NEF", 2, 64, 48, 12));
        let joined = expand(b"raw bytes", &companion);
        let (raw, back) = split(&joined).unwrap();
        assert_eq!((raw, back.len()), (&b"raw bytes"[..], companion.len()));
        let wb = s.camera_wb;
        apply(&mut s, &companion).unwrap();
        assert_eq!((s.width, s.height, s.samples, s.crop.width, s.cfa.is_none(), s.camera_wb), (64, 48, 3, 64, true, wb));
        let sc = Scale::new(&s);
        for (i, want) in channels.iter().enumerate().step_by(97) {
            let got = sc.value((i / 3) % w, (i / 3) / w, i % 3);
            assert!((got - want).abs() < 2e-3, "sample {i}: {got} against {want}");
        }
    }

    #[test]
    fn a_base_of_another_crop_is_refused() {
        let mut s = flat_bayer(32, 24, [200, 400, 300]);
        let other = crate::sensor::Rect::new(2, 2, 30, 22);
        let companion = encode_base("IMG.NEF", 2, 60, 44, other, 10, &vec![0.2; 60 * 44 * 3]);
        assert!(apply(&mut s, &companion).unwrap_err().contains("different crop"));
    }
}
