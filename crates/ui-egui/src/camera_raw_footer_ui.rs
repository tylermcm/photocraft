//! ACR workspace footer. Actions return to Camera Raw's existing transaction path.
use egui::{Align2, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::theme::Tokens;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Design {
    pub button_height: f32,
    pub right_padding: f32,
    pub gap: f32,
    pub open_width: f32,
    pub done_width: f32,
    pub cancel_width: f32,
    pub split_width: f32,
    pub offset_y: f32,
    pub font_size: f32,
    pub open_text_offset_x: f32,
    pub cancel_text_offset_x: f32,
}

impl Default for Design {
    fn default() -> Self {
        Self {
            button_height: 28.0,
            right_padding: 18.0,
            gap: 14.0,
            open_width: 82.0,
            done_width: 70.0,
            cancel_width: 76.0,
            split_width: 23.0,
            offset_y: -1.0,
            font_size: 13.0,
            open_text_offset_x: 2.0,
            cancel_text_offset_x: 1.5,
        }
    }
}

impl Design {
    pub(crate) fn validate(&self) -> Result<(), String> {
        for (name, value, lo, hi) in [
            ("buttonHeight", self.button_height, 18.0, 44.0),
            ("rightPadding", self.right_padding, 0.0, 40.0),
            ("gap", self.gap, 0.0, 32.0),
            ("openWidth", self.open_width, 64.0, 160.0),
            ("doneWidth", self.done_width, 50.0, 120.0),
            ("cancelWidth", self.cancel_width, 50.0, 140.0),
            ("splitWidth", self.split_width, 14.0, 32.0),
            ("offsetY", self.offset_y, -8.0, 8.0),
            ("fontSize", self.font_size, 9.0, 24.0),
            ("openTextOffsetX", self.open_text_offset_x, -8.0, 8.0),
            ("cancelTextOffsetX", self.cancel_text_offset_x, -8.0, 8.0),
        ] {
            if !value.is_finite() || !(lo..=hi).contains(&value) {
                return Err(format!("footer.{name} must be in {lo}..={hi}"));
            }
        }
        Ok(())
    }
}

fn button(ui: &mut Ui, rect: Rect, label: &str, x: f32) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let d = crate::camera_raw_design::get(ui.ctx());
    let response = ui.interact(rect, ui.id().with(("acr-footer", label)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    if response.hovered() && ui.is_enabled() {
        ui.painter().rect_filled(rect, rect.height() / 2.0, t.hover);
    }
    ui.painter().text(rect.center() + vec2(x, 0.0), Align2::CENTER_CENTER, label, FontId::proportional(d.footer.font_size), t.text_dim);
    response
}

pub(crate) fn draw(ui: &mut Ui, footer: Rect) -> Option<&'static str> {
    let design = crate::camera_raw_design::get(ui.ctx());
    let d = &design.footer;
    let t = Tokens::get(ui.ctx());
    let height = d.button_height.min(footer.height());
    let right = footer.right() - d.right_padding;
    let center_y = footer.center().y + d.offset_y;
    let open = Rect::from_center_size(pos2(right - d.open_width / 2.0, center_y), vec2(d.open_width, height));
    let done = Rect::from_center_size(pos2(open.left() - d.gap - d.done_width / 2.0, center_y), vec2(d.done_width, height));
    let cancel = Rect::from_center_size(pos2(done.left() - d.gap - d.cancel_width / 2.0, center_y), vec2(d.cancel_width, height));
    let split_x = open.right() - d.split_width;
    let main = Rect::from_min_max(open.min, pos2(split_x, open.bottom()));
    let mut action = None;
    if button(ui, cancel, tl!("Cancel"), d.cancel_text_offset_x).clicked() {
        action = Some("cancel");
    }
    if button(ui, done, tl!("Done"), 0.0).clicked() {
        action = Some("done");
    }
    if button(ui, main, tl!("Open"), d.open_text_offset_x).clicked() {
        action = Some("ok");
    }
    ui.painter().rect_stroke(done, height / 2.0, Stroke::new(1.0, t.camera_raw_footer_border), StrokeKind::Inside);
    ui.painter().rect_stroke(open, height / 2.0, Stroke::new(1.0, t.text_dim), StrokeKind::Inside);
    ui.painter().line_segment([pos2(split_x, open.top()), pos2(split_x, open.bottom())], Stroke::new(1.0, t.text_dim));
    let arrow = Rect::from_min_max(pos2(split_x, open.top()), open.max);
    let response = ui.interact(arrow, ui.id().with("acr-open-options"), Sense::click()).on_hover_text(tl!("Open"));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), tl!("Open")));
    let c = arrow.center();
    ui.painter().add(egui::Shape::line(vec![c + vec2(-4.0, -2.0), c + vec2(0.0, 2.0), c + vec2(4.0, -2.0)], Stroke::new(1.0, t.text_dim)));
    egui::Popup::menu(&response).show(|ui| {
        if ui.button(tl!("Open")).clicked() {
            action = Some("ok");
            ui.close();
        }
        // RAW-backed smart-object continuation is not yet supported. Do not
        // silently flatten the raw and claim that it opened as a raw object.
        ui.add_enabled(false, egui::Button::new(tl!("Open Object"))).on_hover_text(crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", "Not available"));
    });
    if ui.is_enabled() && !ui.ctx().text_edit_focused() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        action = Some("ok");
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_button_positions_keep_cancel_done_and_open_distinct() {
        for (x, expected) in [(1938.0, "cancel"), (2025.0, "done"), (2104.0, "ok")] {
            let ctx = egui::Context::default();
            let mut actual = None;
            for click in [false, true] {
                let mut input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(2174.0, 1301.0))), ..Default::default() };
                if click {
                    let pos = pos2(x, 1270.0);
                    input.events = vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
                    ];
                }
                ctx.run_ui(input, |ui| actual = draw(ui, Rect::from_min_size(pos2(0.0, 1241.0), vec2(2174.0, 60.0)))).textures_delta.clear();
            }
            assert_eq!(actual, Some(expected));
        }
    }
}
