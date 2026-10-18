//! Functional controls for the native Glow and Point Color recipe stages.
use super::CameraRawDialog;
use egui::{Color32, Rect, Response, Ui, pos2, vec2};
use photocraft_algo::camera_raw::{CameraRaw, GlowStyle, PointColor};
use serde::{Deserialize, Serialize};

fn tr(s: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", s)
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct MixerTools {
    pub all_colors: bool,
    pub point_color: bool,
    pub picker: bool,
    pub visualize: bool,
    pub selected: usize,
}
pub fn glow(ui: &mut Ui, p: &mut CameraRaw, expanded: &mut bool) -> bool {
    let mut dirty = super::super::camera_raw_workspace_ui::disclosed_slider(ui, "Glow", &mut p.glow.amount, 0.0..=100.0, expanded);
    if *expanded {
        dirty |= crate::widgets::dropdown(
            ui,
            "glow-style",
            &mut p.glow.style,
            &[(GlowStyle::Diffusion, tr("Diffusion")), (GlowStyle::Bloom, tr("Bloom")), (GlowStyle::Halation, tr("Halation"))],
            180.0,
        );
        let _ = super::slider_default(ui, &mut dirty, "Range", &mut p.glow.range, 0.0..=100.0, None, 50.0);
        let _ = super::slider_default(ui, &mut dirty, "Spread", &mut p.glow.spread, 0.0..=100.0, None, 50.0);
        super::row(ui, &mut dirty, "Warmth", &mut p.glow.warmth, -100.0..=100.0, None);
    }
    dirty
}
pub fn point(ui: &mut Ui, p: &mut CameraRaw, tools: &mut MixerTools) -> bool {
    let mut dirty = false;
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(p.point_colors.len() < 8 || tools.picker, egui::Button::new(tr("Sample Color")).selected(tools.picker)).clicked() {
            tools.picker = !tools.picker;
        }
        for (i, s) in p.point_colors.iter().enumerate() {
            let (rect, r) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
            let c = s.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            let t = crate::theme::Tokens::get(ui.ctx());
            ui.painter().circle_filled(rect.center(), 7.0, Color32::from_rgb(c[0], c[1], c[2]));
            if tools.selected == i {
                ui.painter().circle_stroke(rect.center(), 9.0, egui::Stroke::new(1.0, t.text));
            }
            r.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), tools.selected == i, format!("{} {}", tr("Point Color"), i + 1))
            });
            if r.clicked() {
                tools.selected = i;
            }
        }
    });
    tools.selected = tools.selected.min(p.point_colors.len().saturating_sub(1));
    if p.point_colors.is_empty() {
        ui.label(tr("Point Color"));
        return false;
    }
    if ui.button(tr("Delete Sample")).clicked() {
        p.point_colors.remove(tools.selected);
        tools.selected = tools.selected.saturating_sub(1);
        return true;
    }
    if let Some(s) = p.point_colors.get_mut(tools.selected) {
        super::row(ui, &mut dirty, "Hue", &mut s.hue, -100.0..=100.0, None);
        super::row(ui, &mut dirty, "Saturation", &mut s.saturation, -100.0..=100.0, None);
        super::row(ui, &mut dirty, "Luminance", &mut s.luminance, -100.0..=100.0, None);
        super::row(ui, &mut dirty, "Variance", &mut s.variance, -100.0..=100.0, None);
        let _ = super::slider_default(ui, &mut dirty, "Range", &mut s.range, 0.0..=100.0, None, 50.0);
    }
    ui.checkbox(&mut tools.visualize, tr("Visualize Range"));
    dirty
}
pub struct RangePreview {
    key: String,
    tex: egui::TextureHandle,
}
fn input_pixels(d: &CameraRawDialog) -> Vec<[f32; 4]> {
    let mut params = d.params.clone();
    params.point_colors.clear();
    let mut px = d.proxy.clone();
    let scale = (d.pw as f32 / d.full_w.max(1) as f32).min(1.0);
    crate::raw_recipe::apply(&mut px, d.pw, d.ph, &params, d.raw_curve.as_deref(), scale, d.float);
    px
}
pub fn preview(ui: &Ui, d: &mut CameraRawDialog, image: Rect, response: &Response, tools: &mut MixerTools, panning: bool) {
    if !tools.point_color {
        tools.picker = false;
        return;
    }
    if tools.picker {
        response.clone().on_hover_cursor(egui::CursorIcon::Crosshair);
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            tools.picker = false;
            return;
        }
        if !panning
            && response.clicked_by(egui::PointerButton::Primary)
            && let Some(at) = response.interact_pointer_pos().filter(|p| image.contains(*p))
            && d.pw > 0
            && d.ph > 0
            && d.params.point_colors.len() < 8
        {
            let x = (((at.x - image.left()) / image.width().max(1.0) * d.pw as f32).floor() as usize).min(d.pw - 1);
            let y = (((at.y - image.top()) / image.height().max(1.0) * d.ph as f32).floor() as usize).min(d.ph - 1);
            let px = input_pixels(d);
            if let Some(q) = y.checked_mul(d.pw).and_then(|v| v.checked_add(x)).and_then(|i| px.get(i)).filter(|q| q[3] > 0.0) {
                let color = [q[0], q[1], q[2]].map(|v| v.clamp(0.0, 1.0));
                d.params.point_colors.push(PointColor { color, ..Default::default() });
                tools.selected = d.params.point_colors.len() - 1;
                tools.picker = false;
                d.dirty = true;
            }
        }
    }
    if !tools.visualize || d.show_before {
        return;
    }
    let Some(sample) = d.params.point_colors.get(tools.selected) else {
        return;
    };
    let key = format!("{}:{}:{}:{:?}", d.preview_revision, d.display.key, tools.selected, sample);
    if d.mixer_preview.as_ref().is_none_or(|p| p.key != key) {
        let input = input_pixels(d);
        let displayed = CameraRawDialog::image(&input, d.pw, d.ph, &d.display);
        let pixels = input
            .into_iter()
            .zip(displayed.pixels)
            .enumerate()
            .map(|(i, (q, display))| {
                let k = sample.weight([q[0], q[1], q[2]]) * d.selection.as_ref().and_then(|v| v.get(i)).copied().unwrap_or(1.0);
                let display = display.to_array();
                let l = 0.2126 * display[0] as f32 + 0.7152 * display[1] as f32 + 0.0722 * display[2] as f32;
                let ch = |i: usize| (l + (display[i] as f32 - l) * k).round().clamp(0.0, 255.0) as u8;
                Color32::from_rgba_premultiplied(ch(0), ch(1), ch(2), display[3])
            })
            .collect();
        let tex = ui.ctx().load_texture("point-color-range", egui::ColorImage::new([d.pw, d.ph], pixels), egui::TextureOptions::LINEAR);
        d.mixer_preview = Some(RangePreview { key, tex });
    }
    if let Some(p) = &d.mixer_preview {
        ui.painter().image(p.tex.id(), image, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}
