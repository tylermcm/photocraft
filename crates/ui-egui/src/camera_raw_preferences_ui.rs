//! Camera Raw General preferences. Unsupported pages/options remain disabled.
use super::save_options::{Binding, Design, Kind};
use crate::{PhotocraftApp, theme::Tokens};
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::Value;
const KEY: &str = "filter.cameraRaw.preferences";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub single_panel: bool,
    pub show_filenames: bool,
    pub click_zoom: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self { single_panel: false, show_filenames: false, click_zoom: 1.0 }
    }
}
impl Settings {
    fn validate(&self) -> Result<(), String> {
        if !self.click_zoom.is_finite() || ![0.5, 1.0, 2.0].contains(&self.click_zoom) {
            return Err("Camera Raw clickZoom must be 0.5, 1 or 2".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct State {
    pub settings: Settings,
    pub draft: Option<Settings>,
    pub rect: Option<[f32; 4]>,
    pub loaded: bool,
    pub applied: bool,
}
impl State {
    pub(super) fn current(&self) -> &Settings {
        self.draft.as_ref().unwrap_or(&self.settings)
    }
    pub(super) fn close(&mut self) {
        self.draft = None;
        self.rect = None;
    }
}
pub(crate) fn effective(ctx: &egui::Context) -> Option<Settings> {
    ctx.data(|d| d.get_temp(egui::Id::new(KEY)))
}
pub(crate) fn default_design() -> Design {
    serde_json::from_str(include_str!("../../../design/camera-raw/preferences-design.json")).unwrap_or_else(|_| {
        let mut d = Design::default();
        d.elements.clear();
        d
    })
}
pub(crate) fn validate_design(d: &Design) -> Result<(), String> {
    d.validate()?;
    for id in ["general-page", "panel-behavior-field", "filenames-checkbox", "click-zoom-field", "ok-button"] {
        if !d.elements.iter().any(|e| e.id == id) {
            return Err(format!("Camera Raw Preferences is missing {id}"));
        }
    }
    Ok(())
}
pub(super) fn prepare(current: &Settings, value: &Value) -> Result<Settings, String> {
    let input = value.as_object().ok_or("Camera Raw preferences must be an object")?;
    let mut next = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let target = next.as_object_mut().ok_or("invalid Camera Raw preferences")?;
    for (k, v) in input {
        if !target.contains_key(k) {
            return Err(format!("unsupported Camera Raw preference {k}"));
        }
        target.insert(k.clone(), v.clone());
    }
    let next: Settings = serde_json::from_value(next).map_err(|e| e.to_string())?;
    next.validate()?;
    Ok(next)
}
fn apply(app: &mut PhotocraftApp, ctx: &egui::Context, next: Settings) {
    app.ui.camera_raw_workspace.panels.single = next.single_panel;
    if next.single_panel {
        app.ui.camera_raw_workspace.panels.open.truncate(1);
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(KEY), next.clone()));
    app.ui.camera_raw_preferences.settings = next;
    app.ui.camera_raw_preferences.applied = true;
}
pub(super) fn sync(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if app.ui.camera_raw_preferences.loaded {
        return;
    }
    app.ui.camera_raw_preferences.loaded = true;
    ctx.data_mut(|d| d.remove_temp::<Settings>(egui::Id::new(KEY)));
    if let Some(v) = app.session.prefs().dialogs.get(KEY) {
        let parsed = serde_json::from_value::<Settings>(v.clone()).map_err(|e| e.to_string()).and_then(|v| {
            v.validate()?;
            Ok(v)
        });
        match parsed {
            Ok(v) => apply(app, ctx, v),
            Err(e) => {
                app.ui.status_error = true;
                app.ui.status = format!("Invalid Camera Raw preferences: {e}");
            }
        }
    }
}
pub(super) fn confirm(app: &mut PhotocraftApp, ctx: &egui::Context) -> Result<(), String> {
    let next = app.ui.camera_raw_preferences.draft.clone().ok_or("Camera Raw Preferences is not open")?;
    next.validate()?;
    let value = serde_json::to_value(&next).map_err(|e| e.to_string())?;
    apply(app, ctx, next);
    app.session.prefs.edit(|p| p.dialogs.insert(KEY.into(), value));
    app.ui.camera_raw_preferences.close();
    app.ui.camera_raw_workspace.preferences = false;
    Ok(())
}
fn tr(s: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", s)
}
fn text(s: &str) -> String {
    if let Some(base) = s.strip_suffix(':') { format!("{}:", tr(base)) } else { tr(s).into() }
}
fn r(origin: egui::Pos2, v: [f32; 4]) -> Rect {
    Rect::from_min_max(origin + vec2(v[0], v[1]), origin + vec2(v[2], v[3]))
}
fn value(id: &str, b: Binding, p: &Settings) -> String {
    match b {
        Binding::PanelMode => tr(if p.single_panel { "Single" } else { "Multiple" }).into(),
        Binding::PrefZoom => format!("{}%", (p.click_zoom * 100.0).round() as u16),
        Binding::PanelInfo => {
            tr(if p.single_panel { "Single: Opening an edit panel closes the others." } else { "Multiple: Edit panels will stay open until you close them." })
                .into()
        }
        _ => match id {
            "theme-field" => tr("Default"),
            "orientation-field" => tr("Horizontal"),
            _ => return String::new(),
        }
        .into(),
    }
}
fn checked(b: Binding, p: &Settings) -> bool {
    match b {
        Binding::PrefNames => p.show_filenames,
        Binding::PrefRich | Binding::PrefHints => true,
        _ => false,
    }
}
fn draw(ui: &mut Ui, origin: egui::Pos2, d: &Design, p: &mut Settings) -> Option<bool> {
    let t = Tokens::get(ui.ctx());
    let mut result = None;
    if d.elements.is_empty() {
        ui.label(tr("Not available"));
        return ui.button(tr("Cancel")).clicked().then_some(false);
    }
    for e in &d.elements {
        let rect = r(origin, e.rect);
        let font = FontId::proportional(d.font_size);
        let enabled = e.enabled
            && ui.is_enabled()
            && match e.kind {
                Kind::Button => matches!(e.id.as_str(), "ok-button" | "cancel-button" | "general-page"),
                Kind::Dropdown => matches!(e.binding, Binding::PanelMode | Binding::PrefZoom),
                Kind::Check => e.binding == Binding::PrefNames,
                _ => false,
            };
        let ink = if e.enabled { t.text } else { t.camera_raw_dialog_disabled };
        if e.id.ends_with("-page") {
            if e.id == "general-page" {
                ui.painter().rect_filled(rect, 0.0, t.row_selected);
            }
            ui.painter().text(rect.left_center() + vec2(3.0, 0.0), Align2::LEFT_CENTER, text(&e.label), font, ink);
            let response = ui.interact(rect, ui.id().with(&e.id), Sense::hover());
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, enabled, e.id == "general-page", text(&e.label)));
            if !e.enabled {
                response.on_hover_text(tr("Not available"));
            }
            continue;
        }
        if e.id == "theme-swatch" {
            ui.painter().rect_stroke(rect.shrink(2.0), 0.0, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
            continue;
        }
        if e.id == "panel-info-glyph" {
            ui.painter().circle_stroke(rect.center(), 6.0, Stroke::new(1.0, t.icon));
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "i", crate::theme::semibold(11.0), t.icon);
            continue;
        }
        match e.kind {
            Kind::Frame => {
                ui.painter().rect(rect, t.camera_raw_dialog_radius, t.dock, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                let title = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 30.0));
                ui.painter().rect_filled(title, t.camera_raw_dialog_radius, t.camera_raw_dialog_title);
                ui.painter().rect_filled(
                    Rect::from_min_max(title.left_bottom() - vec2(0.0, t.camera_raw_dialog_radius), title.right_bottom()),
                    0.0,
                    t.camera_raw_dialog_title,
                );
                ui.painter().rect_stroke(rect, t.camera_raw_dialog_radius, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
            }
            Kind::Title => {
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, text(&e.label), crate::theme::semibold(d.title_size), t.icon);
            }
            Kind::Group => {
                if e.id == "sidebar-frame" {
                    ui.painter().rect_filled(rect, 0.0, t.field);
                }
                ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
            }
            Kind::Label | Kind::Text => {
                if e.legend {
                    ui.painter().rect_filled(rect, 0.0, t.dock);
                }
                let label = if e.binding == Binding::PanelInfo { value(&e.id, e.binding, p) } else { text(&e.label) };
                ui.painter().with_clip_rect(rect).text(rect.left_center() + vec2(d.label_inset, d.label_offset_y), Align2::LEFT_CENTER, label, font, ink);
            }
            Kind::Dropdown => {
                let response = ui.interact(rect, ui.id().with(&e.id), if enabled { Sense::click() } else { Sense::hover() });
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, enabled, text(&e.label)));
                ui.painter().rect(rect, t.radius_sm, t.field, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                ui.painter().text(
                    rect.left_center() + vec2(if e.id == "theme-field" { 30.0 } else { d.field_inset }, d.field_offset_y),
                    Align2::LEFT_CENTER,
                    value(&e.id, e.binding, p),
                    font,
                    ink,
                );
                let c = pos2(rect.right() - 7.0, rect.center().y);
                ui.painter().add(egui::Shape::line(vec![c + vec2(-2.5, -1.25), c + vec2(0.0, 1.25), c + vec2(2.5, -1.25)], Stroke::new(1.0, ink)));
                if enabled {
                    if response.clicked() {
                        response.request_focus();
                    }
                    egui::Popup::menu(&response).show(|ui| {
                        if e.binding == Binding::PanelMode {
                            for (v, label) in [(false, "Multiple"), (true, "Single")] {
                                if ui.selectable_label(p.single_panel == v, tr(label)).clicked() {
                                    p.single_panel = v;
                                    ui.close();
                                }
                            }
                        } else if e.binding == Binding::PrefZoom {
                            for v in [0.5, 1.0, 2.0] {
                                if ui.selectable_label(p.click_zoom == v, format!("{}%", (v * 100.0) as u16)).clicked() {
                                    p.click_zoom = v;
                                    ui.close();
                                }
                            }
                        }
                    });
                } else {
                    response.on_hover_text(tr("Not available"));
                }
            }
            Kind::Check => {
                let lr = e.label_rect.map(|v| r(origin, v));
                let response = ui.interact(lr.map_or(rect, |lr| lr.union(rect)), ui.id().with(&e.id), if enabled { Sense::click() } else { Sense::hover() });
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, checked(e.binding, p), text(&e.label)));
                if enabled && (response.clicked() || response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Space))) {
                    response.request_focus();
                    if e.binding == Binding::PrefNames {
                        p.show_filenames = !p.show_filenames;
                    }
                }
                let on = checked(e.binding, p);
                let fill = if on { if enabled { t.camera_raw_dialog_check_fill } else { ink } } else { t.field };
                ui.painter().rect(
                    rect,
                    t.radius_sm,
                    fill,
                    Stroke::new(1.0, if enabled { t.camera_raw_dialog_check_border } else { ink }),
                    egui::StrokeKind::Inside,
                );
                if on {
                    ui.painter().add(egui::Shape::line(
                        vec![rect.center() + vec2(-3.0, 0.0), rect.center() + vec2(-0.5, 2.5), rect.center() + vec2(3.5, -2.5)],
                        Stroke::new(1.5, t.dock),
                    ));
                }
                if let Some(lr) = lr {
                    ui.painter().with_clip_rect(lr).text(
                        lr.left_center() + vec2(d.label_inset, d.label_offset_y),
                        Align2::LEFT_CENTER,
                        text(&e.label),
                        font,
                        ink,
                    );
                }
                if !enabled {
                    response.on_hover_text(tr("Not available"));
                }
            }
            Kind::Button => {
                let response = ui.interact(rect, ui.id().with(&e.id), if enabled { Sense::click() } else { Sense::hover() });
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text(&e.label)));
                ui.painter().rect(rect, t.camera_raw_action_radius, t.dock, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, text(&e.label), crate::theme::medium(d.action_size), ink);
                if response.clicked() {
                    result = Some(e.id == "ok-button");
                }
            }
            Kind::Field => {}
        }
    }
    result
}
pub(super) fn show(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.camera_raw_workspace.preferences || app.camera_raw.is_none() {
        app.ui.camera_raw_preferences.close();
        return;
    }
    let design = crate::camera_raw_design::get(ctx);
    let d = &design.preferences;
    if app.ui.camera_raw_preferences.draft.is_none() {
        let mut seed = app.ui.camera_raw_preferences.settings.clone();
        if !app.ui.camera_raw_preferences.applied {
            seed.single_panel = app.ui.camera_raw_workspace.panels.single;
            seed.show_filenames = design.filmstrip.show_filenames;
        }
        app.ui.camera_raw_preferences.draft = Some(seed);
    }
    let state = &mut app.ui.camera_raw_preferences;
    let mut result = None;
    egui::Modal::new(egui::Id::new("camera-raw-preferences")).frame(egui::Frame::NONE).backdrop_color(Tokens::get(ctx).scrim).show(ctx, |ui| {
        ctx.set_sublayer(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("camera-raw-dialog")), ui.layer_id());
        let size = vec2(d.width.min((ctx.content_rect().width() - 16.0).max(100.0)), d.height.min((ctx.content_rect().height() - 16.0).max(100.0)));
        ui.set_width(size.x);
        ui.set_height(size.y);
        egui::ScrollArea::both().max_height(size.y).auto_shrink([false, true]).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(d.width, d.height), Sense::hover());
            state.rect = Some([rect.left(), rect.top(), rect.right(), rect.bottom()]);
            let mut child = ui.new_child(egui::UiBuilder::new().id_salt("preferences-content").max_rect(rect));
            child.set_min_size(rect.size());
            child.set_clip_rect(rect.intersect(ui.clip_rect()));
            if let Some(draft) = state.draft.as_mut() {
                result = draw(&mut child, rect.min, d, draft);
            }
        });
    });
    if result == Some(true) {
        if let Err(e) = confirm(app, ctx) {
            app.ui.status_error = true;
            app.ui.status = e;
        }
    } else if result == Some(false) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.close();
        app.ui.camera_raw_workspace.preferences = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preference_profiles_reject_invalid_zoom_and_unknown_capabilities() {
        let p = Settings::default();
        for v in [
            serde_json::json!({"clickZoom": 0}),
            serde_json::json!({"clickZoom": 1.5}),
            serde_json::json!({"showFilenames": "true"}),
            serde_json::json!({"richTooltips": true}),
        ] {
            assert!(prepare(&p, &v).is_err());
        }
        assert!(Settings { click_zoom: f32::NAN, ..p.clone() }.validate().is_err());
        assert!(Settings { click_zoom: f32::INFINITY, ..p }.validate().is_err());
    }
    #[test]
    fn preferences_require_their_own_controls_and_bounded_geometry() {
        let mut d = default_design();
        validate_design(&d).unwrap();
        d.elements.retain(|e| e.id != "general-page");
        assert!(validate_design(&d).is_err());
        assert!(validate_design(&Design::default()).is_err());
        let mut d = default_design();
        d.width = f32::INFINITY;
        assert!(validate_design(&d).is_err());
    }
}
