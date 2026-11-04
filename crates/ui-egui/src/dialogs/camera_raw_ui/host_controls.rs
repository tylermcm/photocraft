//! Image Triage's embedded Camera Raw presentation. The standalone editor uses upstream controls.
use super::controls::{commit, demosaic_row, history_keys, remember_edit, temp_stops, tint_stops};
use super::menu::menu_update;
use super::open::{Target, cancel, raw_stage};
use super::sidecar::{load_settings, report_file_error, save_settings};
use super::{
    Align2, BUSY_AFTER, CURVE_LINE_GRAB, CURVE_STOP_RADIUS, CameraRaw, Color32, ERect, FontId, PhotocraftApp, Pos2, REGION_NAMES, RgbHistogram, Sense, Stroke,
    Tokens, ToneZone, bars, crop, curve_context_menu, curve_lut, curve_regions, done, extra_panels, finishing, host_job, info, json, layout, mask_edit,
    masking, mixer, parametric_curve, parametric_interact, pos2, preferences, presets, region_value, remove, save_options, set_region, settings_menu,
    slider_default, snapshots, tool_panels, vec2, widgets,
};
pub(super) fn draw(app: &mut PhotocraftApp, ctx: &egui::Context, own_window: bool) {
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    preferences::sync(app, ctx);
    let design = crate::camera_raw_design::get(ctx);
    crate::camera_raw_design::apply_preview(ctx, &design, &mut app.ui.camera_raw_workspace);
    crate::camera_raw_tool_bar_ui::sync(ctx, &app.ui.camera_raw_workspace, &app.ui.camera_raw_scope);
    let current_display = app.session.active().and_then(|st| app.session.color.canvas_display(&st.doc).ok());
    let mut action: Option<&str> = None;
    let menu_was_open = (app.ui.camera_raw_workspace.remove_help && app.ui.camera_raw_workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Remove)
        || (app.ui.camera_raw_workspace.masking_help && app.ui.camera_raw_workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Masking)
        || app.ui.camera_raw_workspace.settings_menu
        || app.ui.camera_raw_workspace.preferences
        || app.ui.camera_raw_workspace.save_options
        || app.ui.camera_raw_menu.snapshot_picker
        || app.camera_raw.as_ref().is_some_and(|d| d.snapshots.creating());
    // The photo was left for the next one, which has not taken the dialog over yet: nothing in the dialog may act
    // meanwhile, and if the swap takes more than a moment the controls dim to say so.
    let busy_since = app.camera_raw.as_ref().and_then(|d| d.busy_since);
    let busy = busy_since.is_some();
    let busy_visible = busy_since.is_some_and(|since| since.elapsed() >= BUSY_AFTER);
    if busy && !busy_visible {
        ctx.request_repaint_after(BUSY_AFTER);
    }
    egui::Area::new(egui::Id::unique("camera-raw-dialog")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let Some(d) = app.camera_raw.as_mut() else { return };
        let frame_origin = prepare_preview(ui, ctx, d, &mut app.ui, &current_display, menu_was_open);
        crate::camera_raw_filmstrip_ui::seed(ctx, &d.document_name, d.tex.as_ref());
        let (full, _) = ui.allocate_exact_size(screen.size(), Sense::click());
        let painter = ui.painter().clone();
        painter.rect_filled(full, 0.0, t.chrome);
        let lay = layout(ctx, screen, own_window);
        painter.rect_filled(lay.body, 0.0, t.chrome);
        painter.rect_filled(lay.preview_body, 0.0, t.canvas);
        let title = lay.title;
        {
            painter.rect_filled(title, 0.0, t.dock);
            painter.rect_filled(ERect::from_min_max(pos2(title.left(), title.bottom() - 1.0), title.max), 0.0, t.separator);
            painter.with_clip_rect(ERect::from_min_max(title.min, pos2(lay.preview_body.right(), title.bottom()))).text(
                pos2(lay.preview_body.center().x, title.center().y),
                Align2::CENTER_CENTER,
                d.header_label(),
                FontId::proportional(13.0),
                t.text,
            );
        }
        let ok_label = if d.target == Target::OpenRaw { tl!("Open") } else { tl!("OK") };
        let scope = &mut app.ui.camera_raw_scope;
        bars::top_actions(ui, title, &mut action, scope);
        // Clip both pixels and scope overlays before painting the settings dock/footer.
        let view = lay.view;
        d.viewport = Some(view);
        d.preview_ppp = ctx.pixels_per_point();
        let size = vec2(d.full_w as f32, d.full_h as f32) / ctx.pixels_per_point();
        let navigation = &mut app.ui.camera_raw_preview;
        let workspace = &mut app.ui.camera_raw_workspace;
        let mut preview = ui.new_child(egui::UiBuilder::new().max_rect(view));
        preview.set_clip_rect(view.intersect(ui.clip_rect()));
        preview.painter().rect_filled(view, 0.0, t.canvas);
        let mixer_active =
            workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Edit && workspace.panels.open.iter().any(|name| name == "Color Mixer");
        if !mixer_active {
            workspace.mixer_tools.picker = false;
        }
        if workspace.mixer_tools.picker {
            d.show_before = false;
        }
        let masking_tab = workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Masking;
        let mut preview_sampling =
            scope.sampler_tool || workspace.white_balance_tool || workspace.mixer_tools.picker || workspace.mask_ui.takes_drags(masking_tab);
        let (r, response, panning, zoom_box) = crate::camera_raw_preview_ui::interact_hosted(&mut preview, navigation, view, size, &mut preview_sampling);
        if !preview_sampling {
            scope.sampler_tool = false;
            workspace.white_balance_tool = false;
            workspace.mixer_tools.picker = false;
        }
        let tex = if d.show_before { d.before_tex.as_ref() } else { d.tex.as_ref() };
        if let Some(tex) = tex {
            // Never tile a checker across the offscreen extent of a highly zoomed image.
            widgets::checker(preview.painter(), r.intersect(view), 8.0);
            preview.painter().image(tex.id(), r, ERect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            let preview_parameters = crate::camera_raw_workspace_ui::preview_without(&d.params, d.preview_bypass.as_deref());
            let raw_tone = d.raw_curve.is_some() && !raw_stage(&preview_parameters).is_neutral();
            // A stand-in made for browsing has no more detail to show: zoomed in, the photo is developed in full by Open.
            // While a setting is being dragged the proxy is shown and no full-resolution develop is started for each position the
            // slider passes; the exact detail follows the release, for the final settings.
            let adjusting = ctx.input(|i| i.pointer.primary_down()) && d.edit_origin.as_ref().is_some_and(|origin| *origin != d.params);
            let detail_needed = !adjusting
                && !raw_tone
                && (r.width() * ctx.pixels_per_point() > d.pw as f32 || r.height() * ctx.pixels_per_point() > d.ph as f32)
                && !d.is_stand_in();
            d.detail.paint(&preview, r, d.preview_revision, d.show_before, &preview_parameters, detail_needed, &d.display);
            if crate::camera_raw_filmstrip_ui::comparison_split(ctx)
                && let Some(before) = d.before_tex.as_ref()
            {
                let left = ERect::from_min_max(view.min, pos2(r.center().x, view.bottom())).intersect(view);
                preview.painter().with_clip_rect(left).image(before.id(), r, ERect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                preview.painter().line_segment([pos2(r.center().x, view.top()), pos2(r.center().x, view.bottom())], Stroke::new(1.0, t.separator));
            }
            if d.detail.pending() {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            d.scope.preview_rect = Some(r);
            crate::camera_raw_scope_ui::preview(&mut preview, d, scope, r, &response, panning);
            bars::pick_white_balance(&preview, d, r, &response, &mut workspace.white_balance_tool, panning);
            if masking_tab {
                mask_edit::canvas(&preview, d, r, &response, &mut workspace.mask_ui, panning);
            }
            if mixer_active {
                finishing::preview(&preview, d, r, &response, &mut workspace.mixer_tools, panning);
            }
        }
        crate::camera_raw_workspace_ui::grid(&preview, r, view, workspace);
        ui.add_enabled_ui(!busy_visible, |ui| crate::camera_raw_tool_bar_ui::draw(ui, lay.tool_bar, workspace, scope));
        if let Some(region) = zoom_box {
            preview.painter().rect_stroke(region, 0.0, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        }
        crate::camera_raw_filmstrip_ui::draw(ui, lay.filmstrip);
        painter.rect_filled(lay.nav, 0.0, t.dock);
        painter.line_segment([lay.nav.left_top(), lay.nav.right_top()], Stroke::new(1.0, t.separator));
        let mut nav_ui =
            ui.new_child(egui::UiBuilder::new().max_rect(lay.nav.shrink2(vec2(0.0, 1.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        nav_ui.set_clip_rect(lay.nav.intersect(ui.clip_rect()));
        crate::camera_raw_preview_ui::toolbar_hosted(&mut nav_ui, navigation, view, size, &mut scope.sampler_tool);
        crate::camera_raw_filmstrip_ui::navigation(&mut nav_ui, lay.nav, &mut d.show_before);
        if d.detail.pending() || busy_visible {
            nav_ui.spinner();
        }
        if let Some(error) = &d.detail.error {
            nav_ui.label(egui::RichText::new(tl!("Full-resolution preview unavailable")).color(t.text_faint)).on_hover_text(error);
        }
        // Panels.
        let right = lay.right;
        painter.rect_filled(right, 0.0, t.dock);
        let mut props = ui.new_child(egui::UiBuilder::new().max_rect(right));
        props.style_mut().override_font_id = Some(FontId::proportional(design.panel_font_size));
        props.spacing_mut().item_spacing.y = 0.0;
        // Overflow must not resize a curve while selecting a point reveals its fields.
        props.spacing_mut().scroll.floating = true;
        if busy_visible {
            props.disable();
        }
        // This header is outside the settings scroll area and follows the same Before/After
        // selector as the texture. Both derive from the same completed preview revision.
        crate::camera_raw_scope_ui::header(&mut props, d, scope);
        bars::metadata(&mut props, &d.camera_info);
        let show_snapshots = !workspace.info && workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Snapshots;
        let show_presets = !workspace.info && workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Presets;
        if show_presets {
            presets::scrollbar_style(&mut props);
            presets::header(&mut props);
        } else if show_snapshots {
            snapshots::header(&mut props, &mut d.snapshots, &d.params);
        } else if !workspace.info && workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Edit {
            bars::edit_bar(&mut props, d, false);
            bars::profile_bar(&mut props);
        }
        d.scope.alt_tone = None;
        let _ = ctx.data_mut(|data| data.remove_temp::<String>(crate::camera_raw_workspace_ui::held_bypass_id()));
        let mut dirty = false;
        egui::ScrollArea::vertical().id_salt(if show_presets { "camera-raw-presets" } else { "camera-raw-props" }).auto_shrink([false, false]).show(
            &mut props,
            |ui| {
                if workspace.info {
                    ui.set_width(right.width());
                    info::show(ui, d);
                    return;
                }
                if show_presets {
                    ui.set_width((right.width() - design.settings_scrollbar_gutter).max(1.0));
                    presets::list(ui, workspace);
                    return;
                }
                if show_snapshots {
                    ui.set_width(right.width());
                    if let Some(parameters) = snapshots::list(ui, &mut d.snapshots) {
                        d.params = parameters;
                        dirty = true;
                    }
                    return;
                }
                if workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Remove {
                    ui.set_width(right.width());
                    remove::show(ui, &mut workspace.remove_groups, &mut workspace.remove_help);
                    return;
                }
                if workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Masking {
                    ui.set_width(right.width());
                    // The creation panel until there is a mask (or Create New Mask is pressed), then the list and its settings.
                    if d.params.masks.is_empty() || workspace.mask_ui.creating {
                        let now = ui.ctx().input(|i| i.time);
                        let mut asked = None;
                        let tool_before = workspace.mask_ui.tool;
                        let host_ready = masking::HostReady::of(&d.host_job, now);
                        if workspace.mask_ui.combine.is_some() {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr_ctx(
                                    crate::i18n::current(),
                                    "cameraRaw",
                                    "The next shape you make is combined into the selected mask.",
                                ))
                                .color(Tokens::get(ui.ctx()).text_dim),
                            );
                            ui.add_space(4.0);
                        }
                        masking::show(ui, &mut workspace.masking_people, &mut workspace.masking_help, &mut workspace.mask_ui.tool, &mut asked, host_ready);
                        if let Some(kind) = asked {
                            workspace.mask_ui.notice = match kind {
                                mask_edit::AutoMask::Sky => mask_edit::make_sky(d, &mut workspace.mask_ui),
                                mask_edit::AutoMask::LuminanceRange => {
                                    mask_edit::add_luminance_range(d, &mut workspace.mask_ui);
                                    Ok(())
                                }
                                _ => mask_edit::request_auto(d, &workspace.mask_ui, kind, now),
                            }
                            .err();
                        }
                        // Arming a tool from here starts a new mask: nothing stays selected to be painted into.
                        if workspace.mask_ui.tool != tool_before && workspace.mask_ui.tool.is_some() {
                            workspace.mask_ui.selected = None;
                        }
                        dirty |= mask_edit::candidates_panel(ui, d, &mut workspace.mask_ui);
                        if let Some(notice) = &workspace.mask_ui.notice {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(notice).color(Tokens::get(ui.ctx()).danger));
                        }
                    } else {
                        dirty |= mask_edit::panel(ui, &mut d.params, &mut workspace.mask_ui);
                    }
                    return;
                }
                if workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Crop {
                    ui.set_width(right.width());
                    crop::show(ui, &mut workspace.crop_groups);
                    return;
                }
                if workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::RedEye {
                    ui.set_width(right.width());
                    tool_panels::red_eye(ui);
                    return;
                }
                // Reserve the same dock gutter with or without an active scrollbar. Otherwise
                // a full-height overlay changes wheel/curve widths merely because it fits.
                ui.set_width((right.width() - design.settings_scrollbar_gutter).max(1.0));
                ui.spacing_mut().item_spacing.y = design.row_gap;
                let p = &mut d.params;
                let alt = &mut d.scope.alt_tone;
                let (ts, tn) = (temp_stops(), tint_stops());
                let color_track = t.camera_raw_color_track();
                for panel_name in &design.panel_order {
                    match panel_name.as_str() {
                        "Light" => {
                            crate::camera_raw_workspace_ui::section(ui, "Light", &mut workspace.panels, |ui| {
                                tone_row(ui, &mut dirty, alt, ToneZone::Exposure, &mut p.exposure);
                                row(ui, &mut dirty, "Contrast", &mut p.contrast, -100.0..=100.0, None);
                                tone_row(ui, &mut dirty, alt, ToneZone::Highlights, &mut p.highlights);
                                tone_row(ui, &mut dirty, alt, ToneZone::Shadows, &mut p.shadows);
                                tone_row(ui, &mut dirty, alt, ToneZone::Whites, &mut p.whites);
                                tone_row(ui, &mut dirty, alt, ToneZone::Blacks, &mut p.blacks);
                            });
                        }
                        "Color" => {
                            crate::camera_raw_workspace_ui::section(ui, "Color", &mut workspace.panels, |ui| {
                                dirty |= bars::white_balance(ui, p, &mut workspace.white_balance_tool);
                                if workspace.white_balance_tool {
                                    scope.sampler_tool = false;
                                    workspace.mixer_tools.picker = false;
                                }
                                row(ui, &mut dirty, "Temperature", &mut p.temperature, -100.0..=100.0, Some(&ts));
                                row(ui, &mut dirty, "Tint", &mut p.tint, -100.0..=100.0, Some(&tn));
                                row(ui, &mut dirty, "Vibrance", &mut p.vibrance, -100.0..=100.0, Some(&color_track));
                                row(ui, &mut dirty, "Saturation", &mut p.saturation, -100.0..=100.0, Some(&color_track));
                            });
                        }
                        "Effects" => {
                            crate::camera_raw_workspace_ui::section(ui, "Effects", &mut workspace.panels, |ui| {
                                row(ui, &mut dirty, "Texture", &mut p.texture, -100.0..=100.0, None);
                                row(ui, &mut dirty, "Clarity", &mut p.clarity, -100.0..=100.0, None);
                                row(ui, &mut dirty, "Dehaze", &mut p.dehaze, -100.0..=100.0, None);
                                dirty |= crate::camera_raw_workspace_ui::disclosed_slider(
                                    ui,
                                    "Vignette",
                                    &mut p.vignette_amount,
                                    -100.0..=100.0,
                                    &mut workspace.vignette_details,
                                );
                                if workspace.vignette_details {
                                    let mut style = p.vignette_style.clone();
                                    if widgets::dropdown(
                                        ui,
                                        "cr-vig-style",
                                        tl!("Style"),
                                        &mut style,
                                        &[
                                            ("highlightPriority".to_string(), tl!("Highlight Priority")),
                                            ("colorPriority".to_string(), tl!("Color Priority")),
                                            ("paintOverlay".to_string(), tl!("Paint Overlay")),
                                        ],
                                        200.0,
                                    ) {
                                        p.vignette_style = style;
                                        dirty = true;
                                    }
                                    let _ = slider_default(ui, &mut dirty, "Midpoint", &mut p.vignette_midpoint, 0.0..=100.0, None, 50.0);
                                    row(ui, &mut dirty, "Roundness", &mut p.vignette_roundness, -100.0..=100.0, None);
                                    let _ = slider_default(ui, &mut dirty, "Feather", &mut p.vignette_feather, 0.0..=100.0, None, 50.0);
                                    row(ui, &mut dirty, "Highlights", &mut p.vignette_highlights, 0.0..=100.0, None);
                                }
                                dirty |= crate::camera_raw_workspace_ui::disclosed_slider(
                                    ui,
                                    "Grain",
                                    &mut p.grain_amount,
                                    0.0..=100.0,
                                    &mut workspace.grain_details,
                                );
                                if workspace.grain_details {
                                    let _ = slider_default(ui, &mut dirty, "Size", &mut p.grain_size, 0.0..=100.0, None, 25.0);
                                    let _ = slider_default(ui, &mut dirty, "Roughness", &mut p.grain_roughness, 0.0..=100.0, None, 50.0);
                                }
                                dirty |= finishing::glow(ui, p, &mut workspace.glow_details);
                            });
                        }
                        "Curve" => {
                            crate::camera_raw_workspace_ui::section(ui, "Curve", &mut workspace.panels, |ui| {
                                use crate::camera_raw_workspace_ui::CurveChannel;
                                let previous = workspace.curve_channel;
                                bars::curve_selectors(ui, &mut workspace.curve_channel);
                                if previous != workspace.curve_channel {
                                    d.curve_state = Default::default();
                                }
                                ui.add_space(design.curve_graph_top_gap);
                                d.curve_rect = Some(curve_editor(
                                    ui,
                                    p,
                                    &mut dirty,
                                    &mut d.curve_state,
                                    workspace.curve_channel,
                                    &mut workspace.curve_show_all,
                                    &d.after_histogram,
                                ));
                                if workspace.curve_channel == CurveChannel::Parametric {
                                    dirty |= curve_regions::show(ui, &mut p.curve_splits);
                                    row(ui, &mut dirty, "Highlights", &mut p.curve_highlights, -100.0..=100.0, None);
                                    row(ui, &mut dirty, "Lights", &mut p.curve_lights, -100.0..=100.0, None);
                                    row(ui, &mut dirty, "Darks", &mut p.curve_darks, -100.0..=100.0, None);
                                    row(ui, &mut dirty, "Shadows", &mut p.curve_shadows, -100.0..=100.0, None);
                                }
                            });
                        }
                        "Color Mixer" => {
                            crate::camera_raw_workspace_ui::section(ui, "Color Mixer", &mut workspace.panels, |ui| {
                                dirty |= mixer::show(ui, p, &mut workspace.mixer, &mut workspace.band, &mut workspace.hsl_channel, &mut workspace.mixer_tools);
                            });
                        }
                        "Color Grading" => {
                            crate::camera_raw_workspace_ui::section(ui, "Color Grading", &mut workspace.panels, |ui| {
                                dirty |= crate::camera_raw_workspace_ui::grading(ui, p, &mut workspace.grading);
                            });
                        }
                        "Detail" => {
                            crate::camera_raw_workspace_ui::section(ui, "Detail", &mut workspace.panels, |ui| {
                                // Upstream's open-time demosaic choice (Open re-develops with it; the preview keeps the method the file was opened with).
                                if let Some(raw) = d.raw_open.as_mut() {
                                    demosaic_row(ui, &mut raw.demosaic);
                                }
                                dirty |= extra_panels::detail_enhancements(ui, &mut p.super_resolution);
                                dirty |= crate::camera_raw_workspace_ui::disclosed_slider(
                                    ui,
                                    "Sharpening",
                                    &mut p.sharpen_amount,
                                    0.0..=150.0,
                                    &mut workspace.sharpening_details,
                                );
                                if workspace.sharpening_details {
                                    let _ = slider_default(ui, &mut dirty, "Radius", &mut p.sharpen_radius, 0.5..=3.0, None, 1.0);
                                    let _ = slider_default(ui, &mut dirty, "Detail", &mut p.sharpen_detail, 0.0..=100.0, None, 25.0);
                                    row(ui, &mut dirty, "Masking", &mut p.sharpen_masking, 0.0..=100.0, None);
                                }
                                ui.add_space(design.detail_noise_gap);
                                extra_panels::noise_disclosure(ui, &mut workspace.noise_details);
                                if workspace.noise_details {
                                    row(ui, &mut dirty, "Luminance", &mut p.noise_luminance, 0.0..=100.0, None);
                                    let _ = slider_default(ui, &mut dirty, "Luminance Detail", &mut p.noise_luminance_detail, 0.0..=100.0, None, 50.0);
                                    row(ui, &mut dirty, "Color", &mut p.noise_color, 0.0..=100.0, None);
                                    let _ = slider_default(ui, &mut dirty, "Color Detail", &mut p.noise_color_detail, 0.0..=100.0, None, 50.0);
                                }
                            });
                        }
                        "Optics" | "Lens Blur" | "Calibration" => {
                            crate::camera_raw_workspace_ui::section(ui, panel_name, &mut workspace.panels, |ui| {
                                if panel_name == "Calibration" {
                                    dirty |= extra_panels::calibration(ui, p);
                                } else {
                                    extra_panels::show(ui, panel_name);
                                }
                            });
                        }
                        _ => {}
                    }
                }
            },
        );
        let bypass =
            ctx.data_mut(|data| data.remove_temp::<String>(crate::camera_raw_workspace_ui::held_bypass_id())).or_else(|| workspace.bypass_section.clone());
        if d.preview_bypass != bypass {
            d.preview_bypass = bypass;
            d.dirty = true;
            ctx.request_repaint();
        }
        crate::camera_raw_scope_ui::floating(ctx, d, scope, right);
        if !menu_was_open && workspace.tab == crate::camera_raw_workspace_ui::WorkspaceTab::Masking {
            // Before the scope's own keys: with a mask selected O is the overlay.
            mask_edit::keys(ctx, &mut workspace.mask_ui, &mut d.params, &mut d.dirty);
        }
        if !menu_was_open {
            crate::camera_raw_scope_ui::shortcuts(ctx, scope);
        }
        if scope.sampler_tool {
            workspace.white_balance_tool = false;
            workspace.mixer_tools.picker = false;
        }
        if workspace.mixer_tools.picker {
            scope.sampler_tool = false;
            workspace.white_balance_tool = false;
        }
        d.dirty |= dirty;
        if let Some((tag, output)) = d.host_job.take_mask_ready() {
            workspace.mask_ui.notice = mask_edit::finish_auto(d, &mut workspace.mask_ui, &tag, &output).err();
        }
        d.host_job.show(ctx);
        if d.dirty {
            ctx.request_repaint();
        }
        if d.render_pending() {
            // The worker asks for a repaint when it is done; this catches a missed wake-up.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        ui.add_enabled_ui(!busy_visible, |ui| crate::camera_raw_workspace_ui::rail(ui, lay.rail, workspace, navigation, &mut scope.sampler_tool));
        // Section headers paint opaque backgrounds over the dock's earlier edge.
        // ACR keeps one continuous dock boundary through headers and bodies;
        // paint it after the controls instead of outlining each section.
        painter.line_segment([right.left_top(), right.left_bottom()], Stroke::new(1.0, t.separator));
        // Footer.
        let foot = lay.foot;
        painter.rect_filled(foot, 0.0, t.dock);
        painter.line_segment([foot.left_top(), foot.right_top()], Stroke::new(1.0, t.separator));
        if let Some(selected) = footer_action(ui, foot, app.raw_workspace.enabled, busy_visible, d.target, ok_label) {
            action = Some(selected);
        }
        finish_preview_edit(d, ctx, frame_origin);
    });
    if !busy && !app.ui.camera_raw_workspace.save_options && !app.ui.camera_raw_workspace.preferences {
        settings_menu::show(app, ctx, screen, &mut action);
    }
    if !busy {
        save_options::show(app, ctx);
        preferences::show(app, ctx);
    }
    crate::camera_raw_scope_ui::persist(app, ctx);
    if !menu_was_open
        && !ctx.text_edit_focused()
        && ctx.input(|i| i.key_pressed(egui::Key::Y))
        && let Some(d) = app.camera_raw.as_mut()
    {
        d.show_before = !d.show_before;
    }
    if !menu_was_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = Some("cancel");
    }
    // Super Resolution is being applied for a Done: the popup is modal, and when Image Triage is finished the Done completes.
    let host_popup = app.camera_raw.as_ref().is_some_and(|d| d.host_job.is_open());
    if app.camera_raw.as_mut().is_some_and(|d| d.host_job.take_done_ready()) {
        action = Some("done-now");
    } else if host_popup {
        action = None;
    }
    if busy {
        action = None;
    }
    match action {
        Some("preferences") => app.ui.camera_raw_workspace.preferences = true,
        Some("ok") => {
            if let Err(e) = commit(app) {
                app.ui.status = e;
            }
        }
        Some("cancel") => cancel(app),
        Some("done") => {
            // Super Resolution is applied by Image Triage when the photo is finished; with it on, Done waits for that (with a progress
            // popup). Without Image Triage on the other end Done simply completes.
            let now = ctx.input(|i| i.time);
            let waiting = app
                .camera_raw
                .as_mut()
                .is_some_and(|d| d.target == Target::OpenRaw && d.params.super_resolution && d.host_job.request_before_done(host_job::SUPER_RESOLUTION, now));
            if !waiting && let Err(error) = done(app) {
                app.ui.status = error;
            }
        }
        Some("done-now") => {
            if let Err(error) = done(app) {
                app.ui.status = error;
            }
        }
        Some("reset-open" | "reset-default") => {
            let reset = if action == Some("reset-open") { "open" } else { "default" };
            if let Some(Err(error)) = menu_update(app, ctx, &json!({"reset":reset})) {
                app.ui.status = error;
            }
        }
        Some("save") => {
            let r = save_settings(app);
            report_file_error(app, r);
        }
        Some("load") => {
            let r = load_settings(app);
            report_file_error(app, r);
        }
        _ => {}
    }
}
fn curve_editor(
    ui: &mut egui::Ui,
    p: &mut CameraRaw,
    dirty: &mut bool,
    state: &mut crate::state::PointCurveState,
    channel: crate::camera_raw_workspace_ui::CurveChannel,
    show_all: &mut bool,
    histogram: &RgbHistogram,
) -> ERect {
    let t = Tokens::get(ui.ctx());
    let side = ui.available_width().min(260.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.dock);
    use crate::camera_raw_workspace_ui::CurveChannel;
    let counts: Vec<u64> = (0..256)
        .map(|i| match channel {
            CurveChannel::Red => histogram.channels[0][i],
            CurveChannel::Green => histogram.channels[1][i],
            CurveChannel::Blue => histogram.channels[2][i],
            _ => histogram.channels.iter().fold(0_u64, |sum, band| sum.saturating_add(band[i])),
        })
        .collect();
    let peak = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
    let mut histogram_mesh = egui::Mesh::default();
    for (i, count) in counts.iter().enumerate() {
        let x0 = rect.left() + i as f32 / 256.0 * side;
        let x1 = rect.left() + (i + 1) as f32 / 256.0 * side;
        histogram_mesh.add_colored_rect(ERect::from_min_max(pos2(x0, rect.bottom() - *count as f32 / peak * side), pos2(x1, rect.bottom())), t.canvas);
    }
    painter.add(egui::Shape::mesh(histogram_mesh));
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, t.shadow.to_opaque()), egui::StrokeKind::Inside);
    for i in 1..4 {
        let f = i as f32 / 4.0;
        painter.line_segment([pos2(rect.left() + f * side, rect.top()), pos2(rect.left() + f * side, rect.bottom())], Stroke::new(1.0, t.canvas));
        painter.line_segment([pos2(rect.left(), rect.top() + f * side), pos2(rect.right(), rect.top() + f * side)], Stroke::new(1.0, t.canvas));
    }
    let to_screen = |x: f32, y: f32| pos2(rect.left() + x / 255.0 * side, rect.bottom() - y / 255.0 * side);
    let parametric_hover = if channel == CurveChannel::Parametric { parametric_interact(ui, &resp, rect, p, dirty) } else { None };
    let selected = channel_stops(p, channel);
    // The identity handles are presentation until a gesture actually edits them.
    let mut points = if selected.is_empty() { vec![[0.0, 0.0], [255.0, 255.0]] } else { selected.clone() };
    let mut edited = false;
    if channel != CurveChannel::Parametric {
        // A press on the curve itself adds a stop right there, on the line, so the shape does not jump and the new stop can be dragged at
        // once; a press anywhere else in the graph adds one at the pointer (the shared editor's rule), and a press on a stop selects it.
        let pressed = (resp.is_pointer_button_down_on() || resp.clicked_by(egui::PointerButton::Primary)) && ui.input(|i| i.pointer.primary_pressed());
        if pressed
            && let Some(pos) = ui.input(|i| i.pointer.press_origin()).or_else(|| resp.interact_pointer_pos())
            && rect.contains(pos)
            && !ui.input(|i| i.modifiers.command)
            && crate::point_curve::hit(&points, |q| to_screen(q[0], q[1]), pos, CURVE_STOP_RADIUS).is_none()
        {
            let x = ((pos.x - rect.left()) / side * 255.0).clamp(0.0, 255.0);
            let on_curve = curve_lut(&points, 256).get(x.round() as usize).map(|v| v * 255.0);
            if let Some(y) = on_curve
                && (to_screen(x, y).y - pos.y).abs() <= CURVE_LINE_GRAB
            {
                let before = points.len();
                if crate::point_curve::insert(&mut points, [x, y]).is_some() && points.len() != before {
                    edited = true;
                }
            }
        }
        // Right-click belongs to the context menu below; Ctrl-click deletes a stop.
        if crate::point_curve::interact_with(ui, &resp, rect, &mut points, state, false).changed {
            edited = true;
        }
    }
    if edited {
        *selected = points.clone();
        *dirty = true;
    }
    curve_context_menu(&resp, p, channel, &points, show_all, state, dirty);
    // Curve (point curve after the parametric one).
    let lut = if channel == CurveChannel::Parametric { parametric_curve(p) } else { curve_lut(&points, 256) };
    let pts: Vec<Pos2> = lut.iter().enumerate().map(|(i, value)| to_screen(i as f32, value * 255.0)).collect();
    let color = match channel {
        CurveChannel::Red => Color32::from_rgb(220, 65, 65),
        CurveChannel::Green => Color32::from_rgb(70, 185, 90),
        CurveChannel::Blue => Color32::from_rgb(70, 130, 235),
        _ => t.text_faint,
    };
    if let Some((region, _)) = parametric_hover {
        // How far this region's slider can bend the curve (its whole range, -100 to +100), as the translucent band around the line.
        let edge = |value: f32| {
            let mut q = p.clone();
            set_region(&mut q, region, value);
            parametric_curve(&q)
        };
        let (upper, lower) = (edge(100.0), edge(-100.0));
        let mut band = egui::Mesh::default();
        let shade = Color32::from_white_alpha(40);
        for (i, (hi, lo)) in upper.iter().zip(&lower).enumerate() {
            band.colored_vertex(to_screen(i as f32, hi * 255.0), shade);
            band.colored_vertex(to_screen(i as f32, lo * 255.0), shade);
            if i > 0 {
                let k = (i * 2) as u32;
                band.add_triangle(k - 2, k - 1, k);
                band.add_triangle(k - 1, k, k + 1);
            }
        }
        painter.add(egui::Shape::mesh(band));
    }
    if *show_all && channel != CurveChannel::Parametric {
        // Show All Curves: the other channels' curves, quietly, under the active one.
        for (other, curve, tint) in [
            (CurveChannel::Master, &p.point_curve, t.text_faint),
            (CurveChannel::Red, &p.point_curve_red, Color32::from_rgb(220, 65, 65)),
            (CurveChannel::Green, &p.point_curve_green, Color32::from_rgb(70, 185, 90)),
            (CurveChannel::Blue, &p.point_curve_blue, Color32::from_rgb(70, 130, 235)),
        ] {
            if other == channel {
                continue;
            }
            let stops = if curve.is_empty() { vec![[0.0, 0.0], [255.0, 255.0]] } else { curve.clone() };
            let line: Vec<Pos2> = curve_lut(&stops, 256).iter().enumerate().map(|(i, value)| to_screen(i as f32, value * 255.0)).collect();
            painter.add(egui::Shape::line(line, Stroke::new(1.0, tint.gamma_multiply(0.6))));
        }
    }
    let line = if channel == CurveChannel::Parametric { Stroke::new(2.0, t.text) } else { Stroke::new(1.5, color) };
    painter.add(egui::Shape::line(pts, line));
    if let Some((region, x)) = parametric_hover {
        // The marker on the curve at the pointer's input level, and the region's name and value.
        let height = lut.get(x.round() as usize).map_or(0.0, |v| v * 255.0);
        let marker = to_screen(x, height);
        ui.painter().circle_filled(marker, 4.5, Color32::BLACK);
        ui.painter().circle_stroke(marker, 4.5, Stroke::new(1.5, Color32::WHITE));
        painter.text(
            pos2(rect.right() - 6.0, rect.bottom() - 6.0),
            Align2::RIGHT_BOTTOM,
            format!("{} {}", crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", REGION_NAMES[region]), region_value(p, region).round() as i32),
            FontId::proportional(11.0),
            t.text_dim,
        );
    }
    if channel != CurveChannel::Parametric {
        // The stops, over the graph's edge so the corner ones show whole: the active one is a solid disc, the others hollow rings that
        // light up under the pointer (click one to make it active and adjust it again).
        let handles = ui.painter();
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let hovered = pointer
            .filter(|pos| rect.expand(CURVE_STOP_RADIUS).contains(*pos))
            .and_then(|pos| crate::point_curve::hit(&points, |q| to_screen(q[0], q[1]), pos, CURVE_STOP_RADIUS));
        let ink = if channel == CurveChannel::Master { t.text } else { color };
        for (i, c) in points.iter().enumerate() {
            let center = to_screen(c[0], c[1]);
            if state.selected == Some(i) {
                handles.circle_filled(center, 5.5, ink);
                handles.circle_stroke(center, 5.5, Stroke::new(1.0, t.shadow.to_opaque()));
            } else {
                let radius = if hovered == Some(i) { 5.5 } else { 4.5 };
                handles.circle_filled(center, radius, t.dock);
                handles.circle_stroke(center, radius, Stroke::new(if hovered == Some(i) { 2.0 } else { 1.5 }, ink));
            }
        }
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        } else if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    }
    rect
}
pub(super) fn row(ui: &mut egui::Ui, dirty: &mut bool, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, grad: Option<&[Color32]>) {
    let _ = slider(ui, dirty, label, v, range, grad);
}

fn slider(ui: &mut egui::Ui, dirty: &mut bool, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, grad: Option<&[Color32]>) -> egui::Response {
    slider_default(ui, dirty, label, v, range, grad, 0.0)
}

fn tone_row(ui: &mut egui::Ui, dirty: &mut bool, alt: &mut Option<ToneZone>, zone: ToneZone, v: &mut f32) {
    let r = slider(ui, dirty, zone.name(), v, -zone.limit()..=zone.limit(), None);
    if r.dragged() && ui.input(|i| i.modifiers.alt) {
        *alt = Some(zone);
    }
}

fn prepare_preview(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    d: &mut super::CameraRawDialog,
    state: &mut crate::state::UiState,
    current_display: &Option<std::sync::Arc<photocraft_engine::display_color::CanvasDisplay>>,
    menu_was_open: bool,
) -> CameraRaw {
    if menu_was_open {
        ui.disable();
    } else {
        history_keys(d, ctx);
    }
    let frame_origin = d.params.clone();
    let pointer = ctx.input(|i| i.pointer.primary_down());
    if pointer && d.edit_origin.is_none() {
        d.edit_origin = Some(d.params.clone());
    }
    if let Some(display) = &current_display
        && display.key != d.display.key
    {
        d.display = display.clone();
        d.dirty = true;
    }
    d.curve_rect = None;
    d.scope.vectorscope_rect = None;
    if let Err(e) = d.poll_render(ctx) {
        d.preview_error = Some((e.clone(), d.preview_revision));
        state.status = e;
        state.status_error = true;
    }
    d.request_render(ctx);
    d.clear_recovered_error(state);

    frame_origin
}

fn finish_preview_edit(d: &mut super::CameraRawDialog, ctx: &egui::Context, frame_origin: CameraRaw) {
    if !ctx.input(|i| i.pointer.primary_down()) {
        let origin = d.edit_origin.take().unwrap_or(frame_origin);
        if origin != d.params {
            let edited = std::mem::replace(&mut d.params, origin);
            remember_edit(d);
            d.params = edited;
        }
    }
}

fn channel_stops(p: &mut CameraRaw, channel: crate::camera_raw_workspace_ui::CurveChannel) -> &mut Vec<[f32; 2]> {
    use crate::camera_raw_workspace_ui::CurveChannel;
    match channel {
        CurveChannel::Red => &mut p.point_curve_red,
        CurveChannel::Green => &mut p.point_curve_green,
        CurveChannel::Blue => &mut p.point_curve_blue,
        _ => &mut p.point_curve,
    }
}

fn footer_action(ui: &mut egui::Ui, foot: ERect, workspace: bool, busy_visible: bool, target: Target, ok_label: &str) -> Option<&'static str> {
    let mut action = None;
    if workspace || target == Target::OpenRaw {
        ui.add_enabled_ui(!busy_visible, |ui| {
            if let Some(selected) = crate::camera_raw_footer_ui::draw(ui, foot) {
                action = Some(selected);
            }
        });
    } else {
        let mut fu = ui.new_child(egui::UiBuilder::new().max_rect(foot.shrink2(vec2(16.0, 9.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if busy_visible {
            fu.disable();
        }
        if let Some(role) = widgets::dialog_buttons(
            &mut fu,
            &[
                widgets::DialogButton::new(widgets::ButtonRole::Default, ok_label, 90.0),
                widgets::DialogButton::new(widgets::ButtonRole::Cancel, tl!("Cancel"), 90.0),
            ],
        ) {
            action = Some(if role == widgets::ButtonRole::Default { "ok" } else { "cancel" });
        }
        if target == Target::OpenRaw && widgets::secondary_button(&mut fu, tl!("Done"), 70.0).clicked() {
            action = Some("done");
        }
    }

    action
}
