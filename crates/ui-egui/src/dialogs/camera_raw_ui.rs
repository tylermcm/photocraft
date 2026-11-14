//! Filter › Camera Raw Filter…: a large dialog like Adobe Camera Raw's (preview on the left,
//! edit panels on the right: Light, Color, Effects, Curve, Color Mixer, Color Grading, Detail).
//!
//! The preview runs [`photocraft_algo::camera_raw::develop`] on a CPU proxy of the layer
//! (≤ 900 px, `pixel_scale` keeps pixel radii true to the full image), recomputed when a
//! control changes: on a worker for the dialog's own controls, so a drag never waits for it, and
//! on the UI thread for the control channel, so a reply describes its own settings. Settings
//! whose stages are per pixel, or Shadows and Highlights, develop on the GPU instead where it can
//! (`camera_raw_gpu`).
//! OK runs `filter.cameraRaw` with the non-default settings, so the result is one history step
//! (or a smart filter on a smart object) and exactly replayable.
//!
//! Control channel: `ui.menu.invoke {"id":"filter.cameraRaw","params":{"ui":{"set":{…},
//! "commit":true | "cancel":true}}}`; the reply describes the dialog. `{"ui":{"saveSettings":true}}`
//! and `{"ui":{"loadSettings":true}}` run Save Settings… / Load Settings… (Camera Raw `.xmp`
//! settings files, see [`photocraft_io::camera_raw_xmp`]) through the file dialog.

use egui::{Align2, Color32, FontId, Pos2, Rect as ERect, Sense, Stroke, TextureHandle, pos2, vec2};
use photocraft_algo::camera_raw::{CameraRaw, Wheel, curve_lut};
use photocraft_algo::histogram::RgbHistogram;

use crate::camera_raw_scope_ui::{MAX_SAMPLERS, ToneZone};

use photocraft_doc::LayerId;
use photocraft_geom::Rect;
use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::theme::Tokens;
use crate::widgets;
use host_controls::row;

#[path = "../camera_raw_bars_ui.rs"]
mod bars;
#[path = "../camera_raw_crop_ui.rs"]
pub(crate) mod crop;
#[path = "../camera_raw_curve_regions_ui.rs"]
mod curve_regions;
#[path = "../camera_raw_extra_panels_ui.rs"]
mod extra_panels;
#[path = "../camera_raw_finishing_ui.rs"]
pub(crate) mod finishing;
#[path = "../camera_raw_host_job_ui.rs"]
pub(crate) mod host_job;
#[path = "../camera_raw_info_ui.rs"]
mod info;
#[path = "../camera_raw_mask_edit_ui.rs"]
pub(crate) mod mask_edit;
#[path = "../camera_raw_mask_paint_ui.rs"]
pub(crate) mod mask_paint;
#[path = "../camera_raw_masking_ui.rs"]
pub(crate) mod masking;
#[path = "../camera_raw_mixer_ui.rs"]
pub(crate) mod mixer;
#[path = "../camera_raw_preferences_ui.rs"]
pub(crate) mod preferences;
#[path = "../camera_raw_remove_ui.rs"]
pub(crate) mod remove;
pub use preferences::State as PreferencesState;
#[path = "../camera_raw_save_options_ui.rs"]
mod save_options;
pub use save_options::{Design as SaveOptionsDesign, Settings as SaveOptionsSettings, State as SaveOptionsState};
#[path = "../camera_raw_presets_ui.rs"]
pub(crate) mod presets;
#[path = "../camera_raw_settings_menu_ui.rs"]
mod settings_menu;
pub use presets::Design as PresetsDesign;
#[path = "../camera_raw_snapshots_ui.rs"]
mod snapshots;
pub use settings_menu::{Design as SettingsMenuDesign, State as SettingsMenuState};
#[path = "../camera_raw_tool_panels_ui.rs"]
pub(crate) mod tool_panels;

mod controls;
mod convert;
pub(crate) use convert::{apply_session_raw_conversions, output_sharpening};
mod filmstrip;
mod host_controls;
mod menu;
mod open;
mod proxy;
mod sidecar;

pub use controls::show;

pub(crate) use controls::commit;
#[cfg(test)]
use convert::open_pixels;
pub use filmstrip::HeldRaw;
pub use menu::menu;
use open::Coverage;
#[cfg(test)]
pub(crate) use open::open_raw;
pub use open::{
    REDEVELOP_JOB, RawOpen, Redevelop, on_redevelop_event, open, open_smart_filter, opened_without_dialog, queue_open_dialog, wants_open_dialog,
    without_develop_note,
};
pub(crate) use open::{Target, cancel, non_defaults, raw_stage};
#[cfg(test)]
use open::{commit_open_raw, document_index, finish_redevelop, replace_raw_document};
use proxy::{ProxyFrame, ProxyJob};
pub use sidecar::sidecar_path;
#[cfg(test)]
use sidecar::{existing_sidecar_is_ours, read_bounded};

const PROXY_SIDE: usize = 900;
/// Smallest run of proxy pixels one rayon task takes in the per-pixel passes: below it the
/// split costs more than the work.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const PARALLEL_MIN_PIXELS: usize = 16 * 1024;
const PANEL_W: f32 = 330.0;
const BANDS: [&str; 8] = ["Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta"];

pub struct CameraRawDialog {
    pub layer: LayerId,
    layer_name: String,
    pub params: CameraRaw,
    /// Settings when the dialog opened (stored settings of an edited smart filter).
    original: CameraRaw,
    target: Target,
    curve_state: crate::state::PointCurveState,
    curve_rect: Option<ERect>,
    /// Proxy pixels (straight RGBA) and size.
    pub(crate) proxy: std::sync::Arc<Vec<[f32; 4]>>,
    pub(crate) selection: Option<std::sync::Arc<Vec<f32>>>,
    pub(crate) pw: usize,
    pub(crate) ph: usize,
    pub(crate) full_w: usize,
    full_h: usize,
    preview_ppp: f32,
    pub(crate) detail: crate::camera_raw_detail_ui::DetailPreview,
    display: std::sync::Arc<photocraft_engine::display_color::CanvasDisplay>,
    pub(crate) viewport: Option<ERect>,
    /// Whether the main window had focus last frame (see [`show`]).
    main_focused: bool,
    pub(crate) float: bool,
    tex: Option<TextureHandle>,
    before_tex: Option<TextureHandle>,
    before_display_key: Option<u64>,
    pub(crate) before_histogram: RgbHistogram,
    pub(crate) processed: Vec<[f32; 4]>,
    pub(crate) lab_transform: std::sync::Arc<photocraft_cms::Transform>,
    pub(crate) scope_transform: std::sync::Arc<photocraft_cms::Transform>,
    pub(crate) scope: crate::camera_raw_scope_ui::ScopeView,
    pub(crate) after_histogram: RgbHistogram,
    pub(crate) preview_revision: u64,
    histogram_ms: f64,
    pub(crate) dirty: bool,
    pub show_before: bool,
    mixer_tab: usize,
    pub render_ms: f64,
    /// Set for the open-time dialog ([`Target::OpenRaw`]).
    raw_open: Option<RawOpen>,
    /// The open-time dialog's camera tone curve: Shadows / Highlights act on the raw data there,
    /// in linear light before this curve, and the preview undoes it to show the same.
    raw_curve: Option<std::sync::Arc<Vec<[f32; 2]>>>,
    /// The proxy develop running on a worker ([`Self::request_render`]).
    pending: Option<std::sync::mpsc::Receiver<Result<ProxyFrame, String>>>,
    /// The last worker failure the status bar shows, with the preview revision it failed on.
    preview_error: Option<(String, u64)>,
    /// The GPU develop, when the app may use one.
    pub(crate) gpu: Option<crate::camera_raw_gpu::CameraRawGpu>,
    /// Names [`Self::proxy`] for the GPU pass's upload cache.
    source_key: u64,
    /// The last frame's develop alone, and where it ran ("gpu" or "cpu").
    develop_ms: f64,
    pub(crate) develop_backend: &'static str,
    /// Settings before each finished edit, so Ctrl+Z works inside the dialog (#4100).
    undo: Vec<CameraRaw>,
    redo: Vec<CameraRaw>,
    /// Params at pointer-down. One drag is one undo step.
    edit_origin: Option<CameraRaw>,

    pub(crate) host_job: host_job::HostJob,
    snapshots: snapshots::Store,
    preview_bypass: Option<String>,
    mixer_preview: Option<finishing::RangePreview>,
    camera_info: photocraft_algo::exif::CameraInfo,
    document_name: String,
    busy_since: Option<std::time::Instant>,
}

impl CameraRawDialog {
    pub fn describe(&self, view: &crate::state::CameraRawScopeState, navigation: &crate::state::CameraRawPreviewState) -> Value {
        let h = self.histogram();
        let mut result = json!({"layer": self.layer.0, "params": serde_json::to_value(&self.params).unwrap_or(Value::Null), "proxy": [self.pw, self.ph],
            "renderMs": self.render_ms, "developMs": self.develop_ms, "developBackend": self.develop_backend, "histogramMs": self.histogram_ms, "previewRevision": self.preview_revision, "before": self.show_before,
            "histogram": {"source": if self.show_before { "before" } else { "after" }, "approximate": true, "size": [self.pw, self.ph],
                "sampleDomain": "preview-rgb", "range": [0, 1], "red": h.channels[0].as_slice(), "green": h.channels[1].as_slice(), "blue": h.channels[2].as_slice(),
                "samples": h.samples, "transparent": h.transparent, "invalid": h.invalid, "underflow": h.underflow, "overflow": h.overflow, "shadows": h.shadows, "highlights": h.highlights}});
        result["smartFilter"] = json!(match self.target {
            Target::SmartFilter(index) => Some(index),
            Target::NewFilter | Target::OpenRaw => None,
        });
        result["openingRaw"] = json!(self.raw_open.as_ref().map(|r| json!({"name": r.name, "path": r.path, "demosaic": r.demosaic.id()})));
        result["view"] = json!(navigation);
        result["sourceSize"] = json!([self.full_w, self.full_h]);
        result["previewPending"] = json!(self.render_pending());
        result["previewApproximate"] = json!(!self.detail.ready && (self.pw != self.full_w || self.ph != self.full_h));
        result["detailPending"] = json!(self.detail.pending());
        result["detailError"] = json!(self.detail.error);
        result["viewportRect"] = json!(self.viewport.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result["zoom"] = json!(self.viewport.map(|r| navigation.scale(r, vec2(self.full_w as f32, self.full_h as f32) / self.preview_ppp)));
        result["scope"] = serde_json::to_value(view).unwrap_or(Value::Null);
        result["hasScopeSelection"] = json!(self.selection.is_some());
        result["vectorscope"] = json!(
            self.scope
                .cache
                .as_ref()
                .map(|(_, before, selected, data)| json!({"samples":data.samples,"before":before,"selectedRegion":selected,"bins":data.counts}))
        );
        result["scopeMs"] = json!(self.scope.ms);
        result["scopeRevision"] = json!(self.scope.revision);
        result["overlayRevision"] = json!(self.scope.overlay_key.map(|k| k.0));
        result["pointerReadout"] = self.scope.pointer_sample.and_then(|p| self.readout(p, view.lab)).unwrap_or(Value::Null);
        result["samplerReadouts"] = json!(view.samplers.iter().take(MAX_SAMPLERS).map(|p| self.readout(*p, view.lab)).collect::<Vec<_>>());
        result["hoveredZone"] = json!(self.scope.hovered_zone);
        result["previewRect"] = json!(self.scope.preview_rect.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result["curveState"] = json!(self.curve_state);
        result["curveRect"] = json!(self.curve_rect.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result["hoverSample"] = json!(self.hover_sample(view.vectorscope));
        result["vectorscopeRect"] = json!(self.scope.vectorscope_rect.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result["scopeRect"] = json!(self.scope.rect.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result
    }

    pub(crate) fn histogram(&self) -> &RgbHistogram {
        if self.show_before { &self.before_histogram } else { &self.after_histogram }
    }

    /// Params for the engine: only the settings that differ from the defaults.
    pub fn command_params(&self) -> Value {
        non_defaults(&self.params)
    }

    fn image(px: &[[f32; 4]], w: usize, h: usize, display: &photocraft_engine::display_color::CanvasDisplay) -> egui::ColorImage {
        // Bands copied straight from `px`, converted in parallel (#4682), not a whole copy of the
        // proxy converted on one thread.
        crate::camera_raw_detail_ui::display_image(Rect::new(0, 0, w as i32, h as i32), display, |r, out| {
            let start = (r.y0.max(0) as usize).saturating_mul(w);
            if let Some(src) = px.get(start..start.saturating_add(out.len())) {
                out.copy_from_slice(src);
            }
        })
    }

    /// Develops the proxy with the current settings and shows it, on this thread. The control
    /// channel and the first frame use this, so a reply always describes its own settings; the
    /// dialog's own controls go through [`Self::request_render`].
    pub(crate) fn render(&mut self, ctx: &egui::Context) {
        // A render still running on the worker has older settings than this one.
        self.pending = None;
        let frame = self.proxy_job().run();
        self.publish(ctx, frame);
        self.dirty = false;
    }

    fn proxy_job(&self) -> ProxyJob {
        ProxyJob {
            proxy: self.proxy.clone(),
            selection: self.selection.clone(),
            raw_curve: self.raw_curve.clone(),
            params: crate::camera_raw_workspace_ui::preview_without(&self.params, self.preview_bypass.as_deref()),
            float: self.float,
            size: (self.pw, self.ph),
            full_w: self.full_w,
            display: self.display.clone(),
            gpu: self.gpu.clone(),
            source_key: self.source_key,
        }
    }

    fn publish(&mut self, ctx: &egui::Context, frame: ProxyFrame) {
        match &mut self.tex {
            Some(t) => t.set(frame.image, egui::TextureOptions::LINEAR),
            None => self.tex = Some(ctx.load_texture("camera-raw-after", frame.image, egui::TextureOptions::LINEAR)),
        }
        if self.before_display_key != Some(frame.display.key) {
            let before = Self::image(&self.proxy, self.pw, self.ph, &frame.display);
            match &mut self.before_tex {
                Some(t) => t.set(before, egui::TextureOptions::LINEAR),
                None => self.before_tex = Some(ctx.load_texture("camera-raw-before", before, egui::TextureOptions::LINEAR)),
            }
            self.before_display_key = Some(frame.display.key);
        }
        self.after_histogram = frame.histogram;
        self.histogram_ms = frame.histogram_ms;
        self.processed = frame.px;
        self.scope.cache = None;
        self.render_ms = frame.ms;
        self.develop_ms = frame.develop_ms;
        self.develop_backend = frame.backend;
        self.preview_revision = self.preview_revision.saturating_add(1);
    }

    /// Starts developing the proxy with the current settings on a worker, unless one is already
    /// running: then [`Self::poll_render`] starts the next one with the settings of that moment.
    /// A slider drag never waits for the develop, which takes hundreds of milliseconds with
    /// Shadows or Highlights set (#5964); the preview shows the latest finished frame.
    fn request_render(&mut self, ctx: &egui::Context) {
        if !self.dirty || self.pending.is_some() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let job = self.proxy_job();
            let (tx, rx) = std::sync::mpsc::channel::<Result<ProxyFrame, String>>();
            let repaint = ctx.clone();
            let spawned = std::thread::Builder::new().name("camera-raw-proxy".into()).spawn(move || {
                let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.run())).map_err(|_| "RAW Develop preview failed".to_string());
                // The dialog may have closed or rendered on its own meanwhile; then nobody listens.
                if tx.send(frame).is_ok() {
                    repaint.request_repaint();
                }
            });
            match spawned {
                Ok(_) => {
                    self.pending = Some(rx);
                    self.dirty = false;
                }
                // No thread to spare: develop here rather than show stale settings.
                Err(_) => self.render(ctx),
            }
        }
        #[cfg(target_arch = "wasm32")]
        self.render(ctx);
    }

    /// Shows the worker's frame once it is done. `Err` when the develop failed; the preview keeps
    /// the last frame then.
    fn poll_render(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(rx) = &self.pending else { return Ok(()) };
        let frame = match rx.try_recv() {
            Ok(frame) => frame,
            Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("RAW Develop preview worker stopped".to_string()),
        };
        self.pending = None;
        self.publish(ctx, frame?);
        Ok(())
    }

    pub(crate) fn render_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// A newer frame means the worker failure is over: takes its message down, unless something
    /// else has reported since.
    fn clear_recovered_error(&mut self, ui: &mut crate::UiState) {
        let Some((e, revision)) = &self.preview_error else { return };
        if *revision == self.preview_revision {
            return;
        }
        if ui.status_error && ui.status == *e {
            ui.status.clear();
            ui.status_error = false;
        }
        self.preview_error = None;
    }
}

// The shared Curve editor must never produce a curve `CameraRaw::validate` rejects.
const _: () = assert!(crate::point_curve::MAX_POINTS == photocraft_algo::camera_raw::MAX_CURVE_POINTS && crate::point_curve::MIN_GAP >= 1.0);

#[cfg(test)]
mod tests;

#[cfg(test)]
mod identity_tests;

#[derive(Clone)]
pub(crate) struct ProxyData {
    area: Rect,
    k: usize,
    pw: usize,
    ph: usize,
    pixels: std::sync::Arc<Vec<[f32; 4]>>,
    /// The raw's default tone curve, when the worker that built this also worked it out (see `raw_workspace::Browsed`).
    curve: Option<Vec<[f32; 2]>>,
}

impl ProxyData {
    pub(crate) fn with_curve(mut self, curve: Vec<[f32; 2]>) -> Self {
        self.curve = Some(curve);
        self
    }
}

impl std::fmt::Debug for ProxyData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProxyData({}x{} of {:?}, 1/{})", self.pw, self.ph, self.area, self.k)
    }
}

struct ProxyGeometry {
    area: Rect,
    w: usize,
    h: usize,
    k: usize,
    pw: usize,
    ph: usize,
}

fn proxy_geometry(canvas: Rect, surf: &photocraft_raster::Surface) -> Result<ProxyGeometry, String> {
    let (area, w, h) = convert::preview_area(canvas, surf)?;
    let k = w.max(h).div_ceil(PROXY_SIDE).max(1);
    let (pw, ph) = (w.div_ceil(k), h.div_ceil(k));
    Ok(ProxyGeometry { area, w, h, k, pw, ph })
}

fn proxy_pixels(surf: &photocraft_raster::Surface, g: &ProxyGeometry) -> Vec<[f32; 4]> {
    convert::box_proxy(surf, g.area, g.w, g.h).0
}

pub(crate) fn proxy_for(document: &photocraft_doc::Document) -> Option<ProxyData> {
    let surf = document.walk().into_iter().find_map(|(_, _, layer)| match &layer.content {
        photocraft_doc::LayerContent::Raster(surface) => Some(surface.clone()),
        _ => None,
    })?;
    let geometry = proxy_geometry(document.bounds(), &surf).ok()?;
    let pixels = proxy_pixels(&surf, &geometry);
    Some(ProxyData { area: geometry.area, k: geometry.k, pw: geometry.pw, ph: geometry.ph, pixels: std::sync::Arc::new(pixels), curve: None })
}

impl CameraRawDialog {
    fn header_label(&self) -> String {
        let name = self.raw_open.as_ref().map_or(self.document_name.as_str(), |raw| raw.name.as_str());
        photo_header_label(name, self.camera_info.model.as_deref())
    }
}

impl CameraRawDialog {
    pub(crate) fn busy(&self) -> bool {
        self.busy_since.is_some()
    }
}

impl CameraRawDialog {
    pub(crate) fn is_stand_in(&self) -> bool {
        self.raw_open.as_ref().is_some_and(|raw| raw.full_size.is_some())
    }
}

fn photo_header_label(name: &str, model: Option<&str>) -> String {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name).chars().take(256).collect::<String>();
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else { return name };
    let model = model.chars().take(128).collect::<String>();
    let (brand, rest) = model.split_once(' ').unwrap_or((&model, ""));
    let camera = if brand.len() > 1 && brand.chars().all(|c| c.is_ascii_uppercase()) {
        let mut letters = brand.chars();
        let first = letters.next().unwrap_or_default();
        format!("{first}{}{}{}", letters.as_str().to_ascii_lowercase(), if rest.is_empty() { "" } else { " " }, rest)
    } else {
        model
    };
    format!("{name}  -  {camera}")
}

struct OpenAs {
    coverage: Coverage,
    target: Target,
    /// The preview proxy, when a worker thread already built it.
    prepared: Option<ProxyData>,
    /// The size of the photo the layer is a reduced stand-in for (its pixels are fewer; everything the dialog says about size,
    /// and the scale its pixel-radius effects use, is the photo's).
    full_size: Option<(u32, u32)>,
}

pub(crate) fn changed(app: &PhotocraftApp) -> bool {
    app.camera_raw.as_ref().is_some_and(|d| d.params != d.original)
}

pub(crate) fn target_name(app: &PhotocraftApp) -> Option<&'static str> {
    // Names for a host, not labels: indexed rather than matched to strings, so the translation scan skips them.
    const NAMES: [&str; 3] = ["openRaw", "newFilter", "smartFilter"];
    app.camera_raw.as_ref().map(|d| {
        NAMES[match d.target {
            Target::OpenRaw => 0,
            Target::NewFilter => 1,
            Target::SmartFilter(_) => 2,
        }]
    })
}

fn developed(d: &CameraRawDialog) -> crate::raw_recipe::Developed {
    let full_width = d.raw_open.as_ref().and_then(|raw| raw.full_size).map(|(w, _)| w);
    crate::raw_recipe::Developed { curve: d.raw_curve.as_deref().cloned(), full_width }
}

pub(crate) fn recipe(app: &PhotocraftApp) -> Option<Value> {
    app.camera_raw.as_ref().map(|d| crate::raw_recipe::to_json(&d.params))
}

pub(crate) fn source(app: &PhotocraftApp) -> Option<&str> {
    app.camera_raw.as_ref()?.raw_open.as_ref()?.path.as_deref()
}

pub(crate) fn done(app: &mut PhotocraftApp) -> Result<Value, String> {
    let Some(d) = app.camera_raw.as_ref() else { return Ok(json!({"open": false, "changed": false})) };
    let changed = d.params != d.original;
    if d.target != Target::OpenRaw {
        if changed {
            commit(app)?;
        } else {
            cancel(app);
        }
        return Ok(json!({"open": false, "changed": changed}));
    }
    let raw = d.raw_open.clone().ok_or("no raw file is being opened")?;
    let params = d.params.clone();
    let developed = developed(d);
    // The recipe is kept before the dialog goes: one that cannot be written leaves the dialog, and its settings, as they were.
    if changed {
        crate::raw_recipe::persist(app, raw.document, &raw.targets, &params, &developed)?;
    }
    app.ui.camera_raw_menu.previous = Some(params.clone());
    settings_menu::close_view(&mut app.ui);
    app.camera_raw = None;
    Ok(json!({"open": false, "changed": changed, "recipe": crate::raw_recipe::to_json(&params)}))
}

fn slider_default(
    ui: &mut egui::Ui,
    dirty: &mut bool,
    label: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    grad: Option<&[Color32]>,
    default: f32,
) -> egui::Response {
    let decimals = Some(if label == "Exposure" {
        2
    } else if *range.start() == 0.5 {
        1
    } else {
        0
    });
    let label = crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", label);
    let r = widgets::slider_row_prec_styled(
        ui,
        label,
        v,
        range,
        "",
        grad,
        widgets::RowGestures { reset: Some(default), wheel_step: None, curve: None },
        decimals,
        super::camera_raw_design::get(ui.ctx()).value_field_width,
        Some(super::camera_raw_design::get(ui.ctx()).slider_style()),
    );
    *dirty |= r.changed();
    r
}

fn set_channel_stops(p: &mut CameraRaw, channel: super::camera_raw_workspace_ui::CurveChannel, stops: Vec<[f32; 2]>) {
    use super::camera_raw_workspace_ui::CurveChannel;
    match channel {
        CurveChannel::Red => p.point_curve_red = stops,
        CurveChannel::Green => p.point_curve_green = stops,
        CurveChannel::Blue => p.point_curve_blue = stops,
        _ => p.point_curve = stops,
    }
}

fn reset_all_channels(p: &mut CameraRaw) {
    for curve in [&mut p.point_curve, &mut p.point_curve_red, &mut p.point_curve_green, &mut p.point_curve_blue] {
        curve.clear();
    }
}

fn paste_stops(copied: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    if copied.len() == 2 && copied[0] == [0.0, 0.0] && copied[1] == [255.0, 255.0] { Vec::new() } else { copied }
}

fn curve_context_menu(
    resp: &egui::Response,
    p: &mut CameraRaw,
    channel: super::camera_raw_workspace_ui::CurveChannel,
    shown: &[[f32; 2]],
    show_all: &mut bool,
    state: &mut crate::state::PointCurveState,
    dirty: &mut bool,
) {
    use super::camera_raw_workspace_ui::CurveChannel;
    let clipboard = egui::Id::unique(CURVE_CLIPBOARD);
    let mut changed = false;
    egui::Popup::context_menu(resp).show(|ui| {
        if channel == CurveChannel::Parametric {
            let defaults = CameraRaw::default();
            let regions = |p: &mut CameraRaw| {
                (p.curve_highlights, p.curve_lights, p.curve_darks, p.curve_shadows) = (0.0, 0.0, 0.0, 0.0);
            };
            if ui.button(tl!("Reset Regions")).clicked() {
                regions(p);
                changed = true;
            }
            if ui.button(tl!("Reset Splits")).clicked() {
                p.curve_splits = defaults.curve_splits;
                changed = true;
            }
            if ui.button(tl!("Reset Regions and Splits")).clicked() {
                regions(p);
                p.curve_splits = defaults.curve_splits;
                changed = true;
            }
        } else {
            if ui.button(tl!("Reset Channel")).clicked() {
                set_channel_stops(p, channel, Vec::new());
                changed = true;
            }
            if ui.button(tl!("Reset All Channels")).clicked() {
                reset_all_channels(p);
                changed = true;
            }
            ui.separator();
            if ui.button(tl!("Copy Channel Settings")).clicked() {
                let copied: Vec<[f32; 2]> = shown.to_vec();
                ui.ctx().data_mut(|d| d.insert_temp(clipboard, copied));
            }
            let copied: Option<Vec<[f32; 2]>> = ui.ctx().data(|d| d.get_temp(clipboard));
            if ui.add_enabled(copied.is_some(), egui::Button::new(tl!("Paste Channel Settings"))).clicked()
                && let Some(copied) = copied
            {
                set_channel_stops(p, channel, paste_stops(copied));
                changed = true;
            }
            ui.separator();
            widgets::checkbox(ui, show_all, tl!("Show All Curves"));
        }
    });
    if changed {
        *state = Default::default();
        *dirty = true;
        resp.ctx.request_repaint();
    }
}

fn region_value(p: &CameraRaw, region: usize) -> f32 {
    [p.curve_shadows, p.curve_darks, p.curve_lights, p.curve_highlights][region.min(3)]
}

fn set_region(p: &mut CameraRaw, region: usize, value: f32) {
    match region {
        0 => p.curve_shadows = value,
        1 => p.curve_darks = value,
        2 => p.curve_lights = value,
        _ => p.curve_highlights = value,
    }
}

fn region_at(splits: &[f32; 3], x: f32) -> usize {
    let percent = x / 255.0 * 100.0;
    splits.iter().take_while(|split| percent >= **split).count().min(3)
}

fn parametric_curve(p: &CameraRaw) -> Vec<f32> {
    let curve = CameraRaw {
        curve_highlights: p.curve_highlights,
        curve_lights: p.curve_lights,
        curve_darks: p.curve_darks,
        curve_shadows: p.curve_shadows,
        curve_splits: p.curve_splits,
        ..Default::default()
    };
    let mut ramp: Vec<[f32; 4]> = (0..256)
        .map(|i| {
            let v = i as f32 / 255.0;
            [v, v, v, 1.0]
        })
        .collect();
    photocraft_algo::camera_raw::develop(&mut ramp, 256, 1, &curve, true);
    ramp.into_iter().map(|pixel| pixel[0]).collect()
}

fn solve_region(p: &CameraRaw, region: usize, x: usize, target: f32) -> f32 {
    let at = |value: f32| {
        let mut q = p.clone();
        set_region(&mut q, region, value);
        parametric_curve(&q).get(x).copied().unwrap_or(0.0) * 255.0
    };
    let (mut lo, mut hi) = (-100.0f32, 100.0f32);
    if target <= at(lo) {
        return lo;
    }
    if target >= at(hi) {
        return hi;
    }
    for _ in 0..14 {
        let mid = (lo + hi) / 2.0;
        if at(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    ((lo + hi) / 2.0).round()
}

#[derive(Clone, Copy)]
struct ParametricGrab {
    region: usize,
    x: f32,
    grab: f32,
}

fn parametric_interact(ui: &egui::Ui, resp: &egui::Response, rect: ERect, p: &mut CameraRaw, dirty: &mut bool) -> Option<(usize, f32)> {
    let side = rect.width();
    let id = egui::Id::unique("camera-raw-parametric-grab");
    let to_value = |pos: Pos2| [((pos.x - rect.left()) / side * 255.0).clamp(0.0, 255.0), ((rect.bottom() - pos.y) / side * 255.0).clamp(0.0, 255.0)];
    let held = ui.input(|i| i.pointer.primary_down());
    let mut grab: Option<ParametricGrab> = ui.ctx().data(|d| d.get_temp(id));
    if !held {
        grab = None;
    }
    let pressed = (resp.is_pointer_button_down_on() || resp.clicked_by(egui::PointerButton::Primary)) && ui.input(|i| i.pointer.primary_pressed());
    if pressed && let Some(pos) = ui.input(|i| i.pointer.press_origin()).or_else(|| resp.interact_pointer_pos()) {
        let [x, y] = to_value(pos);
        let on = parametric_curve(p).get(x.round() as usize).map_or(y, |v| v * 255.0);
        grab = Some(ParametricGrab { region: region_at(&p.curve_splits, x), x, grab: on - y });
    }
    if let Some(g) = grab
        && held
        && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
    {
        let target = to_value(pos)[1] + g.grab;
        let value = solve_region(p, g.region, g.x.round() as usize, target);
        if value != region_value(p, g.region) {
            set_region(p, g.region, value);
            *dirty = true;
        }
    }
    ui.ctx().data_mut(|d| {
        if let Some(g) = grab {
            d.insert_temp(id, g);
        } else {
            d.remove::<ParametricGrab>(id);
        }
    });
    if let Some(g) = grab {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        return Some((g.region, g.x));
    }
    let pos = ui.input(|i| i.pointer.hover_pos()).filter(|pos| rect.contains(*pos))?;
    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    let x = to_value(pos)[0];
    Some((region_at(&p.curve_splits, x), x))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layout {
    pub title: ERect,
    pub body: ERect,
    pub preview_body: ERect,
    pub tool_bar: ERect,
    /// Where the photo is shown.
    pub view: ERect,
    /// The settings panel.
    pub right: ERect,
    pub rail: ERect,
    pub nav: ERect,
    pub filmstrip: ERect,
    pub foot: ERect,
}

pub(crate) fn layout(ctx: &egui::Context, screen: ERect, _own_window: bool) -> Layout {
    let design = super::camera_raw_design::get(ctx);
    let footer_h = design.footer_height.min(screen.height() * 0.2);
    let rail_w = design.rail_width.min(screen.width() * 0.1);
    let panel_w = design.panel_width.min(screen.width() * 0.45);
    let title = ERect::from_min_size(screen.min, vec2(screen.width(), design.header_height.min(screen.height() * 0.15)));
    let body = ERect::from_min_max(pos2(screen.left(), title.bottom()), pos2(screen.right(), screen.bottom() - footer_h));
    let rail = ERect::from_min_max(pos2(body.right() - rail_w, body.top()), body.max);
    let right = ERect::from_min_max(pos2(rail.left() - panel_w, body.top()), rail.left_bottom());
    let preview_body = ERect::from_min_max(body.min, right.left_bottom());
    let nav = ERect::from_min_max(
        pos2(preview_body.left(), (preview_body.bottom() - design.navigation_height.min(preview_body.height() * 0.25)).max(preview_body.top())),
        preview_body.max,
    );
    let strip_height = super::camera_raw_filmstrip_ui::height(ctx).min((nav.top() - preview_body.top()) * 0.4);
    let filmstrip = ERect::from_min_max(pos2(preview_body.left(), nav.top() - strip_height), nav.right_top());
    let tool_height = super::camera_raw_tool_bar_ui::height(ctx).min((filmstrip.top() - preview_body.top()).max(0.0) * 0.4);
    let tool_bar = ERect::from_min_size(preview_body.min, vec2(preview_body.width(), tool_height));
    let margin = design.preview_margin.min(preview_body.width() * 0.25).min((filmstrip.top() - preview_body.top()) * 0.25);
    let view = ERect::from_min_max(
        tool_bar.left_bottom() + vec2(margin, margin),
        pos2((preview_body.right() - margin).max(preview_body.left() + margin + 1.0), (filmstrip.top() - margin).max(preview_body.top() + margin + 1.0)),
    );
    let foot = ERect::from_min_max(pos2(screen.left(), screen.bottom() - footer_h), screen.max);
    Layout { title, body, preview_body, tool_bar, view, right, rail, nav, filmstrip, foot }
}

pub(crate) fn leave_in_place(app: &mut PhotocraftApp) -> Result<(), String> {
    let Some(d) = app.camera_raw.as_ref() else { return Ok(()) };
    if d.target != Target::OpenRaw {
        return Err("only the open-time dialog can hand over to the next photo".into());
    }
    let raw = d.raw_open.clone().ok_or("no raw file is being opened")?;
    let params = d.params.clone();
    let changed = d.params != d.original;
    let developed = developed(d);
    // Kept first: a recipe that cannot be written leaves the dialog on this photo, unchanged, and the next photo is not opened.
    if changed {
        crate::raw_recipe::persist(app, raw.document, &raw.targets, &params, &developed)?;
    }
    if let Some(d) = app.camera_raw.as_mut() {
        d.original = d.params.clone();
        d.busy_since = d.busy_since.or_else(|| Some(std::time::Instant::now()));
    }
    Ok(())
}

pub(crate) fn draw_shell(ctx: &egui::Context, name: &str) {
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let lay = layout(ctx, screen, false);
    // No fade-in: while it fades, the editor's own windows show through, which is what the frame is there to hide.
    egui::Area::new(egui::Id::unique("camera-raw-shell")).order(egui::Order::Foreground).fixed_pos(screen.min).fade_in(false).show(ctx, |ui| {
        let (full, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
        let painter = ui.painter().clone();
        painter.rect_filled(full, 0.0, t.chrome);
        painter.rect_filled(lay.preview_body, 0.0, t.canvas);
        painter.rect_filled(lay.title, 0.0, t.dock);
        painter.rect_filled(ERect::from_min_max(pos2(lay.title.left(), lay.title.bottom() - 1.0), lay.title.max), 0.0, t.separator);
        painter.rect_filled(lay.view, 0.0, t.canvas);
        super::camera_raw_filmstrip_ui::draw(ui, lay.filmstrip);
        // The photo's name is not known in the moment between leaving one photo and being asked for the next: no title
        // rather than the wrong one.
        if !name.is_empty() {
            painter.text(
                pos2(lay.preview_body.center().x, lay.title.center().y),
                Align2::CENTER_CENTER,
                photo_header_label(name, None),
                FontId::proportional(13.0),
                t.text,
            );
            painter.text(
                lay.view.center(),
                Align2::CENTER_CENTER,
                crate::i18n::fmt(tl!("Developing {name}"), &[("name", name)]),
                FontId::proportional(13.0),
                t.text_faint,
            );
        }
        painter.rect_filled(lay.right, 0.0, t.dock);
        painter.line_segment([lay.right.left_top(), lay.right.left_bottom()], Stroke::new(1.0, t.separator));
        painter.rect_filled(lay.nav, 0.0, t.dock);
        painter.line_segment([lay.rail.left_top(), lay.rail.left_bottom()], Stroke::new(1.0, t.separator));
        let mut y = lay.right.top() + 158.0;
        for section in ["Light", "Color", "Effects", "Curve", "Color Mixer", "Color Grading", "Detail"] {
            painter.text(
                pos2(lay.right.left() + 18.0, y),
                Align2::LEFT_CENTER,
                crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", section),
                FontId::proportional(13.0),
                t.text_faint,
            );
            y += 30.0;
        }
        painter.rect_filled(lay.foot, 0.0, t.dock);
        painter.line_segment([lay.foot.left_top(), lay.foot.right_top()], Stroke::new(1.0, t.separator));
        let mut fu = ui.new_child(egui::UiBuilder::new().max_rect(lay.foot.shrink2(vec2(16.0, 9.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
        fu.add_enabled_ui(false, |fu| {
            widgets::dialog_buttons(
                fu,
                &[
                    widgets::DialogButton::new(widgets::ButtonRole::Default, tl!("Open"), 90.0),
                    widgets::DialogButton::new(widgets::ButtonRole::Cancel, tl!("Cancel"), 90.0),
                ],
            );
        });
    });
}

impl CameraRawDialog {
    pub fn describe_host(
        &self,
        view: &crate::state::CameraRawScopeState,
        navigation: &crate::state::CameraRawPreviewState,
        workspace: &crate::camera_raw_workspace_ui::WorkspaceState,
    ) -> Value {
        let h = self.histogram();
        let mut result = json!({"layer": self.layer.0, "params": serde_json::to_value(&self.params).unwrap_or(Value::Null), "proxy": [self.pw, self.ph],
            "renderMs": self.render_ms, "histogramMs": self.histogram_ms, "previewRevision": self.preview_revision, "before": self.show_before,
            "histogram": {"source": if self.show_before { "before" } else { "after" }, "approximate": true, "size": [self.pw, self.ph],
                "sampleDomain": "preview-rgb", "range": [0, 1], "red": h.channels[0].as_slice(), "green": h.channels[1].as_slice(), "blue": h.channels[2].as_slice(),
                "samples": h.samples, "transparent": h.transparent, "invalid": h.invalid, "underflow": h.underflow, "overflow": h.overflow, "shadows": h.shadows, "highlights": h.highlights}});
        result["smartFilter"] = json!(match self.target {
            Target::SmartFilter(index) => Some(index),
            Target::NewFilter | Target::OpenRaw => None,
        });
        result["openingRaw"] = json!(self.raw_open.as_ref().map(|r| json!({"name": r.name, "path": r.path, "demosaic": r.demosaic.id()})));
        result["view"] = json!(navigation);
        result["workspace"] = json!(workspace);
        result["snapshots"] = json!(self.snapshots.names());
        result["previewBypass"] = json!(self.preview_bypass);
        result["sourceSize"] = json!([self.full_w, self.full_h]);
        result["previewPending"] = json!(self.render_pending());
        result["previewApproximate"] = json!(!self.detail.ready && (self.pw != self.full_w || self.ph != self.full_h));
        result["detailPending"] = json!(self.detail.pending());
        result["detailError"] = json!(self.detail.error);
        result["viewportRect"] = json!(self.viewport.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result["zoom"] = json!(self.viewport.map(|r| navigation.scale(r, vec2(self.full_w as f32, self.full_h as f32) / self.preview_ppp)));
        result["scope"] = serde_json::to_value(view).unwrap_or(Value::Null);
        result["hasScopeSelection"] = json!(self.selection.is_some());
        result["vectorscope"] = json!(
            self.scope
                .cache
                .as_ref()
                .map(|(_, before, selected, data)| json!({"samples":data.samples,"before":before,"selectedRegion":selected,"bins":data.counts}))
        );
        result["scopeMs"] = json!(self.scope.ms);
        result["scopeRevision"] = json!(self.scope.revision);
        result["overlayRevision"] = json!(self.scope.overlay_key.map(|k| k.0));
        result["pointerReadout"] = self.scope.pointer_sample.and_then(|p| self.readout(p, view.lab)).unwrap_or(Value::Null);
        result["samplerReadouts"] = json!(view.samplers.iter().take(MAX_SAMPLERS).map(|p| self.readout(*p, view.lab)).collect::<Vec<_>>());
        result["hoveredZone"] = json!(self.scope.hovered_zone);
        result["previewRect"] = json!(self.scope.preview_rect.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result["curveState"] = json!(self.curve_state);
        result["curveRect"] = json!(self.curve_rect.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result["hoverSample"] = json!(self.hover_sample(view.vectorscope));
        result["vectorscopeRect"] = json!(self.scope.vectorscope_rect.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result["scopeRect"] = json!(self.scope.rect.map(|r| [r.left(), r.top(), r.right(), r.bottom()]));
        result
    }
    pub fn describe_hosted(
        &self,
        view: &crate::state::CameraRawScopeState,
        navigation: &crate::state::CameraRawPreviewState,
        workspace: &crate::camera_raw_workspace_ui::WorkspaceState,
    ) -> Value {
        let mut result = self.describe(view, navigation);
        merge_host_description(&mut result, self.describe_host(view, navigation, workspace));
        result
    }
}
fn merge_host_description(base: &mut Value, host: Value) {
    if let (Some(base), Value::Object(host)) = (base.as_object_mut(), host) {
        for (key, value) in host {
            if let Some(existing) = base.get_mut(&key) {
                merge_host_description(existing, value);
            } else {
                base.insert(key, value);
            }
        }
    }
}

const BUSY_AFTER: std::time::Duration = std::time::Duration::from_millis(250);

const CURVE_CLIPBOARD: &str = "camera-raw-curve-clipboard";

const CURVE_STOP_RADIUS: f32 = 9.0;

const CURVE_LINE_GRAB: f32 = 7.0;

const REGION_NAMES: [&str; 4] = ["Shadows", "Darks", "Lights", "Highlights"];
