//! Observed Remove view. Retouch operations need a replayable Camera Raw recipe;
//! only distraction disclosures and local availability help are actionable here.
use crate::{icons, theme::Tokens};
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, vec2};
use serde::{Deserialize, Serialize};
pub(crate) const GROUPS: [&str; 4] = ["Reflections", "People", "Blemishes", "Dust"];
const PREFIXES: [&str; 4] = ["reflections-", "people-", "blemishes-", "dust-"];
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Kind {
    Text,
    Icon,
    Panel,
    Check,
    Field,
    Track,
    Handle,
    Divider,
    Disclosure,
    Frame,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Element {
    id: String,
    kind: Kind,
    rect: [f32; 4],
    label: String,
    display: String,
    checked: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Design {
    width: f32,
    height: f32,
    font_size: f32,
    heading_size: f32,
    unavailable_height: f32,
    elements: Vec<Element>,
}
impl Default for Design {
    fn default() -> Self {
        serde_json::from_str(include_str!("../../../design/camera-raw/remove-design.json")).unwrap_or_else(|_| Self {
            width: 270.0,
            height: 545.0,
            font_size: 12.0,
            heading_size: 14.0,
            unavailable_height: 26.0,
            elements: Vec::new(),
        })
    }
}
impl Design {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite()
            || !(220.0..=600.0).contains(&self.width)
            || !self.height.is_finite()
            || !(500.0..=1000.0).contains(&self.height)
            || !self.font_size.is_finite()
            || !(9.0..=20.0).contains(&self.font_size)
            || !self.heading_size.is_finite()
            || !(10.0..=24.0).contains(&self.heading_size)
            || !self.unavailable_height.is_finite()
            || !(20.0..=80.0).contains(&self.unavailable_height)
            || self.elements.len() > 96
        {
            return Err("invalid Camera Raw Remove geometry".into());
        }
        let mut ids = std::collections::HashSet::new();
        for e in &self.elements {
            let [l, t, r, b] = e.rect;
            if !ids.insert(&e.id)
                || e.id.is_empty()
                || e.id.len() > 80
                || e.label.len() > 256
                || e.display.len() > 40
                || e.rect.iter().any(|v| !v.is_finite())
                || l < 0.0
                || t < 0.0
                || r <= l
                || b <= t
                || r > self.width
                || b > self.height
            {
                return Err(format!("invalid Camera Raw Remove element {}", e.id));
            }
        }
        for (id, kind) in [
            ("heading", Kind::Text),
            ("remove-selected-frame", Kind::Frame),
            ("size-value", Kind::Field),
            ("generative-checkbox", Kind::Check),
            ("generative-info", Kind::Icon),
        ] {
            if !self.elements.iter().any(|e| e.id == id && e.kind == kind) {
                return Err(format!("Remove design requires {id}"));
            }
        }
        for prefix in PREFIXES {
            for (suffix, kind) in [("panel", Kind::Panel), ("title", Kind::Text), ("disclosure", Kind::Disclosure)] {
                if !self.elements.iter().any(|e| e.id == format!("{prefix}{suffix}") && e.kind == kind) {
                    return Err(format!("Remove design requires {prefix}{suffix}"));
                }
            }
        }
        Ok(())
    }
}
fn tr(s: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", s)
}
fn group(id: &str) -> Option<usize> {
    PREFIXES.iter().position(|p| id.starts_with(p))
}
fn open(groups: &[String], i: usize) -> bool {
    GROUPS.get(i).is_some_and(|name| groups.iter().any(|g| g == name))
}
fn line(ui: &Ui, points: Vec<egui::Pos2>, color: egui::Color32) {
    ui.painter().add(egui::Shape::line(points, Stroke::new(1.25, color)));
}
fn person(ui: &Ui, c: egui::Pos2, ink: egui::Color32, size: f32) {
    ui.painter().circle_filled(c + vec2(0.0, -4.0) * size, 2.0 * size, ink);
    ui.painter().rect_filled(Rect::from_center_size(c + vec2(0.0, 1.0) * size, vec2(4.0, 6.0) * size), 0.0, ink);
    for x in [-1.2, 1.2] {
        line(ui, vec![c + vec2(x, 3.0) * size, c + vec2(x, 7.0) * size], ink);
    }
}
fn icon(ui: &Ui, r: Rect, id: &str) {
    let t = Tokens::get(ui.ctx());
    let c = r.center();
    let ink = t.icon;
    if id.ends_with("-glyph") && group(id).is_some() {
        ui.painter().rect_filled(r, t.radius_sm, t.dock);
    }
    match id {
        "reset" | "distractions-reset" => icons::paint(ui, r, "undo-2", 18.0, t.text_faint.gamma_multiply(0.45)),
        id if id.ends_with("-eye") => icons::paint(ui, r, "eye", 14.0, t.text_faint.gamma_multiply(0.45)),
        "remove-mode" => {
            ui.painter().add(egui::Shape::convex_polygon(
                vec![c + vec2(-8.0, 3.0), c + vec2(1.0, -8.0), c + vec2(9.0, -1.0), c + vec2(0.0, 9.0), c + vec2(-4.0, 9.0)],
                ink,
                Stroke::NONE,
            ));
            ui.painter().add(egui::Shape::convex_polygon(
                vec![c + vec2(-6.0, 3.0), c + vec2(-2.0, -2.0), c + vec2(4.0, 3.0), c + vec2(0.0, 7.0)],
                t.field,
                Stroke::NONE,
            ));
        }
        "heal-mode" => {
            let p = |x: f32, y: f32| c + vec2((x + y) * std::f32::consts::FRAC_1_SQRT_2, (y - x) * std::f32::consts::FRAC_1_SQRT_2);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![p(-9.0, -2.0), p(-7.0, -4.0), p(7.0, -4.0), p(9.0, -2.0), p(9.0, 2.0), p(7.0, 4.0), p(-7.0, 4.0), p(-9.0, 2.0)],
                ink,
                Stroke::NONE,
            ));
            for x in [-3.0, 1.0] {
                for y in [-1.5, 1.5] {
                    ui.painter().circle_filled(p(x, y), 0.8, t.field);
                }
            }
        }
        "clone-mode" => {
            ui.painter().circle_filled(c + vec2(0.0, -6.0), 3.5, ink);
            ui.painter().rect_filled(Rect::from_center_size(c, vec2(3.0, 8.0)), 0.0, ink);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![c + vec2(-9.0, 7.0), c + vec2(-8.0, 4.0), c + vec2(8.0, 4.0), c + vec2(9.0, 7.0)],
                ink,
                Stroke::NONE,
            ));
            line(ui, vec![c + vec2(-9.0, 10.0), c + vec2(9.0, 10.0)], ink);
        }
        "generative-info" => icons::paint(ui, r, "info", 13.0, ink),
        "feedback-glyph" => {
            ui.painter().rect_filled(Rect::from_center_size(c - vec2(0.0, 2.0), vec2(16.0, 12.0)), t.radius, ink);
            ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-6.0, 1.0), c + vec2(-6.0, 10.0), c + vec2(1.0, 1.0)], ink, Stroke::NONE));
            for x in [-4.0, 0.0, 4.0] {
                ui.painter().circle_filled(c + vec2(x, -2.0), 1.2, t.field);
            }
        }
        "reflections-glyph" => {
            for x in [-4.0, 3.0] {
                for y in [-5.0, 3.0] {
                    let p = c + vec2(x, y);
                    line(ui, vec![p + vec2(0.0, -2.0), p + vec2(4.0, -1.0), p + vec2(2.0, 4.0), p + vec2(-2.0, 3.0), p + vec2(0.0, -2.0)], ink);
                }
            }
        }
        "people-glyph" => {
            person(ui, c, ink, 1.0);
            person(ui, c + vec2(-6.0, -2.0), ink, 0.7);
            person(ui, c + vec2(6.0, -2.0), ink, 0.7);
        }
        "blemishes-glyph" => {
            ui.painter().circle_stroke(c, 5.0, Stroke::new(1.0, ink));
            ui.painter().circle_stroke(c + vec2(4.0, 2.0), 3.0, Stroke::new(1.0, ink));
            for (x, y) in [(-7.0, -5.0), (-3.0, -7.0), (-7.0, 0.0), (1.0, -2.0)] {
                ui.painter().circle_filled(c + vec2(x, y), 1.0, ink);
            }
            line(ui, vec![c + vec2(6.0, 4.0), c + vec2(9.0, 7.0)], ink);
        }
        "dust-glyph" => {
            ui.painter().rect_stroke(r.shrink(4.0), 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
            for (x, y) in [(-3.0, -3.0), (3.0, -1.0), (0.0, 4.0)] {
                line(ui, vec![c + vec2(x, y), c + vec2(x + 1.0, y - 2.0)], ink);
            }
        }
        _ => {}
    }
}
pub(super) fn show(ui: &mut Ui, groups: &mut Vec<String>, help: &mut bool) {
    let design = crate::camera_raw_design::get(ui.ctx());
    let d = &design.remove;
    let t = Tokens::get(ui.ctx());
    let (panel, _) = ui.allocate_exact_size(vec2(d.width, d.height + groups.len() as f32 * d.unavailable_height), Sense::hover());
    if d.elements.is_empty() {
        ui.label(tr("Not available"));
        return;
    }
    let mut toggle = None;
    for e in &d.elements {
        let gi = group(&e.id);
        let dy = gi.map_or(0.0, |i| (0..i).filter(|j| open(groups, *j)).count() as f32 * d.unavailable_height);
        let r = Rect::from_min_max(panel.min + vec2(e.rect[0], e.rect[1] + dy), panel.min + vec2(e.rect[2], e.rect[3] + dy));
        match e.kind {
            Kind::Text => {
                let font = if matches!(e.id.as_str(), "heading" | "distractions-heading") {
                    crate::theme::semibold(d.heading_size)
                } else {
                    FontId::proportional(d.font_size)
                };
                ui.painter().with_clip_rect(r).text(r.left_center(), Align2::LEFT_CENTER, tr(&e.label), font, t.text);
                if e.id == "feedback-label" {
                    let response = ui.interact(r, ui.id().with(&e.id), Sense::hover());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, false, tr(&e.label)));
                    response.on_hover_text(tr("Not available"));
                }
            }
            Kind::Frame => {
                ui.painter().rect(r, t.radius_sm, t.field, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                ui.painter().rect_stroke(r.shrink2(vec2(1.5, 2.0)), t.radius_sm, Stroke::new(1.0, t.accent_border), egui::StrokeKind::Inside);
            }
            Kind::Panel => {
                let extra = if gi.is_some_and(|i| open(groups, i)) { d.unavailable_height } else { 0.0 };
                ui.painter().rect_filled(Rect::from_min_max(r.min, r.max + vec2(0.0, extra)), t.radius, t.field);
            }
            Kind::Check => {
                let rr = r.shrink(2.5);
                ui.painter().rect(rr, t.radius_sm, if e.checked { t.icon } else { t.dock }, Stroke::new(1.0, t.text_faint), egui::StrokeKind::Inside);
                if e.checked {
                    let c = rr.center();
                    line(ui, vec![c + vec2(-3.0, 0.0), c + vec2(-1.0, 2.0), c + vec2(3.5, -2.5)], t.dock);
                }
                let label = d.elements.iter().find(|v| v.id == e.id.replace("checkbox", "label"));
                let hit =
                    label.map(|v| r.union(Rect::from_min_max(panel.min + vec2(v.rect[0], v.rect[1]), panel.min + vec2(v.rect[2], v.rect[3])))).unwrap_or(r);
                let response = ui.interact(hit, ui.id().with(&e.id), Sense::hover());
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, false, e.checked, tr(&e.label)));
                response.on_hover_text(tr("Not available"));
            }
            Kind::Field => {
                let rr = r.shrink2(vec2(1.0, 2.0));
                ui.painter().rect(rr, t.radius_sm, t.field, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                ui.painter().text(rr.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, &e.display, FontId::proportional(d.font_size), t.text);
                let response = ui.interact(r, ui.id().with(&e.id), Sense::hover());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, false, tr(&e.label)));
                response.on_hover_text(tr("Not available"));
            }
            Kind::Track => {
                ui.painter().rect_filled(r.shrink2(vec2(0.0, 2.0)), t.radius_sm, t.field_border);
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
            Kind::Disclosure => {
                let Some(i) = gi else { continue };
                let opened = open(groups, i);
                let label = d.elements.iter().find(|v| v.id == format!("{}title", PREFIXES.get(i).copied().unwrap_or_default()));
                let hit = label
                    .map(|v| r.union(Rect::from_min_max(panel.min + vec2(v.rect[0], v.rect[1] + dy), panel.min + vec2(v.rect[2], v.rect[3] + dy))))
                    .unwrap_or(r);
                let response = ui.interact(hit, ui.id().with(&e.id), Sense::click());
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, ui.is_enabled(), opened, tr(&e.label)));
                if response.clicked() || response.has_focus() && ui.input(|v| v.key_pressed(egui::Key::Space) || v.key_pressed(egui::Key::Enter)) {
                    response.request_focus();
                    toggle = GROUPS.get(i).copied();
                }
                let c = r.center();
                ui.painter().add(egui::Shape::convex_polygon(
                    if opened {
                        vec![c + vec2(-4.0, -3.0), c + vec2(4.0, -3.0), c + vec2(0.0, 4.0)]
                    } else {
                        vec![c + vec2(3.0, -4.0), c + vec2(3.0, 4.0), c + vec2(-4.0, 0.0)]
                    },
                    t.icon,
                    Stroke::NONE,
                ));
                if opened {
                    ui.painter().text(
                        r.left_bottom() + vec2(-160.0, 15.0),
                        Align2::LEFT_CENTER,
                        tr("Not available"),
                        FontId::proportional(d.font_size),
                        t.text_faint,
                    );
                }
            }
            Kind::Icon => {
                icon(ui, r, &e.id);
                if !e.label.is_empty() {
                    let enabled = e.id == "generative-info" && ui.is_enabled();
                    let response = ui.interact(r, ui.id().with(&e.id), if enabled { Sense::click() } else { Sense::hover() });
                    if e.id.ends_with("-mode") {
                        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, false, e.id == "remove-mode", tr(&e.label)));
                    } else {
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tr(&e.label)));
                    }
                    if enabled && response.clicked() {
                        *help = true;
                    } else if !enabled {
                        response.on_hover_text(tr("Not available"));
                    }
                }
            }
        }
    }
    if let Some(g) = toggle {
        if groups.iter().any(|v| v == g) {
            groups.retain(|v| v != g);
        } else {
            groups.push(g.into());
        }
    }
    if *help {
        let ctx = ui.ctx().clone();
        egui::Modal::new(egui::Id::new("camera-raw-remove-help")).show(&ctx, |ui| {
            ctx.set_sublayer(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("camera-raw-dialog")), ui.layer_id());
            ui.set_max_width(300.0);
            ui.label(tr("Remove, Heal and Clone are not available in Camera Raw yet."));
            ui.separator();
            ui.label(tr("Automatic distraction removal is not available in this build."));
            if ui.button(tr("Close")).clicked() {
                *help = false;
            }
        });
        if ctx.input(|v| v.key_pressed(egui::Key::Escape)) {
            *help = false;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remove_design_rejects_nonfinite_duplicate_and_missing_controls() {
        Design::default().validate().unwrap();
        assert!(Design { unavailable_height: f32::NAN, ..Default::default() }.validate().is_err());
        let mut d = Design::default();
        d.elements.push(d.elements[0].clone());
        assert!(d.validate().is_err());
        let mut d = Design::default();
        d.elements.retain(|e| e.id != "size-value");
        assert!(d.validate().is_err());
        let mut d = Design::default();
        d.elements.retain(|e| e.id != "dust-disclosure");
        assert!(d.validate().is_err());
    }
}
