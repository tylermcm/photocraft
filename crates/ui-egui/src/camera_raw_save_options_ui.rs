//! Camera Raw Save Options: measured native presentation and transactional export draft.
//! DNG encoding is unavailable. No setting here changes the photo or writes a file.
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{PhotocraftApp, theme::Tokens};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NamePart {
    #[default]
    None,
    DocumentName,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub naming: [NamePart; 4],
    pub uppercase_extension: bool,
    pub fast_load: bool,
    pub lossy: bool,
    pub embed_original: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            naming: [NamePart::DocumentName, NamePart::None, NamePart::None, NamePart::None],
            uppercase_extension: false,
            fast_load: true,
            lossy: false,
            embed_original: false,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct State {
    pub settings: Settings,
    pub draft: Option<Settings>,
    pub rect: Option<[f32; 4]>,
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
pub(super) fn prepare(current: &Settings, value: &Value) -> Result<Settings, String> {
    let fields = value.as_object().ok_or("Camera Raw saveOptions must be an object")?;
    let mut next = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let dst = next.as_object_mut().ok_or("invalid Save Options draft")?;
    for (key, v) in fields {
        if !dst.contains_key(key) {
            return Err(format!("unknown Save Options property {key}"));
        }
        dst.insert(key.clone(), v.clone());
    }
    serde_json::from_value(next).map_err(|e| format!("invalid Save Options draft: {e}"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Frame,
    Title,
    Group,
    Label,
    Text,
    Dropdown,
    Check,
    Button,
    #[default]
    Field,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Binding {
    #[default]
    None,
    Preset,
    Destination,
    Naming0,
    Naming1,
    Naming2,
    Naming3,
    Extension,
    Format,
    Compatibility,
    JpegPreview,
    FastLoad,
    Lossy,
    LossyCount,
    EmbedOriginal,
    CredentialsStorage,
    EditsActivity,
    FileExample,
    Folder,
    Account,
    Connected,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Element {
    pub id: String,
    pub kind: Kind,
    pub rect: [f32; 4],
    pub label: String,
    pub label_rect: Option<[f32; 4]>,
    pub text_offset: Option<[f32; 2]>,
    pub binding: Binding,
    pub enabled: bool,
    pub legend: bool,
    pub centered: bool,
}
impl Default for Element {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: Kind::Field,
            rect: [0.0; 4],
            label: String::new(),
            label_rect: None,
            text_offset: None,
            binding: Binding::None,
            enabled: false,
            legend: false,
            centered: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Design {
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub title_size: f32,
    pub action_size: f32,
    pub label_inset: f32,
    pub label_offset_y: f32,
    pub field_inset: f32,
    pub field_offset_y: f32,
    pub elements: Vec<Element>,
}
impl Default for Design {
    fn default() -> Self {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Definition {
            width: f32,
            height: f32,
            font_size: f32,
            title_size: f32,
            action_size: f32,
            label_inset: f32,
            label_offset_y: f32,
            field_inset: f32,
            field_offset_y: f32,
            elements: Vec<Element>,
        }
        match serde_json::from_str::<Definition>(include_str!("../../../design/camera-raw/save-options-design.json")) {
            Ok(v) => Self {
                width: v.width,
                height: v.height,
                font_size: v.font_size,
                title_size: v.title_size,
                action_size: v.action_size,
                label_inset: v.label_inset,
                label_offset_y: v.label_offset_y,
                field_inset: v.field_inset,
                field_offset_y: v.field_offset_y,
                elements: v.elements,
            },
            Err(_) => Self {
                width: 787.0,
                height: 1128.0,
                font_size: 11.0,
                title_size: 13.0,
                action_size: 13.0,
                label_inset: 3.0,
                label_offset_y: -1.5,
                field_inset: 8.0,
                field_offset_y: -1.5,
                elements: Vec::new(),
            },
        }
    }
}
impl Design {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite()
            || !(640.0..=1000.0).contains(&self.width)
            || !self.height.is_finite()
            || !(800.0..=1400.0).contains(&self.height)
            || !self.font_size.is_finite()
            || !(9.0..=20.0).contains(&self.font_size)
            || !self.title_size.is_finite()
            || !(10.0..=24.0).contains(&self.title_size)
            || self.elements.len() > 128
        {
            return Err("Save Options design dimensions or element count are out of bounds".into());
        }
        for (v, lo, hi) in [
            (self.action_size, 9.0, 24.0),
            (self.label_inset, 0.0, 24.0),
            (self.label_offset_y, -8.0, 8.0),
            (self.field_inset, 0.0, 24.0),
            (self.field_offset_y, -8.0, 8.0),
        ] {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                return Err("invalid Save Options text style".into());
            }
        }
        for (i, e) in self.elements.iter().enumerate() {
            if e.id.is_empty()
                || e.id.len() > 64
                || !e.id.is_ascii()
                || e.label.len() > 256
                || self.elements.iter().take(i).any(|old| old.id == e.id)
                || !self.valid_rect(e.rect)
                || e.label_rect.is_some_and(|r| !self.valid_rect(r))
                || e.text_offset.is_some_and(|v| v.iter().any(|x| !x.is_finite() || !(-8.0..=24.0).contains(x)))
            {
                return Err(format!("invalid Save Options element {}", e.id));
            }
        }
        for (id, kind) in [("window-frame", Kind::Frame), ("title", Kind::Title), ("save-button", Kind::Button), ("cancel-button", Kind::Button)] {
            if !self.elements.iter().any(|e| e.id == id && e.kind == kind) {
                return Err(format!("Save Options design requires {id}"));
            }
        }
        Ok(())
    }
    fn valid_rect(&self, r: [f32; 4]) -> bool {
        r.iter().all(|v| v.is_finite()) && r[0] >= 0.0 && r[1] >= 0.0 && r[2] > r[0] && r[3] > r[1] && r[2] <= self.width && r[3] <= self.height
    }
}
fn tr(s: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", s)
}
fn label(s: &str) -> String {
    if let Some(base) = s.strip_suffix(':') {
        format!("{}:", tr(base))
    } else if let Some(base) = s.strip_suffix('…') {
        format!("{}…", tr(base))
    } else {
        tr(s).into()
    }
}
fn rect(origin: egui::Pos2, r: [f32; 4]) -> Rect {
    Rect::from_min_max(origin + vec2(r[0], r[1]), origin + vec2(r[2], r[3]))
}
fn naming_index(binding: Binding) -> Option<usize> {
    match binding {
        Binding::Naming0 => Some(0),
        Binding::Naming1 => Some(1),
        Binding::Naming2 => Some(2),
        Binding::Naming3 => Some(3),
        _ => None,
    }
}
fn value(binding: Binding, p: &Settings, name: &str, folder: &str) -> String {
    if let Some(i) = naming_index(binding) {
        return if p.naming.get(i) == Some(&NamePart::DocumentName) { tr("Document Name").into() } else { String::new() };
    }
    match binding {
        Binding::Preset => tr("Custom").into(),
        Binding::Destination => tr("Save in Same Location").into(),
        Binding::Extension => if p.uppercase_extension { ".DNG" } else { ".dng" }.into(),
        Binding::Format => tr("Digital Negative").into(),
        Binding::Compatibility => tr("Camera Raw 16.0 and later").into(),
        Binding::JpegPreview => tr("Medium Size").into(),
        Binding::LossyCount => tr("Preserve Pixel Count").into(),
        Binding::CredentialsStorage => tr("Publish to Content Credentials cloud").into(),
        Binding::Account => format!("{} —", label("Adobe account name:")),
        Binding::Connected => format!("{} {}  ({})", label("Connected accounts:"), tr("None"), tr("Manage")),
        Binding::Folder => folder.into(),
        Binding::FileExample => {
            let stem = p.naming.iter().filter(|v| **v == NamePart::DocumentName).map(|_| name).collect::<String>();
            format!("{}{}", stem, if p.uppercase_extension { ".DNG" } else { ".dng" })
        }
        _ => String::new(),
    }
}
fn checked(binding: Binding, p: &Settings) -> bool {
    match binding {
        Binding::FastLoad => p.fast_load,
        Binding::Lossy => p.lossy,
        Binding::EmbedOriginal => p.embed_original,
        Binding::EditsActivity => true,
        _ => false,
    }
}
fn set_checked(binding: Binding, p: &mut Settings, v: bool) {
    match binding {
        Binding::FastLoad => p.fast_load = v,
        Binding::Lossy => p.lossy = v,
        Binding::EmbedOriginal => p.embed_original = v,
        _ => {}
    }
}
fn enabled(e: &Element) -> bool {
    e.enabled
        && match e.kind {
            Kind::Button => e.id == "cancel-button",
            Kind::Check => matches!(e.binding, Binding::FastLoad | Binding::Lossy | Binding::EmbedOriginal),
            Kind::Dropdown => e.binding == Binding::Extension || naming_index(e.binding).is_some(),
            _ => false,
        }
}
fn draw(ui: &mut Ui, origin: egui::Pos2, d: &Design, p: &mut Settings, name: &str, folder: &str) -> bool {
    if d.elements.is_empty() {
        ui.label(tr("Not available"));
        return ui.button(tr("Cancel")).clicked();
    }
    let t = Tokens::get(ui.ctx());
    let mut close = false;
    for e in &d.elements {
        let r = rect(origin, e.rect);
        let active = enabled(e) && ui.is_enabled();
        let visually_enabled = e.enabled && (e.binding != Binding::LossyCount || p.lossy);
        let ink = if visually_enabled { t.text } else { t.camera_raw_dialog_disabled };
        let font = FontId::proportional(d.font_size);
        match e.kind {
            Kind::Frame => {
                ui.painter().rect(r, t.camera_raw_dialog_radius, t.dock, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                let title = Rect::from_min_max(r.min, pos2(r.right(), r.top() + 30.0));
                ui.painter().rect_filled(title, t.camera_raw_dialog_radius, t.camera_raw_dialog_title);
                ui.painter().rect_filled(
                    Rect::from_min_max(title.left_bottom() - vec2(0.0, t.camera_raw_dialog_radius), title.right_bottom()),
                    0.0,
                    t.camera_raw_dialog_title,
                );
                ui.painter().line_segment([title.left_bottom(), title.right_bottom()], Stroke::new(1.0, t.separator));
                ui.painter().rect_stroke(r, t.camera_raw_dialog_radius, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
            }
            Kind::Title => {
                ui.painter().text(
                    r.center() + e.text_offset.map_or(egui::Vec2::ZERO, |v| vec2(v[0], v[1])),
                    Align2::CENTER_CENTER,
                    label(&e.label),
                    crate::theme::semibold(d.title_size),
                    t.icon,
                );
            }
            Kind::Group => {
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
            }
            Kind::Label | Kind::Text => {
                if e.legend {
                    ui.painter().rect_filled(r, 0.0, t.dock);
                }
                let text = if e.binding == Binding::None { label(&e.label) } else { value(e.binding, p, name, folder) };
                ui.painter().with_clip_rect(r.intersect(ui.clip_rect())).text(
                    if e.centered { r.center() } else { r.left_center() + e.text_offset.map_or(vec2(d.label_inset, d.label_offset_y), |v| vec2(v[0], v[1])) },
                    if e.centered { Align2::CENTER_CENTER } else { Align2::LEFT_CENTER },
                    text,
                    font,
                    ink,
                );
            }
            Kind::Dropdown | Kind::Field => {
                let response = ui.interact(r, ui.id().with(&e.id), if active { Sense::click() } else { Sense::hover() });
                let caption = naming_index(e.binding).map_or_else(|| label(&e.label), |i| format!("{} {}", label(&e.label), i + 1));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(if e.kind == Kind::Dropdown { egui::WidgetType::ComboBox } else { egui::WidgetType::TextEdit }, active, &caption)
                });
                let fill = if active && response.hovered() { t.hover } else { t.field };
                ui.painter().rect(
                    r,
                    t.radius_sm,
                    fill,
                    Stroke::new(1.0, if visually_enabled { t.field_border } else { t.field_border.gamma_multiply(0.35) }),
                    egui::StrokeKind::Inside,
                );
                let text = value(e.binding, p, name, folder);
                ui.painter().with_clip_rect(r.shrink(2.0)).text(
                    r.left_center() + e.text_offset.map_or(vec2(d.field_inset, d.field_offset_y), |v| vec2(v[0], v[1])),
                    Align2::LEFT_CENTER,
                    text,
                    font,
                    ink,
                );
                if e.kind == Kind::Dropdown {
                    let c = pos2(r.right() - 7.0, r.center().y);
                    ui.painter().add(egui::Shape::line(vec![c + vec2(-2.5, -1.25), c + vec2(0.0, 1.25), c + vec2(2.5, -1.25)], Stroke::new(1.0, ink)));
                }
                if active {
                    if response.clicked() {
                        response.request_focus();
                    }
                    egui::Popup::menu(&response).show(|ui| {
                        if let Some(i) = naming_index(e.binding) {
                            for (part, text) in [(NamePart::None, "None"), (NamePart::DocumentName, "Document Name")] {
                                if ui.selectable_label(p.naming.get(i) == Some(&part), tr(text)).clicked() {
                                    if let Some(v) = p.naming.get_mut(i) {
                                        *v = part;
                                    }
                                    ui.close();
                                }
                            }
                        } else if e.binding == Binding::Extension {
                            for (upper, text) in [(false, ".dng"), (true, ".DNG")] {
                                if ui.selectable_label(p.uppercase_extension == upper, text).clicked() {
                                    p.uppercase_extension = upper;
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
                let lr = e.label_rect.map(|v| rect(origin, v));
                let hit = lr.map_or(r, |v| v.union(r));
                let response = ui.interact(hit, ui.id().with(&e.id), if active { Sense::click() } else { Sense::hover() });

                let caption = if matches!(e.binding, Binding::Account | Binding::Connected) { value(e.binding, p, name, folder) } else { label(&e.label) };
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, active, checked(e.binding, p), &caption));
                if response.clicked() || active && response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Space)) {
                    response.request_focus();
                    set_checked(e.binding, p, !checked(e.binding, p));
                }
                let on = checked(e.binding, p);
                ui.painter().rect(
                    r,
                    t.radius_sm,
                    if on { if e.enabled { t.camera_raw_dialog_check_fill } else { ink } } else { if e.enabled { t.field } else { t.dock } },
                    Stroke::new(
                        1.0,
                        if on && e.enabled {
                            t.camera_raw_dialog_check_fill
                        } else if e.enabled {
                            t.camera_raw_dialog_check_border
                        } else {
                            t.camera_raw_dialog_disabled
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                if on {
                    ui.painter().add(egui::Shape::line(
                        vec![r.center() + vec2(-3.0, 0.0), r.center() + vec2(-0.5, 2.5), r.center() + vec2(3.5, -2.5)],
                        Stroke::new(1.5, t.dock),
                    ));
                }
                if let Some(lr) = lr {
                    ui.painter().with_clip_rect(lr.intersect(ui.clip_rect())).text(
                        lr.left_center() + e.text_offset.map_or(vec2(d.label_inset, d.label_offset_y), |v| vec2(v[0], v[1])),
                        Align2::LEFT_CENTER,
                        caption,
                        font,
                        ink,
                    );
                }
                if !active {
                    response.on_hover_text(tr("Not available"));
                }
            }
            Kind::Button => {
                let response = ui.interact(r, ui.id().with(&e.id), if active { Sense::click() } else { Sense::hover() });
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, active, label(&e.label)));
                ui.painter().rect(
                    r,
                    if e.id == "select-folder" { t.radius_sm } else { t.camera_raw_action_radius },
                    if active && response.hovered() { t.hover } else { t.dock },
                    Stroke::new(1.0, ink),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    label(&e.label),
                    if e.id == "select-folder" { font } else { crate::theme::medium(d.action_size) },
                    ink,
                );
                if response.clicked() && e.id == "cancel-button" {
                    close = true;
                }
                if !active {
                    response.on_hover_text(tr("Not available"));
                }
            }
        }
    }
    close
}
pub(super) fn show(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.camera_raw_workspace.save_options || app.camera_raw.is_none() {
        app.ui.camera_raw_save_options.close();
        return;
    }
    let name = app
        .camera_raw
        .as_ref()
        .and_then(|d| d.raw_open.as_ref())
        .map(|r| r.name.as_str())
        .or_else(|| app.session.active().map(|s| s.doc.name.as_str()))
        .unwrap_or("Untitled");
    let stem = std::path::Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or(name).chars().take(128).collect::<String>();
    let folder = app
        .camera_raw
        .as_ref()
        .and_then(|d| d.raw_open.as_ref())
        .and_then(|r| r.path.as_deref())
        .or_else(|| app.session.active().and_then(|s| s.path.as_deref()))
        .and_then(|s| std::path::Path::new(s).parent())
        .and_then(|p| p.to_str())
        .unwrap_or("—")
        .chars()
        .take(512)
        .collect::<String>();
    let design = crate::camera_raw_design::get(ctx);
    let d = &design.save_options;
    let state = &mut app.ui.camera_raw_save_options;
    let draft = state.draft.get_or_insert_with(|| state.settings.clone());
    let mut close = false;
    egui::Modal::new(egui::Id::new("camera-raw-save-options")).frame(egui::Frame::NONE).backdrop_color(Tokens::get(ctx).scrim).show(ctx, |ui| {
        ctx.set_sublayer(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("camera-raw-dialog")), ui.layer_id());
        ui.set_width(d.width.min((ctx.content_rect().width() - 16.0).max(100.0)));
        let h = (ctx.content_rect().height() - 16.0).max(100.0).min(d.height);
        ui.set_height(h);
        egui::ScrollArea::both().max_height(h).auto_shrink([false, true]).show(ui, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(d.width, d.height), Sense::hover());
            state.rect = Some([r.left(), r.top(), r.right(), r.bottom()]);
            let mut content = ui.new_child(egui::UiBuilder::new().id_salt("save-options-content").max_rect(r));
            content.set_min_size(r.size());
            content.set_clip_rect(r.intersect(ui.clip_rect()));
            close = draw(&mut content, r.min, d, draft, &stem, &folder);
        });
    });
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.close();
        app.ui.camera_raw_workspace.save_options = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_default_is_valid_and_geometry_rejects_hostile_values() {
        let d = Design::default();
        assert!(d.validate().is_ok());
        assert_eq!(d.elements.len(), 72);
        let explicit: Design = serde_json::from_str(include_str!("../../../design/camera-raw/save-options-design.json")).unwrap();
        assert_eq!(d, explicit);
        for x in [f32::NAN, f32::INFINITY, -1.0, 2001.0] {
            let mut bad = d.clone();
            bad.width = x;
            assert!(bad.validate().is_err());
        }
        let mut bad = d.clone();
        bad.elements.first_mut().unwrap().rect = [0.0, 0.0, 0.0, 0.0];
        assert!(bad.validate().is_err());
        let mut duplicate = d.clone();
        duplicate.elements.push(d.elements.first().unwrap().clone());
        assert!(duplicate.validate().is_err());
        let mut missing = d.clone();
        missing.elements.retain(|e| e.id != "cancel-button");
        assert!(missing.validate().is_err());
    }
    #[test]
    fn geometry_cannot_turn_save_into_an_available_output_action() {
        let mut save = Design::default().elements.into_iter().find(|e| e.id == "save-button").unwrap();
        save.enabled = true;
        save.binding = Binding::FastLoad;
        assert!(!enabled(&save));
        let current = Settings::default();
        assert!(prepare(&current, &serde_json::json!({"account":"synthetic-account"})).is_err());
        assert_eq!(current, Settings::default());
    }
}
