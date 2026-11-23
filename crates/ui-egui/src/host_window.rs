//! The editor hosted by Image Triage in a window of its own (`--hosted-window`).
//!
//! On Windows, Image Triage puts the hosted editor's native window inside its own. Linux and macOS cannot reparent another process's
//! window, so there the editor keeps a window of its own, with the system's title bar: hidden until Image Triage shows it with a photo,
//! and hidden again, never closed, when the user closes it. Closing only asks Image Triage to put the editor away (save the photo and
//! show its grid again); the process stays up for the next photo, as an embedded editor does. Image Triage shows and hides the window,
//! and collects that request, through the `ui.hostWindow` control method.
//!
//! Wayland cannot hide a window (winit has no way to), so on Linux the window opens through Xwayland where it can. Where it cannot
//! (`can_hide` false: a Wayland session without Xwayland), the window stays up: closing it still only asks the host, which saves the
//! photo and then ends the editor (`app.quit`) instead of keeping it hidden.

use serde_json::{Value, json};

use crate::PhotocraftApp;

#[derive(Debug, Default)]
pub struct HostWindow {
    /// Whether this editor runs in a window of its own for a host (`--hosted-window`).
    pub enabled: bool,
    /// Whether the host has the window shown.
    shown: bool,
    /// Whether the first frame, which eframe always shows, has been hidden again.
    first_hidden: bool,
    /// The user closed the window since the host last asked (`take_close_request`).
    close_requested: bool,
    /// Whether the window system lets the window be hidden (see the module's notes).
    can_hide: bool,
}

/// Run the editor in a window of its own for a host. `can_hide`: whether the window system can hide it (false on Wayland itself).
pub fn enable(app: &mut PhotocraftApp, can_hide: bool) {
    app.host_window.enabled = true;
    app.host_window.can_hide = can_hide;
}

/// A hosted render helper off Windows (`--hosted` without `--hosted-window`): it is never shown, so its first frame, which eframe
/// shows whatever the viewport said, is hidden at once. On Windows the host hides it itself.
pub fn enable_background(app: &mut PhotocraftApp) {
    enable(app, true);
}

/// Once per frame, before the ordinary close guard: keeps the window hidden until the host shows it, and turns the user's close into a
/// request to the host. Returns whether it took the close request (the ordinary guard then has nothing to do).
pub fn tick(app: &mut PhotocraftApp, ctx: &egui::Context) -> bool {
    let window = &mut app.host_window;
    if !window.enabled {
        return false;
    }
    if !window.first_hidden {
        window.first_hidden = true;
        if !window.shown && window.can_hide {
            // eframe shows the first rendered frame whatever the viewport said: the editor starts in the background, waiting for a photo.
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }
    // `app.quit` (the host ending the editor) sets `allow_close`: that close goes through.
    if app.allow_close || !ctx.input(|i| i.viewport().close_requested()) {
        return false;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
    if window.can_hide {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        window.shown = false;
    }
    window.close_requested = true;
    true
}

/// The `ui.hostWindow` control method. `visible` (optional) shows the window, raised and focused, or hides it. Answers whether the window
/// is shown and whether the user closed it since the last call (`closeRequested`, answered once).
pub fn control(app: &mut PhotocraftApp, ctx: &egui::Context, p: &Value) -> Result<Value, String> {
    let window = &mut app.host_window;
    if !window.enabled {
        return Err("ui.hostWindow: this editor is not running in a window of its own (--hosted-window)".into());
    }
    match p.get("visible") {
        None | Some(Value::Null) => {}
        Some(Value::Bool(true)) => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            window.shown = true;
            window.first_hidden = true;
            // A close from before it was shown again is no longer wanted.
            window.close_requested = false;
        }
        Some(Value::Bool(false)) => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            window.shown = false;
        }
        Some(other) => return Err(format!("ui.hostWindow: `visible` must be true or false, not {other}")),
    }
    ctx.request_repaint();
    let close_requested = std::mem::take(&mut window.close_requested);
    Ok(json!({"visible": window.shown, "closeRequested": close_requested, "canHide": window.can_hide}))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commands a frame sent, without the theme egui sets on a context's first frame.
    fn commands(out: &egui::FullOutput) -> Vec<egui::ViewportCommand> {
        out.viewport_output
            .values()
            .flat_map(|v| v.commands.clone())
            .filter(|c| !matches!(c, egui::ViewportCommand::SetTheme(_)))
            .collect()
    }

    fn frame(app: &mut PhotocraftApp, close: bool) -> (bool, Vec<egui::ViewportCommand>) {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        if close {
            let mut info = egui::ViewportInfo::default();
            info.events.push(egui::ViewportEvent::Close);
            input.viewports.insert(egui::ViewportId::ROOT, info);
        }
        let mut took = false;
        let mut out = ctx.run_ui(input, |ui| took = tick(app, ui.ctx()));
        out.textures_delta.clear(); // no renderer here: the font atlas upload is dropped on purpose
        (took, commands(&out))
    }

    fn call(app: &mut PhotocraftApp, p: Value) -> (Result<Value, String>, Vec<egui::ViewportCommand>) {
        let ctx = egui::Context::default();
        let mut result = Err(String::new());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| result = control(app, ui.ctx(), &p));
        out.textures_delta.clear();
        (result, commands(&out))
    }

    #[test]
    fn an_ordinary_editor_is_left_alone() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        let (took, commands) = frame(&mut app, true);
        assert!(!took && commands.is_empty());
        assert!(call(&mut app, json!({})).0.is_err(), "only a hosted window answers ui.hostWindow");
    }

    #[test]
    fn the_window_starts_hidden_and_the_host_shows_it() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        enable(&mut app, true);
        let (_, first) = frame(&mut app, false);
        assert_eq!(first, [egui::ViewportCommand::Visible(false)], "the first frame eframe shows is hidden again");
        assert!(frame(&mut app, false).1.is_empty(), "only once");
        let (reply, shown) = call(&mut app, json!({"visible": true}));
        assert_eq!(reply.unwrap(), json!({"visible": true, "closeRequested": false, "canHide": true}));
        assert!(shown.contains(&egui::ViewportCommand::Visible(true)) && shown.contains(&egui::ViewportCommand::Focus));
    }

    #[test]
    fn closing_hides_the_window_and_asks_the_host_once() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        enable(&mut app, true);
        call(&mut app, json!({"visible": true})).0.unwrap();
        let (took, commands) = frame(&mut app, true);
        assert!(took);
        assert!(commands.contains(&egui::ViewportCommand::CancelClose) && commands.contains(&egui::ViewportCommand::Visible(false)));
        assert_eq!(call(&mut app, json!({})).0.unwrap(), json!({"visible": false, "closeRequested": true, "canHide": true}));
        assert_eq!(call(&mut app, json!({})).0.unwrap(), json!({"visible": false, "closeRequested": false, "canHide": true}), "answered once");
    }

    #[test]
    fn the_host_ending_the_editor_closes_it() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        enable(&mut app, true);
        app.allow_close = true; // app.quit
        let (took, commands) = frame(&mut app, true);
        assert!(!took && !commands.contains(&egui::ViewportCommand::CancelClose));
    }

    #[test]
    fn where_the_window_cannot_be_hidden_closing_still_only_asks_the_host() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        enable(&mut app, false);
        assert!(frame(&mut app, false).1.is_empty(), "no hiding the first frame either");
        call(&mut app, json!({"visible": true})).0.unwrap();
        let (took, commands) = frame(&mut app, true);
        assert!(took && commands == [egui::ViewportCommand::CancelClose], "{commands:?}");
        assert_eq!(call(&mut app, json!({})).0.unwrap(), json!({"visible": true, "closeRequested": true, "canHide": false}));
    }

    #[test]
    fn a_bad_visible_is_refused() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        enable(&mut app, true);
        assert!(call(&mut app, json!({"visible": "yes"})).0.is_err());
    }
}
