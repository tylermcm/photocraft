//! Observed preset-browser presentation. Catalog titles identify the reference groups;
//! no proprietary preset recipes or assets are bundled or simulated.
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::{camera_raw_workspace_ui::WorkspaceState, theme::Tokens};

fn tr(text: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", text)
}

pub(crate) const GROUPS: [&str; 38] = [
    "Adaptive: Portrait",
    "Adaptive: Sky",
    "Adaptive: Subject",
    "Adaptive: Landscape",
    "Adaptive: Blur Background",
    "Portraits: Deep Skin",
    "Portraits: Medium Skin",
    "Portraits: Light Skin",
    "Portraits: Black & White",
    "Portraits: Edgy",
    "Portraits: Group",
    "Auto+: Retro",
    "Seasons: Spring",
    "Seasons: Summer",
    "Seasons: Autumn",
    "Seasons: Winter",
    "Style: Black & White",
    "Style: Cinematic",
    "Style: Cinematic II",
    "Style: Film-Inspired",
    "Style: Futuristic",
    "Style: Vintage",
    "Subject: Concerts",
    "Subject: Food",
    "Subject: Landscape",
    "Subject: Lifestyle",
    "Subject: Travel",
    "Subject: Travel II",
    "Subject: Urban Architecture",
    "Video: Creative",
    "Color",
    "Creative",
    "B&W",
    "Portraits",
    "Defaults",
    "Curve",
    "Grain",
    "Optics",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Design {
    pub heading_height: f32,
    pub heading_inset: f32,
    pub heading_offset_y: f32,
    pub amount_height: f32,
    pub row_height: f32,
    pub font_size: f32,
    pub list_top_padding: f32,
    pub label_inset: f32,
    pub label_offset_y: f32,
    pub disclosure_offset_y: f32,
    pub disclosure_inset: f32,
    pub group_gaps: [f32; 3],
    pub scrollbar_width: f32,
    pub scrollbar_inner_margin: f32,
    pub scrollbar_outer_margin: f32,
}
impl Default for Design {
    fn default() -> Self {
        Self {
            heading_height: 39.0,
            heading_inset: 18.0,
            heading_offset_y: -1.0,
            amount_height: 50.0,
            row_height: 24.0,
            font_size: 11.0,
            list_top_padding: 6.0,
            label_inset: 23.0,
            label_offset_y: 0.5,
            disclosure_offset_y: 1.5,
            disclosure_inset: 12.0,
            group_gaps: [17.0, 17.0, 16.0],
            scrollbar_width: 11.0,
            scrollbar_inner_margin: 2.0,
            scrollbar_outer_margin: 1.5,
        }
    }
}
impl Design {
    pub(crate) fn validate(&self) -> Result<(), String> {
        for (name, value, lo, hi) in [
            ("headingHeight", self.heading_height, 24.0, 64.0),
            ("scrollbarWidth", self.scrollbar_width, 4.0, 20.0),
            ("scrollbarInnerMargin", self.scrollbar_inner_margin, 0.0, 8.0),
            ("scrollbarOuterMargin", self.scrollbar_outer_margin, 0.0, 8.0),
            ("headingInset", self.heading_inset, 4.0, 40.0),
            ("headingOffsetY", self.heading_offset_y, -8.0, 8.0),
            ("labelOffsetY", self.label_offset_y, -8.0, 8.0),
            ("disclosureOffsetY", self.disclosure_offset_y, -8.0, 8.0),
            ("amountHeight", self.amount_height, 30.0, 80.0),
            ("rowHeight", self.row_height, 18.0, 48.0),
            ("fontSize", self.font_size, 9.0, 24.0),
            ("listTopPadding", self.list_top_padding, 0.0, 24.0),
            ("labelInset", self.label_inset, 12.0, 40.0),
            ("disclosureInset", self.disclosure_inset, 4.0, 20.0),
        ] {
            if !value.is_finite() || !(lo..=hi).contains(&value) {
                return Err(format!("presets.{name} must be in {lo}..={hi}"));
            }
        }
        if self.group_gaps.iter().any(|v| !v.is_finite() || !(0.0..=40.0).contains(v)) {
            return Err("presets.groupGaps must contain finite values in 0..=40".into());
        }
        Ok(())
    }
}

pub(super) fn scrollbar_style(ui: &mut Ui) {
    let design = crate::camera_raw_design::get(ui.ctx());
    let d = &design.presets;
    let t = Tokens::get(ui.ctx());
    ui.spacing_mut().scroll = egui::style::ScrollStyle {
        floating: true,
        bar_width: d.scrollbar_width,
        floating_width: d.scrollbar_width,
        floating_allocated_width: 0.0,
        bar_inner_margin: d.scrollbar_inner_margin,
        bar_outer_margin: d.scrollbar_outer_margin,
        dormant_background_opacity: 1.0,
        active_background_opacity: 1.0,
        interact_background_opacity: 1.0,
        dormant_handle_opacity: 1.0,
        active_handle_opacity: 1.0,
        interact_handle_opacity: 1.0,
        ..egui::style::ScrollStyle::solid()
    };
    let visuals = &mut ui.style_mut().visuals;
    visuals.extreme_bg_color = t.camera_raw_scrollbar_track;
    for widget in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.noninteractive] {
        widget.bg_fill = t.camera_raw_scrollbar_handle;
        widget.corner_radius = egui::CornerRadius::from(t.radius_lg);
    }
}

pub(super) fn header(ui: &mut Ui) {
    let design = crate::camera_raw_design::get(ui.ctx());
    let d = &design.presets;
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width();
    let (header, _) = ui.allocate_exact_size(vec2(width, d.heading_height), Sense::hover());
    super::info::heading(ui, header.shrink2(vec2(d.heading_inset, 0.0)).translate(vec2(0.0, d.heading_offset_y)), tl!("Presets"), 13.0);
    let create = Rect::from_center_size(pos2(header.right() - 58.0, header.center().y), vec2(22.0, 22.0));
    let r = ui.interact(create, ui.id().with("create-camera-raw-preset"), Sense::hover());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, false, tl!("Create Preset…")));
    r.on_hover_text(tr("Not available"));
    let p = create.center() - vec2(4.5, 5.0);
    ui.painter().add(egui::Shape::closed_line(
        vec![p, p + vec2(9.0, 0.0), p + vec2(9.0, 10.0), p + vec2(5.0, 10.0), p + vec2(0.0, 5.0)],
        Stroke::new(1.2, t.text_faint),
    ));
    ui.painter().line_segment([p + vec2(0.0, 5.0), p + vec2(5.0, 5.0)], Stroke::new(1.2, t.text_faint));
    ui.painter().line_segment([p + vec2(5.0, 5.0), p + vec2(5.0, 10.0)], Stroke::new(1.2, t.text_faint));
    let menu = Rect::from_center_size(pos2(header.right() - 32.0, header.center().y), vec2(22.0, 22.0));
    let r = ui.interact(menu, ui.id().with("camera-raw-preset-settings"), Sense::hover());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, false, tl!("Settings")));
    r.on_hover_text(tr("Not available"));
    for x in [-4.5, 0.0, 4.5] {
        ui.painter().circle_filled(menu.center() + vec2(x, 0.0), 1.2, t.text_faint);
    }
    ui.painter().line_segment([header.left_bottom(), header.right_bottom()], Stroke::new(1.0, t.separator));
    let (amount, _) = ui.allocate_exact_size(vec2(width, d.amount_height), Sense::hover());
    ui.painter().text(amount.left_top() + vec2(13.5, 17.5), Align2::LEFT_CENTER, tl!("None"), FontId::proportional(d.font_size), t.text_faint);
    let field = Rect::from_min_size(amount.right_top() + vec2(-57.0, 11.0), vec2(41.0, 17.0));
    ui.painter().rect_stroke(field, 0.0, Stroke::new(1.0, t.field_border.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    ui.painter()
        .line_segment([amount.left_top() + vec2(10.0, 32.5), amount.right_top() + vec2(-13.0, 32.5)], Stroke::new(3.0, t.text_faint.gamma_multiply(0.5)));
    let r = ui.interact(amount, ui.id().with("camera-raw-preset-amount"), Sense::hover());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, false, tl!("Amount")));
    r.on_hover_text(tr("Not available"));
    ui.painter().line_segment([amount.left_bottom(), amount.right_bottom()], Stroke::new(1.0, t.separator));
}

pub(super) fn list(ui: &mut Ui, workspace: &mut WorkspaceState) {
    let design = crate::camera_raw_design::get(ui.ctx());
    let d = &design.presets;
    let t = Tokens::get(ui.ctx());
    ui.add_space(d.list_top_padding);
    for (i, name) in GROUPS.iter().enumerate() {
        let gap = match i {
            30 => d.group_gaps.first(),
            34 => d.group_gaps.get(1),
            35 => d.group_gaps.get(2),
            _ => None,
        };
        if let Some(gap) = gap {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), *gap), Sense::hover());
            let y = rect.top() + gap * 0.6;
            ui.painter().line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, t.separator));
        }
        let open = workspace.preset_groups.iter().any(|v| v == name);
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), d.row_height), Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, ui.is_enabled(), open, *name));
        if response.clicked() || response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)) {
            response.request_focus();
            if open {
                workspace.preset_groups.retain(|v| v != name);
            } else {
                workspace.preset_groups.push((*name).into());
            }
        }
        let c = pos2(rect.left() + d.disclosure_inset, rect.center().y + d.disclosure_offset_y);
        let points = if open {
            vec![c + vec2(-4.0, -2.5), c + vec2(4.0, -2.5), c + vec2(0.0, 3.5)]
        } else {
            vec![c + vec2(-4.0, -5.0), c + vec2(-4.0, 5.0), c + vec2(4.0, 0.0)]
        };
        ui.painter().add(egui::Shape::convex_polygon(points, t.icon, Stroke::NONE));
        ui.painter().text(pos2(rect.left() + d.label_inset, rect.center().y + d.label_offset_y), Align2::LEFT_CENTER, *name, FontId::proportional(d.font_size), t.text);
        if response.has_focus() {
            ui.painter().rect_stroke(rect.shrink(1.0), t.radius_sm, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        }
        if open {
            // Expanding a category is a real view operation. Never imply a recipe exists.
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.row_height), Sense::hover());
            ui.painter().text(
                pos2(rect.left() + d.label_inset, rect.center().y + d.label_offset_y),
                Align2::LEFT_CENTER,
                tr("Not available"),
                FontId::proportional(d.font_size),
                t.text_faint,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_rejects_nonfinite_and_out_of_bounds_geometry() {
        assert!(Design::default().validate().is_ok());
        for value in [f32::NAN, f32::INFINITY, -1.0, 49.0] {
            assert!(Design { row_height: value, ..Default::default() }.validate().is_err());
        }
        for value in [f32::NAN, -1.0, 41.0] {
            assert!(Design { group_gaps: [20.0, value, 16.0], ..Default::default() }.validate().is_err());
        }
        let encoded = serde_json::to_value(Design::default()).unwrap();
        assert_eq!(serde_json::from_value::<Design>(encoded).unwrap(), Design::default());
    }
}
