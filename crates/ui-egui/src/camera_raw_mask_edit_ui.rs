//! Camera Raw Masking, the editing side: making a gradient mask by dragging on the photo, moving and resizing it with its handles, the red
//! overlay, and the panel with the mask list and the selected mask's settings and local adjustments.
//!
//! Masks live in `CameraRaw.masks` (see `algo::camera_raw::masks`), so undo, redo, the preview, the saved recipe and exports all work on
//! them with no code here. This file only edits that list. Geometry is image-relative (0 … 1); the screen rectangle of the picture maps it.
//! The layout is a plain one built from the app's own widgets: matching Adobe's exactly is a measurement pass for later.
use egui::{Align2, Color32, FontId, Key, Modifiers, Pos2, Rect, Response, Sense, Stroke, TextureOptions, Ui, pos2, vec2};
use photocraft_algo::camera_raw::{CameraRaw, LocalMask, MaskShape};
use serde::{Deserialize, Serialize};

use super::CameraRawDialog;
use super::mask_paint::Brush;
use crate::theme::Tokens;

fn tr(text: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", text)
}

/// A mask tool that is waiting for a drag on the photo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MaskTool {
    Linear,
    Radial,
    /// Paint coverage (K).
    Brush,
    /// Drag a box (or click) on an object.
    Objects,
    /// Click a colour to select pixels of that colour.
    Colour,
}

/// A mask Image Triage finds for us (BiRefNet), asked for from the Create New Mask panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoMask {
    Subject,
    /// Found here, with the editor's own sky selection (no model, no Image Triage).
    Sky,
    /// The subject's matte, inverted.
    Background,
    /// The brightest part of the picture, to be set with Min and Max.
    LuminanceRange,
}

impl AutoMask {
    fn tag(self) -> &'static str {
        match self {
            AutoMask::Subject => "subject",
            AutoMask::Sky => "sky",
            AutoMask::LuminanceRange => "luminanceRange",
            AutoMask::Background => "background",
        }
    }
}

const AUTO_INPUT: &str = "photocraft-mask-input.png";
const AUTO_OUTPUT: &str = "photocraft-mask-output.png";

/// Write the preview picture and ask Image Triage to find the subject in it. The answer arrives later (`finish_auto`).
pub(super) fn request_auto(d: &mut CameraRawDialog, kind: AutoMask, now: f64) -> Result<(), String> {
    if d.params.masks.len() >= photocraft_algo::camera_raw::MAX_MASKS {
        return Err(tr("A photo can have at most 16 masks.").to_string());
    }
    let image = CameraRawDialog::image(&d.proxy, d.pw, d.ph, &d.display);
    let rgb: Vec<u8> = image.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b()]).collect();
    let picture = photocraft_codecs::Image::from_u8(d.pw as u32, d.ph as u32, photocraft_codecs::ChannelLayout::Rgb, rgb).map_err(|e| e.to_string())?;
    let png = photocraft_codecs::encode(&picture, photocraft_codecs::Format::Png, &Default::default()).map_err(|e| e.to_string())?;
    let dir = std::env::temp_dir();
    let (input, output) = (dir.join(AUTO_INPUT), dir.join(AUTO_OUTPUT));
    std::fs::write(&input, png).map_err(|e| e.to_string())?;
    let asked = serde_json::json!({"input": input.to_string_lossy(), "output": output.to_string_lossy()});
    if d.host_job.request_mask(super::host_job::SUBJECT_MASK, asked, kind.tag(), now) {
        Ok(())
    } else {
        Err(tr("Needs Image Triage. Open Camera Raw from Image Triage to use it.").to_string())
    }
}

/// Image Triage has written the subject's coverage map: it becomes a new mask (inverted for Background) and is selected.
pub(super) fn finish_auto(d: &mut CameraRawDialog, st: &mut MaskUiState, tag: &str, output: &str) -> Result<(), String> {
    let bytes = std::fs::read(output).map_err(|e| e.to_string())?;
    if d.params.masks.len() >= photocraft_algo::camera_raw::MAX_MASKS {
        return Err(tr("A photo can have at most 16 masks.").to_string());
    }
    let background = tag == AutoMask::Background.tag();
    d.params.masks.push(mask_from_png(&bytes, background)?);
    st.selected = Some(d.params.masks.len() - 1);
    st.creating = false;
    d.dirty = true;
    Ok(())
}

/// A raster mask from a selection result over the preview picture (`w` x `h`); `None` when nothing was selected.
fn mask_from_region(region: &photocraft_algo::selection::Region, w: usize, h: usize, name: &str) -> Option<LocalMask> {
    let mut coverage = vec![0.0f32; w * h];
    let mut any = 0usize;
    for y in region.bbox.y0.max(0)..region.bbox.y1.min(h as i32) {
        for x in region.bbox.x0.max(0)..region.bbox.x1.min(w as i32) {
            let v = region.at(x, y);
            coverage[y as usize * w + x as usize] = v;
            any += usize::from(v > 0.5);
        }
    }
    // Under a thousandth of the picture is noise, not a sky.
    (any * 1000 >= w * h).then(|| LocalMask { name: name.to_string(), shape: MaskShape::raster(w as u32, h as u32, &coverage), ..Default::default() })
}

/// Sky: the editor's own colour-and-gradient sky selection on the preview picture.
pub(super) fn make_sky(d: &mut CameraRawDialog, st: &mut MaskUiState) -> Result<(), String> {
    use photocraft_algo::segment::{ImageSampler, RgbImage, sky};
    if d.params.masks.len() >= photocraft_algo::camera_raw::MAX_MASKS {
        return Err(tr("A photo can have at most 16 masks.").to_string());
    }
    let (w, h) = (d.pw, d.ph);
    let shown = CameraRawDialog::image(&d.proxy, w, h, &d.display);
    let img = RgbImage::from_fn(w, h, |x, y| {
        let c = shown.pixels[y * w + x];
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0]
    });
    let sampler = ImageSampler { img: &img, origin: (0, 0) };
    let region = sky::select_sky(&sampler, photocraft_geom::Rect::new(0, 0, w as i32, h as i32), sky::SkyParams::default());
    let mask = region.and_then(|r| mask_from_region(&r, w, h, tr("Sky"))).ok_or_else(|| tr("No sky was found in this photo.").to_string())?;
    d.params.masks.push(mask);
    st.selected = Some(d.params.masks.len() - 1);
    st.creating = false;
    d.dirty = true;
    Ok(())
}

/// A raster mask from the coverage map Image Triage wrote (a grayscale PNG, white = the subject).
fn mask_from_png(bytes: &[u8], background: bool) -> Result<LocalMask, String> {
    let gray = photocraft_codecs::decode(bytes).map_err(|e| e.to_string())?.convert(photocraft_codecs::ChannelLayout::Gray, photocraft_codecs::SampleType::U8);
    let (w, h) = (gray.width(), gray.height());
    let side = photocraft_algo::camera_raw::MAX_RASTER_SIDE;
    if w == 0 || h == 0 || w > side || h > side || gray.data().len() != w as usize * h as usize {
        return Err(tr("The mask Image Triage made has an unusable size.").to_string());
    }
    let coverage: Vec<f32> = gray.data().iter().map(|v| *v as f32 / 255.0).collect();
    Ok(LocalMask { name: tr(if background { "Background" } else { "Subject" }).to_string(), shape: MaskShape::raster(w, h, &coverage), invert: background, ..Default::default() })
}

/// What the Masking tab remembers while it is open. View state only: never part of the photo's settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct MaskUiState {
    /// The mask being edited (an index into `CameraRaw.masks`).
    pub selected: Option<usize>,
    /// A tool armed from the Create New Mask panel (or G / J): the next drag on the photo makes a mask.
    pub tool: Option<MaskTool>,
    /// The red overlay on the selected mask's coverage.
    pub overlay: bool,
    /// Show the Create New Mask panel although masks exist.
    pub creating: bool,
    /// Why the last Subject / Background request did not make a mask (shown under the cards).
    pub notice: Option<String>,
    pub brush: super::mask_paint::Brush,
}

impl Default for MaskUiState {
    fn default() -> Self {
        Self { selected: None, tool: None, overlay: true, creating: false, notice: None, brush: Default::default() }
    }
}

impl MaskUiState {
    /// The photo should take mask drags (not zoom drags) while the Masking tab has a tool armed or a mask selected.
    pub fn takes_drags(&self, masking_tab: bool) -> bool {
        masking_tab && (self.tool.is_some() || self.selected.is_some())
    }

    /// Keep `selected` pointing at a mask that exists.
    pub fn settle(&mut self, masks: usize) {
        if self.selected.is_some_and(|i| i >= masks) {
            self.selected = masks.checked_sub(1);
        }
    }
}

// --- making and describing masks --------------------------------------------------------------------------------------------------

fn kind_name(shape: &MaskShape) -> &'static str {
    match shape {
        MaskShape::Linear { .. } => "Linear Gradient",
        MaskShape::Radial { .. } => "Radial Gradient",
        MaskShape::Raster { .. } => "Mask",
        MaskShape::ColorRange { .. } => "Color Range",
        MaskShape::LuminanceRange { .. } => "Luminance Range",
    }
}

fn display_name(mask: &LocalMask, index: usize) -> String {
    if mask.name.is_empty() { format!("{} {}", tr(kind_name(&mask.shape)), index + 1) } else { mask.name.clone() }
}

/// A linear gradient from `a` (full) to `b` (none), in image-relative units.
fn linear(a: [f32; 2], b: [f32; 2]) -> LocalMask {
    LocalMask { shape: MaskShape::Linear { x0: a[0], y0: a[1], x1: b[0], y1: b[1] }, ..Default::default() }
}

/// An ellipse about `centre` with radii in image-relative units.
fn radial(centre: [f32; 2], rx: f32, ry: f32) -> LocalMask {
    LocalMask { shape: MaskShape::Radial { cx: centre[0], cy: centre[1], rx, ry, angle: 0.0, feather: 50.0 }, ..Default::default() }
}

const DEFAULT_RADIUS_PX: f32 = 0.2;
const MIN_RADIUS: f32 = 0.01;
/// A press that did not move this many pixels is a click, which makes a default-sized mask.
const CLICK_SLOP: f32 = 4.0;
const HANDLE_RADIUS: f32 = 6.0;
const HIT_RADIUS: f32 = 10.0;

// --- the photo: creating, dragging handles, overlay ---------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Part {
    /// A mask being drawn: its anchor (screen) stays where the press was.
    Create(MaskTool, Pos2),
    LinearStart,
    LinearEnd,
    LinearMove,
    RadialCentre,
    RadialX,
    RadialY,
    RadialTurn,
}

#[derive(Clone, Copy)]
struct Drag {
    mask: usize,
    part: Part,
    /// Pointer minus the point it moves, so a grab does not jump.
    offset: [f32; 2],
}

fn drag_id() -> egui::Id {
    egui::Id::new("camera-raw-mask-drag")
}

struct Frame {
    rect: Rect,
}

impl Frame {
    fn screen(&self, p: [f32; 2]) -> Pos2 {
        pos2(self.rect.left() + p[0] * self.rect.width(), self.rect.top() + p[1] * self.rect.height())
    }
    fn norm(&self, p: Pos2) -> [f32; 2] {
        [(p.x - self.rect.left()) / self.rect.width().max(1.0), (p.y - self.rect.top()) / self.rect.height().max(1.0)]
    }
}

/// The ellipse's frame in screen space: centre and the unit vectors along its two axes.
fn ellipse_axes(angle: f32) -> ([f32; 2], [f32; 2]) {
    let a = angle.to_radians();
    ([a.cos(), a.sin()], [-a.sin(), a.cos()])
}

/// The handle points of `mask` on screen, with what each one does.
fn handles(frame: &Frame, mask: &LocalMask) -> Vec<(Pos2, Part)> {
    match &mask.shape {
        MaskShape::Linear { x0, y0, x1, y1 } => {
            let (a, b) = (frame.screen([*x0, *y0]), frame.screen([*x1, *y1]));
            vec![(a, Part::LinearStart), (b, Part::LinearEnd), (a.lerp(b, 0.5), Part::LinearMove)]
        }
        MaskShape::Radial { cx, cy, rx, ry, angle, .. } => {
            let c = frame.screen([*cx, *cy]);
            let (ux, uy) = ellipse_axes(*angle);
            let (ex, ey) = (vec2(ux[0], ux[1]) * (rx * frame.rect.width()), vec2(uy[0], uy[1]) * (ry * frame.rect.height()));
            let turn = c + vec2(ux[0], ux[1]) * (rx * frame.rect.width() + 22.0);
            vec![(c, Part::RadialCentre), (c + ex, Part::RadialX), (c + ey, Part::RadialY), (turn, Part::RadialTurn)]
        }
        _ => Vec::new(),
    }
}

fn clamp_unit(v: f32) -> f32 {
    v.clamp(-1.5, 2.5)
}

/// Move the part of `mask` that `drag` holds to the pointer at `pos`.
fn drag_to(frame: &Frame, mask: &mut LocalMask, drag: &Drag, pos: Pos2, shift: bool) {
    let target = frame.norm(pos);
    let moved = [clamp_unit(target[0] - drag.offset[0]), clamp_unit(target[1] - drag.offset[1])];
    match (&mut mask.shape, drag.part) {
        (MaskShape::Linear { x0, y0, x1, y1 }, Part::Create(MaskTool::Linear, anchor)) => {
            let a = frame.norm(anchor);
            let mut end = target;
            if shift {
                // Keep it straight up/down or across.
                if (end[0] - a[0]).abs() * frame.rect.width() > (end[1] - a[1]).abs() * frame.rect.height() {
                    end[1] = a[1];
                } else {
                    end[0] = a[0];
                }
            }
            (*x0, *y0, *x1, *y1) = (a[0], a[1], clamp_unit(end[0]), clamp_unit(end[1]));
        }
        (MaskShape::Linear { x0, y0, .. }, Part::LinearStart) => (*x0, *y0) = (moved[0], moved[1]),
        (MaskShape::Linear { x1, y1, .. }, Part::LinearEnd) => (*x1, *y1) = (moved[0], moved[1]),
        (MaskShape::Linear { x0, y0, x1, y1 }, Part::LinearMove) => {
            let (mx, my) = ((*x0 + *x1) / 2.0, (*y0 + *y1) / 2.0);
            let (dx, dy) = (moved[0] - mx, moved[1] - my);
            (*x0, *y0, *x1, *y1) = (clamp_unit(*x0 + dx), clamp_unit(*y0 + dy), clamp_unit(*x1 + dx), clamp_unit(*y1 + dy));
        }
        (MaskShape::Radial { cx, cy, rx, ry, .. }, Part::Create(MaskTool::Radial, anchor)) => {
            let c = frame.norm(anchor);
            let (dx, dy) = ((pos.x - anchor.x).abs(), (pos.y - anchor.y).abs());
            let (px, py) = if shift { (dx.max(dy), dx.max(dy)) } else { (dx, dy) };
            (*cx, *cy) = (c[0], c[1]);
            (*rx, *ry) = ((px / frame.rect.width().max(1.0)).max(MIN_RADIUS), (py / frame.rect.height().max(1.0)).max(MIN_RADIUS));
        }
        (MaskShape::Radial { cx, cy, .. }, Part::RadialCentre) => (*cx, *cy) = (moved[0], moved[1]),
        (MaskShape::Radial { cx, cy, rx, angle, .. }, Part::RadialX) => {
            let (ux, _) = ellipse_axes(*angle);
            let c = frame.screen([*cx, *cy]);
            let along = (pos.x - c.x) * ux[0] + (pos.y - c.y) * ux[1];
            *rx = (along / frame.rect.width().max(1.0)).clamp(MIN_RADIUS, 4.0);
        }
        (MaskShape::Radial { cx, cy, ry, angle, .. }, Part::RadialY) => {
            let (_, uy) = ellipse_axes(*angle);
            let c = frame.screen([*cx, *cy]);
            let along = (pos.x - c.x) * uy[0] + (pos.y - c.y) * uy[1];
            *ry = (along / frame.rect.height().max(1.0)).clamp(MIN_RADIUS, 4.0);
        }
        (MaskShape::Radial { cx, cy, angle, .. }, Part::RadialTurn) => {
            let c = frame.screen([*cx, *cy]);
            *angle = (pos.y - c.y).atan2(pos.x - c.x).to_degrees();
        }
        _ => {}
    }
}

/// Everything the photo does while the Masking tab is open: the red overlay, the handles and the drags. Returns whether the masks changed.
pub(super) fn canvas(ui: &Ui, d: &mut CameraRawDialog, rect: Rect, response: &Response, st: &mut MaskUiState, panning: bool) -> bool {
    let frame = Frame { rect };
    st.settle(d.params.masks.len());
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    if st.overlay && let Some(mask) = st.selected.and_then(|i| d.params.masks.get(i)) {
        let pixels = if d.processed.len() == d.pw * d.ph { &d.processed } else { &d.proxy };
        draw_overlay(ui, rect, mask, pixels, d.pw, d.ph, d.preview_revision);
    }
    if let Some(tool) = st.tool.filter(|t| matches!(t, MaskTool::Brush | MaskTool::Objects | MaskTool::Colour)) {
        let changed = tool_canvas(ui, d, rect, response, st, panning, tool);
        if changed {
            d.dirty = true;
        }
        return changed;
    }
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let shift = ui.input(|i| i.modifiers.shift);
    let pointer = ui.input(|i| i.pointer.hover_pos());
    // Where a press would land: a handle of the selected mask, else the photo (a tool is armed).
    let handle_hit = |masks: &[LocalMask], selected: Option<usize>, pos: Pos2| {
        let i = selected?;
        let mask = masks.get(i)?;
        handles(&frame, mask).into_iter().filter(|(p, _)| p.distance(pos) <= HIT_RADIUS).min_by(|a, b| a.0.distance(pos).total_cmp(&b.0.distance(pos))).map(|(p, part)| (i, p, part))
    };
    let mut drag: Option<Drag> = ui.ctx().data(|data| data.get_temp(drag_id()));
    if !primary_down {
        drag = None;
    }
    if !panning && response.drag_started_by(egui::PointerButton::Primary) && drag.is_none() {
        let press = ui.input(|i| i.pointer.press_origin()).or(pointer);
        if let Some(pos) = press {
            if let Some((i, point, part)) = handle_hit(&d.params.masks, st.selected, pos) {
                drag = Some(Drag { mask: i, part, offset: [(pos.x - point.x) / rect.width().max(1.0), (pos.y - point.y) / rect.height().max(1.0)] });
            } else if let Some(tool) = st.tool.filter(|_| rect.contains(pos)) {
                if d.params.masks.len() < photocraft_algo::camera_raw::MAX_MASKS {
                    let at = frame.norm(pos);
                    d.params.masks.push(match tool {
                        MaskTool::Linear | MaskTool::Brush | MaskTool::Objects | MaskTool::Colour => linear(at, at),
                        MaskTool::Radial => radial(at, MIN_RADIUS, MIN_RADIUS),
                    });
                    let index = d.params.masks.len() - 1;
                    st.selected = Some(index);
                    st.creating = false;
                    drag = Some(Drag { mask: index, part: Part::Create(tool, pos), offset: [0.0, 0.0] });
                    changed = true;
                }
            }
        }
    }
    if let Some(active) = drag
        && primary_down
        && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        && let Some(mask) = d.params.masks.get_mut(active.mask)
    {
        let before = mask.clone();
        drag_to(&frame, mask, &active, pos, shift);
        changed |= *mask != before;
    }
    // A press that did not move makes a default-sized mask of the armed tool.
    if !panning && response.clicked_by(egui::PointerButton::Primary) && drag.is_none() {
        if let (Some(tool), Some(pos)) = (st.tool, ui.input(|i| i.pointer.interact_pos())) {
            if rect.contains(pos) && d.params.masks.len() < photocraft_algo::camera_raw::MAX_MASKS {
                let at = frame.norm(pos);
                d.params.masks.push(match tool {
                    MaskTool::Linear | MaskTool::Brush | MaskTool::Objects | MaskTool::Colour => linear([at[0], clamp_unit(at[1] - 0.15)], [at[0], clamp_unit(at[1] + 0.15)]),
                    MaskTool::Radial => radial(at, DEFAULT_RADIUS_PX * rect.height() / rect.width().max(1.0), DEFAULT_RADIUS_PX),
                });
                st.selected = Some(d.params.masks.len() - 1);
                st.creating = false;
                st.tool = None;
                changed = true;
            }
        }
    }
    // Releasing a creation drag that hardly moved leaves a default-sized mask, and the tool is used up.
    let previous: Option<Drag> = ui.ctx().data(|data| data.get_temp(drag_id()));
    if let Some(prev) = previous
        && drag.is_none()
        && let Part::Create(tool, anchor) = prev.part
    {
        if let Some(mask) = d.params.masks.get_mut(prev.mask) {
            let end = ui.input(|i| i.pointer.interact_pos()).unwrap_or(anchor);
            if end.distance(anchor) < CLICK_SLOP {
                let at = frame.norm(anchor);
                *mask = match tool {
                    MaskTool::Linear | MaskTool::Brush | MaskTool::Objects | MaskTool::Colour => linear([at[0], clamp_unit(at[1] - 0.15)], [at[0], clamp_unit(at[1] + 0.15)]),
                    MaskTool::Radial => radial(at, DEFAULT_RADIUS_PX * rect.height() / rect.width().max(1.0), DEFAULT_RADIUS_PX),
                };
                changed = true;
            }
        }
        st.tool = None;
    }
    ui.ctx().data_mut(|data| {
        if let Some(active) = drag {
            data.insert_temp(drag_id(), active);
        } else {
            data.remove::<Drag>(drag_id());
        }
    });
    // Handles of the selected mask, over the photo.
    if let Some(mask) = st.selected.and_then(|i| d.params.masks.get(i)) {
        draw_guides(ui, &frame, mask, drag.map(|g| g.part), pointer, &t);
    }
    if st.tool.is_some() && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    } else if let Some(pos) = pointer.filter(|_| response.hovered() && !panning)
        && handle_hit(&d.params.masks, st.selected, pos).is_some()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if changed {
        d.dirty = true;
    }
    changed
}

/// The tools that work on the picture's pixels instead of a drawn shape: Brush, Objects and Color Range. Returns whether the masks changed.
fn tool_canvas(ui: &Ui, d: &mut CameraRawDialog, rect: Rect, response: &Response, st: &mut MaskUiState, panning: bool, tool: MaskTool) -> bool {
    let frame = Frame { rect };
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let at = ui.input(|i| i.pointer.interact_pos());
    let down = ui.input(|i| i.pointer.primary_down());
    let mut changed = false;
    match tool {
        MaskTool::Brush => {
            let id = egui::Id::new("camera-raw-brush-last");
            let last: Option<Pos2> = ui.ctx().data(|data| data.get_temp(id));
            let dragging = !panning && down && (response.dragged_by(egui::PointerButton::Primary) || response.drag_started_by(egui::PointerButton::Primary));
            match at.filter(|p| dragging && (last.is_some() || rect.contains(*p))) {
                Some(pos) => {
                    let erase = st.brush.erase || ui.input(|i| i.modifiers.alt);
                    changed = paint(d, st, &frame, last, pos, erase);
                    ui.ctx().data_mut(|data| data.insert_temp(id, pos));
                }
                None => ui.ctx().data_mut(|data| data.remove::<Pos2>(id)),
            }
            if let Some(p) = pointer.filter(|p| response.hovered() && rect.contains(*p)) {
                let radius = st.brush.size / 100.0 * rect.width().max(rect.height()) / 2.0;
                ui.painter().circle_stroke(p, radius.max(2.0), Stroke::new(2.5, Color32::from_black_alpha(140)));
                ui.painter().circle_stroke(p, radius.max(2.0), Stroke::new(1.0, Color32::WHITE));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
        MaskTool::Objects => {
            let id = egui::Id::new("camera-raw-objects-anchor");
            let anchor: Option<Pos2> = ui.ctx().data(|data| data.get_temp(id));
            if !panning
                && response.drag_started_by(egui::PointerButton::Primary)
                && let Some(press) = ui.input(|i| i.pointer.press_origin()).filter(|p| rect.contains(*p))
            {
                ui.ctx().data_mut(|data| data.insert_temp(id, press));
            } else if let Some(start) = anchor {
                if down {
                    if let Some(now) = at {
                        let r = Rect::from_two_pos(start, now);
                        ui.painter().rect_stroke(r, 0.0, Stroke::new(2.5, Color32::from_black_alpha(140)), egui::StrokeKind::Outside);
                        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Outside);
                    }
                } else {
                    ui.ctx().data_mut(|data| data.remove::<Pos2>(id));
                    if let Some(end) = at {
                        changed = select_object(d, st, &frame, (start, end));
                    }
                }
            }
            if !panning
                && response.clicked_by(egui::PointerButton::Primary)
                && let Some(pos) = at.filter(|p| rect.contains(*p))
            {
                changed |= select_object(d, st, &frame, (pos, pos));
            }
            if pointer.is_some_and(|p| response.hovered() && rect.contains(p)) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
        MaskTool::Colour => {
            if !panning
                && response.clicked_by(egui::PointerButton::Primary)
                && let Some(pos) = at.filter(|p| rect.contains(*p))
            {
                changed = pick_colour(d, st, &frame, pos);
            }
            if pointer.is_some_and(|p| response.hovered() && rect.contains(p)) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
        MaskTool::Linear | MaskTool::Radial => {}
    }
    changed
}

/// A mask was made or changed: select it and leave the creation panel.
fn adopt(st: &mut MaskUiState, d: &CameraRawDialog) {
    st.selected = Some(d.params.masks.len().saturating_sub(1));
    st.creating = false;
}

/// One Brush stroke segment, into the selected raster mask (a new Brush mask when none is selected).
fn paint(d: &mut CameraRawDialog, st: &mut MaskUiState, frame: &Frame, last: Option<Pos2>, pos: Pos2, erase: bool) -> bool {
    let target = st.selected.filter(|i| d.params.masks.get(*i).is_some_and(|m| matches!(m.shape, MaskShape::Raster { .. })));
    let index = match target {
        Some(i) => i,
        None => {
            if d.params.masks.len() >= photocraft_algo::camera_raw::MAX_MASKS {
                return false;
            }
            let (w, h) = Brush::map_size(d.pw, d.ph);
            d.params.masks.push(LocalMask { name: tr("Brush").to_string(), shape: MaskShape::raster_from_bytes(w, h, &vec![0u8; w as usize * h as usize]), ..Default::default() });
            adopt(st, d);
            d.params.masks.len() - 1
        }
    };
    let Some((w, h, mut bytes)) = d.params.masks[index].shape.raster_bytes() else { return false };
    let to_map = |p: Pos2| {
        let n = frame.norm(p);
        (n[0] * w as f32, n[1] * h as f32)
    };
    let to = to_map(pos);
    st.brush.stroke(&mut bytes, w, h, last.map_or(to, to_map), to, erase);
    d.params.masks[index].shape = MaskShape::raster_from_bytes(w, h, &bytes);
    true
}

/// The preview picture as the segmentation code reads it.
fn picture(d: &CameraRawDialog) -> photocraft_algo::segment::RgbImage {
    let (w, h) = (d.pw, d.ph);
    let shown = CameraRawDialog::image(&d.proxy, w, h, &d.display);
    photocraft_algo::segment::RgbImage::from_fn(w, h, |x, y| {
        let c = shown.pixels[y * w + x];
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0]
    })
}

/// Objects: the object inside a dragged box, or under a click (`a` and `b` close together).
fn select_object(d: &mut CameraRawDialog, st: &mut MaskUiState, frame: &Frame, (a, b): (Pos2, Pos2)) -> bool {
    use photocraft_algo::segment::{ImageSampler, grabcut};
    if d.params.masks.len() >= photocraft_algo::camera_raw::MAX_MASKS {
        st.notice = Some(tr("A photo can have at most 16 masks.").to_string());
        return false;
    }
    let (w, h) = (d.pw, d.ph);
    let to_px = |p: Pos2| {
        let n = frame.norm(p);
        ((n[0] * w as f32).clamp(0.0, w as f32 - 1.0), (n[1] * h as f32).clamp(0.0, h as f32 - 1.0))
    };
    let (pa, pb) = (to_px(a), to_px(b));
    let img = picture(d);
    let sampler = ImageSampler { img: &img, origin: (0, 0) };
    let canvas = photocraft_geom::Rect::new(0, 0, w as i32, h as i32);
    let region = if a.distance(b) < CLICK_SLOP {
        grabcut::object_at(&sampler, canvas, pa.0, pa.1, 600)
    } else {
        let boxed = photocraft_geom::Rect::new(pa.0.min(pb.0) as i32, pa.1.min(pb.1) as i32, pa.0.max(pb.0) as i32 + 1, pa.1.max(pb.1) as i32 + 1);
        grabcut::object_select(&sampler, canvas, boxed, 600)
    };
    match region.and_then(|r| mask_from_region(&r, w, h, tr("Objects"))) {
        Some(mask) => {
            d.params.masks.push(mask);
            adopt(st, d);
            st.tool = None;
            st.notice = None;
            true
        }
        None => {
            st.notice = Some(tr("Nothing was found there.").to_string());
            false
        }
    }
}

/// Color Range: every pixel near the colour that was clicked.
fn pick_colour(d: &mut CameraRawDialog, st: &mut MaskUiState, frame: &Frame, pos: Pos2) -> bool {
    if d.params.masks.len() >= photocraft_algo::camera_raw::MAX_MASKS {
        st.notice = Some(tr("A photo can have at most 16 masks.").to_string());
        return false;
    }
    let n = frame.norm(pos);
    let (x, y) = (((n[0] * d.pw as f32) as usize).min(d.pw.saturating_sub(1)), ((n[1] * d.ph as f32) as usize).min(d.ph.saturating_sub(1)));
    let pixels = if d.processed.len() == d.pw * d.ph { &d.processed } else { &d.proxy };
    let Some(p) = pixels.get(y * d.pw + x) else { return false };
    let color = [p[0].clamp(-0.5, 2.0), p[1].clamp(-0.5, 2.0), p[2].clamp(-0.5, 2.0)];
    d.params.masks.push(LocalMask { name: tr("Color Range").to_string(), shape: MaskShape::ColorRange { color, range: 30.0, smooth: 30.0 }, ..Default::default() });
    adopt(st, d);
    st.tool = None;
    st.notice = None;
    true
}

/// Luminance Range: the brightest part of the picture to start with; its Min and Max are set in the panel.
pub(super) fn add_luminance_range(d: &mut CameraRawDialog, st: &mut MaskUiState) -> bool {
    if d.params.masks.len() >= photocraft_algo::camera_raw::MAX_MASKS {
        st.notice = Some(tr("A photo can have at most 16 masks.").to_string());
        return false;
    }
    d.params.masks.push(LocalMask { name: tr("Luminance Range").to_string(), shape: MaskShape::LuminanceRange { low: 0.5, high: 1.0, smooth: 20.0 }, ..Default::default() });
    adopt(st, d);
    st.tool = None;
    st.notice = None;
    d.dirty = true;
    true
}

/// The mask's coverage as a red veil over the photo (made at most a few hundred pixels across and kept until the mask changes).
fn draw_overlay(ui: &Ui, rect: Rect, mask: &LocalMask, pixels: &[[f32; 4]], pw: usize, ph: usize, revision: u64) {
    use std::hash::{Hash, Hasher};
    let key = {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(mask).unwrap_or_default().hash(&mut hasher);
        // A Range mask follows the picture, so the veil is made again when the picture changes.
        if matches!(mask.shape, MaskShape::ColorRange { .. } | MaskShape::LuminanceRange { .. }) {
            revision.hash(&mut hasher);
        }
        hasher.finish()
    };
    let id = egui::Id::new("camera-raw-mask-overlay");
    let cached: Option<(u64, egui::TextureHandle)> = ui.ctx().data(|data| data.get_temp(id));
    let texture = match cached {
        Some((k, handle)) if k == key => handle,
        _ => {
            let aspect = rect.width().max(1.0) / rect.height().max(1.0);
            let (w, h) = if aspect >= 1.0 { (320usize, (320.0 / aspect).round().max(1.0) as usize) } else { ((320.0 * aspect).round().max(1.0) as usize, 320usize) };
            let coverage = mask.coverage_fn(w, h);
            let at = |x: usize, y: usize| {
                if coverage.needs_pixels() && pw > 0 && ph > 0 && pixels.len() >= pw * ph {
                    let p = pixels[(y * ph / h).min(ph - 1) * pw + (x * pw / w).min(pw - 1)];
                    coverage.at_px(x, y, [p[0], p[1], p[2]])
                } else {
                    coverage.at(x, y)
                }
            };
            let veil: Vec<Color32> = (0..w * h).map(|i| Color32::from_rgba_unmultiplied(235, 40, 40, (at(i % w, i / w).clamp(0.0, 1.0) * 150.0) as u8)).collect();
            let image = egui::ColorImage { size: [w, h], source_size: vec2(w as f32, h as f32), pixels: veil };
            let handle = ui.ctx().load_texture("camera-raw-mask-overlay", image, TextureOptions::LINEAR);
            ui.ctx().data_mut(|data| data.insert_temp(id, (key, handle.clone())));
            handle
        }
    };
    ui.painter().image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

fn line(ui: &Ui, a: Pos2, b: Pos2, width: f32) {
    ui.painter().line_segment([a, b], Stroke::new(width + 2.0, Color32::from_black_alpha(130)));
    ui.painter().line_segment([a, b], Stroke::new(width, Color32::WHITE));
}

fn draw_guides(ui: &Ui, frame: &Frame, mask: &LocalMask, active: Option<Part>, pointer: Option<Pos2>, t: &Tokens) {
    let clip = frame.rect;
    let painter = ui.painter().with_clip_rect(clip.expand(20.0));
    let _ = (&painter, t);
    match &mask.shape {
        MaskShape::Linear { x0, y0, x1, y1 } => {
            let (a, b) = (frame.screen([*x0, *y0]), frame.screen([*x1, *y1]));
            let along = (b - a).normalized();
            let across = if along.is_finite() { vec2(-along.y, along.x) } else { vec2(1.0, 0.0) };
            let reach = clip.width().max(clip.height()) * 1.5;
            // The start line (full effect) and the end line (none), and the axis between them.
            line(ui, a - across * reach, a + across * reach, 1.5);
            line(ui, b - across * reach, b + across * reach, 1.0);
            line(ui, a, b, 1.0);
        }
        MaskShape::Radial { cx, cy, rx, ry, angle, feather } => {
            let c = frame.screen([*cx, *cy]);
            let (ux, uy) = ellipse_axes(*angle);
            let ring = |scale: f32| -> Vec<Pos2> {
                (0..=96)
                    .map(|i| {
                        let a = i as f32 / 96.0 * std::f32::consts::TAU;
                        let (x, y) = (a.cos() * rx * scale * clip.width(), a.sin() * ry * scale * clip.height());
                        c + vec2(ux[0] * x + uy[0] * y, ux[1] * x + uy[1] * y)
                    })
                    .collect()
            };
            ui.painter().add(egui::Shape::line(ring(1.0), Stroke::new(3.5, Color32::from_black_alpha(130))));
            ui.painter().add(egui::Shape::line(ring(1.0), Stroke::new(1.5, Color32::WHITE)));
            let inner = 1.0 - (feather / 100.0).clamp(0.0, 1.0);
            if inner > 0.02 && inner < 0.98 {
                ui.painter().add(egui::Shape::line(ring(inner), Stroke::new(1.0, Color32::from_white_alpha(150))));
            }
            let x_handle = c + vec2(ux[0], ux[1]) * (rx * clip.width());
            let turn = c + vec2(ux[0], ux[1]) * (rx * clip.width() + 22.0);
            line(ui, x_handle, turn, 1.0);
        }
        _ => {}
    }
    for (point, part) in handles(frame, mask) {
        let hot = active == Some(part) || pointer.is_some_and(|p| p.distance(point) <= HIT_RADIUS);
        let radius = if hot { HANDLE_RADIUS + 1.5 } else { HANDLE_RADIUS };
        let fill = if matches!(part, Part::RadialTurn) { Color32::from_rgb(240, 200, 60) } else { Color32::WHITE };
        ui.painter().circle_filled(point, radius, if active == Some(part) { fill } else { Color32::from_black_alpha(120) });
        ui.painter().circle_stroke(point, radius, Stroke::new(1.5, fill));
    }
}

// --- keys -------------------------------------------------------------------------------------------------------------------------

/// G and J arm the gradient tools, O toggles the overlay, Delete removes the selected mask. Only on the Masking tab, never while typing.
pub(super) fn keys(ctx: &egui::Context, st: &mut MaskUiState, params: &mut CameraRaw, dirty: &mut bool) {
    if ctx.text_edit_focused() {
        return;
    }
    let pressed = |key: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    if pressed(Key::G) {
        st.tool = Some(MaskTool::Linear);
    }
    if pressed(Key::J) {
        st.tool = Some(MaskTool::Radial);
    }
    if pressed(Key::K) {
        st.tool = Some(MaskTool::Brush);
        st.selected = None;
    }
    if st.tool.is_some() && pressed(Key::Escape) {
        st.tool = None;
    }
    if st.selected.is_some() && pressed(Key::O) {
        st.overlay = !st.overlay;
    }
    if (pressed(Key::Delete) || pressed(Key::Backspace))
        && let Some(i) = st.selected
        && i < params.masks.len()
    {
        params.masks.remove(i);
        st.settle(params.masks.len());
        *dirty = true;
    }
}

// --- the panel ----------------------------------------------------------------------------------------------------------------------

/// The Masking tab once masks exist: the list, and the selected mask's settings and local adjustments. Returns whether anything changed.
pub(super) fn panel(ui: &mut Ui, p: &mut CameraRaw, st: &mut MaskUiState) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut dirty = false;
    st.settle(p.masks.len());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("Masks")).color(t.text).size(14.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(tr("Create New Mask")).clicked() {
                st.creating = true;
            }
        });
    });
    ui.add_space(6.0);
    let mut remove = None;
    for (i, mask) in p.masks.iter_mut().enumerate() {
        let selected = st.selected == Some(i);
        let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
        if selected {
            ui.painter().rect_filled(row, t.radius_sm, t.accent_soft);
        } else if response.hovered() {
            ui.painter().rect_filled(row, t.radius_sm, t.hover);
        }
        let eye = Rect::from_center_size(pos2(row.left() + 14.0, row.center().y), vec2(20.0, 20.0));
        let eye_response = ui.interact(eye, ui.id().with(("mask-eye", i)), Sense::click());
        let colour = if mask.enabled { t.text } else { t.text_faint };
        ui.painter().circle_stroke(eye.center(), 6.0, Stroke::new(1.5, colour));
        if mask.enabled {
            ui.painter().circle_filled(eye.center(), 2.5, colour);
        }
        eye_response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, mask.enabled, tr("Show")));
        if eye_response.clicked() {
            mask.enabled = !mask.enabled;
            dirty = true;
        }
        let close = Rect::from_center_size(pos2(row.right() - 14.0, row.center().y), vec2(20.0, 20.0));
        let close_response = ui.interact(close, ui.id().with(("mask-delete", i)), Sense::click());
        ui.painter().text(close.center(), Align2::CENTER_CENTER, "x", FontId::proportional(13.0), if close_response.hovered() { t.text } else { t.text_faint });
        close_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr("Delete Mask")));
        if close_response.clicked() {
            remove = Some(i);
        }
        ui.painter().text(pos2(row.left() + 30.0, row.center().y), Align2::LEFT_CENTER, display_name(mask, i), FontId::proportional(12.5), colour);
        if response.clicked() && !eye_response.clicked() && !close_response.clicked() {
            st.selected = Some(i);
            st.tool = None;
        }
    }
    if let Some(i) = remove {
        p.masks.remove(i);
        st.settle(p.masks.len());
        dirty = true;
    }
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(6.0);
    if st.tool == Some(MaskTool::Brush) {
        // Brush settings are view state, not part of the photo's settings, so they never mark the photo changed.
        let mut ignored = false;
        super::row(ui, &mut ignored, "Size", &mut st.brush.size, Brush::SIZE, None);
        super::row(ui, &mut ignored, "Feather", &mut st.brush.feather, 0.0..=100.0, None);
        super::row(ui, &mut ignored, "Flow", &mut st.brush.flow, 1.0..=100.0, None);
        ui.checkbox(&mut st.brush.erase, tr("Erase"));
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);
    }
    let Some(index) = st.selected.filter(|i| *i < p.masks.len()) else {
        ui.label(egui::RichText::new(tr("Select a mask to edit it.")).color(t.text_faint));
        return dirty;
    };
    ui.checkbox(&mut st.overlay, tr("Show Overlay"));
    let mask = &mut p.masks[index];
    dirty |= ui.checkbox(&mut mask.invert, tr("Invert")).changed();
    if let MaskShape::Radial { feather, .. } = &mut mask.shape {
        super::row(ui, &mut dirty, "Feather", feather, 0.0..=100.0, None);
    }
    match &mut mask.shape {
        MaskShape::ColorRange { range, smooth, .. } => {
            super::row(ui, &mut dirty, "Range", range, 0.0..=100.0, None);
            super::row(ui, &mut dirty, "Smoothness", smooth, 0.0..=100.0, None);
        }
        MaskShape::LuminanceRange { low, high, smooth } => {
            // Stored as 0 to 1, shown as 0 to 100; the two ends never cross.
            let (mut lo, mut hi) = (*low * 100.0, *high * 100.0);
            let (mut lo_moved, mut hi_moved) = (false, false);
            super::row(ui, &mut lo_moved, "Luminance Min", &mut lo, 0.0..=100.0, None);
            super::row(ui, &mut hi_moved, "Luminance Max", &mut hi, 0.0..=100.0, None);
            if lo_moved {
                *low = (lo / 100.0).clamp(0.0, 1.0);
                *high = high.max(*low);
                dirty = true;
            }
            if hi_moved {
                *high = (hi / 100.0).clamp(0.0, 1.0);
                *low = low.min(*high);
                dirty = true;
            }
            super::row(ui, &mut dirty, "Smoothness", smooth, 0.0..=100.0, None);
        }
        _ => {}
    }
    super::row(ui, &mut dirty, "Amount", &mut mask.amount, 0.0..=100.0, None);
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    let a = &mut mask.adjust;
    super::row(ui, &mut dirty, "Temperature", &mut a.temperature, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Tint", &mut a.tint, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Exposure", &mut a.exposure, -5.0..=5.0, None);
    super::row(ui, &mut dirty, "Contrast", &mut a.contrast, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Highlights", &mut a.highlights, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Shadows", &mut a.shadows, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Whites", &mut a.whites, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Blacks", &mut a.blacks, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Texture", &mut a.texture, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Clarity", &mut a.clarity, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Dehaze", &mut a.dehaze, -100.0..=100.0, None);
    super::row(ui, &mut dirty, "Saturation", &mut a.saturation, -100.0..=100.0, None);
    dirty
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame { rect: Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 200.0)) }
    }

    #[test]
    fn screen_and_image_coordinates_round_trip() {
        let f = frame();
        for p in [[0.0, 0.0], [1.0, 1.0], [0.25, 0.8], [-0.1, 1.3]] {
            let back = f.norm(f.screen(p));
            assert!((back[0] - p[0]).abs() < 1e-5 && (back[1] - p[1]).abs() < 1e-5, "{p:?} -> {back:?}");
        }
        assert_eq!(f.screen([0.5, 0.5]), pos2(300.0, 150.0));
    }

    #[test]
    fn dragging_out_a_linear_gradient_anchors_the_start_and_follows_the_pointer() {
        let f = frame();
        let anchor = f.screen([0.5, 0.2]);
        let mut mask = linear([0.5, 0.2], [0.5, 0.2]);
        let hold = Drag { mask: 0, part: Part::Create(MaskTool::Linear, anchor), offset: [0.0, 0.0] };
        drag_to(&f, &mut mask, &hold, f.screen([0.5, 0.7]), false);
        assert_eq!(mask.shape, MaskShape::Linear { x0: 0.5, y0: 0.2, x1: 0.5, y1: 0.7 });
        // Shift keeps it straight along the longer way.
        drag_to(&f, &mut mask, &hold, f.screen([0.58, 0.9]), true);
        let MaskShape::Linear { x1, y1, .. } = mask.shape else { panic!() };
        assert!((x1 - 0.5).abs() < 1e-5 && (y1 - 0.9).abs() < 1e-5);
    }

    #[test]
    fn dragging_a_linear_gradients_handles_moves_one_end_or_both() {
        let f = frame();
        let mut mask = linear([0.2, 0.2], [0.2, 0.6]);
        drag_to(&f, &mut mask, &Drag { mask: 0, part: Part::LinearEnd, offset: [0.0, 0.0] }, f.screen([0.8, 0.9]), false);
        assert_eq!(mask.shape, MaskShape::Linear { x0: 0.2, y0: 0.2, x1: 0.8, y1: 0.9 });
        drag_to(&f, &mut mask, &Drag { mask: 0, part: Part::LinearStart, offset: [0.0, 0.0] }, f.screen([0.1, 0.1]), false);
        assert_eq!(mask.shape, MaskShape::Linear { x0: 0.1, y0: 0.1, x1: 0.8, y1: 0.9 });
        // Grabbing the middle moves both ends together.
        let mid = [(0.1 + 0.8) / 2.0, (0.1 + 0.9) / 2.0];
        drag_to(&f, &mut mask, &Drag { mask: 0, part: Part::LinearMove, offset: [0.0, 0.0] }, f.screen([mid[0] + 0.1, mid[1] - 0.05]), false);
        let MaskShape::Linear { x0, y0, x1, y1 } = mask.shape else { panic!() };
        assert!((x0 - 0.2).abs() < 1e-5 && (y0 - 0.05).abs() < 1e-5 && (x1 - 0.9).abs() < 1e-5 && (y1 - 0.85).abs() < 1e-5);
    }

    #[test]
    fn a_radial_mask_is_drawn_from_its_centre_resized_and_turned() {
        let f = frame();
        let anchor = f.screen([0.5, 0.5]);
        let mut mask = radial([0.5, 0.5], MIN_RADIUS, MIN_RADIUS);
        drag_to(&f, &mut mask, &Drag { mask: 0, part: Part::Create(MaskTool::Radial, anchor), offset: [0.0, 0.0] }, f.screen([0.7, 0.6]), false);
        let MaskShape::Radial { cx, cy, rx, ry, .. } = mask.shape else { panic!() };
        assert!((cx - 0.5).abs() < 1e-5 && (cy - 0.5).abs() < 1e-5 && (rx - 0.2).abs() < 1e-4 && (ry - 0.1).abs() < 1e-4, "{rx} {ry}");
        // Shift makes it round on screen: equal pixel radii.
        drag_to(&f, &mut mask, &Drag { mask: 0, part: Part::Create(MaskTool::Radial, anchor), offset: [0.0, 0.0] }, f.screen([0.7, 0.6]), true);
        let MaskShape::Radial { rx, ry, .. } = mask.shape else { panic!() };
        assert!((rx * 400.0 - ry * 200.0).abs() < 0.01, "round on screen");
        // The X handle sets rx along the axis; turning it rotates the ellipse.
        drag_to(&f, &mut mask, &Drag { mask: 0, part: Part::RadialX, offset: [0.0, 0.0] }, f.screen([0.9, 0.5]), false);
        let MaskShape::Radial { rx, .. } = mask.shape else { panic!() };
        assert!((rx - 0.4).abs() < 1e-4, "{rx}");
        drag_to(&f, &mut mask, &Drag { mask: 0, part: Part::RadialTurn, offset: [0.0, 0.0] }, pos2(f.screen([0.5, 0.5]).x, f.screen([0.5, 0.5]).y + 50.0), false);
        let MaskShape::Radial { angle, .. } = mask.shape else { panic!() };
        assert!((angle - 90.0).abs() < 1e-3, "{angle}");
        // Radii never collapse or run away.
        drag_to(&f, &mut mask, &Drag { mask: 0, part: Part::RadialY, offset: [0.0, 0.0] }, f.screen([0.5, 0.5]), false);
        let MaskShape::Radial { ry, .. } = mask.shape else { panic!() };
        assert!(ry >= MIN_RADIUS);
    }

    #[test]
    fn handles_are_found_for_the_selected_mask_and_a_dragged_one_does_not_jump() {
        let f = frame();
        let mask = linear([0.2, 0.2], [0.2, 0.8]);
        let hs = handles(&f, &mask);
        assert_eq!(hs.len(), 3);
        assert!(hs.iter().any(|(p, part)| *part == Part::LinearEnd && p.distance(f.screen([0.2, 0.8])) < 1e-4));
        // Grabbing 5 px away from the end handle keeps that 5 px while it moves.
        let grab = Pos2::new(f.screen([0.2, 0.8]).x + 5.0, f.screen([0.2, 0.8]).y);
        let hold = Drag { mask: 0, part: Part::LinearEnd, offset: [5.0 / 400.0, 0.0] };
        let mut moved = mask.clone();
        drag_to(&f, &mut moved, &hold, grab, false);
        assert_eq!(moved.shape, mask.shape, "no movement, no jump");
    }

    #[test]
    fn the_subject_map_from_image_triage_becomes_a_raster_mask() {
        let (w, h) = (8u32, 4u32);
        let data: Vec<u8> = (0..w * h).map(|i| if i % w < 4 { 255 } else { 0 }).collect();
        let image = photocraft_codecs::Image::from_u8(w, h, photocraft_codecs::ChannelLayout::Gray, data).unwrap();
        let png = photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default()).unwrap();
        let subject = mask_from_png(&png, false).unwrap();
        let background = mask_from_png(&png, true).unwrap();
        assert!(!subject.invert && background.invert);
        assert_eq!(subject.shape, background.shape);
        let cover = subject.coverage_fn(8, 4);
        assert!(cover.at(1, 2) > 0.99 && cover.at(6, 2) < 0.01, "left half is the subject");
        let outside = background.coverage_fn(8, 4);
        assert!(outside.at(1, 2) < 0.01 && outside.at(6, 2) > 0.99, "Background is the rest");
        // Damaged or oversized answers are refused, not drawn.
        assert!(mask_from_png(b"not a png", false).is_err());
        let big = photocraft_codecs::Image::from_u8(1537, 2, photocraft_codecs::ChannelLayout::Gray, vec![0; 1537 * 2]).unwrap();
        let big = photocraft_codecs::encode(&big, photocraft_codecs::Format::Png, &Default::default()).unwrap();
        assert!(mask_from_png(&big, false).is_err());
    }

    #[test]
    fn keys_and_state_stay_sane() {
        let mut st = MaskUiState::default();
        st.selected = Some(3);
        st.settle(2);
        assert_eq!(st.selected, Some(1));
        st.settle(0);
        assert_eq!(st.selected, None);
        assert!(!st.takes_drags(true));
        st.tool = Some(MaskTool::Linear);
        assert!(st.takes_drags(true) && !st.takes_drags(false), "only on the Masking tab");
        assert_eq!(serde_json::from_value::<MaskUiState>(serde_json::to_value(&st).unwrap()).unwrap(), st);
    }
}
