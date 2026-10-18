//! ACR filmstrip presentation over a bounded, host-owned photo window.
//! This module never opens files, develops photos, or owns the host's browse order.
use std::collections::BTreeMap;

use base64::Engine as _;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::theme::Tokens;

mod icons;

const MAX_ITEMS: usize = 32;
const MAX_TOTAL: usize = 1_000_000;
const MAX_EDGE: usize = 384;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Design {
    pub enabled: bool,
    pub height: f32,
    pub cell_width: f32,
    pub horizontal_padding: f32,
    pub top_padding: f32,
    pub bottom_padding: f32,
    pub gap: f32,
    pub selection_width: f32,
    pub image_inset: f32,
    pub show_filenames: bool,
    pub show_ratings: bool,
}

impl Default for Design {
    fn default() -> Self {
        Self {
            enabled: true,
            height: 140.0,
            cell_width: 171.5,
            horizontal_padding: 0.0,
            top_padding: 2.0,
            bottom_padding: 1.0,
            gap: 4.0,
            selection_width: 3.0,
            image_inset: 4.0,
            show_filenames: false,
            show_ratings: false,
        }
    }
}

impl Design {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value, low, high) in [
            ("height", self.height, 48.0, 280.0),
            ("cellWidth", self.cell_width, 60.0, 360.0),
            ("horizontalPadding", self.horizontal_padding, 0.0, 16.0),
            ("topPadding", self.top_padding, 0.0, 16.0),
            ("bottomPadding", self.bottom_padding, 0.0, 16.0),
            ("gap", self.gap, 0.0, 24.0),
            ("selectionWidth", self.selection_width, 0.5, 6.0),
            ("imageInset", self.image_inset, 2.0, 16.0),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(format!("filmstrip.{name} must be within {low}..={high}"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Item {
    index: usize,
    id: String,
    name: String,
    #[serde(default)]
    rating: u8,
    #[serde(default)]
    rejected: bool,
    #[serde(default)]
    edited: bool,
    #[serde(default)]
    version: String,
    #[serde(default)]
    thumbnail: Option<Thumbnail>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Thumbnail {
    width: usize,
    height: usize,
    rgba: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Update {
    generation: u64,
    total: usize,
    current: usize,
    items: Vec<Item>,
    #[serde(default = "yes")]
    enabled: bool,
    collapsed: Option<bool>,
    ack: Option<u64>,
}
fn yes() -> bool {
    true
}

#[derive(Clone, Default)]
struct State {
    hosted: bool,
    enabled: bool,
    collapsed: bool,
    comparison_split: bool,
    generation: u64,
    total: usize,
    current: usize,
    items: Vec<Item>,
    textures: BTreeMap<String, (String, TextureHandle)>,
    scroll: f32,
    first: usize,
    last: usize,
    serial: u64,
    request: Option<Value>,
    rect: Option<Rect>,
}

fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp(egui::Id::new("camera-raw-filmstrip"))).unwrap_or_default()
}
fn store(ctx: &egui::Context, value: State) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("camera-raw-filmstrip"), value));
}

/// Hosted launches negotiate with the parent before drawing a strip. Standalone
/// previews can seed their one-photo strip directly from the dialog's texture.
pub fn hosted(ctx: &egui::Context) {
    let mut s = state(ctx);
    s.hosted = true;
    s.enabled = false;
    store(ctx, s);
}

pub(crate) fn height(ctx: &egui::Context) -> f32 {
    let s = state(ctx);
    let design = crate::camera_raw_design::get(ctx);
    let available = !s.hosted || (s.enabled && s.total > 0);
    if available && !s.collapsed && design.filmstrip.enabled { design.filmstrip.height } else { 0.0 }
}

pub(crate) fn seed(ctx: &egui::Context, name: &str, texture: Option<&TextureHandle>) {
    let mut s = state(ctx);
    if s.hosted {
        return;
    }
    s.enabled = true;
    s.total = 1;
    s.items = vec![Item {
        index: 0,
        id: "standalone".into(),
        name: name.into(),
        rating: 0,
        rejected: false,
        edited: false,
        version: "standalone".into(),
        thumbnail: None,
    }];
    if let Some(texture) = texture {
        s.textures.insert("standalone".into(), ("standalone".into(), texture.clone()));
    }
    store(ctx, s);
}

fn view_design(ctx:&egui::Context)->Design {
    let mut d=crate::camera_raw_design::get(ctx).filmstrip.clone();
    if let Some(p)=crate::camera_raw_ui::preferences::effective(ctx) {d.show_filenames=p.show_filenames;}
    d
}
pub(crate) fn inspect(ctx: &egui::Context) -> Value {
    let s = state(ctx);
    json!({"version":1, "ready":s.hosted && s.generation > 0, "enabled":s.enabled, "collapsed":s.collapsed, "comparisonSplit":s.comparison_split, "generation":s.generation,
        "display":{"showFilenames":view_design(ctx).show_filenames,"showRatings":view_design(ctx).show_ratings},
        "total":s.total, "current":s.current, "first":s.first, "last":s.last, "request":s.request,
        "selected":s.items.iter().find(|item| item.index == s.current).map(|item|json!({"id":item.id,"name":item.name,"rating":item.rating,"rejected":item.rejected,"edited":item.edited})),
        "rect":s.rect.map(|r|json!({"x":r.left(),"y":r.top(),"width":r.width(),"height":r.height()}))})
}

pub(crate) fn comparison_split(ctx: &egui::Context) -> bool {
    state(ctx).comparison_split
}

/// One bounded batch, at most one RGBA display thumbnail, no filesystem access.
/// Validate everything before replacing the last good browse state or acknowledging an action.
pub(crate) fn control(ctx: &egui::Context, p: &Value) -> Result<Value, String> {
    if p.as_object().is_some_and(serde_json::Map::is_empty) {
        return Ok(inspect(ctx));
    }
    let update: Update = serde_json::from_value(p.clone()).map_err(|e| format!("filmstrip: {e}"))?;
    let mut s = state(ctx);
    if update.generation == 0 || update.total > MAX_TOTAL || (update.total > 0 && update.current >= update.total) || update.items.len() > MAX_ITEMS {
        return Err("filmstrip: invalid generation, current photo or window size".into());
    }
    if s.hosted && update.generation < s.generation {
        return Err("filmstrip: stale browse generation".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut decoded = Vec::new();
    for item in &update.items {
        if item.index >= update.total
            || item.id.is_empty()
            || item.id.len() > 4096
            || item.name.len() > 1024
            || item.version.len() > 128
            || item.rating > 5
            || !seen.insert(item.index)
            || !ids.insert(item.id.clone())
        {
            return Err("filmstrip: invalid or duplicate photo metadata".into());
        }
        if let Some(thumb) = &item.thumbnail {
            if !decoded.is_empty()
                || thumb.width == 0
                || thumb.height == 0
                || thumb.width > MAX_EDGE
                || thumb.height > MAX_EDGE
                || thumb.rgba.len() > MAX_EDGE * MAX_EDGE * 4 * 4 / 3 + 4
            {
                return Err("filmstrip: thumbnail budget exceeded".into());
            }
            let rgba = base64::engine::general_purpose::STANDARD.decode(&thumb.rgba).map_err(|_| "filmstrip: invalid base64 thumbnail")?;
            if rgba.len() != thumb.width * thumb.height * 4 {
                return Err("filmstrip: thumbnail dimensions do not match its pixels".into());
            }
            decoded.push((item.id.clone(), item.version.clone(), egui::ColorImage::from_rgba_unmultiplied([thumb.width, thumb.height], &rgba)));
        }
    }
    let new_generation = update.generation != s.generation;
    if new_generation {
        s.textures.clear();
        s.request = None;
        s.scroll = 0.0;
    }
    let old_current = s.current;
    s.hosted = true;
    s.enabled = update.enabled;
    s.generation = update.generation;
    s.total = update.total;
    s.current = update.current;
    if let Some(collapsed) = update.collapsed {
        s.collapsed = collapsed;
    }
    if update.ack.is_some_and(|ack| s.request.as_ref().and_then(|r| r.get("serial")).and_then(Value::as_u64).is_some_and(|serial| serial <= ack)) {
        s.request = None;
    }
    s.items = update
        .items
        .into_iter()
        .map(|mut item| {
            item.thumbnail = None;
            item
        })
        .collect();
    s.textures.retain(|id, _| ids.contains(id));
    for (id, version, image) in decoded {
        let texture = ctx.load_texture(format!("camera-raw-thumb-{id}"), image, egui::TextureOptions::LINEAR);
        s.textures.insert(id, (version, texture));
    }
    if old_current != s.current || new_generation {
        let pitch = crate::camera_raw_design::get(ctx).filmstrip.cell_width + crate::camera_raw_design::get(ctx).filmstrip.gap;
        let width = s.rect.map_or(pitch, |r| r.width());
        let left = s.current as f32 * pitch;
        if left < s.scroll {
            s.scroll = left;
        }
        if left + pitch > s.scroll + width {
            s.scroll = (left + pitch - width).max(0.0);
        }
    }
    store(ctx, s);
    ctx.request_repaint();
    Ok(inspect(ctx))
}

fn select(s: &mut State, index: usize) {
    request(s, index, "select", Value::Null);
}

fn request(s: &mut State, index: usize, kind: &str, value: Value) {
    if !s.hosted || index >= s.total {
        return;
    }
    let Some(item) = s.items.iter().find(|item| item.index == index) else {
        return;
    };
    s.serial = s.serial.saturating_add(1);
    s.request = Some(json!({"serial":s.serial,"generation":s.generation,"kind":kind,"index":index,"id":item.id,"value":value}));
}

pub(crate) fn draw(ui: &mut Ui, rect: Rect) {
    let mut s = state(ui.ctx());
    if !s.enabled || s.collapsed || rect.height() <= 0.0 {
        return;
    }
    let d=view_design(ui.ctx());
    let t = Tokens::get(ui.ctx());
    s.rect = Some(rect);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.tab_strip);
    let response = ui.interact(rect, ui.id().with("acr-filmstrip-scroll"), Sense::hover());
    let pitch = d.cell_width + d.gap;
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        s.scroll -= if scroll.x.abs() > scroll.y.abs() { scroll.x } else { scroll.y };
    }
    s.scroll = s.scroll.clamp(0.0, (s.total as f32 * pitch - rect.width() + d.horizontal_padding * 2.0).max(0.0));
    s.first = (s.scroll / pitch).floor() as usize;
    s.last = (s.first + (rect.width() / pitch).ceil() as usize + 1).min(s.total).min(s.first.saturating_add(MAX_ITEMS));
    let mut clicked = None;
    for item in s.items.iter().filter(|item| item.index >= s.first && item.index < s.last) {
        let cell = Rect::from_min_size(
            pos2(rect.left() + d.horizontal_padding + item.index as f32 * pitch - s.scroll, rect.top() + d.top_padding),
            vec2(d.cell_width, (rect.height() - d.top_padding - d.bottom_padding).max(1.0)),
        );
        let response = ui.interact(cell.intersect(rect), ui.id().with(("acr-filmstrip-photo", &item.id)), Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, item.index == s.current, &item.name));
        if response.clicked() {
            clicked = Some(item.index);
        }
        let mut image_rect = cell.shrink(d.image_inset);
        if d.show_filenames {
            image_rect.max.y -= 16.0;
        }
        if d.show_ratings {
            image_rect.max.y -= 14.0;
        }
        if let Some((version, texture)) = s.textures.get(&item.id).filter(|(version, _)| *version == item.version) {
            let _ = version;
            let size = texture.size_vec2();
            let scale = (image_rect.width() / size.x).min(image_rect.height() / size.y);
            let fitted = Rect::from_center_size(image_rect.center(), size * scale);
            painter.image(texture.id(), fitted, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        if item.index == s.current {
            painter.rect_stroke(cell, 0.0, Stroke::new(d.selection_width, t.filmstrip_selection), StrokeKind::Inside);
        }
        if d.show_filenames {
            painter.text(pos2(cell.center().x, cell.bottom() - 8.0), Align2::CENTER_CENTER, &item.name, FontId::proportional(10.0), t.text_dim);
        }
        if d.show_ratings {
            painter.text(
                pos2(cell.center().x, image_rect.bottom() + 8.0),
                Align2::CENTER_CENTER,
                "★".repeat(usize::from(item.rating)),
                FontId::proportional(10.0),
                t.text_dim,
            );
        }
        if response.hovered() {
            response.on_hover_text(format!("{}{}{}", item.name, if item.rejected { " · Rejected" } else { "" }, if item.edited { " · Edited" } else { "" }));
        }
    }
    if let Some(index) = clicked {
        select(&mut s, index);
    }
    store(ui.ctx(), s);
}

pub(crate) fn navigation(ui: &mut Ui, navigation: Rect, before: &mut bool) {
    toggle(ui);
    let t = Tokens::get(ui.ctx());
    let mut s = state(ui.ctx());
    ui.add_space(31.0);
    for (kind, label) in [("sort", tl!("Sort")), ("filter", tl!("Filter"))] {
        let (rect, response) = ui.allocate_exact_size(vec2(36.0, 38.0), Sense::click());
        let c = rect.center() + if kind == "filter" { vec2(-3.0, 0.0) } else { vec2(0.5, 0.0) };
        if kind == "sort" {
            icons::sort(ui.painter(), c, t.icon);
        } else {
            icons::filter(ui.painter(), c, t.icon);
        }
        let response = response.on_hover_text(label);
        egui::Popup::menu(&response).show(|ui| {
            let options: &[&str] =
                if kind == "sort" { &["Filename", "Date Modified", "File Size", "File Type"] } else { &["All", "Rejects Only", "Unreviewed", "Edited"] };
            for option in options {
                if ui.add_enabled(s.hosted, egui::Button::new(*option)).clicked() {
                    let current = s.current;
                    request(&mut s, current, kind, json!(option));
                    ui.close();
                }
            }
        });
    }
    let review = Rect::from_center_size(navigation.center(), vec2(150.0, 24.0));
    ui.painter().rect_filled(review, 12.0, t.filmstrip_review);
    let rating = s.items.iter().find(|item| item.index == s.current).map_or(0, |item| item.rating);
    for star in 1..=5u8 {
        let c = pos2(review.left() + 18.0 + f32::from(star - 1) * 22.0, review.center().y);
        let points = (0..10)
            .map(|point| {
                let angle = std::f32::consts::TAU * point as f32 / 10.0 - std::f32::consts::FRAC_PI_2;
                let radius = if point % 2 == 0 { 5.7 } else { 2.3 };
                c + vec2(angle.cos(), angle.sin()) * radius
            })
            .collect();
        ui.painter().add(egui::Shape::closed_line(points, Stroke::new(1.1, if star <= rating { t.text } else { t.filmstrip_review_icon })));
        let response = ui.interact(Rect::from_center_size(c, vec2(22.0, 24.0)), ui.id().with(("acr-rating", star)), Sense::click());
        if response.clicked() {
            let current = s.current;
            request(&mut s, current, "rating", json!(if rating == star { 0 } else { star }));
        }
    }
    let x = review.right() - 30.0;
    ui.painter().line_segment([pos2(x, review.top()), pos2(x, review.bottom())], Stroke::new(0.5, t.separator));
    let c = pos2(review.right() - 15.5, review.center().y);
    icons::reject(ui.painter(), c, t.filmstrip_review_icon);
    if ui.interact(Rect::from_center_size(c, vec2(30.0, 24.0)), ui.id().with("acr-reject"), Sense::click()).on_hover_text(tl!("Toggle rejected")).clicked() {
        let current = s.current;
        request(&mut s, current, "reject", Value::Null);
    }
    let rect = Rect::from_center_size(pos2(navigation.right() - 69.0, navigation.center().y), vec2(36.0, 38.0));
    let c = rect.center();
    ui.painter().rect_filled(Rect::from_center_size(c, vec2(12.0, 12.0)), 0.0, t.icon);
    icons::corner(ui.painter(), c, t.icon);
    if ui.interact(rect, ui.id().with("acr-before"), Sense::click()).on_hover_text(tl!("Before (Y)")).clicked() {
        *before = !*before;
        s.comparison_split = false;
    }
    let x = navigation.right() - 49.5;
    ui.painter().line_segment([pos2(x, navigation.top() + 6.0), pos2(x, navigation.bottom() - 6.0)], Stroke::new(1.0, t.separator));
    let rect = Rect::from_center_size(pos2(navigation.right() - 29.5, navigation.center().y), vec2(36.0, 38.0));
    let ink = if s.comparison_split { t.icon } else { t.filmstrip_review_icon };
    icons::comparison(ui.painter(), rect.center(), ink);
    if ui.interact(rect, ui.id().with("acr-before-after-layout"), Sense::click()).on_hover_text(tl!("Before / After")).clicked() {
        s.comparison_split = !s.comparison_split;
        *before = false;
    }
    if s.hosted && !ui.ctx().text_edit_focused() && !ui.input(|i| i.modifiers.any()) && !egui::Popup::is_any_open(ui.ctx()) {
        let target = ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                Some(s.current.saturating_add(1))
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                Some(s.current.saturating_sub(1))
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown) {
                Some(s.current.saturating_add(s.last.saturating_sub(s.first).max(1)))
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp) {
                Some(s.current.saturating_sub(s.last.saturating_sub(s.first).max(1)))
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                Some(0)
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                Some(s.total.saturating_sub(1))
            } else {
                None
            }
        });
        if let Some(target) = target {
            let current = s.current;
            let target = target.min(s.total.saturating_sub(1));
            request(&mut s, current, "navigate", json!(target));
        }
    }
    store(ui.ctx(), s);
}

fn toggle(ui: &mut Ui) {
    let mut s = state(ui.ctx());
    let t = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(38.0, ui.available_height().clamp(20.0, 40.0)), Sense::click());
    let center = rect.center();
    icons::toggle(ui.painter(), center, s.collapsed, t.icon, t.dock);
    if response.clicked() {
        s.collapsed = !s.collapsed;
        store(ui.ctx(), s);
        ui.ctx().request_repaint();
    }
    response.on_hover_text(tl!("Show or hide filmstrip"));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hosted_strip_waits_for_the_parent_to_detach_its_strip() {
        let ctx = egui::Context::default();
        hosted(&ctx);
        seed(&ctx, "photo", None);
        assert_eq!(height(&ctx), 0.0);
        let mut packet = update(1);
        packet["enabled"] = json!(false);
        assert_eq!(control(&ctx, &packet).unwrap()["ready"], true);
        assert_eq!(height(&ctx), 0.0);
        packet["enabled"] = json!(true);
        control(&ctx, &packet).unwrap();
        assert!(height(&ctx) > 0.0);
    }
    fn update(generation: u64) -> Value {
        json!({"generation":generation,"total":2,"current":0,"items":[{"index":0,"id":"a","name":"a.nef"},{"index":1,"id":"b","name":"b.nef"}]})
    }
    #[test]
    fn invalid_updates_keep_the_last_good_generation_and_pending_selection() {
        let ctx = egui::Context::default();
        control(&ctx, &update(2)).unwrap();
        let mut s = state(&ctx);
        select(&mut s, 1);
        store(&ctx, s);
        assert!(control(&ctx, &update(1)).is_err());
        let mut bad = update(3);
        bad["items"][0]["thumbnail"] = json!({"width":384,"height":384,"rgba":"AA=="});
        assert!(control(&ctx, &bad).is_err());
        assert_eq!(inspect(&ctx)["generation"], 2);
        assert_eq!(inspect(&ctx)["request"]["id"], "b");
    }
    #[test]
    fn acknowledgement_is_idempotent_and_new_browse_order_discards_old_actions() {
        let ctx = egui::Context::default();
        control(&ctx, &update(1)).unwrap();
        let mut s = state(&ctx);
        select(&mut s, 1);
        store(&ctx, s);
        assert_eq!(inspect(&ctx)["request"]["serial"], 1);
        let mut next = update(1);
        next["ack"] = json!(1);
        control(&ctx, &next).unwrap();
        assert!(inspect(&ctx)["request"].is_null());
        control(&ctx, &update(2)).unwrap();
        assert!(inspect(&ctx)["request"].is_null());
    }

    #[test]
    fn empty_windows_remove_the_tray_without_losing_the_session() {
        let ctx = egui::Context::default();
        control(&ctx, &update(1)).unwrap();
        control(&ctx, &json!({"generation":2,"total":0,"current":0,"items":[]})).unwrap();
        assert_eq!(height(&ctx), 0.0);
        assert_eq!(inspect(&ctx)["generation"], 2);
    }

    #[test]
    fn invalid_metadata_and_oversized_thumbnails_are_atomic() {
        let ctx = egui::Context::default();
        control(&ctx, &update(1)).unwrap();
        for replacement in [json!({"rating":6}), json!({"index":2}), json!({"id":"b"}), json!({"thumbnail":{"width":385,"height":1,"rgba":"AAAA"}})] {
            let mut packet = update(2);
            for (key, value) in replacement.as_object().unwrap() {
                packet["items"][0][key] = value.clone();
            }
            assert!(control(&ctx, &packet).is_err());
            assert_eq!(inspect(&ctx)["generation"], 1);
        }
    }

    #[test]
    fn collapsed_tray_keyboard_navigation_uses_the_stable_current_anchor() {
        let ctx = egui::Context::default();
        let mut packet = update(1);
        packet["total"] = json!(100);
        packet["collapsed"] = json!(true);
        control(&ctx, &packet).unwrap();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
            events: vec![egui::Event::Key { key: egui::Key::End, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }],
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            navigation(ui, Rect::from_min_size(pos2(0.0, 500.0), vec2(800.0, 40.0)), &mut false);
        })
        .textures_delta
        .clear();
        let request = inspect(&ctx)["request"].clone();
        assert_eq!(request["kind"], "navigate");
        assert_eq!(request["id"], "a");
        assert_eq!(request["value"], 99);
    }
}
