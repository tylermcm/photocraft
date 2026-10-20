//! Camera Raw Masking, painting: the Brush's strokes into a raster mask's coverage map.
//!
//! A brush mask is an ordinary raster mask (`MaskShape::Raster`) whose bytes the strokes edit, so it previews, exports and saves like the
//! automatic masks. Pure arithmetic on a byte map; the pointer handling lives in `camera_raw_mask_edit_ui.rs`.
use serde::{Deserialize, Serialize};

/// The longest side of a new brush mask's coverage map. Strokes are at most this sharp; the preview and the export sample it smoothly.
pub const BRUSH_MAP_SIDE: u32 = 1024;

/// The Brush's settings (view state: they are not part of the photo's settings).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Brush {
    /// Diameter, percent of the picture's longer side.
    pub size: f32,
    /// Percent of the radius over which the edge falls off (0 = hard).
    pub feather: f32,
    /// How much one pass adds (percent).
    pub flow: f32,
    /// Rubs out instead of painting (holding Alt does the same for a stroke).
    pub erase: bool,
}

impl Default for Brush {
    fn default() -> Self {
        Self { size: 6.0, feather: 50.0, flow: 100.0, erase: false }
    }
}

impl Brush {
    pub const SIZE: std::ops::RangeInclusive<f32> = 0.5..=40.0;

    /// A map size for a picture `w x h`: the longer side is [`BRUSH_MAP_SIDE`], the aspect is kept.
    pub fn map_size(w: usize, h: usize) -> (u32, u32) {
        let long = w.max(h).max(1) as f32;
        let scale = BRUSH_MAP_SIDE as f32 / long;
        (((w as f32 * scale).round() as u32).clamp(1, BRUSH_MAP_SIDE), ((h as f32 * scale).round() as u32).clamp(1, BRUSH_MAP_SIDE))
    }

    /// The radius in map pixels.
    pub fn radius(&self, map_w: u32, map_h: u32) -> f32 {
        (self.size.clamp(*Self::SIZE.start(), *Self::SIZE.end()) / 100.0 * map_w.max(map_h) as f32 / 2.0).max(0.5)
    }

    /// One dab at `(cx, cy)` (map pixels).
    pub fn stamp(&self, map: &mut [u8], w: u32, h: u32, cx: f32, cy: f32, erase: bool) {
        let (w, h) = (w as usize, h as usize);
        if map.len() != w * h || !cx.is_finite() || !cy.is_finite() {
            return;
        }
        let r = self.radius(w as u32, h as u32);
        let hard = 1.0 - (self.feather / 100.0).clamp(0.0, 1.0);
        let flow = (self.flow / 100.0).clamp(0.0, 1.0);
        let (x0, x1) = ((cx - r).floor().max(0.0) as usize, ((cx + r).ceil().max(0.0) as usize).min(w.saturating_sub(1)));
        let (y0, y1) = ((cy - r).floor().max(0.0) as usize, ((cy + r).ceil().max(0.0) as usize).min(h.saturating_sub(1)));
        if cx + r < 0.0 || cy + r < 0.0 {
            return;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() / r;
                let a = if d <= hard {
                    1.0
                } else if d >= 1.0 {
                    0.0
                } else {
                    let t = (d - hard) / (1.0 - hard).max(1e-4);
                    1.0 - t * t * (3.0 - 2.0 * t)
                } * flow;
                if a <= 0.0 {
                    continue;
                }
                let v = f32::from(map[y * w + x]);
                map[y * w + x] = (if erase { v * (1.0 - a) } else { v + (255.0 - v) * a }).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// A stroke segment from `from` to `to` (map pixels): dabs a quarter of the brush apart, so the line is even.
    pub fn stroke(&self, map: &mut [u8], w: u32, h: u32, from: (f32, f32), to: (f32, f32), erase: bool) {
        let length = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
        let step = (self.radius(w, h) * 0.25).max(1.0);
        let n = (length / step).ceil().max(1.0) as usize;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            self.stamp(map, w, h, from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t, erase);
        }
        if length == 0.0 {
            self.stamp(map, w, h, to.0, to.1, erase);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_keeps_the_pictures_aspect_and_is_at_most_the_longest_side() {
        assert_eq!(Brush::map_size(6000, 4000), (1024, 683));
        assert_eq!(Brush::map_size(900, 1350), (683, 1024));
        assert_eq!(Brush::map_size(1, 1), (1024, 1024));
    }

    #[test]
    fn a_dab_paints_a_round_spot_and_leaves_the_rest() {
        let (w, h) = (100u32, 100u32);
        let mut map = vec![0u8; 10_000];
        let brush = Brush { size: 20.0, feather: 0.0, flow: 100.0, erase: false };
        brush.stamp(&mut map, w, h, 50.0, 50.0, false);
        assert_eq!(map[50 * 100 + 50], 255, "the centre is fully painted");
        assert_eq!(map[50 * 100 + 59], 255, "inside the radius (10)");
        assert_eq!(map[50 * 100 + 65], 0, "outside it");
        assert_eq!(map[10 * 100 + 10], 0);
    }

    #[test]
    fn feather_softens_the_edge_and_flow_builds_up_in_passes() {
        let (w, h) = (100u32, 100u32);
        let mut hard = vec![0u8; 10_000];
        let mut soft = vec![0u8; 10_000];
        Brush { size: 40.0, feather: 0.0, flow: 100.0, erase: false }.stamp(&mut hard, w, h, 50.0, 50.0, false);
        Brush { size: 40.0, feather: 100.0, flow: 100.0, erase: false }.stamp(&mut soft, w, h, 50.0, 50.0, false);
        let edge = 50 * 100 + 66; // 16 of the radius 20 away
        assert_eq!(hard[edge], 255);
        assert!(soft[edge] > 0 && soft[edge] < 200, "{}", soft[edge]);
        let mut weak = vec![0u8; 10_000];
        let light = Brush { size: 20.0, feather: 0.0, flow: 30.0, erase: false };
        light.stamp(&mut weak, w, h, 50.0, 50.0, false);
        let first = weak[50 * 100 + 50];
        light.stamp(&mut weak, w, h, 50.0, 50.0, false);
        assert!(first > 60 && first < 100 && weak[50 * 100 + 50] > first + 30, "{first} then {}", weak[50 * 100 + 50]);
    }

    #[test]
    fn erasing_removes_what_was_painted() {
        let (w, h) = (100u32, 100u32);
        let mut map = vec![255u8; 10_000];
        let brush = Brush { size: 20.0, feather: 0.0, flow: 100.0, erase: false };
        brush.stamp(&mut map, w, h, 50.0, 50.0, true);
        assert_eq!((map[50 * 100 + 50], map[10 * 100 + 10]), (0, 255));
    }

    #[test]
    fn a_stroke_is_an_unbroken_line_and_stays_inside_the_map() {
        let (w, h) = (200u32, 100u32);
        let mut map = vec![0u8; 20_000];
        let brush = Brush { size: 5.0, feather: 0.0, flow: 100.0, erase: false };
        brush.stroke(&mut map, w, h, (20.0, 50.0), (180.0, 50.0), false);
        assert!((20..=180).all(|x| map[50 * 200 + x] == 255), "no gaps along the line");
        assert_eq!(map[10 * 200 + 100], 0);
        // Off the edge and non-finite positions do nothing and do not panic.
        brush.stroke(&mut map, w, h, (-500.0, -500.0), (-400.0, -400.0), false);
        brush.stamp(&mut map, w, h, f32::NAN, 5.0, false);
        brush.stroke(&mut map, w, h, (190.0, 90.0), (400.0, 300.0), false);
        assert_eq!(map.len(), 20_000);
    }

    #[test]
    fn a_zero_length_stroke_still_leaves_a_dab() {
        let mut map = vec![0u8; 10_000];
        Brush::default().stroke(&mut map, 100, 100, (50.0, 50.0), (50.0, 50.0), false);
        assert!(map[50 * 100 + 50] > 0);
    }
}
