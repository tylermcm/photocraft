//! Screenshot-measured Camera Raw settings menu. Recipe edits use the dialog's transaction path.
use crate::{PhotocraftApp, theme::Tokens};
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use photocraft_algo::camera_raw::CameraRaw;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct State {
    pub previous: Option<CameraRaw>,
    pub clipboard_ready: bool,
    pub paste_pending: bool,
    pub paste_deadline: Option<f64>,
    pub rect: Option<[f32; 4]>,
    pub snapshot_picker: bool,
    pub focus: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Design {
    pub width: f32,
    pub row_height: f32,
    pub group_gap: f32,
    pub top_padding: f32,
    pub bottom_padding: f32,
    pub text_inset: f32,
    pub shortcut_inset: f32,
    pub font_size: f32,
    pub text_offset_y: f32,
    pub radius: f32,
    pub separator_offset: f32,
}
impl Default for Design {
    fn default() -> Self {
        Self {
            width: 269.0,
            row_height: 22.0,
            group_gap: 11.0,
            top_padding: 6.5,
            bottom_padding: 3.5,
            text_inset: 14.0,
            shortcut_inset: 13.0,
            font_size: 12.65,
            text_offset_y: -1.5,
            radius: 6.0,
            separator_offset: 4.0,
        }
    }
}
impl Design {
    pub(crate) fn validate(&self) -> Result<(), String> {
        for (key, v, lo, hi) in [
            ("width", self.width, 240.0, 600.0),
            ("rowHeight", self.row_height, 18.0, 40.0),
            ("groupGap", self.group_gap, 4.0, 24.0),
            ("topPadding", self.top_padding, 0.0, 20.0),
            ("bottomPadding", self.bottom_padding, 0.0, 20.0),
            ("textInset", self.text_inset, 4.0, 32.0),
            ("shortcutInset", self.shortcut_inset, 4.0, 32.0),
            ("fontSize", self.font_size, 9.0, 24.0),
            ("textOffsetY", self.text_offset_y, -8.0, 8.0),
            ("radius", self.radius, 0.0, 16.0),
            ("separatorOffset", self.separator_offset, 0.0, 12.0),
        ] {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                return Err(format!("settingsMenu.{key} must be in {lo}..={hi}"));
            }
        }
        Ok(())
    }
    fn height(&self) -> f32 {
        self.top_padding + self.bottom_padding + 22.0 * self.row_height + 11.0 * self.group_gap
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    ResetOpen,
    ResetDefault,
    Previous,
    Snapshot,
    Copy,
    Paste,
    Load,
    Save,
    SnapshotPicker,
}

fn item(ui: &mut Ui, rect: Rect, label: &str, shortcut: &str, enabled: bool, submenu: bool, d: &Design) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let r = ui.interact(rect, ui.id().with(label), if enabled { Sense::click() } else { Sense::hover() });
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if enabled && (r.hovered() || r.has_focus()) {
        ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 3.0, t.hover);
    }
    let ink = if enabled { t.camera_raw_menu_text } else { t.camera_raw_menu_disabled };
    ui.painter().text(pos2(rect.left() + d.text_inset, rect.center().y), Align2::LEFT_CENTER, label, crate::theme::medium(d.font_size), ink);
    if !shortcut.is_empty() {
        ui.painter().text(
            pos2(rect.right() - d.shortcut_inset, rect.center().y + d.text_offset_y),
            Align2::RIGHT_CENTER,
            shortcut,
            FontId::proportional(d.font_size),
            t.camera_raw_menu_disabled,
        );
    }
    if submenu {
        let c = pos2(rect.right() - 18.0, rect.center().y);
        ui.painter().add(egui::Shape::line(vec![c + vec2(-2.0, -4.0), c + vec2(2.0, 0.0), c + vec2(-2.0, 4.0)], Stroke::new(1.5, ink)));
    }
    if !enabled {
        r.clone().on_hover_text(crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", "Not available"));
    }
    r
}

#[derive(Clone, Copy)]
struct Availability {
    reset: bool,
    defaults: bool,
    previous: bool,
    clipboard: bool,
    snapshots: bool,
    create: bool,
}
type Entry<'a> = (&'a str, &'a str, Option<Action>, bool, bool);
fn rows(ui: &mut Ui, rect: Rect, d: &Design, a: Availability, focus: &mut Option<usize>) -> Option<Action> {
    let Availability { reset: can_reset, defaults: can_default, previous, clipboard, snapshots, create: can_create } = a;
    let mut action = None;
    let groups: Vec<Vec<Entry<'_>>> = vec![
        vec![
            (tl!("Reset to Open"), "", Some(Action::ResetOpen), can_reset, false),
            (tl!("Reset to Default"), "", Some(Action::ResetDefault), can_default, false),
        ],
        vec![
            (tl!("Apply Previous Settings"), "", Some(Action::Previous), previous, false),
            (tl!("Apply Snapshot"), "", Some(Action::SnapshotPicker), snapshots, snapshots),
            (tl!("Apply Favorite Preset"), "", None, false, false),
        ],
        vec![(tl!("Create Snapshot…"), "⇧⌘ S", Some(Action::Snapshot), can_create, false), (tl!("Create Preset…"), "⇧⌘ P", None, false, false)],
        vec![
            (tl!("Copy Edit Settings"), "⌘ C", Some(Action::Copy), true, false),
            (tl!("Copy Selected Edit Settings…"), "⌥⌘ C", None, false, false),
            (tl!("Paste Edit Settings"), "⌘ V", Some(Action::Paste), clipboard, false),
        ],
        vec![(tl!("Load Settings…"), "", Some(Action::Load), true, false), (tl!("Save Settings…"), "", Some(Action::Save), true, false)],
        vec![
            (tl!("Update AI Settings"), "⇧⌘ U", None, false, false),
            (tl!("Ignore Update AI Settings"), "", None, false, false),
            (tl!("Flatten AI Settings"), "", None, false, false),
            (tl!("Reset Flattened AI Settings"), "", None, false, false),
        ],
        vec![(tl!("Save Image"), "", None, false, true)],
        vec![(tl!("Render to DNG"), "", None, false, false)],
        vec![(tl!("Export Settings to XMP"), "", Some(Action::Save), true, false)],
        vec![(tl!("Update DNG Previews…"), "", None, false, false)],
        vec![(tl!("Set Raw Defaults…"), "", None, false, false)],
        vec![(tl!("AI Model Downloads…"), "", None, false, false)],
    ];
    let enabled = groups.iter().flatten().enumerate().filter_map(|(i, row)| row.3.then_some(i)).collect::<Vec<_>>();
    let key = ui.input_mut(|i| {
        [egui::Key::ArrowDown, egui::Key::ArrowUp, egui::Key::Home, egui::Key::End].into_iter().find(|k| i.consume_key(egui::Modifiers::NONE, *k))
    });
    if let Some(key) = key {
        let old = focus.and_then(|f| enabled.iter().position(|i| *i == f));
        let next = match key {
            egui::Key::Home => 0,
            egui::Key::End => enabled.len().saturating_sub(1),
            egui::Key::ArrowUp => old.map_or(enabled.len().saturating_sub(1), |i| if i == 0 { enabled.len().saturating_sub(1) } else { i - 1 }),
            _ => old.map_or(0, |i| (i + 1) % enabled.len().max(1)),
        };
        *focus = enabled.get(next).copied();
    }
    let activate = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) || i.consume_key(egui::Modifiers::NONE, egui::Key::Space));
    let mut index = 0usize;
    let mut y = rect.top() + d.top_padding;
    for (group, items) in groups.iter().enumerate() {
        if group > 0 {
            let sy = y + d.separator_offset;
            ui.painter().line_segment(
                [pos2(rect.left() + d.text_inset, sy), pos2(rect.right() - d.text_inset, sy)],
                Stroke::new(0.5, Tokens::get(ui.ctx()).camera_raw_menu_separator),
            );
            y += d.group_gap;
        }
        for (label, key, id, enabled, submenu) in items {
            let r = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), d.row_height));
            let response = item(ui, r, label, key, *enabled, *submenu, d);
            if *focus == Some(index) && *enabled {
                response.request_focus();
            }
            if (response.clicked() || (activate && *focus == Some(index))) && *enabled {
                action = *id;
            }
            index += 1;
            y += d.row_height;
        }
    }
    action
}

/// Reject hostile clipboard scalars before they reach the dialog's proxy renderer.
fn validate_clipboard(p: &CameraRaw) -> Result<(), String> {
    let value = serde_json::to_value(p).map_err(|e| e.to_string())?;
    let fields = value.as_object().ok_or("Camera Raw settings must be an object")?;
    let number = |key: &str, n: &serde_json::Value, lo: f64, hi: f64| -> Result<(), String> {
        let v = n.as_f64().filter(|v| v.is_finite() && (lo..=hi).contains(v));
        if v.is_none() {
            return Err(format!("Camera Raw clipboard {key} must be in {lo}..={hi}"));
        }
        Ok(())
    };
    for (key, v) in fields {
        match key.as_str() {
            "exposure" => number(key, v, -5.0, 5.0)?,
            "sharpenRadius" => number(key, v, 0.5, 3.0)?,
            "sharpenAmount" => number(key, v, 0.0, 150.0)?,
            "gradeBlending"
            | "sharpenDetail"
            | "sharpenMasking"
            | "noiseLuminance"
            | "noiseLuminanceDetail"
            | "noiseColor"
            | "noiseColorDetail"
            | "grainAmount"
            | "grainSize"
            | "grainRoughness"
            | "vignetteMidpoint"
            | "vignetteFeather"
            | "vignetteHighlights" => number(key, v, 0.0, 100.0)?,
            "hslHue" | "hslSat" | "hslLum" => {
                for n in v.as_array().ok_or("Color mixer settings must be arrays")? {
                    number(key, n, -100.0, 100.0)?;
                }
            }
            "gradeShadows" | "gradeMidtones" | "gradeHighlights" | "gradeGlobal" => {
                for (k, n) in v.as_object().ok_or("Color grading settings must be objects")? {
                    let (lo, hi) = match k.as_str() {
                        "hue" => (0.0, 360.0),
                        "sat" => (0.0, 100.0),
                        _ => (-100.0, 100.0),
                    };
                    number(key, n, lo, hi)?;
                }
            }
            "curveSplits" => {
                if p.curve_splits.iter().any(|n| !n.is_finite() || !(5.0..=95.0).contains(n))
                    || p.curve_splits.windows(2).any(|w| w.last().copied().unwrap_or(0.0) - w.first().copied().unwrap_or(0.0) < 1.0)
                {
                    return Err("Camera Raw clipboard curve regions must stay ordered and at least one percent apart".into());
                }
            }
            "pointCurve" | "pointCurveRed" | "pointCurveGreen" | "pointCurveBlue" => {}
            "vignetteStyle" => {
                if !matches!(v.as_str(), Some("highlightPriority" | "colorPriority" | "paintOverlay")) {
                    return Err("Unsupported Camera Raw clipboard vignette style".into());
                }
            }
            "seed" | "pixelScale" | "encodingGamma" => {}
            _ => number(key, v, -100.0, 100.0)?,
        }
    }
    Ok(())
}

fn paste(current: &CameraRaw, original: &CameraRaw, text: &str) -> Result<CameraRaw, String> {
    if text.len() > 4 * 1024 * 1024 {
        return Err("Camera Raw clipboard exceeds 4 MiB".into());
    }
    let next = if text.trim_start().starts_with('{') {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("Camera Raw clipboard: {e}"))?;
        let known = serde_json::to_value(CameraRaw::default()).map_err(|e| e.to_string())?;
        if let Some(fields) = v.get("cameraRaw").and_then(serde_json::Value::as_object) {
            for key in fields.keys() {
                if known.get(key).is_none() {
                    return Err(format!("Unknown Camera Raw clipboard setting {key}"));
                }
            }
        }
        crate::raw_recipe::from_json(&v)?
    } else {
        let mut next = current.clone();
        let report = photocraft_io::camera_raw_xmp::load(&mut next, text.as_bytes())?;
        if !report.ignored.is_empty() {
            return Err(format!("Unsupported Camera Raw clipboard settings: {}", report.ignored.join(", ")));
        }
        next
    };
    transfer(next, current, original)
}

fn transfer(mut params: CameraRaw, current: &CameraRaw, original: &CameraRaw) -> Result<CameraRaw, String> {
    // Color encoding and proxy scale belong to the destination document.
    params.encoding_gamma = current.encoding_gamma;
    params.pixel_scale = current.pixel_scale;
    validate_clipboard(&params)?;
    params.validate_changes(original)?;
    Ok(params)
}

fn apply(app: &mut PhotocraftApp, ctx: &egui::Context, params: CameraRaw) -> Result<(), String> {
    let d = app.camera_raw.as_ref().ok_or("Camera Raw isn't open")?;
    let params = transfer(params, &d.params, &d.original)?;
    super::menu(app, ctx, "filter.cameraRaw", &json!({"ui":{"set":serde_json::to_value(params).map_err(|e|e.to_string())?}}))
        .ok_or("Camera Raw isn't open")??;
    app.ui.status_error = false;
    app.ui.status.clear();
    Ok(())
}

/// Transient menus cannot follow a closed/replaced photo into another dialog.
pub(super) fn close_view(ui: &mut crate::state::UiState) {
    ui.camera_raw_workspace.settings_menu = false;
    ui.camera_raw_menu.rect = None;
    ui.camera_raw_menu.snapshot_picker = false;
    ui.camera_raw_menu.focus = None;
    ui.camera_raw_menu.paste_pending = false;
    ui.camera_raw_menu.paste_deadline = None;
}

pub(super) fn show(app: &mut PhotocraftApp, ctx: &egui::Context, screen: Rect, action: &mut Option<&'static str>) {
    let Some(d) = app.camera_raw.as_ref() else { return };
    let can_reset = d.params != d.original;
    let defaults = CameraRaw { encoding_gamma: d.params.encoding_gamma, pixel_scale: d.params.pixel_scale, ..Default::default() };
    let can_default = d.params != defaults;
    let previous = app.ui.camera_raw_menu.previous.as_ref().is_some_and(|p| transfer(p.clone(), &d.params, &d.original).is_ok());
    let names = d.snapshots.names().into_iter().map(str::to_owned).collect::<Vec<_>>();
    let can_create = d.snapshots.can_capture();
    let design = crate::camera_raw_design::get(ctx);
    let md = &design.settings_menu;
    let rail_left = screen.right() - design.rail_width;
    let anchor = pos2(
        (rail_left - md.width - 8.0).max(screen.left()),
        (screen.top() + design.header_height + 293.0).min(screen.bottom() - md.height() - 8.0).max(screen.top() + 8.0),
    );
    let mut open = app.ui.camera_raw_workspace.settings_menu;
    let mut chosen = None;
    app.ui.camera_raw_menu.rect = None;
    egui::Popup::new(egui::Id::new("acr-settings-menu"), ctx.clone(), anchor, egui::LayerId::new(egui::Order::Foreground, egui::Id::new("camera-raw-dialog")))
        .kind(egui::PopupKind::Menu)
        .open_bool(&mut open)
        .frame(egui::Frame::NONE)
        .width(md.width)
        .show(|ui| {
            let max_height = (screen.height() - 16.0).max(1.0);
            let (rect, _) = ui.allocate_exact_size(vec2(md.width, md.height().min(max_height)), Sense::hover());
            app.ui.camera_raw_menu.rect = Some([rect.left(), rect.top(), rect.right(), rect.bottom()]);
            let t = Tokens::get(ctx);
            ui.painter().add(egui::Shadow { offset: [0, 2], blur: 6, spread: 1, color: t.shadow }.as_shape(rect, md.radius));
            ui.painter().rect(rect, md.radius, t.camera_raw_menu_background, Stroke::new(1.0, t.camera_raw_menu_border), egui::StrokeKind::Inside);
            let mut content = ui.new_child(egui::UiBuilder::new().id_salt("settings-menu-content").max_rect(rect));
            content.set_clip_rect(rect.intersect(ui.clip_rect()));
            if rect.height() + 0.1 < md.height() {
                egui::ScrollArea::vertical().max_height(rect.height()).show(&mut content, |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(md.width - 10.0, md.height()), Sense::hover());
                    chosen = rows(
                        ui,
                        r,
                        md,
                        Availability {
                            reset: can_reset,
                            defaults: can_default,
                            previous,
                            clipboard: app.ui.camera_raw_menu.clipboard_ready,
                            snapshots: !names.is_empty(),
                            create: can_create,
                        },
                        &mut app.ui.camera_raw_menu.focus,
                    );
                });
            } else {
                chosen = rows(
                    &mut content,
                    rect,
                    md,
                    Availability {
                        reset: can_reset,
                        defaults: can_default,
                        previous,
                        clipboard: app.ui.camera_raw_menu.clipboard_ready,
                        snapshots: !names.is_empty(),
                        create: can_create,
                    },
                    &mut app.ui.camera_raw_menu.focus,
                );
            }
            if chosen.is_some() {
                ui.close();
            }
        });
    app.ui.camera_raw_workspace.settings_menu = open;
    if !open {
        app.ui.camera_raw_menu.focus = None;
    }
    let mut error = None;
    match chosen {
        Some(Action::ResetOpen) => *action = Some("reset-open"),
        Some(Action::ResetDefault) => *action = Some("reset-default"),
        Some(Action::Load) => *action = Some("load"),
        Some(Action::Save) => *action = Some("save"),
        Some(Action::Previous) => {
            if let Some(p) = app.ui.camera_raw_menu.previous.clone() {
                error = apply(app, ctx, p).err();
            }
        }
        Some(Action::Snapshot) => {
            if let Some(d) = app.camera_raw.as_mut() {
                d.snapshots.request_creation();
            }
        }
        Some(Action::SnapshotPicker) => app.ui.camera_raw_menu.snapshot_picker = true,
        Some(Action::Copy) => {
            if let Some(d) = app.camera_raw.as_ref() {
                match serde_json::to_string(&crate::raw_recipe::to_json(&d.params)) {
                    Ok(text) => {
                        ctx.copy_text(text);
                        app.ui.camera_raw_menu.clipboard_ready = true;
                    }
                    Err(e) => error = Some(e.to_string()),
                }
            }
        }
        Some(Action::Paste) => {
            app.ui.camera_raw_menu.paste_pending = true;
            app.ui.camera_raw_menu.paste_deadline = Some(ctx.input(|i| i.time) + 1.0);
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        None => {}
    }
    if app.ui.camera_raw_menu.snapshot_picker {
        let mut restore = None;
        egui::Modal::new(egui::Id::new("acr-apply-snapshot")).show(ctx, |ui| {
            ui.label(tl!("Apply Snapshot"));
            for (index, name) in names.iter().enumerate() {
                if ui.button(name).clicked() {
                    restore = Some(index);
                }
            }
            if ui.button(tl!("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.ui.camera_raw_menu.snapshot_picker = false;
            }
        });
        if let Some(index) = restore {
            if let Some(p) = app.camera_raw.as_ref().and_then(|d| d.snapshots.parameters(index)) {
                error = apply(app, ctx, p).err();
            }
            app.ui.camera_raw_menu.snapshot_picker = false;
        }
    }
    if let Some(d) = app.camera_raw.as_mut() {
        d.snapshots.creation_dialog(ctx, &d.params);
    }
    if !ctx.text_edit_focused() {
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Copy => {
                    if let Some(d) = app.camera_raw.as_ref() {
                        match serde_json::to_string(&crate::raw_recipe::to_json(&d.params)) {
                            Ok(text) => {
                                ctx.copy_text(text);
                                app.ui.camera_raw_menu.clipboard_ready = true;
                            }
                            Err(e) => error = Some(e.to_string()),
                        }
                    }
                }
                egui::Event::Paste(text) => {
                    app.ui.camera_raw_menu.paste_pending = false;
                    app.ui.camera_raw_menu.paste_deadline = None;
                    if let Some(d) = app.camera_raw.as_ref() {
                        match paste(&d.params, &d.original, &text) {
                            Ok(p) => {
                                error = apply(app, ctx, p).err();
                                if error.is_none() {
                                    app.ui.camera_raw_menu.clipboard_ready = true;
                                }
                            }
                            Err(e) => error = Some(e),
                        }
                    }
                }
                _ => {}
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::S))
            && let Some(d) = app.camera_raw.as_mut()
        {
            d.snapshots.request_creation();
        }
    }
    if app.ui.camera_raw_menu.paste_pending && app.ui.camera_raw_menu.paste_deadline.is_none_or(|t| ctx.input(|i| i.time) > t) {
        app.ui.camera_raw_menu.paste_pending = false;
        app.ui.camera_raw_menu.paste_deadline = None;
        error = Some("Clipboard did not provide Camera Raw settings".into());
    }
    if let Some(error) = error {
        app.ui.status_error = true;
        app.ui.status = error;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_preserves_recipes_and_rejects_invalid_input_atomically() {
        let original = CameraRaw::default();
        let p = CameraRaw { exposure: 1.25, tint: 17.0, ..Default::default() };
        let text = serde_json::to_string(&crate::raw_recipe::to_json(&p)).unwrap();
        assert_eq!(paste(&original, &original, &text).unwrap(), p);
        for bad in ["{", r#"{"version":900,"cameraRaw":{}}"#, r#"{"version":1,"cameraRaw":{"exposure":1000}}"#, "not settings"] {
            assert!(paste(&p, &original, bad).is_err(), "accepted {bad}");
        }
        assert!(paste(&p, &original, &"x".repeat(4 * 1024 * 1024 + 1)).is_err());
        let xmp = photocraft_io::camera_raw_xmp::save(&p);
        assert_eq!(paste(&original, &original, &xmp).unwrap().exposure, 1.25);
    }
}
