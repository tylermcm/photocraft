use super::controls::{commit, redo_edit, remember_edit, undo_edit};
use super::open::{cancel, open, open_smart_filter};
use super::sidecar::{load_settings, save_settings};
use super::{CameraRaw, LayerId, PhotocraftApp, Value, json};
use super::{preferences, save_options};

/// Menu / control-channel entry point. `None` when the call isn't for this dialog.
pub fn menu(app: &mut PhotocraftApp, ctx: &egui::Context, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id != "filter.cameraRaw" {
        return None;
    }
    let Some(fields) = params.as_object() else {
        return Some(Err("Camera Raw params must be an object".into()));
    };
    if fields.is_empty() {
        return Some(open(app, ctx).map(|_| {
            app.camera_raw
                .as_ref()
                .map(|d| d.describe_hosted(&app.ui.camera_raw_scope, &app.ui.camera_raw_preview, &app.ui.camera_raw_workspace))
                .unwrap_or(Value::Null)
        }));
    }
    if let Some(sf) = params.get("smartFilter") {
        let opened = (|| {
            if fields.len() > 1 {
                return Err("smartFilter opens the dialog; send ui requests separately".to_string());
            }
            let layer = sf.get("layer").and_then(Value::as_u64).ok_or("smartFilter.layer must be a layer id")?;
            let index = sf.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).ok_or("smartFilter.index must be a filter index")?;
            open_smart_filter(app, ctx, LayerId(layer), index)?;
            Ok(app
                .camera_raw
                .as_ref()
                .map(|d| d.describe_hosted(&app.ui.camera_raw_scope, &app.ui.camera_raw_preview, &app.ui.camera_raw_workspace))
                .unwrap_or(Value::Null))
        })();
        return Some(opened);
    }
    let ui = params.get("ui")?;
    let Some(fields) = ui.as_object() else {
        return Some(Err("Camera Raw ui must be an object".into()));
    };
    for (key, value) in fields {
        match key.as_str() {
            "set" | "scope" | "view" | "demosaic" | "workspace" | "saveOptions" | "preferences" => {}
            "reset" if matches!(value.as_str(), Some("open" | "default")) => {}
            "reset" => return Some(Err("Camera Raw reset must be open or default".into())),
            "before" | "commit" | "cancel" | "saveSettings" | "loadSettings" | "undo" | "redo" | "auto" | "applyPreferences" if value.is_boolean() => {}
            "before" | "commit" | "cancel" | "saveSettings" | "loadSettings" | "undo" | "redo" | "auto" | "applyPreferences" => {
                return Some(Err(format!("Camera Raw {key} must be a boolean")));
            }
            _ => return Some(Err(format!("unknown Camera Raw ui property {key}"))),
        }
    }
    if ui.get("commit").and_then(Value::as_bool) == Some(true) && ui.get("cancel").and_then(Value::as_bool) == Some(true) {
        return Some(Err("Camera Raw cannot commit and cancel together".into()));
    }
    if ui.get("applyPreferences").and_then(Value::as_bool) == Some(true) {
        if fields.len() != 1 {
            return Some(Err("applyPreferences must be sent on its own".into()));
        }
        return Some(preferences::confirm(app, ctx).map(|_| json!({"applied":true})));
    }
    for (key, run) in [("saveSettings", save_settings as fn(&mut PhotocraftApp) -> Result<Value, String>), ("loadSettings", load_settings)] {
        if ui.get(key).and_then(Value::as_bool) == Some(true) {
            if fields.len() > 1 {
                return Some(Err(format!("Camera Raw {key} must be sent on its own")));
            }
            return Some(run(app));
        }
    }
    let was_closed = app.camera_raw.is_none();
    let previous_view = app.ui.camera_raw_scope.clone();
    let previous_navigation = app.ui.camera_raw_preview;
    let previous_workspace = app.ui.camera_raw_workspace.clone();
    if was_closed && let Err(e) = open(app, ctx) {
        return Some(Err(e));
    }
    let result = menu_update(app, ctx, ui);
    if was_closed && result.as_ref().is_some_and(Result::is_err) {
        app.camera_raw = None;
        app.ui.camera_raw_scope = previous_view;
        app.ui.camera_raw_preview = previous_navigation;
        app.ui.camera_raw_workspace = previous_workspace;
    }
    result
}

pub(super) fn menu_update(app: &mut PhotocraftApp, ctx: &egui::Context, ui: &Value) -> Option<Result<Value, String>> {
    // Stage both filter and presentation updates. No texture, preference or dialog mutation
    // occurs until every part of the request has passed validation.
    let d = app.camera_raw.as_ref()?;
    let next_params = match prepared_params(d, ui) {
        Ok(params) => params,
        Err(error) => return Some(Err(error)),
    };
    let workspace = match ui.get("workspace") {
        Some(value) => match crate::camera_raw_workspace_ui::prepare(&app.ui.camera_raw_workspace, value) {
            Ok(next) => Some(next),
            Err(error) => return Some(Err(error)),
        },
        None => None,
    };
    let save_options = match ui.get("saveOptions") {
        Some(v) => match save_options::prepare(app.ui.camera_raw_save_options.current(), v) {
            Ok(next) => Some(next),
            Err(e) => return Some(Err(e)),
        },
        None => None,
    };
    let preferences = match ui.get("preferences") {
        Some(v) => match preferences::prepare(app.ui.camera_raw_preferences.current(), v) {
            Ok(next) => Some(next),
            Err(e) => return Some(Err(e)),
        },
        None => None,
    };
    let scope_update = match ui.get("scope") {
        Some(scope) => match crate::camera_raw_scope_ui::prepare_control(&next_params, d.selection.is_some(), &app.ui.camera_raw_scope, scope) {
            Ok(update) => Some(update),
            Err(e) => return Some(Err(e)),
        },
        None => None,
    };
    let navigation = match ui.get("view") {
        Some(view) => match crate::camera_raw_preview_ui::prepare(app.ui.camera_raw_preview, view) {
            Ok(next) => Some(next),
            Err(e) => return Some(Err(e)),
        },
        None => None,
    };
    let demosaic = match ui.get("demosaic") {
        None => None,
        Some(_) if d.raw_open.is_none() => return Some(Err("demosaic applies only while a raw file opens".into())),
        Some(v) => match v.as_str().and_then(photocraft_io::raw::Demosaic::from_id) {
            Some(m) => Some(m),
            None => return Some(Err("demosaic must be ahd, mhc or bilinear".into())),
        },
    };
    let d = app.camera_raw.as_mut()?;
    if let Some(next) = workspace {
        app.ui.camera_raw_workspace = next;
    }
    if let Some(next) = save_options {
        if app.ui.camera_raw_workspace.save_options {
            app.ui.camera_raw_save_options.draft = Some(next);
        } else {
            app.ui.camera_raw_save_options.settings = next;
        }
    }
    if let Some(next) = preferences {
        if app.ui.camera_raw_workspace.preferences {
            app.ui.camera_raw_preferences.draft = Some(next);
        } else {
            app.ui.camera_raw_preferences.settings = next;
        }
    }
    if let (Some(m), Some(raw)) = (demosaic, d.raw_open.as_mut()) {
        raw.demosaic = m;
    }
    if let Some(view) = navigation {
        app.ui.camera_raw_preview = view;
    }
    if d.params.point_curve != next_params.point_curve
        || d.params.point_curve_red != next_params.point_curve_red
        || d.params.point_curve_green != next_params.point_curve_green
        || d.params.point_curve_blue != next_params.point_curve_blue
    {
        d.curve_state = Default::default();
    }
    if d.params != next_params {
        remember_edit(d);
        d.dirty = true;
        d.params = next_params;
    }
    if let Some(b) = ui.get("before").and_then(Value::as_bool) {
        d.show_before = b;
    }
    if let Some(update) = scope_update {
        update.apply(d, &mut app.ui.camera_raw_scope);
    }
    if ui.get("undo").and_then(Value::as_bool) == Some(true) {
        undo_edit(d);
    }
    if ui.get("redo").and_then(Value::as_bool) == Some(true) {
        redo_edit(d);
    }
    if d.dirty {
        d.render(ctx);
    }
    // The control channel can read the status before the next frame draws.
    d.clear_recovered_error(&mut app.ui);
    if app.ui.camera_raw_scope.vectorscope
        && let Some(d) = app.camera_raw.as_mut()
    {
        d.ensure_scope(app.ui.camera_raw_scope.selected_region);
    }
    crate::camera_raw_scope_ui::persist(app, ctx);
    if ui.get("cancel").and_then(Value::as_bool) == Some(true) {
        cancel(app);
        return Some(Ok(json!({"cancelled": true})));
    }
    if ui.get("commit").and_then(Value::as_bool) == Some(true) {
        return Some(commit(app));
    }
    Some(Ok(app
        .camera_raw
        .as_ref()
        .map(|d| d.describe_hosted(&app.ui.camera_raw_scope, &app.ui.camera_raw_preview, &app.ui.camera_raw_workspace))
        .unwrap_or(Value::Null)))
}

fn prepared_params(d: &super::CameraRawDialog, ui: &Value) -> Result<CameraRaw, String> {
    let mut next = match ui.get("reset").and_then(Value::as_str) {
        Some("open") => d.original.clone(),
        Some("default") => CameraRaw { encoding_gamma: d.params.encoding_gamma, pixel_scale: d.params.pixel_scale, ..Default::default() },
        _ => d.params.clone(),
    };
    if ui.get("reset").is_some() && ui.get("set").is_some() {
        return Err("Camera Raw reset and set must be separate requests".into());
    }
    if ui.get("auto").and_then(Value::as_bool) == Some(true) {
        photocraft_algo::camera_raw::auto_tone(&d.proxy, &mut next);
    }
    if let Some(set) = ui.get("set") {
        let set = set.as_object().ok_or("Camera Raw set must be an object")?;
        let mut current = serde_json::to_value(&next).map_err(|e| e.to_string())?;
        let fields = current.as_object_mut().ok_or("Camera Raw settings must be an object")?;
        for (key, value) in set {
            let canonical = match key.as_str() {
                "calibShadowTint" => "shadowTint",
                "calibHue" => "calibrationHue",
                "calibSat" => "calibrationSat",
                _ => key.as_str(),
            };
            if canonical != key && set.contains_key(canonical) {
                return Err(format!("Camera Raw setting {canonical} was supplied twice"));
            }
            if !fields.contains_key(canonical) {
                return Err(format!("unknown Camera Raw setting {key}"));
            }
            fields.insert(canonical.into(), value.clone());
        }
        next = serde_json::from_value(current).map_err(|e| format!("bad Camera Raw settings: {e}"))?;
        next.validate_changes(&d.original).map_err(|e| format!("bad Camera Raw settings: {e}"))?;
    }
    Ok(next)
}
