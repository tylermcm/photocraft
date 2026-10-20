//! Local masks: a list of masks, each limiting a set of local adjustments to part of the picture, as Camera Raw's Masking does.
//!
//! A mask is a [`MaskShape`] (a linear or radial gradient, or a raster coverage map made by a selection tool such as Select Subject), an
//! optional inversion, an amount, and [`LocalAdjust`]: the adjustments that apply where the mask covers. Geometry is stored in image-relative
//! units (0 … 1 of the width and of the height), so a mask means the same thing on the preview proxy and on the full-size picture.
//!
//! Local adjustments are made with the same pipeline as the global ones (`develop`), then blended in by the mask's coverage, so every global
//! control that can be local is local with no second implementation.
use serde::{Deserialize, Serialize};

use super::CameraRaw;

/// Most masks one photo can hold.
pub const MAX_MASKS: usize = 16;
/// Longest side (pixels) of a raster mask's coverage map.
pub const MAX_RASTER_SIDE: u32 = 1536;
/// Most shapes added to (or taken from) one mask.
pub const MAX_PARTS: usize = 8;
/// Longest encoded raster mask (characters): a worst case, noisy 1536 x 1024 map still has to fit.
const MAX_RASTER_TEXT: usize = 6 * 1024 * 1024;

/// The adjustments a mask applies, in Camera Raw's slider units. All zero is no change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct LocalAdjust {
    pub temperature: f32,
    pub tint: f32,
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub texture: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub saturation: f32,
}

impl Default for LocalAdjust {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            tint: 0.0,
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            saturation: 0.0,
        }
    }
}

impl LocalAdjust {
    fn fields(&self) -> [(&'static str, f32, f32); 12] {
        [
            ("temperature", self.temperature, 100.0),
            ("tint", self.tint, 100.0),
            ("exposure", self.exposure, 5.0),
            ("contrast", self.contrast, 100.0),
            ("highlights", self.highlights, 100.0),
            ("shadows", self.shadows, 100.0),
            ("whites", self.whites, 100.0),
            ("blacks", self.blacks, 100.0),
            ("texture", self.texture, 100.0),
            ("clarity", self.clarity, 100.0),
            ("dehaze", self.dehaze, 100.0),
            ("saturation", self.saturation, 100.0),
        ]
    }

    /// True when no adjustment changes anything.
    pub fn neutral(&self) -> bool {
        self.fields().iter().all(|(_, value, _)| *value == 0.0)
    }

    /// The global settings that do exactly this adjustment, at `base`'s scale and encoding.
    fn on(&self, base: &CameraRaw) -> CameraRaw {
        CameraRaw {
            temperature: self.temperature,
            tint: self.tint,
            exposure: self.exposure,
            contrast: self.contrast,
            highlights: self.highlights,
            shadows: self.shadows,
            whites: self.whites,
            blacks: self.blacks,
            texture: self.texture,
            clarity: self.clarity,
            dehaze: self.dehaze,
            saturation: self.saturation,
            pixel_scale: base.pixel_scale,
            encoding_gamma: base.encoding_gamma,
            ..CameraRaw::default()
        }
    }
}

/// Where a mask covers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum MaskShape {
    /// Full coverage at `(x0, y0)`, none at `(x1, y1)`, falling off linearly between them and beyond.
    Linear { x0: f32, y0: f32, x1: f32, y1: f32 },
    /// An ellipse centred on `(cx, cy)` with radii `rx` (of the width) and `ry` (of the height), turned by `angle` degrees. Full inside,
    /// falling to none at the edge over `feather` percent of the radius.
    Radial { cx: f32, cy: f32, rx: f32, ry: f32, angle: f32, feather: f32 },
    /// A coverage map made by a selection tool, `w x h` bytes, run-length coded and written in base64 (see [`MaskShape::raster`]).
    Raster { w: u32, h: u32, data: String },
    /// Pixels whose colour is near `color` (in the picture's own working values, 0 … 1 per channel). `range` 0 … 100 is how far a colour may be.
    ColorRange { color: [f32; 3], range: f32, smooth: f32 },
    /// Pixels whose brightness lies in `low` … `high` (0 … 1), softened over `smooth` percent of a quarter of the scale at each end.
    LuminanceRange { low: f32, high: f32, smooth: f32 },
}

impl Default for MaskShape {
    fn default() -> Self {
        MaskShape::Linear { x0: 0.5, y0: 0.25, x1: 0.5, y1: 0.75 }
    }
}

impl MaskShape {
    /// The coverage map of a raster mask, `(w, h, bytes)`; `None` for other shapes or a damaged map.
    pub fn raster_bytes(&self) -> Option<(u32, u32, Vec<u8>)> {
        match self {
            MaskShape::Raster { w, h, data } if *w > 0 && *h > 0 && *w <= MAX_RASTER_SIDE && *h <= MAX_RASTER_SIDE => decode_runs(data, *w as usize * *h as usize).map(|bytes| (*w, *h, bytes)),
            _ => None,
        }
    }

    /// A raster mask from 8-bit coverage (`w x h`, row-major).
    pub fn raster_from_bytes(w: u32, h: u32, bytes: &[u8]) -> Self {
        MaskShape::Raster { w, h, data: encode_runs(bytes) }
    }

    /// A raster mask from coverage values in 0 … 1 (`w x h`, row-major).
    pub fn raster(w: u32, h: u32, coverage: &[f32]) -> Self {
        let bytes: Vec<u8> = coverage.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
        MaskShape::Raster { w, h, data: encode_runs(&bytes) }
    }
}

/// How a part joins what the mask covers so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MaskOp {
    /// Cover this too (the larger of the two at each pixel).
    #[default]
    Add,
    /// Take this out of what is covered.
    Subtract,
    /// Cover only where both do.
    Intersect,
}

/// A shape combined into a mask after its first one: Add, Subtract or Intersect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct MaskPart {
    pub op: MaskOp,
    pub shape: MaskShape,
    /// Use what the shape does not cover.
    pub invert: bool,
}

impl Default for MaskPart {
    fn default() -> Self {
        Self { op: MaskOp::Add, shape: MaskShape::default(), invert: false }
    }
}

/// One local mask and what it does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct LocalMask {
    pub name: String,
    /// Switched off, the mask keeps its settings but changes nothing.
    pub enabled: bool,
    pub shape: MaskShape,
    /// Cover everything the shape does not.
    pub invert: bool,
    /// Shapes combined with `shape`, in order (Add, Subtract, Intersect).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<MaskPart>,
    /// How much of the adjustments show, 0 … 100.
    pub amount: f32,
    pub adjust: LocalAdjust,
}

impl Default for LocalMask {
    fn default() -> Self {
        Self { name: String::new(), enabled: true, shape: MaskShape::default(), invert: false, parts: Vec::new(), amount: 100.0, adjust: LocalAdjust::default() }
    }
}

impl LocalMask {
    /// Whether this mask changes anything.
    pub fn active(&self) -> bool {
        self.enabled && self.amount > 0.0 && !self.adjust.neutral()
    }

    /// Coverage (0 … 1) at pixel `(x, y)` of a `w x h` picture.
    pub fn coverage_fn(&self, w: usize, h: usize) -> Coverage {
        Coverage::new(self, w, h)
    }
}

/// A mask prepared to be asked about pixels of one picture size: its first shape, then each part combined in.
pub struct Coverage {
    base: Layer,
    parts: Vec<(MaskOp, Layer)>,
}

impl Coverage {
    fn new(mask: &LocalMask, w: usize, h: usize) -> Self {
        Coverage { base: Layer::new(&mask.shape, mask.invert, w, h), parts: mask.parts.iter().map(|p| (p.op, Layer::new(&p.shape, p.invert, w, h))).collect() }
    }

    /// Whether coverage depends on the picture's pixels (a Range shape somewhere), so that [`Coverage::at_px`] must be used.
    pub fn needs_pixels(&self) -> bool {
        self.base.needs_pixels() || self.parts.iter().any(|(_, l)| l.needs_pixels())
    }

    /// Coverage (0 … 1) at pixel `(x, y)` whose colour is `rgb`.
    pub fn at_px(&self, x: usize, y: usize, rgb: [f32; 3]) -> f32 {
        self.parts.iter().fold(self.base.at_px(x, y, rgb), |v, (op, layer)| combine(v, layer.at_px(x, y, rgb), *op))
    }

    /// Coverage (0 … 1) at pixel `(x, y)`. A Range shape has no answer without the picture: use [`Coverage::at_px`].
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.parts.iter().fold(self.base.at(x, y), |v, (op, layer)| combine(v, layer.at(x, y), *op))
    }
}

/// The three ways of joining a shape in (the same rules Image Triage's masks used: add is the larger, subtract is multiplicative so a
/// soft edge stays soft, intersect multiplies).
fn combine(so_far: f32, part: f32, op: MaskOp) -> f32 {
    match op {
        MaskOp::Add => so_far.max(part),
        MaskOp::Subtract => so_far * (1.0 - part),
        MaskOp::Intersect => so_far * part,
    }
}

struct Layer {
    kind: Kind,
    invert: bool,
}

enum Kind {
    Linear { p0: [f32; 2], v: [f32; 2], len2: f32 },
    Radial { c: [f32; 2], r: [f32; 2], cos: f32, sin: f32, inner: f32 },
    Raster { w: usize, h: usize, bytes: Vec<u8>, size: [f32; 2] },
    /// Decided per pixel from the picture itself (see [`Coverage::at_px`]).
    Colour { color: [f32; 3], tolerance: f32, soft: f32 },
    Luma { low: f32, high: f32, soft: f32 },
    /// A raster that could not be read: covers nothing.
    Nothing,
}

impl Layer {
    fn new(shape: &MaskShape, invert: bool, w: usize, h: usize) -> Self {
        let (fw, fh) = (w as f32, h as f32);
        let kind = match shape {
            MaskShape::Linear { x0, y0, x1, y1 } => {
                let p0 = [x0 * fw, y0 * fh];
                let v = [x1 * fw - p0[0], y1 * fh - p0[1]];
                Kind::Linear { p0, v, len2: (v[0] * v[0] + v[1] * v[1]).max(1e-6) }
            }
            MaskShape::Radial { cx, cy, rx, ry, angle, feather } => {
                let a = angle.to_radians();
                Kind::Radial {
                    c: [cx * fw, cy * fh],
                    r: [(rx * fw).max(1e-3), (ry * fh).max(1e-3)],
                    cos: a.cos(),
                    sin: a.sin(),
                    inner: 1.0 - (feather / 100.0).clamp(0.0, 1.0),
                }
            }
            MaskShape::Raster { w: mw, h: mh, data } => match decode_runs(data, *mw as usize * *mh as usize) {
                Some(bytes) if *mw > 0 && *mh > 0 => Kind::Raster { w: *mw as usize, h: *mh as usize, bytes, size: [fw, fh] },
                _ => Kind::Nothing,
            },
            MaskShape::ColorRange { color, range, smooth } => Kind::Colour { color: *color, tolerance: (range / 100.0).clamp(0.0, 1.0) * 0.5 + 1e-3, soft: (smooth / 100.0).clamp(0.0, 1.0) },
            MaskShape::LuminanceRange { low, high, smooth } => Kind::Luma { low: *low, high: *high, soft: (smooth / 100.0).clamp(0.0, 1.0) * 0.25 },
        };
        Layer { kind, invert }
    }

    fn needs_pixels(&self) -> bool {
        matches!(self.kind, Kind::Colour { .. } | Kind::Luma { .. })
    }

    /// Coverage (0 … 1) at pixel `(x, y)` whose colour is `rgb`. Shapes that do not look at colour ignore it.
    fn at_px(&self, x: usize, y: usize, rgb: [f32; 3]) -> f32 {
        let c = match &self.kind {
            Kind::Colour { color, tolerance, soft } => {
                let d = (0..3).map(|i| (rgb[i] - color[i]).abs()).fold(0.0f32, f32::max);
                // Full coverage inside the inner part of the tolerance, falling off to none at its edge.
                let inner = tolerance * (1.0 - soft);
                if d <= inner { 1.0 } else if d >= *tolerance { 0.0 } else { 1.0 - smooth_step((d - inner) / (tolerance - inner).max(1e-6)) }
            }
            Kind::Luma { low, high, soft } => {
                let y = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
                let rise = if *soft <= 0.0 { f32::from(y >= *low) } else { smooth_step((y - (low - soft)) / soft.max(1e-6)) };
                let fall = if *soft <= 0.0 { f32::from(y <= *high) } else { 1.0 - smooth_step((y - high) / soft.max(1e-6)) };
                rise * fall
            }
            _ => return self.at(x, y),
        };
        if self.invert { 1.0 - c } else { c }
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let c = match &self.kind {
            Kind::Linear { p0, v, len2 } => {
                let t = ((px - p0[0]) * v[0] + (py - p0[1]) * v[1]) / len2;
                1.0 - t.clamp(0.0, 1.0)
            }
            Kind::Radial { c, r, cos, sin, inner } => {
                let (dx, dy) = (px - c[0], py - c[1]);
                let (u, w) = ((dx * cos + dy * sin) / r[0], (-dx * sin + dy * cos) / r[1]);
                let d = (u * u + w * w).sqrt();
                if d <= *inner {
                    1.0
                } else if d >= 1.0 {
                    0.0
                } else {
                    let t = (d - inner) / (1.0 - inner).max(1e-4);
                    1.0 - t * t * (3.0 - 2.0 * t)
                }
            }
            Kind::Raster { w, h, bytes, size } => sample(bytes, *w, *h, px / size[0], py / size[1]),
            Kind::Nothing | Kind::Colour { .. } | Kind::Luma { .. } => 0.0,
        };
        if self.invert { 1.0 - c } else { c }
    }
}

fn smooth_step(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bilinear sample of a `w x h` byte map at normalized `(u, v)` (pixel centres).
fn sample(bytes: &[u8], w: usize, h: usize, u: f32, v: f32) -> f32 {
    let (fx, fy) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (ax, ay) = (fx - x0, fy - y0);
    let at = |x: f32, y: f32| f32::from(bytes[(y.clamp(0.0, h as f32 - 1.0) as usize) * w + x.clamp(0.0, w as f32 - 1.0) as usize]) / 255.0;
    let top = at(x0, y0) * (1.0 - ax) + at(x0 + 1.0, y0) * ax;
    let bottom = at(x0, y0 + 1.0) * (1.0 - ax) + at(x0 + 1.0, y0 + 1.0) * ax;
    top * (1.0 - ay) + bottom * ay
}

// --- the raster mask's text form: runs of one byte value (value, LEB128 length), then base64 -----------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_runs(bytes: &[u8]) -> String {
    let mut raw = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let value = bytes[i];
        let mut run = 1usize;
        while i + run < bytes.len() && bytes[i + run] == value {
            run += 1;
        }
        raw.push(value);
        let mut n = run;
        loop {
            let low = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                raw.push(low);
                break;
            }
            raw.push(low | 0x80);
        }
        i += run;
    }
    let mut out = String::with_capacity(raw.len() * 4 / 3 + 4);
    for chunk in raw.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The `expected` bytes the text holds, or `None` when it is not exactly that many valid runs.
fn decode_runs(text: &str, expected: usize) -> Option<Vec<u8>> {
    if text.len() % 4 != 0 || text.len() > MAX_RASTER_TEXT || expected == 0 || expected > (MAX_RASTER_SIDE as usize).pow(2) {
        return None;
    }
    let value = |c: u8| -> Option<u32> { B64.iter().position(|b| *b == c).map(|p| p as u32) };
    let mut raw = Vec::with_capacity(text.len() / 4 * 3);
    for quad in text.as_bytes().chunks(4) {
        let pad = quad.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in quad.iter().enumerate() {
            n = (n << 6) | if i >= 4 - pad { 0 } else { value(*c)? };
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        raw.extend_from_slice(&bytes[..3 - pad]);
    }
    let mut out = Vec::with_capacity(expected);
    let mut i = 0;
    while i < raw.len() {
        let value = raw[i];
        i += 1;
        let (mut run, mut shift) = (0usize, 0u32);
        loop {
            let byte = *raw.get(i)?;
            i += 1;
            run |= usize::from(byte & 0x7f).checked_shl(shift)?;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return None;
            }
        }
        if run == 0 || out.len() + run > expected {
            return None;
        }
        out.resize(out.len() + run, value);
    }
    (out.len() == expected).then_some(out)
}

pub fn validate(p: &CameraRaw) -> Result<(), String> {
    if p.masks.len() > MAX_MASKS {
        return Err(format!("at most {MAX_MASKS} masks"));
    }
    for (index, mask) in p.masks.iter().enumerate() {
        let name = format!("mask {}", index + 1);
        if mask.name.chars().count() > 64 {
            return Err(format!("{name}: the name is too long"));
        }
        if !mask.amount.is_finite() || !(0.0..=100.0).contains(&mask.amount) {
            return Err(format!("{name}: amount must be within 0..100"));
        }
        for (field, value, limit) in mask.adjust.fields() {
            if !value.is_finite() || value.abs() > limit {
                return Err(format!("{name}: {field} must be finite and within -{limit}..{limit}"));
            }
        }
        if mask.parts.len() > MAX_PARTS {
            return Err(format!("{name}: at most {MAX_PARTS} combined shapes"));
        }
        for part in &mask.parts {
            validate_shape(&part.shape, &name)?;
        }
        validate_shape(&mask.shape, &name)?;
    }
    Ok(())
}

fn validate_shape(shape: &MaskShape, name: &str) -> Result<(), String> {
        match shape {
            MaskShape::Linear { x0, y0, x1, y1 } => {
                if [x0, y0, x1, y1].iter().any(|v| !v.is_finite() || !(-2.0..=3.0).contains(*v)) {
                    return Err(format!("{name}: gradient points must be finite and near the picture"));
                }
            }
            MaskShape::Radial { cx, cy, rx, ry, angle, feather } => {
                if [cx, cy].iter().any(|v| !v.is_finite() || !(-2.0..=3.0).contains(*v))
                    || [rx, ry].iter().any(|v| !v.is_finite() || !(0.0..=4.0).contains(*v))
                    || !angle.is_finite()
                    || !(-360.0..=360.0).contains(angle)
                    || !feather.is_finite()
                    || !(0.0..=100.0).contains(feather)
                {
                    return Err(format!("{name}: ellipse out of range"));
                }
            }
            MaskShape::Raster { w, h, data } => {
                if *w == 0 || *h == 0 || *w > MAX_RASTER_SIDE || *h > MAX_RASTER_SIDE {
                    return Err(format!("{name}: coverage map size out of range"));
                }
                if decode_runs(data, *w as usize * *h as usize).is_none() {
                    return Err(format!("{name}: coverage map is damaged"));
                }
            }
            MaskShape::ColorRange { color, range, smooth } => {
                if color.iter().any(|v| !v.is_finite() || !(-0.5..=2.0).contains(v)) || !range.is_finite() || !(0.0..=100.0).contains(range) || !smooth.is_finite() || !(0.0..=100.0).contains(smooth) {
                    return Err(format!("{name}: colour range out of bounds"));
                }
            }
            MaskShape::LuminanceRange { low, high, smooth } => {
                if !low.is_finite() || !high.is_finite() || !smooth.is_finite() || !(0.0..=1.0).contains(low) || !(0.0..=1.0).contains(high) || low > high || !(0.0..=100.0).contains(smooth) {
                    return Err(format!("{name}: luminance range out of bounds"));
                }
            }
        }
    Ok(())
}

/// Applies every active mask's adjustments to `px` (`w x h`), after the global edits.
pub(super) fn apply(px: &mut [[f32; 4]], w: usize, h: usize, p: &CameraRaw, float: bool) {
    if w == 0 || h == 0 || px.len() != w * h {
        return;
    }
    for mask in p.masks.iter().filter(|m| m.active()) {
        let coverage = mask.coverage_fn(w, h);
        let ranged = coverage.needs_pixels();
        let mut local = px.to_vec();
        super::develop(&mut local, w, h, &mask.adjust.on(p), float);
        let strength = mask.amount / 100.0;
        crate::photo_util::par_rows(px, w, 1, |y, row| {
            let source = &local[y * w..(y + 1) * w];
            for (x, (out, adjusted)) in row.iter_mut().zip(source).enumerate() {
                let k = if ranged { coverage.at_px(x, y, [out[0], out[1], out[2]]) } else { coverage.at(x, y) } * strength;
                if k > 0.0 {
                    for c in 0..3 {
                        out[c] += (adjusted[c] - out[c]) * k;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_round_trip_and_reject_damage() {
        let bytes: Vec<u8> = (0..5000u32).map(|i| if i % 700 < 300 { 0 } else { (i % 251) as u8 }).collect();
        let text = encode_runs(&bytes);
        assert_eq!(decode_runs(&text, bytes.len()).unwrap(), bytes);
        assert!(decode_runs(&text, bytes.len() + 1).is_none(), "wrong size");
        assert!(decode_runs(&text[1..], bytes.len()).is_none(), "truncated");
        assert!(decode_runs("not base64!!", 4).is_none());
        assert!(decode_runs("", 4).is_none());
        // A flat map is a few bytes.
        assert!(encode_runs(&vec![255u8; 1_000_000]).len() < 16);
    }
}
