//! A request from the Camera Raw dialog to Image Triage (the host), and its progress popup.
//!
//! Super Resolution is a setting; it is applied when the photo is finished (Done), as Photoshop applies it on exit. The editor is the
//! control server and cannot call the host, so the request waits here until the host polls `ui.hostJob`, claims it and reports
//! progress; this module shows that progress ("Applying Super Resolution…", a bar, an estimate, Cancel) until the host finishes it or
//! the user cancels, and tells the dialog when it may complete the Done that started it.
use egui::{Align2, FontId, Sense, vec2};
use serde_json::{Value, json};

use crate::theme::Tokens;

pub const SUPER_RESOLUTION: &str = "superResolution";
/// BiRefNet in Image Triage finds the subject in a picture the editor wrote (`input`) and writes its coverage map (`output`).
pub const SUBJECT_MASK: &str = "subjectMask";
/// A request the host never picks up (not running, or an older build) is given up on after this long.
const CLAIM_TIMEOUT_S: f64 = 20.0;
/// The host polls on its autosave tick (about every 1.5 s): one that has not asked for this long is not connected.
const HOST_PRESENT_S: f64 = 8.0;

fn tr(s: &str) -> &str {
    crate::i18n::tr_ctx(crate::i18n::current(), "cameraRaw", s)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Requested,
    Running,
    Finished,
    Failed,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Requested => "requested",
            Phase::Running => "running",
            Phase::Finished => "finished",
            Phase::Failed => "failed",
        }
    }
}

#[derive(Debug)]
pub struct HostJob {
    last_id: u64,
    id: u64,
    kind: &'static str,
    phase: Phase,
    fraction: f32,
    message: String,
    eta_secs: Option<f32>,
    cancel: bool,
    requested_at: f64,
    /// When the host last asked for a request (`None`: never): whether anything is on the other end.
    last_poll: Option<f64>,
    /// The request was made by Done: when it succeeds the dialog completes that Done.
    then_done: bool,
    /// What the host is told besides the kind (file paths for a mask), and what the dialog wants done with the answer.
    extra: Value,
    tag: String,
}

impl Default for HostJob {
    fn default() -> Self {
        HostJob {
            last_id: 0,
            id: 0,
            kind: "",
            phase: Phase::Idle,
            fraction: 0.0,
            message: String::new(),
            eta_secs: None,
            cancel: false,
            requested_at: 0.0,
            last_poll: None,
            then_done: false,
            extra: Value::Null,
            tag: String::new(),
        }
    }
}

impl HostJob {
    /// The popup is up (a request is in flight, or its outcome is being shown).
    pub fn is_open(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Whether Image Triage has been polling lately, i.e. whether a request would be answered.
    pub fn host_present(&self, now: f64) -> bool {
        self.last_poll.is_some_and(|at| now - at <= HOST_PRESENT_S)
    }

    #[cfg(test)]
    fn phase(&self) -> Phase {
        self.phase
    }

    fn busy(&self) -> bool {
        matches!(self.phase, Phase::Requested | Phase::Running)
    }

    fn reset(&mut self) {
        *self = HostJob { last_id: self.last_id, last_poll: self.last_poll, ..Default::default() };
    }

    /// Done with Super Resolution on: ask the host to apply it first. Returns whether a request was made; `false` (no host, or one
    /// already in flight) means Done should simply complete.
    pub fn request_before_done(&mut self, kind: &'static str, now: f64) -> bool {
        if self.busy() || !self.host_present(now) {
            return false;
        }
        self.last_id += 1;
        let (last_id, last_poll) = (self.last_id, self.last_poll);
        *self = HostJob { last_id, id: last_id, kind, phase: Phase::Requested, requested_at: now, last_poll, then_done: true, ..Default::default() };
        self.message = tr("Waiting for Image Triage…").to_string();
        true
    }

    /// A request that is not part of Done (an automatic mask): `extra` goes to the host with the kind, `tag` comes back with the
    /// answer (`take_mask_ready`). Returns whether it was made; `false` means no host is polling or one request is already in flight.
    pub fn request_mask(&mut self, kind: &'static str, extra: Value, tag: &str, now: f64) -> bool {
        if self.busy() || !self.host_present(now) {
            return false;
        }
        self.last_id += 1;
        let (last_id, last_poll) = (self.last_id, self.last_poll);
        *self = HostJob { last_id, id: last_id, kind, phase: Phase::Requested, requested_at: now, last_poll, extra, tag: tag.to_string(), ..Default::default() };
        self.message = tr("Waiting for Image Triage…").to_string();
        true
    }

    /// Once, when the host has finished a mask request: its tag and the file the coverage map is in. The popup closes quietly.
    pub fn take_mask_ready(&mut self) -> Option<(String, String)> {
        if self.phase == Phase::Finished && !self.then_done && self.kind == SUBJECT_MASK {
            let answer = (self.tag.clone(), self.extra.get("output").and_then(Value::as_str).unwrap_or("").to_string());
            self.reset();
            return Some(answer);
        }
        None
    }

    /// True once, when the host has applied what Done asked for: the dialog completes the Done now.
    pub fn take_done_ready(&mut self) -> bool {
        if self.phase == Phase::Finished && self.then_done {
            self.reset();
            return true;
        }
        false
    }

    /// Give up on a request the host never claimed.
    pub fn tick(&mut self, now: f64) {
        if self.phase == Phase::Requested && now - self.requested_at > CLAIM_TIMEOUT_S {
            self.phase = Phase::Failed;
            self.message = if self.kind == SUBJECT_MASK {
                tr("Image Triage did not respond. Subject masks are made by Image Triage when Camera Raw is opened from it.").to_string()
            } else {
                tr("Image Triage did not respond. Super Resolution is applied by Image Triage when Camera Raw is opened from it.").to_string()
            };
        }
    }

    fn state(&self) -> Value {
        let mut state = json!({"id": self.id, "phase": self.phase.name(), "cancel": self.cancel, "pending": null});
        if self.phase == Phase::Requested {
            let mut pending = json!({"id": self.id, "kind": self.kind});
            if let (Some(into), Some(extra)) = (pending.as_object_mut(), self.extra.as_object()) {
                for (key, value) in extra {
                    into.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
            state["pending"] = pending;
        }
        state
    }

    /// `ui.hostJob`: no field polls; `claim`, `progress` and `finish` are the host's replies (one per call).
    pub fn control(&mut self, p: &Value, now: f64) -> Result<Value, String> {
        if let Some(object) = p.as_object()
            && let Some(key) = object.keys().find(|key| !["claim", "progress", "finish"].contains(&key.as_str()))
        {
            return Err(format!("unknown ui.hostJob field `{key}`"));
        }
        self.last_poll = Some(now);
        let given = ["claim", "progress", "finish"].iter().filter(|name| p.get(**name).is_some()).count();
        if given > 1 {
            return Err("ui.hostJob takes one of claim, progress or finish".into());
        }
        let id_of = |value: &Value| value.as_u64().ok_or_else(|| "id must be a whole number".to_string());
        if let Some(claim) = p.get("claim") {
            if id_of(claim)? != self.id || self.phase != Phase::Requested {
                return Err("that host request is not waiting".into());
            }
            self.phase = Phase::Running;
            self.message = tr("Preparing…").to_string();
        } else if let Some(progress) = p.get("progress") {
            let id = id_of(progress.get("id").unwrap_or(&Value::Null))?;
            if id != self.id || self.phase != Phase::Running {
                return Err("that host request is not running".into());
            }
            let fraction = progress.get("fraction").and_then(Value::as_f64).ok_or("fraction must be a number")?;
            if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                return Err("fraction must be within 0..1".into());
            }
            self.fraction = fraction as f32;
            if let Some(message) = progress.get("message") {
                let text = message.as_str().ok_or("message must be a string")?;
                if text.chars().count() > 200 {
                    return Err("message is too long".into());
                }
                self.message = text.to_string();
            }
            self.eta_secs = match progress.get("etaSeconds") {
                None | Some(Value::Null) => None,
                Some(value) => {
                    let eta = value.as_f64().ok_or("etaSeconds must be a number")?;
                    if !eta.is_finite() || eta < 0.0 {
                        return Err("etaSeconds must be zero or more".into());
                    }
                    Some(eta as f32)
                }
            };
        } else if let Some(finish) = p.get("finish") {
            let id = id_of(finish.get("id").unwrap_or(&Value::Null))?;
            if id != self.id || !self.busy() {
                return Err("that host request is not in flight".into());
            }
            let ok = finish.get("ok").and_then(Value::as_bool).ok_or("ok must be true or false")?;
            let message = finish.get("message").and_then(Value::as_str).unwrap_or("");
            if message.chars().count() > 400 {
                return Err("message is too long".into());
            }
            if ok {
                self.phase = Phase::Finished;
                self.fraction = 1.0;
                self.message = message.to_string();
            } else if self.cancel {
                self.reset(); // the user cancelled: the dialog stays as it was, nothing to report
            } else {
                self.phase = Phase::Failed;
                self.message = message.to_string();
            }
        }
        Ok(self.state())
    }

    fn title(&self) -> &str {
        match self.kind {
            SUPER_RESOLUTION => tr("Applying Super Resolution…"),
            SUBJECT_MASK => tr("Finding the subject…"),
            _ => tr("Working…"),
        }
    }

    /// The popup: a scrim over the dialog, then a small "Camera Raw" window, as in Photoshop.
    pub fn show(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.tick(now);
        if self.phase == Phase::Idle {
            return;
        }
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100)); // the host's reports arrive between frames
        }
        let t = Tokens::get(ctx);
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("camera-raw-host-job-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            ui.painter().rect_filled(r, 0.0, t.scrim);
        });
        let mut close = false;
        let mut cancel = false;
        egui::Window::new(tr("Camera Raw"))
            .id(egui::Id::new("camera-raw-host-job"))
            .order(egui::Order::Tooltip)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(480.0, 0.0))
            .show(ctx, |ui| {
                ui.add_space(8.0);
                match self.phase {
                    Phase::Requested | Phase::Running => {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(self.title()).color(t.text));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if self.phase == Phase::Running {
                                    ui.label(egui::RichText::new(format!("{}%", (self.fraction * 100.0).round() as u32)).color(t.text));
                                }
                            });
                        });
                        ui.add_space(6.0);
                        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
                        ui.painter().rect_filled(bar, 0.0, t.field);
                        let mut fill = bar;
                        fill.set_width(bar.width() * self.fraction.clamp(0.0, 1.0));
                        ui.painter().rect_filled(fill, 0.0, t.accent);
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(&self.message).color(t.text_dim).font(FontId::proportional(12.0)));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let eta = match self.eta_secs {
                                Some(secs) if self.phase == Phase::Running => format!("{} {}", tr("Estimated time:"), format_eta(secs)),
                                _ => String::new(),
                            };
                            ui.label(egui::RichText::new(eta).color(t.text_dim));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add_enabled(!self.cancel, egui::Button::new(tr("Cancel")).min_size(vec2(90.0, 24.0))).clicked() {
                                    cancel = true;
                                }
                            });
                        });
                    }
                    Phase::Finished | Phase::Failed => {
                        let (heading, colour) = match (self.phase, self.kind) {
                            (Phase::Finished, _) => (tr("Super Resolution finished"), t.text),
                            (_, SUBJECT_MASK) => (tr("The subject could not be found"), t.danger),
                            _ => (tr("Super Resolution could not finish"), t.danger),
                        };
                        ui.label(egui::RichText::new(heading).color(colour));
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(&self.message).color(t.text_dim).font(FontId::proportional(12.0)));
                        ui.add_space(8.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(tr("Close")).min_size(vec2(90.0, 24.0))).clicked() {
                                close = true;
                            }
                        });
                    }
                    Phase::Idle => {}
                }
                ui.add_space(4.0);
            });
        if cancel {
            if self.phase == Phase::Requested {
                self.reset(); // never claimed: simply withdraw it
            } else {
                self.cancel = true;
                self.message = tr("Cancelling…").to_string();
            }
        }
        if close {
            self.reset();
        }
    }
}

fn format_eta(secs: f32) -> String {
    let s = secs.round().max(0.0) as u32;
    match s {
        0..=59 => format!("{s} sec"),
        _ => format!("{} min {} sec", s / 60, s % 60),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A job the host is polling, with a request made by Done and claimed.
    fn running() -> HostJob {
        let mut job = HostJob::default();
        job.control(&json!({}), 0.0).unwrap();
        assert!(job.request_before_done(SUPER_RESOLUTION, 1.0));
        job.control(&json!({"claim": job.id}), 1.0).unwrap();
        job
    }

    #[test]
    fn nothing_is_requested_without_a_host_that_has_been_polling() {
        let mut job = HostJob::default();
        assert!(!job.request_before_done(SUPER_RESOLUTION, 0.0), "never polled");
        job.control(&json!({}), 10.0).unwrap();
        assert!(!job.request_before_done(SUPER_RESOLUTION, 10.0 + HOST_PRESENT_S + 1.0), "polled long ago");
        assert!(job.request_before_done(SUPER_RESOLUTION, 10.0 + HOST_PRESENT_S - 1.0));
        assert!(!job.request_before_done(SUPER_RESOLUTION, 11.0), "one request at a time");
    }

    #[test]
    fn poll_shows_the_pending_request_until_it_is_claimed() {
        let mut job = HostJob::default();
        assert_eq!(job.control(&json!({}), 0.0).unwrap()["pending"], Value::Null);
        job.request_before_done(SUPER_RESOLUTION, 0.5);
        let state = job.control(&json!({}), 1.0).unwrap();
        assert_eq!(state["pending"]["kind"], SUPER_RESOLUTION);
        assert_eq!(state["phase"], "requested");
        job.control(&json!({"claim": job.id}), 1.0).unwrap();
        assert_eq!(job.control(&json!({}), 2.0).unwrap()["pending"], Value::Null);
        assert_eq!(job.phase(), Phase::Running);
    }

    #[test]
    fn ids_never_repeat() {
        let mut job = running();
        let first = job.id;
        job.control(&json!({"finish": {"id": first, "ok": true, "message": "done"}}), 2.0).unwrap();
        assert!(job.take_done_ready());
        assert!(job.request_before_done(SUPER_RESOLUTION, 3.0));
        assert_ne!(job.id, first);
    }

    #[test]
    fn done_completes_only_after_the_host_succeeds_and_only_once() {
        let mut job = running();
        let id = job.id;
        assert!(!job.take_done_ready(), "still running");
        job.control(&json!({"finish": {"id": id, "ok": true, "message": "applied"}}), 2.0).unwrap();
        assert_eq!((job.phase(), job.fraction), (Phase::Finished, 1.0));
        assert!(job.take_done_ready());
        assert!(!job.take_done_ready(), "once");
        assert!(!job.is_open());
    }

    #[test]
    fn a_failure_stays_up_for_the_user_and_does_not_complete_done() {
        let mut job = running();
        let id = job.id;
        job.control(&json!({"finish": {"id": id, "ok": false, "message": "model missing"}}), 2.0).unwrap();
        assert_eq!(job.phase(), Phase::Failed);
        assert_eq!(job.message, "model missing");
        assert!(!job.take_done_ready() && job.is_open());
    }

    #[test]
    fn a_cancelled_job_closes_quietly_and_keeps_the_dialog() {
        let mut job = running();
        let id = job.id;
        job.cancel = true;
        job.control(&json!({"finish": {"id": id, "ok": false, "message": "cancelled"}}), 2.0).unwrap();
        assert_eq!(job.phase(), Phase::Idle);
        assert!(!job.take_done_ready());
        assert!(job.host_present(2.0), "the host is still known to be there");
    }

    #[test]
    fn progress_is_validated_and_reports_the_cancel_flag() {
        let mut job = running();
        let id = job.id;
        let state = job.control(&json!({"progress": {"id": id, "fraction": 0.4, "message": "Upscaling", "etaSeconds": 12.0}}), 2.0).unwrap();
        assert_eq!(state["cancel"], false);
        assert_eq!(job.fraction, 0.4);
        for bad in [
            json!({"id": id, "fraction": 1.5}),
            json!({"id": id, "fraction": -0.1}),
            json!({"id": id}),
            json!({"id": id + 1, "fraction": 0.5}),
            json!({"id": id, "fraction": 0.5, "etaSeconds": -1.0}),
            json!({"id": id, "fraction": 0.5, "message": 5}),
        ] {
            assert!(job.control(&json!({"progress": bad}), 2.0).is_err());
        }
        job.cancel = true;
        assert_eq!(job.control(&json!({"progress": {"id": id, "fraction": 0.5}}), 2.0).unwrap()["cancel"], true);
    }

    #[test]
    fn stale_or_malformed_requests_are_rejected() {
        let mut never_polled = HostJob::default();
        assert!(!never_polled.request_before_done(SUPER_RESOLUTION, 0.0));
        assert_eq!(never_polled.phase(), Phase::Idle);
        let mut job = HostJob::default();
        assert!(job.control(&json!({"claim": 1}), 0.0).is_err(), "nothing is waiting; even this counts as the host checking in");
        assert!(job.request_before_done(SUPER_RESOLUTION, 0.5));
        let id = job.id;
        assert!(job.control(&json!({"claim": id + 1}), 1.0).is_err());
        assert!(job.control(&json!({"unknown": 1}), 1.0).is_err());
        assert!(job.control(&json!({"claim": id, "finish": {}}), 1.0).is_err());
        assert!(job.control(&json!({"progress": {"id": id, "fraction": 0.1}}), 1.0).is_err(), "progress before claim");
        job.control(&json!({"claim": id}), 1.0).unwrap();
        assert!(job.control(&json!({"claim": id}), 1.0).is_err(), "claimed twice");
    }

    #[test]
    fn an_unclaimed_request_gives_up() {
        let mut job = HostJob::default();
        job.control(&json!({}), 100.0).unwrap();
        job.request_before_done(SUPER_RESOLUTION, 100.0);
        job.tick(100.0 + CLAIM_TIMEOUT_S - 1.0);
        assert_eq!(job.phase(), Phase::Requested);
        job.tick(100.0 + CLAIM_TIMEOUT_S + 1.0);
        assert_eq!(job.phase(), Phase::Failed);
        assert!(!job.take_done_ready());
    }

    #[test]
    fn a_mask_request_carries_its_files_and_comes_back_with_its_tag() {
        let mut job = HostJob::default();
        job.control(&json!({}), 0.0).unwrap();
        assert!(job.request_mask(SUBJECT_MASK, json!({"input": "a.png", "output": "b.png"}), "background", 1.0));
        let state = job.control(&json!({}), 1.0).unwrap();
        assert_eq!((state["pending"]["kind"].as_str(), state["pending"]["input"].as_str(), state["pending"]["output"].as_str()), (Some(SUBJECT_MASK), Some("a.png"), Some("b.png")));
        let id = job.id;
        job.control(&json!({"claim": id}), 1.0).unwrap();
        assert_eq!(job.take_mask_ready(), None, "not finished");
        job.control(&json!({"finish": {"id": id, "ok": true, "message": "Subject found."}}), 2.0).unwrap();
        assert!(!job.take_done_ready(), "a mask is not a Done");
        assert_eq!(job.take_mask_ready(), Some(("background".to_string(), "b.png".to_string())));
        assert_eq!(job.take_mask_ready(), None, "once");
        assert!(!job.is_open());
    }

    #[test]
    fn a_failed_mask_stays_up_with_its_reason() {
        let mut job = HostJob::default();
        job.control(&json!({}), 0.0).unwrap();
        assert!(job.request_mask(SUBJECT_MASK, json!({"output": "b.png"}), "subject", 1.0));
        let id = job.id;
        job.control(&json!({"claim": id}), 1.0).unwrap();
        job.control(&json!({"finish": {"id": id, "ok": false, "message": "model missing"}}), 2.0).unwrap();
        assert_eq!((job.phase(), job.take_mask_ready()), (Phase::Failed, None));
        assert!(job.is_open());
    }

    #[test]
    fn eta_reads_naturally() {
        assert_eq!(format_eta(2.0), "2 sec");
        assert_eq!(format_eta(61.0), "1 min 1 sec");
    }
}
