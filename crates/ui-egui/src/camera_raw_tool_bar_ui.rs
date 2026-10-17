//! Temporary Grid / Color Sampler bars. These controls change view state only.
use egui::{Align2, Rect, Ui, pos2, vec2};

use crate::{camera_raw_workspace_ui::WorkspaceState, state::CameraRawScopeState, theme::Tokens, widgets};

#[derive(Clone, Copy)]
enum Kind {
    None,
    Grid,
    Sampler,
}

fn kind(ctx: &egui::Context) -> Kind {
    ctx.data(|d| d.get_temp(egui::Id::new("camera-raw-tool-bar")))
        .unwrap_or_else(|| if crate::camera_raw_design::get(ctx).preview.as_ref().is_some_and(|v| v.grid) { Kind::Grid } else { Kind::None })
}

pub(crate) fn sync(ctx: &egui::Context, workspace: &WorkspaceState, scope: &CameraRawScopeState) {
    let value = if scope.sampler_tool {
        Kind::Sampler
    } else if workspace.grid {
        Kind::Grid
    } else {
        Kind::None
    };
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("camera-raw-tool-bar"), value));
}

pub(crate) fn height(ctx: &egui::Context) -> f32 {
    let design = crate::camera_raw_design::get(ctx);
    match kind(ctx) {
        Kind::None => 0.0,
        Kind::Grid => design.grid_bar_height,
        Kind::Sampler => design.sampler_bar_height,
    }
}

fn icon(ui: &mut Ui, rect: Rect, name: &str, label: &str, enabled: bool) -> bool {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(label).max_rect(rect));
    child.add_enabled_ui(enabled, |ui| crate::icons::button_with_icon_size(ui, name, rect.width(), 16.0, false, label).clicked()).inner
}

pub(crate) fn draw(ui: &mut Ui, rect: Rect, workspace: &mut WorkspaceState, scope: &mut CameraRawScopeState) {
    if !rect.is_positive() {
        return;
    }
    let design = crate::camera_raw_design::get(ui.ctx());
    let t = Tokens::get(ui.ctx());
    ui.painter().rect_filled(rect, 0.0, t.dock);
    let font = crate::theme::medium(design.tool_bar_font_size);
    let sampler = matches!(kind(ui.ctx()), Kind::Sampler);
    let y = if sampler { rect.top() + 15.0 } else { rect.center().y };
    if icon(ui, Rect::from_center_size(pos2(rect.right() - 17.0, y), vec2(20.0, 20.0)), "x", tl!("Close"), true) {
        if sampler {
            scope.sampler_tool = false;
            scope.new_sampler = false;
        } else {
            workspace.grid = false;
        }
        ui.ctx().request_repaint();
    }
    if sampler {
        ui.painter().text(
            rect.min + vec2(16.0, 15.0),
            Align2::LEFT_CENTER,
            tl!("Color Sampler"),
            crate::theme::medium(design.tool_bar_font_size + 2.0),
            t.text_dim,
        );
        if icon(ui, Rect::from_center_size(rect.min + vec2(142.0, 15.0), vec2(20.0, 20.0)), "undo-2", tl!("Reset"), !scope.samplers.is_empty()) {
            scope.samplers.clear();
            scope.new_sampler = false;
        }
        let x = rect.left() + 165.0;
        ui.painter().line_segment([pos2(x, rect.top() + 9.0), pos2(x, rect.bottom() - 9.0)], egui::Stroke::new(1.0, t.field_border));
        ui.painter().text(rect.min + vec2(16.0, 39.0), Align2::LEFT_CENTER, format!("{}:", tl!("New Sample")), font, t.text_dim);
        if icon(
            ui,
            Rect::from_center_size(rect.min + vec2(104.0, 39.0), vec2(20.0, 20.0)),
            "pipette",
            tl!("New Sample"),
            scope.samplers.len() < crate::camera_raw_scope_ui::MAX_SAMPLERS,
        ) {
            scope.new_sampler = true;
        }
    } else {
        let left = rect.center().x - 170.0;
        for (label, x, value, range) in
            [(tl!("Grid Size"), left, &mut workspace.grid_size, 12.0..=200.0), (tl!("Opacity"), left + 186.0, &mut workspace.grid_opacity, 0.0..=1.0)]
        {
            ui.painter().text(pos2(x, y), Align2::LEFT_CENTER, label, font.clone(), t.text_dim);
            let slider = Rect::from_min_size(pos2(x + if label == tl!("Grid Size") { 56.0 } else { 49.0 }, y - 10.0), vec2(100.0, 20.0));
            let mut child = ui.new_child(egui::UiBuilder::new().id_salt(label).max_rect(slider));
            child.set_width(slider.width());
            let mut style = design.slider_style();
            style.track_height = 20.0;
            style.track_inset = 0.0;
            widgets::slider_styled(&mut child, value, range, style).on_hover_text(label);
        }
    }
    ui.painter().rect_filled(Rect::from_min_max(pos2(rect.left(), rect.bottom() - 1.0), rect.max), 0.0, t.separator);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, workspace: &mut WorkspaceState, scope: &mut CameraRawScopeState, click: Option<egui::Pos2>) {
        sync(ctx, workspace, scope);
        let mut input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 200.0))), ..Default::default() };
        if let Some(pos) = click {
            input.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
            ];
        }
        ctx.run_ui(input, |ui| draw(ui, Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, height(ctx))), workspace, scope)).textures_delta.clear();
    }

    #[test]
    fn grid_bar_changes_density_and_closes_without_changing_opacity() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        crate::theme::apply(&ctx, crate::theme::ThemeKind::ProMedium);
        let mut workspace = WorkspaceState { grid: true, ..Default::default() };
        let mut scope = CameraRawScopeState::default();
        frame(&ctx, &mut workspace, &mut scope, None);
        assert_eq!(height(&ctx), 40.0);
        let opacity = workspace.grid_opacity;
        frame(&ctx, &mut workspace, &mut scope, Some(pos2(360.0, 20.0)));
        assert!(workspace.grid_size > 100.0);
        assert_eq!(workspace.grid_opacity, opacity);
        frame(&ctx, &mut workspace, &mut scope, Some(pos2(783.0, 20.0)));
        assert!(!workspace.grid);
    }

    #[test]
    fn sampler_bar_arms_a_new_point_resets_readouts_and_closes() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        crate::theme::apply(&ctx, crate::theme::ThemeKind::ProMedium);
        let mut workspace = WorkspaceState::default();
        let mut scope = CameraRawScopeState { sampler_tool: true, samplers: vec![[0.3, 0.3]], ..Default::default() };
        frame(&ctx, &mut workspace, &mut scope, None);
        assert_eq!(height(&ctx), 55.0);
        frame(&ctx, &mut workspace, &mut scope, Some(pos2(104.0, 39.0)));
        assert!(scope.new_sampler);
        frame(&ctx, &mut workspace, &mut scope, Some(pos2(142.0, 15.0)));
        assert!(scope.samplers.is_empty() && !scope.new_sampler && scope.sampler_tool);
        frame(&ctx, &mut workspace, &mut scope, Some(pos2(783.0, 15.0)));
        assert!(!scope.sampler_tool);
    }
}
