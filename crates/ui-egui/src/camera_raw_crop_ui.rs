//! Measured Crop & Expand view. Disclosures are view state; transforms are not
//! offered until the Camera Raw transaction can preview and replay them.
use crate::{icons, theme::Tokens};
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

pub(crate) const GROUPS: [&str; 4] = ["Generative Expand", "Trim", "Geometry", "Anamorphic"];
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Kind {
    Text,
    Icon,
    Field,
    Panel,
    Check,
    Track,
    Handle,
    Divider,
    Disclosure,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Element {
    id: String,
    kind: Kind,
    rect: [f32; 4],
    label: String,
    display: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Design {
    width: f32,
    height: f32,
    font_size: f32,
    heading_size: f32,
    elements: Vec<Element>,
}
impl Default for Design {
    fn default() -> Self {
        serde_json::from_str(include_str!("../../../design/camera-raw/crop-design.json")).unwrap_or_else(|_| Self {
            width: 270.0,
            height: 730.0,
            font_size: 12.0,
            heading_size: 14.0,
            elements: Vec::new(),
        })
    }
}
impl Design {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite()
            || !(220.0..=600.0).contains(&self.width)
            || !self.height.is_finite()
            || !(650.0..=1200.0).contains(&self.height)
            || !self.font_size.is_finite()
            || !(9.0..=20.0).contains(&self.font_size)
            || !self.heading_size.is_finite()
            || !(10.0..=24.0).contains(&self.heading_size)
            || self.elements.len() > 128
        {
            return Err("invalid Camera Raw Crop geometry".into());
        }
        let mut ids = std::collections::HashSet::new();
        for e in &self.elements {
            let [l, t, r, b] = e.rect;
            if !ids.insert(&e.id)
                || e.id.is_empty()
                || e.id.len() > 80
                || e.label.len() > 256
                || e.display.len() > 80
                || e.rect.iter().any(|v| !v.is_finite())
                || l < 0.0
                || t < 0.0
                || r <= l
                || b <= t
                || r > self.width
                || b > self.height
            {
                return Err(format!("invalid Camera Raw Crop element {}", e.id));
            }
        }
        for (id, kind) in [
            ("heading", Kind::Text),
            ("geometry-panel", Kind::Panel),
            ("geometry-disclosure", Kind::Disclosure),
            ("angle-value", Kind::Field),
            ("anamorphic-panel", Kind::Panel),
        ] {
            if !self.elements.iter().any(|e| e.id == id && e.kind == kind) {
                return Err(format!("Crop design requires {id}"));
            }
        }
        Ok(())
    }
}
fn tr(s: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", s)
}
fn group(id: &str) -> Option<&'static str> {
    if id.starts_with("generative-expand-") || id == "expand-options" {
        Some(GROUPS[0])
    } else if id.starts_with("trim-") {
        Some(GROUPS[1])
    } else if id.starts_with("geometry-") {
        Some(GROUPS[2])
    } else if id.starts_with("anamorphic-") {
        Some(GROUPS[3])
    } else {
        None
    }
}
fn opened(groups: &[String], name: &str) -> bool {
    groups.iter().any(|v| v == name)
}
fn shift(y: f32, groups: &[String]) -> f32 {
    let mut offset = 0.0;
    if y >= 241.0 && opened(groups, GROUPS[0]) {
        offset += 26.0;
    }
    if y >= 279.0 && opened(groups, GROUPS[1]) {
        offset += 26.0;
    }
    if y >= 676.0 && !opened(groups, GROUPS[2]) {
        offset -= 358.0;
    }
    offset
}
fn line(ui: &Ui, points: Vec<egui::Pos2>, color: egui::Color32) {
    ui.painter().add(egui::Shape::line(points, Stroke::new(1.25, color)));
}
fn icon(ui: &Ui, r: Rect, id: &str) {
    let t = Tokens::get(ui.ctx());
    let ink = t.icon;
    let c = r.center();
    match id {
        "crop-overlay-toggle" | "geometry-glyph" => {
            let pts = vec![c + vec2(-6.0, -6.0), c + vec2(6.0, -6.0), c + vec2(8.0, 6.0), c + vec2(-8.0, 6.0), c + vec2(-6.0, -6.0)];
            line(ui, pts, ink);
            for (x, y) in [(-6.0, -6.0), (6.0, -6.0), (-8.0, 6.0), (8.0, 6.0)] {
                ui.painter().circle_filled(c + vec2(x, y), 1.7, ink);
            }
        }
        "reset" => icons::paint(ui, r, "undo-2", 18.0, ink),
        "aspect-lock" => icons::paint(ui, r, "lock-open", 17.0, ink),
        "swap-orientation" => icons::paint(ui, r, "rotate-cw", 18.0, ink),
        "angle-level-tool" => {
            let rr = Rect::from_center_size(c + vec2(0.0, 3.0), vec2(17.0, 8.0));
            ui.painter().rect_stroke(rr, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
            line(ui, vec![c + vec2(-7.0, 1.0), c + vec2(7.0, 1.0)], ink);
            ui.painter().circle_stroke(c + vec2(0.0, 2.0), 3.0, Stroke::new(1.0, ink));
            for x in [-7.0, -3.5, 0.0, 3.5, 7.0] {
                ui.painter().circle_filled(c + vec2(x, -4.0 + (x.abs() / 3.5)), 0.6, ink);
            }
        }
        "trim-glyph" => {
            for y in [-4.0, 4.0] {
                ui.painter().circle_stroke(c + vec2(-6.0, y), 3.0, Stroke::new(1.5, ink));
                line(ui, vec![c + vec2(-3.0, y), c + vec2(8.0, -y)], ink);
            }
        }
        "generative-expand-glyph" => {
            let rr = r.shrink(3.0);
            ui.painter().rect_stroke(rr, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
            for (x, y) in [(-5.0, -4.0), (0.0, 5.0), (-5.0, 5.0)] {
                ui.painter().rect_filled(Rect::from_center_size(c + vec2(x, y), vec2(3.0, 3.0)), 0.0, ink);
            }
            let q = c + vec2(5.0, -3.0);
            line(ui, vec![q + vec2(0.0, -5.0), q + vec2(0.0, 5.0)], ink);
            line(ui, vec![q + vec2(-5.0, 0.0), q + vec2(5.0, 0.0)], ink);
            line(ui, vec![q + vec2(-3.0, -3.0), q + vec2(3.0, 3.0)], ink);
        }
        "geometry-info-glyph" => icons::paint(ui, r, "info", 13.0, ink),
        "geometry-eye" | "anamorphic-eye" => icons::paint(ui, r, "eye", 14.0, t.text_faint),
        "expand-options" => {
            for x in [-4.0, 0.0, 4.0] {
                ui.painter().circle_filled(c + vec2(x, 0.0), 1.2, ink);
            }
        }
        "rotate-left" | "rotate-right" => {
            let sign = if id == "rotate-left" { -1.0 } else { 1.0 };
            let pts = (0..=24)
                .map(|i| {
                    let a = (-60.0 + i as f32 * 12.0).to_radians();
                    c + vec2(sign * a.cos() * 7.0, a.sin() * 7.0)
                })
                .collect();
            line(ui, pts, ink);
            line(ui, vec![c + vec2(sign * 1.0, -9.0), c + vec2(sign * 5.0, -5.0), c + vec2(sign * 0.0, -5.0)], ink);
        }
        "flip-horizontal" | "flip-vertical" => {
            let p = |x, y| if id == "flip-horizontal" { c + vec2(x, y) } else { c + vec2(y, x) };
            for s in [-1.0, 1.0] {
                line(ui, vec![p(s * 7.0, -6.0), p(s * 2.0, 0.0), p(s * 7.0, 6.0), p(s * 7.0, -6.0)], ink);
            }
            for y in [-7.0, -3.5, 0.0, 3.5, 7.0] {
                line(ui, vec![p(0.0, y), p(0.0, y + 1.5)], ink);
            }
        }
        "upright-off" => {
            ui.painter().rect_filled(r.shrink(2.0), t.radius_sm, t.hover);
            ui.painter().circle_stroke(c, 7.0, Stroke::new(1.5, ink));
            line(ui, vec![c - vec2(5.0, 5.0), c + vec2(5.0, 5.0)], ink);
        }
        "upright-auto" => {
            ui.painter().text(c, Align2::CENTER_CENTER, "A", FontId::proportional(21.0), t.text_faint);
        }
        "upright-level" | "upright-vertical" | "upright-full" => {
            let rr = Rect::from_center_size(c, vec2(16.0, 16.0));
            ui.painter().rect_stroke(rr, 0.0, Stroke::new(1.0, t.text_faint), egui::StrokeKind::Inside);
            for v in [-3.0, 3.0] {
                if id != "upright-vertical" {
                    line(ui, vec![pos2(rr.left(), c.y + v), pos2(rr.right(), c.y + v)], t.text_faint);
                }
                if id != "upright-level" {
                    line(ui, vec![pos2(c.x + v, rr.top()), pos2(c.x + v, rr.bottom())], t.text_faint);
                }
            }
        }
        "upright-guided" => {
            for v in [-3.0, 3.0] {
                line(ui, vec![c + vec2(v, -8.0), c + vec2(v, 8.0)], t.text_faint);
                line(ui, vec![c + vec2(-8.0, v), c + vec2(8.0, v)], t.text_faint);
            }
        }
        "anamorphic-glyph" => {
            ui.painter().rect_stroke(r.shrink(4.0), 0.0, Stroke::new(1.25, ink), egui::StrokeKind::Inside);
            ui.painter().circle_stroke(c, 6.0, Stroke::new(1.25, ink));
        }
        _ => {}
    }
}
pub(super) fn show(ui: &mut Ui, groups: &mut Vec<String>) {
    let design = crate::camera_raw_design::get(ui.ctx());
    let d = &design.crop;
    let t = Tokens::get(ui.ctx());
    let mut height = d.height + shift(676.0, groups);
    if opened(groups, GROUPS[3]) {
        height += 26.0;
    }
    let (panel, _) = ui.allocate_exact_size(vec2(d.width, height), Sense::hover());
    if d.elements.is_empty() {
        ui.label(tr("Not available"));
        return;
    }
    let mut toggle = None;
    for e in &d.elements {
        let geometry_body = e.rect[1] >= 323.0 && e.rect[1] < 676.0;
        if geometry_body && !opened(groups, GROUPS[2]) {
            continue;
        }
        let offset = shift(e.rect[1], groups);
        let r = Rect::from_min_max(panel.min + vec2(e.rect[0], e.rect[1] + offset), panel.min + vec2(e.rect[2], e.rect[3] + offset));
        match e.kind {
            Kind::Panel => {
                let extra = if group(&e.id) == Some(GROUPS[2]) && opened(groups, GROUPS[2]) { 358.0 } else { 0.0 };
                let extra = extra + if group(&e.id).is_some_and(|g| g != GROUPS[2] && opened(groups, g)) { 26.0 } else { 0.0 };
                ui.painter().rect_filled(Rect::from_min_max(r.min, r.max + vec2(0.0, extra)), t.radius, t.field);
            }
            Kind::Text => {
                let font = if e.id == "heading" { crate::theme::semibold(d.heading_size) } else { FontId::proportional(d.font_size) };
                if e.id == "geometry-info-text" {
                    let job = egui::text::LayoutJob::simple(tr(&e.label).into(), font, t.text, r.width());
                    let galley = ui.painter().layout_job(job);
                    ui.painter().galley(r.left_top(), galley, t.text);
                } else {
                    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, tr(&e.label), font, t.text);
                }
            }
            Kind::Disclosure => {
                let Some(g) = group(&e.id) else { continue };
                let open = opened(groups, g);
                let response = ui.interact(r, ui.id().with(&e.id), Sense::click());
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, ui.is_enabled(), open, tr(&e.label)));
                if response.clicked() || response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)) {
                    response.request_focus();
                    toggle = Some(g);
                }
                let c = r.center();
                let pts = if open {
                    vec![c + vec2(-4.0, -3.0), c + vec2(4.0, -3.0), c + vec2(0.0, 4.0)]
                } else {
                    vec![c + vec2(3.0, -4.0), c + vec2(3.0, 4.0), c + vec2(-4.0, 0.0)]
                };
                ui.painter().add(egui::Shape::convex_polygon(pts, t.icon, Stroke::NONE));
                if open && g != GROUPS[2] {
                    ui.painter().text(
                        r.left_bottom() + vec2(-160.0, 16.0),
                        Align2::LEFT_CENTER,
                        tr("Not available"),
                        FontId::proportional(d.font_size),
                        t.text_faint,
                    );
                }
            }
            Kind::Field => {
                let r = r.shrink2(vec2(1.0, 2.0));
                ui.painter().rect(r, t.radius_sm, t.field, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                ui.painter().text(r.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, tr(&e.display), FontId::proportional(d.font_size), t.text);
                if e.id == "preset-field" {
                    let c = r.right_center() - vec2(7.0, 0.0);
                    line(ui, vec![c + vec2(-3.0, -1.5), c + vec2(0.0, 1.5), c + vec2(3.0, -1.5)], t.icon);
                }
                let response = ui.interact(r, ui.id().with(&e.id), Sense::hover());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        if e.id == "preset-field" { egui::WidgetType::ComboBox } else { egui::WidgetType::DragValue },
                        false,
                        tr(&e.label),
                    )
                });
                response.on_hover_text(tr("Not available"));
            }
            Kind::Track => {
                ui.painter().rect_filled(r.shrink2(vec2(0.0, 1.5)), t.radius_sm, t.field_border);
            }
            Kind::Handle => {
                let c = r.center();
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![c + vec2(-6.0, 4.0), c + vec2(-6.0, 0.0), c + vec2(0.0, -6.0), c + vec2(6.0, 0.0), c + vec2(6.0, 4.0)],
                    t.icon,
                    Stroke::new(1.0, t.separator),
                ));
            }
            Kind::Divider => {
                ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, t.separator));
            }
            Kind::Check => {
                ui.painter().rect_stroke(r.shrink(3.0), t.radius_sm, Stroke::new(1.0, t.text_faint), egui::StrokeKind::Inside);
                let response = ui.interact(r, ui.id().with(&e.id), Sense::hover());
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, false, false, tr(&e.label)));
                response.on_hover_text(tr("Not available"));
            }
            Kind::Icon => {
                icon(ui, r, &e.id);
                if !e.label.is_empty() {
                    let response = ui.interact(r, ui.id().with(&e.id), Sense::hover());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, false, tr(&e.label)));
                    response.on_hover_text(tr("Not available"));
                }
            }
        }
    }
    if let Some(g) = toggle {
        if opened(groups, g) {
            groups.retain(|v| v != g);
        } else {
            groups.push(g.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crop_design_requires_controls_and_rejects_invalid_geometry() {
        let mut d = Design::default();
        d.validate().unwrap();
        d.elements[0].rect[0] = f32::NAN;
        assert!(d.validate().is_err());
        let mut d = Design::default();
        d.elements.pop();
        d.elements.push(d.elements[0].clone());
        assert!(d.validate().is_err());
        let mut d = Design::default();
        d.elements.retain(|e| e.id != "geometry-disclosure");
        assert!(d.validate().is_err());
    }
}
