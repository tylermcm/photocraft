//! Original vector paths measured against reference 19 at uniform scale two.
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

pub(super) fn corner(p: &Painter, c: Pos2, ink: Color32) {
    p.add(Shape::convex_polygon(vec![c + vec2(5.5, 9.0), c + vec2(9.0, 9.0), c + vec2(9.0, 5.5)], ink, Stroke::NONE));
}

pub(super) fn sort(p: &Painter, c: Pos2, ink: Color32) {
    for (y, width) in [(-6.0, 16.0), (0.0, 12.0), (6.0, 8.0)] {
        p.rect_filled(Rect::from_min_size(c + vec2(-8.0, y - 1.0), vec2(width, 2.0)), 1.0, ink);
    }
    corner(p, c, ink);
}

pub(super) fn filter(p: &Painter, c: Pos2, ink: Color32) {
    // The funnel is concave. Separate convex parts preserve the straight stem.
    p.add(Shape::convex_polygon(vec![c + vec2(-7.5, -8.0), c + vec2(7.5, -8.0), c + vec2(1.5, 0.0), c + vec2(-1.5, 0.0)], ink, Stroke::NONE));
    p.add(Shape::convex_polygon(vec![c + vec2(-1.5, -0.5), c + vec2(1.5, -0.5), c + vec2(1.5, 5.5), c + vec2(-1.5, 8.5)], ink, Stroke::NONE));
    corner(p, c + vec2(0.5, 0.0), ink);
}

pub(super) fn reject(p: &Painter, c: Pos2, ink: Color32) {
    let handle = (0..=12)
        .map(|step| {
            let angle = std::f32::consts::PI + step as f32 / 12.0 * std::f32::consts::PI;
            c + vec2(angle.cos() * 2.0, -3.5 + angle.sin() * 2.0)
        })
        .collect();
    p.add(Shape::line(handle, Stroke::new(1.0, ink)));
    p.rect_filled(Rect::from_center_size(c + vec2(0.0, -3.5), vec2(11.0, 1.0)), 0.0, ink);
    p.add(Shape::line(vec![c + vec2(-4.0, -3.0), c + vec2(-4.0, 5.0), c + vec2(4.0, 5.0), c + vec2(4.0, -3.0)], Stroke::new(1.0, ink)));
    for dx in [-2.0, 0.0, 2.0] {
        p.rect_filled(Rect::from_center_size(c + vec2(dx, 1.0), vec2(1.0, 6.0)), 0.0, ink);
    }
}

pub(super) fn comparison(p: &Painter, c: Pos2, ink: Color32) {
    let left = c + vec2(-8.5, -7.0);
    p.rect_filled(Rect::from_min_size(left, vec2(1.5, 14.0)), 0.0, ink);
    p.rect_filled(Rect::from_min_size(left, vec2(5.5, 1.5)), 0.0, ink);
    p.rect_filled(Rect::from_min_size(left + vec2(0.0, 12.5), vec2(5.5, 1.5)), 0.0, ink);
    p.rect_filled(Rect::from_center_size(c + vec2(-0.75, 0.0), vec2(1.5, 18.0)), 0.0, ink);
    p.rect_filled(Rect::from_center_size(c + vec2(4.5, 0.0), vec2(6.0, 14.0)), 0.0, ink);
}

pub(super) fn toggle(p: &Painter, c: Pos2, collapsed: bool, ink: Color32, hole: Color32) {
    p.rect_filled(Rect::from_center_size(c - vec2(0.0, 2.5), vec2(17.0, 9.0)), 0.0, ink);
    let direction = if collapsed { -1.0 } else { 1.0 };
    let points = [c + vec2(-3.25, -2.5 - direction), c + vec2(0.0, -2.5 + 2.0 * direction), c + vec2(3.25, -2.5 - direction)];
    p.add(Shape::line(points.to_vec(), Stroke::new(1.7, hole)));
    for point in points {
        p.circle_filled(point, 0.85, hole);
    }
    for offset in [-7.0, -2.5, 2.5, 7.0] {
        p.rect_filled(Rect::from_center_size(pos2(c.x + offset, c.y + 5.5), vec2(3.0, 3.0)), 0.0, ink);
    }
    corner(p, c + vec2(4.5, 0.0), ink);
}
