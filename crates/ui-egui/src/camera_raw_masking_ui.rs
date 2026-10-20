//! Observed Masking creation panel. Local mask actions are unavailable until
//! Camera Raw can preview and replay them; People disclosure and help are views.
use crate::{icons, theme::Tokens};
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Kind {
    Text,
    Icon,
    Card,
    Key,
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
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Design {
    width: f32,
    height: f32,
    font_size: f32,
    heading_size: f32,
    people_body_height: f32,
    elements: Vec<Element>,
}
impl Default for Design {
    fn default() -> Self {
        serde_json::from_str(include_str!("../../../design/camera-raw/masking-design.json")).unwrap_or_else(|_| Self {
            width: 270.0,
            height: 530.0,
            font_size: 12.0,
            heading_size: 14.0,
            people_body_height: 64.0,
            elements: Vec::new(),
        })
    }
}
impl Design {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite()
            || !(220.0..=600.0).contains(&self.width)
            || !self.height.is_finite()
            || !(450.0..=900.0).contains(&self.height)
            || !self.font_size.is_finite()
            || !(9.0..=20.0).contains(&self.font_size)
            || !self.heading_size.is_finite()
            || !(10.0..=24.0).contains(&self.heading_size)
            || !self.people_body_height.is_finite()
            || !(40.0..=100.0).contains(&self.people_body_height)
            || self.elements.len() > 96
        {
            return Err("invalid Camera Raw Masking geometry".into());
        }
        let mut ids = std::collections::HashSet::new();
        for e in &self.elements {
            let [l, t, r, b] = e.rect;
            if !ids.insert(&e.id)
                || e.id.is_empty()
                || e.id.len() > 80
                || e.label.len() > 256
                || e.rect.iter().any(|v| !v.is_finite())
                || l < 0.0
                || t < 0.0
                || r <= l
                || b <= t
                || r > self.width
                || b > self.height
            {
                return Err(format!("invalid Camera Raw Masking element {}", e.id));
            }
        }
        for (id, kind) in [
            ("heading", Kind::Text),
            ("subject-card", Kind::Card),
            ("sky-card", Kind::Card),
            ("background-card", Kind::Card),
            ("people-disclosure", Kind::Disclosure),
            ("help-text", Kind::Text),
        ] {
            if !self.elements.iter().any(|e| e.id == id && e.kind == kind) {
                return Err(format!("Masking design requires {id}"));
            }
        }
        Ok(())
    }
}
fn tr(s: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", s)
}
fn line(ui: &Ui, p: Vec<egui::Pos2>, color: egui::Color32) {
    ui.painter().add(egui::Shape::line(p, Stroke::new(1.25, color)));
}
fn dotted(ui: &Ui, points: &[egui::Pos2], color: egui::Color32) {
    for pair in points.windows(2) {
        let len = pair[0].distance(pair[1]);
        let steps = (len / 3.5).ceil().max(1.0) as usize;
        for step in 0..steps {
            let p = pair[0].lerp(pair[1], step as f32 / steps as f32);
            ui.painter().circle_filled(p, 0.8, color);
        }
    }
}
fn person(ui: &Ui, c: egui::Pos2, color: egui::Color32) {
    ui.painter().circle_filled(c + vec2(0.0, -6.0), 4.0, color);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![c + vec2(-9.0, 11.0), c + vec2(-7.0, 5.0), c + vec2(-3.0, 1.0), c + vec2(3.0, 1.0), c + vec2(7.0, 5.0), c + vec2(9.0, 11.0)],
        color,
        Stroke::NONE,
    ));
}
fn icon(ui: &Ui, r: Rect, id: &str) {
    let t = Tokens::get(ui.ctx());
    let c = r.center();
    let ink = t.icon;
    match id {
        "subject-glyph" => {
            person(ui, c, ink);
            let pts = (0..=32)
                .map(|i| {
                    let a = (i as f32 * 360.0 / 32.0).to_radians();
                    c + vec2(a.cos() * 13.0, a.sin() * 15.0)
                })
                .collect::<Vec<_>>();
            dotted(ui, &pts, ink);
        }
        "sky-glyph" | "background-glyph" => {
            let rr = Rect::from_center_size(c, vec2(28.0, 22.0));
            dotted(ui, &[rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()], ink);
            if id == "sky-glyph" {
                ui.painter().circle_filled(c + vec2(6.0, -4.0), 2.2, ink);
                ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-10.0, 7.0), c + vec2(-4.0, -1.0), c + vec2(3.0, 7.0)], ink, Stroke::NONE));
                ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-2.0, 7.0), c + vec2(5.0, 1.0), c + vec2(10.0, 7.0)], ink, Stroke::NONE));
            } else {
                ui.painter().rect_filled(rr.shrink(3.0), 0.0, ink);
                person(ui, c + vec2(0.0, 2.0), t.dock);
            }
        }
        "landscape-glyph" => {
            let pts = vec![c + vec2(-8.0, 8.0), c + vec2(-4.0, -7.0), c + vec2(2.0, 1.0), c + vec2(6.0, -2.0), c + vec2(10.0, 8.0), c + vec2(-8.0, 8.0)];
            dotted(ui, &pts, ink);
            line(ui, vec![c + vec2(-4.0, 3.0), c + vec2(1.0, 5.0), c + vec2(6.0, 3.0)], ink);
        }
        "objects-glyph" => icons::paint(ui, r, "square-dashed-mouse-pointer", 19.0, ink),
        "brush-glyph" => icons::paint(ui, r, "brush", 20.0, ink),
        "linear-gradient-glyph" => {
            let rr = Rect::from_center_size(c, vec2(14.0, 14.0));
            ui.painter().rect_stroke(rr, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
            for i in 0..10 {
                let col = egui::Color32::from_rgba_premultiplied(ink.r(), ink.g(), ink.b(), 255).gamma_multiply(0.25 + i as f32 * 0.06);
                let y = rr.top() + 2.0 + i as f32;
                ui.painter().line_segment([egui::pos2(rr.left() + 2.0, y), egui::pos2(rr.right() - 2.0, y)], Stroke::new(1.0, col));
            }
        }
        "radial-gradient-glyph" => {
            for i in (1..=8).rev() {
                ui.painter().circle_filled(c, i as f32, t.field.lerp_to_gamma(ink, 0.8 - i as f32 * 0.07));
            }
            ui.painter().circle_stroke(c, 8.0, Stroke::new(1.0, ink));
        }
        "range-glyph" => {
            ui.painter().circle_filled(c, 8.0, ink);
            icons::paint(ui, r, "pipette", 19.0, t.text_dim);
        }
        "range-menu-indicator" => {
            ui.painter().add(egui::Shape::convex_polygon(vec![r.left_bottom(), r.right_bottom(), r.right_top()], ink, Stroke::NONE));
        }
        "people-warning-icon" => icons::paint(ui, r, "triangle-alert", 13.0, ink),
        "help-icon" => {
            ui.painter().circle_stroke(c, 8.0, Stroke::new(1.25, t.text_faint));
            ui.painter().text(c, Align2::CENTER_CENTER, "?", FontId::proportional(14.0), t.text_faint);
        }
        _ => {}
    }
}
pub(super) fn show(ui: &mut Ui, people: &mut bool, help: &mut bool, armed: &mut Option<super::mask_edit::MaskTool>, request: &mut Option<super::mask_edit::AutoMask>, host_ready: bool) {
    use super::mask_edit::{AutoMask, MaskTool};
    let design = crate::camera_raw_design::get(ui.ctx());
    let d = &design.masking;
    let t = Tokens::get(ui.ctx());
    let (panel, _) = ui.allocate_exact_size(vec2(d.width, d.height - if *people { 0.0 } else { d.people_body_height }), Sense::hover());
    if d.elements.is_empty() {
        ui.label(tr("Not available"));
        return;
    }
    let mut toggle = false;
    for e in &d.elements {
        if !*people && e.id.starts_with("people-warning-") {
            continue;
        }
        let shift = if !*people && e.id.starts_with("help-") { -d.people_body_height } else { 0.0 };
        let r = Rect::from_min_max(panel.min + vec2(e.rect[0], e.rect[1] + shift), panel.min + vec2(e.rect[2], e.rect[3] + shift));
        let font = FontId::proportional(if e.id == "heading" || e.id == "people-label" { d.heading_size } else { d.font_size });
        match e.kind {
            Kind::Text => {
                let color = if e.id == "help-text" { t.text_faint } else { t.text };
                if matches!(e.id.as_str(), "people-warning-text" | "help-text") {
                    let galley = ui.painter().layout_job(egui::text::LayoutJob::simple(tr(&e.label).into(), font, color, r.width()));
                    ui.painter().galley(r.left_top(), galley, color);
                } else {
                    ui.painter().text(
                        if e.id.ends_with("-label") && matches!(e.id.as_str(), "subject-label" | "sky-label" | "background-label") {
                            r.center()
                        } else {
                            r.left_center()
                        },
                        if matches!(e.id.as_str(), "subject-label" | "sky-label" | "background-label") { Align2::CENTER_CENTER } else { Align2::LEFT_CENTER },
                        tr(&e.label),
                        font,
                        color,
                    );
                }
                if e.id == "help-text" {
                    let response = ui.interact(r, ui.id().with("masking-help"), Sense::click());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tr(&e.label)));
                    if response.clicked() {
                        *help = true;
                    }
                }
                if e.id.ends_with("-label") && !matches!(e.id.as_str(), "subject-label" | "sky-label" | "background-label" | "people-label") {
                    // The gradients work: a click arms the tool and the next drag on the photo makes the mask. The rest are not built yet.
                    let tool = match e.id.as_str() {
                        "linear-gradient-label" => Some(MaskTool::Linear),
                        "radial-gradient-label" => Some(MaskTool::Radial),
                        "objects-label" => Some(MaskTool::Objects),
                        "brush-label" => Some(MaskTool::Brush),
                        "range-label" => Some(MaskTool::Colour),
                        _ => None,
                    };
                    if let Some(tool) = tool {
                        let glyph = d.elements.iter().find(|v| v.id == e.id.replace("-label", "-glyph"));
                        let hit = glyph.map_or(r, |v| r.union(Rect::from_min_max(panel.min + vec2(v.rect[0], v.rect[1]), panel.min + vec2(v.rect[2], v.rect[3]))));
                        let response = ui.interact(hit.expand2(vec2(4.0, 2.0)), ui.id().with(&e.id), Sense::click());
                        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, *armed == Some(tool), tr(&e.label)));
                        if *armed == Some(tool) {
                            ui.painter().rect_stroke(hit.expand2(vec2(4.0, 2.0)), t.radius_sm, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
                        }
                        if response.clicked() {
                            *armed = Some(tool);
                        }
                        response.on_hover_text(match tool {
                            MaskTool::Colour => tr("Click here, then click a colour in the photo."),
                            MaskTool::Brush => tr("Click here, then paint on the photo."),
                            MaskTool::Objects => tr("Click here, then drag a box around an object."),
                            _ => tr("Click here, then drag on the photo."),
                        });
                    } else {
                        let response = ui.interact(r, ui.id().with(&e.id), Sense::hover());
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, false, tr(&e.label)));
                        response.on_hover_text(tr("Not available"));
                    }
                }
            }
            Kind::Card => {
                // Subject and Background are found by Image Triage (BiRefNet); the others are not built yet.
                let auto = match e.id.as_str() {
                    "subject-card" => Some(AutoMask::Subject),
                    "sky-card" => Some(AutoMask::Sky),
                    "background-card" => Some(AutoMask::Background),
                    _ => None,
                };
                let live = auto.is_some_and(|kind| kind == AutoMask::Sky || host_ready);
                let response = ui.interact(r, ui.id().with(&e.id), if live { Sense::click() } else { Sense::hover() });
                let outline = if live && response.hovered() { t.text } else { t.text_faint };
                ui.painter().rect(r, t.radius_lg, if live && response.hovered() { t.hover } else { t.field }, Stroke::new(1.5, outline), egui::StrokeKind::Inside);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, live, tr(&e.label)));
                if live {
                    if response.clicked() {
                        *request = auto;
                    }
                } else if auto.is_some() {
                    response.on_hover_text(tr("Needs Image Triage. Open Camera Raw from Image Triage to use it."));
                } else {
                    response.on_hover_text(tr("Not available"));
                }
            }
            Kind::Key => {
                ui.painter().rect_stroke(r.shrink(2.0), t.radius_sm, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, &e.label, FontId::proportional(11.0), t.camera_raw_dialog_disabled);
            }
            Kind::Divider => {
                ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, t.separator));
            }
            Kind::Disclosure => {
                let hit = d
                    .elements
                    .iter()
                    .find(|v| v.id == "people-label")
                    .map(|v| r.union(Rect::from_min_max(panel.min + vec2(v.rect[0], v.rect[1]), panel.min + vec2(v.rect[2], v.rect[3]))))
                    .unwrap_or(r);
                let response = ui.interact(hit, ui.id().with(&e.id), Sense::click());
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, ui.is_enabled(), *people, tr(&e.label)));
                if response.clicked() || response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)) {
                    response.request_focus();
                    toggle = true;
                }
                let c = r.center();
                line(
                    ui,
                    if *people {
                        vec![c + vec2(-4.0, -2.0), c + vec2(0.0, 2.0), c + vec2(4.0, -2.0)]
                    } else {
                        vec![c + vec2(-2.0, -4.0), c + vec2(2.0, 0.0), c + vec2(-2.0, 4.0)]
                    },
                    t.icon,
                );
            }
            Kind::Icon => {
                icon(ui, r, &e.id);
                if e.id == "range-menu-indicator" {
                    // The corner of the Range card: a luminance range instead of a colour range.
                    let response = ui.interact(r.expand(4.0), ui.id().with(&e.id), Sense::click());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr("Luminance Range")));
                    if response.clicked() {
                        *request = Some(AutoMask::LuminanceRange);
                    }
                    response.on_hover_text(tr("Luminance Range"));
                }
            }
        }
    }
    if toggle {
        *people = !*people;
    }
    if *help {
        let ctx = ui.ctx().clone();
        egui::Modal::new(egui::Id::new("camera-raw-masking-help")).show(&ctx, |ui| {
            ctx.set_sublayer(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("camera-raw-dialog")), ui.layer_id());
            ui.set_max_width(300.0);
            ui.label(tr("Local masking is not available in Camera Raw yet."));
            ui.separator();
            ui.label(tr("K, G and J shortcuts are unavailable until local masks can preview and commit."));
            if ui.button(tr("Close")).clicked() {
                *help = false;
            }
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            *help = false;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masking_design_rejects_nonfinite_duplicate_and_missing_controls() {
        Design::default().validate().unwrap();
        let d = Design { people_body_height: f32::NAN, ..Default::default() };
        assert!(d.validate().is_err());
        let mut d = Design::default();
        d.elements.push(d.elements[0].clone());
        assert!(d.validate().is_err());
        let mut d = Design::default();
        d.elements.retain(|e| e.id != "subject-card");
        assert!(d.validate().is_err());
    }
}
