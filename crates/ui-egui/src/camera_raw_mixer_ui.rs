//! Measured Color Mixer view. Existing eight-band recipe edits remain real;
//! Point Color and the all-colors view are explicitly unavailable.
use crate::{camera_raw_workspace_ui::MixerMode, theme::Tokens, widgets};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, vec2};
use photocraft_algo::camera_raw::CameraRaw;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Design {
    tabs_height: f32,
    adjust_top: f32,
    adjust_height: f32,
    adjust_inset: f32,
    adjust_width: f32,
    swatches_top: f32,
    swatch_size: f32,
    swatch_step: f32,
    swatch_inset: f32,
    swatch_radius: f32,
    sliders_top: f32,
    slider_inset: f32,
    slider_width: f32,
    row_height: f32,
    value_width: f32,
    color_height: f32,
    pub bottom_padding: f32,
    hues: [f32; 8],
    saturations: [f32; 8],
}
impl Default for Design {
    fn default() -> Self {
        serde_json::from_str(include_str!("../../../design/camera-raw/mixer-design.json")).unwrap_or(Self {
            tabs_height: 31.0,
            adjust_top: 34.0,
            adjust_height: 22.0,
            adjust_inset: 40.0,
            adjust_width: 151.0,
            swatches_top: 66.5,
            swatch_size: 20.0,
            swatch_step: 25.0,
            swatch_inset: -5.5,
            swatch_radius: 7.5,
            sliders_top: 101.0,
            slider_inset: -2.0,
            slider_width: 217.0,
            row_height: 31.0,
            value_width: 45.0,
            color_height: 194.0,
            bottom_padding: 12.0,
            hues: [0.0, 32.0, 60.0, 117.0, 174.0, 217.0, 270.0, 295.0],
            saturations: [0.75, 0.7, 0.65, 0.65, 0.4, 0.8, 0.6, 0.6],
        })
    }
}
impl Design {
    pub(crate) fn validate(&self) -> Result<(), String> {
        for (v, lo, hi) in [
            (self.tabs_height, 20.0, 50.0),
            (self.adjust_top, 25.0, 70.0),
            (self.adjust_height, 16.0, 30.0),
            (self.adjust_inset, 25.0, 60.0),
            (self.adjust_width, 100.0, 180.0),
            (self.swatches_top, 50.0, 100.0),
            (self.swatch_size, 16.0, 24.0),
            (self.swatch_step, 20.0, 27.0),
            (self.swatch_inset, -8.0, 0.0),
            (self.swatch_radius, 5.0, 9.0),
            (self.sliders_top, 85.0, 130.0),
            (self.slider_inset, -5.0, 5.0),
            (self.slider_width, 190.0, 225.0),
            (self.row_height, 28.0, 40.0),
            (self.value_width, 40.0, 60.0),
            (self.color_height, 170.0, 250.0),
            (self.bottom_padding, 0.0, 30.0),
        ] {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                return Err("invalid Camera Raw Color Mixer geometry".into());
            }
        }
        if self.tabs_height > self.adjust_top
            || self.adjust_top + self.adjust_height > self.swatches_top
            || self.swatches_top + self.swatch_size > self.sliders_top
            || self.sliders_top + 3.0 * self.row_height > self.color_height
            || self.swatch_radius * 2.0 > self.swatch_size
            || self.swatch_step < self.swatch_size
            || self.hues.iter().any(|v| !v.is_finite() || !(0.0..360.0).contains(v))
            || self.saturations.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("Camera Raw Color Mixer controls must be ordered and bounded".into());
        }
        Ok(())
    }
}
fn tr(s: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", s)
}
fn row(ui: &mut Ui, rect: Rect, label: &str, value: &mut f32, gradient: &[Color32], d: &Design) -> bool {
    let root = crate::camera_raw_design::get(ui.ctx());
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(label).max_rect(rect));
    child.set_width(rect.width());
    child.spacing_mut().item_spacing.y = 0.0;
    let response = widgets::slider_row_prec_styled(
        &mut child,
        tr(label),
        value,
        -100.0..=100.0,
        "",
        Some(gradient),
        widgets::RowGestures { reset: Some(0.0), wheel_step: None },
        Some(0),
        d.value_width,
        Some(root.slider_style()),
    );
    response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), f64::from(*value), tr(label)));
    response.changed()
}
pub(super) fn show(
    ui: &mut Ui,
    p: &mut CameraRaw,
    mode_state: &mut MixerMode,
    band_state: &mut usize,
    channel_state: &mut usize,
    tools: &mut super::finishing::MixerTools,
) -> bool {
    let root = crate::camera_raw_design::get(ui.ctx());
    let d = &root.color_mixer;
    let t = Tokens::get(ui.ctx());
    let top = ui.cursor().min;
    let width = ui.available_width();
    let color = *mode_state == MixerMode::Color;
    let height = if tools.point_color {
        330.0
    } else if color && tools.all_colors {
        d.swatches_top + 8.0 * (3.0 * d.row_height + 22.0) + 30.0
    } else if color {
        d.color_height
    } else {
        d.sliders_top + 8.0 * d.row_height + 24.0
    };
    let (bounds, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let top = egui::pos2(top.x, bounds.top());
    let font = FontId::proportional(root.panel_font_size);
    let mixer = Rect::from_min_size(top, vec2(41.0, d.tabs_height));
    let r = ui.interact(mixer, ui.id().with("mixer-tab"), Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), !tools.point_color, tr("Mixer")));
    if r.clicked() {
        tools.point_color = false;
        tools.picker = false;
    }
    ui.painter().text(top + vec2(0.0, 10.0), Align2::LEFT_CENTER, tr("Mixer"), font.clone(), if tools.point_color { t.text_faint } else { t.text });
    let point = Rect::from_min_size(top + vec2(42.5, 0.0), vec2(76.5, d.tabs_height));
    let r = ui.interact(point, ui.id().with("point-color-tab"), Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), tools.point_color, tr("Point Color")));
    if r.clicked() {
        tools.point_color = true;
    }
    ui.painter().text(
        point.left_top() + vec2(0.0, 10.0),
        Align2::LEFT_CENTER,
        tr("Point Color"),
        font.clone(),
        if tools.point_color { t.text } else { t.text_faint },
    );
    let y = top.y + d.tabs_height - 3.0;
    ui.painter().line_segment([egui::pos2(top.x + 1.0, y), egui::pos2(top.x + width - 1.0, y)], Stroke::new(1.0, t.separator));
    ui.painter().line_segment(
        [egui::pos2(top.x + if tools.point_color { 42.5 } else { 1.0 }, y), egui::pos2(top.x + if tools.point_color { 119.0 } else { 32.5 }, y)],
        Stroke::new(1.0, t.text),
    );
    if tools.point_color {
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt("point-color").max_rect(Rect::from_min_max(top + vec2(0.0, d.tabs_height), bounds.max)));
        return super::finishing::point(&mut child, p, tools);
    }
    ui.painter().text(top + vec2(-2.0, d.adjust_top + d.adjust_height / 2.0), Align2::LEFT_CENTER, tr("Adjust"), font.clone(), t.text);
    let field = Rect::from_min_size(top + vec2(d.adjust_inset, d.adjust_top), vec2(d.adjust_width, d.adjust_height));
    let r = ui.interact(field, ui.id().with("mixer-mode"), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), tr("Adjust")));
    ui.painter().rect(field.shrink2(vec2(0.0, 1.5)), t.radius_sm, t.field, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    ui.painter().text(field.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, if color { tr("Color") } else { "HSL" }, font, t.text);
    let c = field.right_center() - vec2(7.0, 0.0);
    ui.painter().add(egui::Shape::line(vec![c + vec2(-3.0, -1.5), c + vec2(0.0, 1.5), c + vec2(3.0, -1.5)], Stroke::new(1.0, t.icon)));
    egui::Popup::menu(&r).show(|ui| {
        for (mode, label) in [(MixerMode::Color, tr("Color")), (MixerMode::Hsl, "HSL")] {
            if ui.selectable_label(*mode_state == mode, label).clicked() {
                *mode_state = mode;
                ui.close();
            }
        }
    });
    let mut dirty = false;
    if color {
        for k in 0..9 {
            let rect = Rect::from_min_size(top + vec2(d.swatch_inset + k as f32 * d.swatch_step, d.swatches_top), vec2(d.swatch_size, d.swatch_size));
            let c = rect.center();
            let enabled = ui.is_enabled();
            let label = super::BANDS.get(k).copied().unwrap_or("All Colors");
            let r = ui.interact(rect, ui.id().with(("mixer-band", k)), if enabled { Sense::click() } else { Sense::hover() });
            r.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    enabled,
                    if k == 8 { tools.all_colors } else { !tools.all_colors && *band_state == k },
                    tr(label),
                )
            });
            if enabled && (r.clicked() || r.has_focus() && ui.input(|v| v.key_pressed(egui::Key::Space) || v.key_pressed(egui::Key::Enter))) {
                r.request_focus();
                tools.all_colors = k == 8;
                if k < 8 {
                    *band_state = k;
                }
            }
            if let (Some(h), Some(s)) = (d.hues.get(k), d.saturations.get(k)) {
                let ink = t.scope_hue(*h / 360.0, *s).lerp_to_gamma(t.dock, 0.1);
                if !tools.all_colors && *band_state == k {
                    ui.painter().circle_filled(c, d.swatch_radius, ink);
                    ui.painter().circle_filled(c, d.swatch_radius * 0.28, t.text);
                } else {
                    ui.painter().circle_stroke(c, d.swatch_radius, Stroke::new(1.8, ink));
                }
            } else {
                for i in 0..8 {
                    let h = *d.hues.get(i).unwrap_or(&0.0);
                    let points = (0..=4)
                        .map(|j| {
                            let a = (i as f32 * 45.0 + j as f32 * 11.25).to_radians();
                            c + vec2(a.cos(), a.sin()) * d.swatch_radius
                        })
                        .collect();
                    ui.painter().add(egui::Shape::line(points, Stroke::new(1.8, t.scope_hue(h / 360.0, 0.6))));
                }
            }
            r.on_hover_text(if enabled { tr(label) } else { tr("Not available") });
        }
        if tools.all_colors {
            let mut child = ui.new_child(egui::UiBuilder::new().id_salt("all-colors").max_rect(Rect::from_min_max(top + vec2(0.0, d.sliders_top), bounds.max)));
            for (k, name) in super::BANDS.iter().enumerate() {
                child.label(tr(name));
                let grad = [t.text_faint, t.scope_hue(d.hues.get(k).copied().unwrap_or(0.0) / 360.0, 0.7)];
                child.push_id(k, |ui| {
                    for (label, value) in [("Hue", &mut p.hsl_hue[k]), ("Saturation", &mut p.hsl_sat[k]), ("Luminance", &mut p.hsl_lum[k])] {
                        let (rect, _) = ui.allocate_exact_size(vec2(d.slider_width, d.row_height), Sense::hover());
                        dirty |= row(ui, rect, label, value, &grad, d);
                    }
                });
            }
            return dirty;
        }
        let band = (*band_state).min(7);
        let left = *d.hues.get((band + 7) % 8).unwrap_or(&0.0);
        let right = *d.hues.get((band + 1) % 8).unwrap_or(&0.0);
        let h = photocraft_algo::camera_raw::HSL_BANDS.get(band).copied().unwrap_or(0.0) / 360.0;
        let gradients = [
            [t.scope_hue(left / 360.0, 0.4).lerp_to_gamma(t.dock, 0.35), t.scope_hue(right / 360.0, 0.4).lerp_to_gamma(t.dock, 0.35)],
            [t.text_faint.lerp_to_gamma(t.dock, 0.25), t.scope_hue(h, 0.75).lerp_to_gamma(t.dock, 0.35)],
            [t.scope_hue(h, 0.7).lerp_to_gamma(t.canvas, 0.75), t.scope_hue((h + 0.055).rem_euclid(1.0), 0.45).lerp_to_gamma(t.dock, 0.25)],
        ];
        for (i, (label, value)) in
            [("Hue", &mut p.hsl_hue[band]), ("Saturation", &mut p.hsl_sat[band]), ("Luminance", &mut p.hsl_lum[band])].into_iter().enumerate()
        {
            let r = Rect::from_min_size(top + vec2(d.slider_inset, d.sliders_top + i as f32 * d.row_height), vec2(d.slider_width, d.row_height));
            if let Some(g) = gradients.get(i) {
                dirty |= row(ui, r, label, value, g, d);
            }
        }
    } else {
        let mut child = ui.new_child(
            egui::UiBuilder::new().id_salt("mixer-hsl").max_rect(Rect::from_min_size(top + vec2(0.0, d.swatches_top), vec2(width, height - d.swatches_top))),
        );
        child.horizontal_wrapped(|ui| {
            for (i, label) in ["Hue", "Saturation", "Luminance"].iter().enumerate() {
                if widgets::pill_tab(ui, tr(label), *channel_state == i).clicked() {
                    *channel_state = i;
                }
            }
        });
        let arr = match *channel_state {
            0 => &mut p.hsl_hue,
            1 => &mut p.hsl_sat,
            _ => &mut p.hsl_lum,
        };
        for (k, name) in super::BANDS.iter().enumerate() {
            if let Some(v) = arr.get_mut(k) {
                let grad = [t.text_faint, t.scope_hue(d.hues.get(k).copied().unwrap_or(0.0) / 360.0, 0.7)];
                super::row(&mut child, &mut dirty, name, v, -100.0..=100.0, Some(&grad));
            }
        }
    }
    dirty
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixer_geometry_and_palette_must_be_finite_and_nonoverlapping() {
        Design::default().validate().unwrap();
        for d in [
            Design { sliders_top: 50.0, ..Default::default() },
            Design { swatch_radius: f32::NAN, ..Default::default() },
            Design { hues: [360.0; 8], ..Default::default() },
        ] {
            assert!(d.validate().is_err());
        }
    }
}
