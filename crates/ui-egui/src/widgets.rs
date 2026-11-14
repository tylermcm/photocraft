//! Custom widgets for the PhotoCraft look: cards with pill tabs, thin sliders with round knobs,
//! monospace value fields with dimmed units, toggle switches, primary/secondary buttons.

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use crate::theme::{self, Tokens};

mod angle_dial;
mod color_count;
pub use angle_dial::{angle_dial_line, angle_dial_row};
pub use color_count::color_count_row;
mod type_ahead;
pub use type_ahead::close_list as close_dropdown_list;
pub use type_ahead::list_open as dropdown_list_open;

mod buttons;
mod color;
mod dropdown;
pub mod field_tab;
pub mod field_wheel;
mod fields;
mod label_column;
pub(super) mod numeric_expression;
mod panel;
pub mod point_curve;
pub mod press_menu;
mod scrub;
mod slider;
pub mod swap_arrow;
pub(super) mod tab_strip;
pub mod task_bar;
mod text_field;
pub mod text_field_menu;
pub mod tone;
mod units;

pub use buttons::{
    ButtonRole, DialogButton, button_height, button_width, check_box, check_box_fill, checkbox, compact_button, dialog_buttons, primary_button,
    secondary_button, toggle,
};
pub(crate) use color::color_swatch;
pub use color::{
    checker, checker_at, checker_rounded, color_edit_button_rgb, color_edit_button_srgb, color_edit_button_srgba, color_swatch_button, hue_color, hue_stops,
    pick_pos, rgb_histogram,
};
#[cfg(test)]
use color::{checker_tile_id, checker_uv};
pub use dropdown::{
    combo_list, dropdown, dropdown_enabled, dropdown_hovered, dropdown_literal, dropdown_wheel_grouped, dropdown_wheel_grouped_enabled, dropdown_wheel_hovered,
    dropdown_with_tooltips, menu_scroll, swatch_popup,
};
pub(crate) use fields::compact_wrapping_value_field;
#[cfg(test)]
use fields::{FieldOpts, NUMERIC_INPUT_LIMIT, value_field_in};
pub use fields::{
    POPUP_ARROW_W, PopupFieldResponse, VALUE_FIELD_H, compact_value_field, elided_label, elided_label_sensed, popup_value_field, typed_text_parses,
    value_field, value_field_named, value_field_whole, value_field_whole_alerting, value_field_with_trailing,
};
pub use label_column::{LABEL_COLUMN_GAP, column_label, column_label_padded, label_column_width};
pub(crate) use panel::body_margin;
pub use panel::{
    FOOTER_BAR, chevron_icon, drop_line, footer_height, hairline, panel_footer, panel_menu_button, pill_tab, section_label, style_spectrum_popup_menu, surface,
    vline,
};
pub(crate) use scrub::scrub_label_within;
pub use scrub::{scrub_label, scrub_option_icon, scrub_option_label};
#[cfg(test)]
use slider::DOUBLE_CLICK_S;
pub use slider::{RowGestures, row_label, second_click, slider, slider_line, slider_row, slider_row_prec, slider_row_with};
pub(crate) use slider::{count_notches, wheel_notches_of};
#[cfg(test)]
pub(crate) use slider::{curve_fraction, curve_value};
pub use text_field::{
    INPUT_EVENT_LIMIT, TEXT_FIELD_H, TEXT_FIELD_LIMIT, cap_text_events, focus_first_field, text_field, text_field_ex, text_field_with_trailing,
};
pub use units::{
    FieldUnits, fmt_num, fmt_num_prec, fmt_num2, parse_num, set_field_resolution, unit_decimals, unit_of_key, value_field_extent, value_field_fixed,
    value_field_len, value_field_prec, value_field_prec_blank, value_field_units,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "widgets/color_button_tests.rs"]
mod color_button_tests;

#[cfg(test)]
#[path = "widgets/numeric_input_tests.rs"]
mod numeric_input_tests;

pub(crate) fn slider_styled(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, style: SliderRowStyle) -> Response {
    slider::host_slider_with(ui, value, range, None, RowGestures::default(), Some(style))
}

#[allow(clippy::too_many_arguments)]
pub fn slider_row_prec_width(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    gradient: Option<&[Color32]>,
    g: RowGestures,
    decimals: Option<u32>,
    field_width: f32,
) -> Response {
    slider_row_prec_styled(ui, label, value, range, suffix, gradient, g, decimals, field_width, None)
}

#[derive(Clone, Copy, Debug)]
pub struct ValueFieldAppearance {
    pub font_size: f32,
    pub monospace: bool,
    pub left_aligned: bool,
    pub text_padding: f32,
    pub text_offset_y: f32,
    pub box_offset_y: f32,
}

#[derive(Clone, Copy)]
pub struct SliderRowStyle {
    pub field_appearance: ValueFieldAppearance,
    pub field_height: f32,
    pub label_row_height: f32,
    pub track_inset: f32,
    pub track_height: f32,
    pub inner_gap: f32,
    pub bottom_gap: f32,
    pub triangle_handle: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn slider_row_prec_styled(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    gradient: Option<&[Color32]>,
    g: RowGestures,
    decimals: Option<u32>,
    field_width: f32,
    style: Option<SliderRowStyle>,
) -> Response {
    slider_row_prec_styled_trailing(ui, label, value, range, suffix, gradient, g, decimals, field_width, style, |_| {})
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn slider_row_prec_styled_trailing(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    gradient: Option<&[Color32]>,
    g: RowGestures,
    decimals: Option<u32>,
    field_width: f32,
    style: Option<SliderRowStyle>,
    trailing: impl FnOnce(&mut Ui),
) -> Response {
    let t = Tokens::get(ui.ctx());
    let original_gap = ui.spacing().item_spacing.y;
    let original_height = ui.spacing().interact_size.y;
    if let Some(style) = style {
        ui.spacing_mut().item_spacing.y = style.inner_gap;
        ui.spacing_mut().interact_size.y = style.label_row_height;
    }
    let mut changed_resp = None;
    ui.horizontal(|ui| {
        // The label scrubs the row's field, across the row (Photoshop's scrubby labels).
        let l = ui.label(egui::RichText::new(tl!(label)).color(t.text_dim));
        scrub::scrub_label_within(ui, &l, f32::INFINITY);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            trailing(ui);
            changed_resp = Some(
                fields::value_field_box(
                    ui,
                    value,
                    range.clone(),
                    suffix,
                    vec2(field_width.clamp(40.0, 120.0), style.map_or(VALUE_FIELD_H, |s| s.field_height)),
                    0.0,
                    fields::FieldOpts {
                        decimals,
                        keep_zeros: style.is_some(),
                        appearance: style.map(|s| s.field_appearance),
                        step_from_decimals: decimals.is_some(),
                        ..Default::default()
                    },
                    Some(label),
                )
                .0,
            );
        });
    });
    let s = slider::host_slider_with(ui, value, range, gradient, g, style);
    s.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), f64::from(*value), label));
    ui.spacing_mut().item_spacing.y = original_gap;
    ui.spacing_mut().interact_size.y = original_height;
    let mut r = s.clone();
    if let Some(v) = changed_resp {
        if v.changed() {
            r.mark_changed();
        }
        // A scrub of the field or its label is one drag, like the slider's, for history.
        use egui::response::Flags;
        r.flags |= v.flags & (Flags::DRAG_STARTED | Flags::DRAGGED | Flags::DRAG_STOPPED);
    }
    ui.add_space(style.map_or(4.0, |s| s.bottom_gap));
    r
}
