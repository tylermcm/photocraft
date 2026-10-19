//! The Camera Raw dialog's preview, made off the UI thread: a slider move submits the settings it now has, and only the newest
//! submission is ever worked on, so a drag never queues up work or blocks input (the previous preview stays on screen until the next
//! one is ready). Every result is for exactly the settings it was submitted with, and the last one made is for the final settings.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

use photocraft_algo::camera_raw::CameraRaw;
use photocraft_algo::histogram::RgbHistogram;
use photocraft_engine::display_color::CanvasDisplay;

use super::CameraRawDialog;

/// A slot that holds only the newest value put into it: putting replaces what has not been taken yet.
pub(crate) struct Latest<T> {
    slot: Mutex<Option<T>>,
    wake: Condvar,
    stop: AtomicBool,
}

impl<T> Latest<T> {
    pub fn new() -> Self {
        Latest { slot: Mutex::new(None), wake: Condvar::new(), stop: AtomicBool::new(false) }
    }

    pub fn put(&self, value: T) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(value);
            self.wake.notify_one();
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake.notify_all();
    }

    /// Wait for a value; `None` once stopped.
    pub fn take_blocking(&self) -> Option<T> {
        let mut slot = self.slot.lock().ok()?;
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return None;
            }
            if let Some(value) = slot.take() {
                return Some(value);
            }
            slot = self.wake.wait(slot).ok()?;
        }
    }

    fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// Everything one preview needs, by value or shared: nothing here changes while the job runs.
pub(crate) struct PreviewJob {
    pub id: u64,
    pub params: CameraRaw,
    pub raw_curve: Option<Vec<[f32; 2]>>,
    pub scale: f32,
    pub display: Arc<CanvasDisplay>,
}

/// What a finished job produced. The texture is made on the UI thread from `image`.
pub(crate) struct PreviewResult {
    pub id: u64,
    pub pixels: Vec<[f32; 4]>,
    pub histogram: RgbHistogram,
    pub image: egui::ColorImage,
    pub render_ms: f64,
    pub histogram_ms: f64,
}

/// The photo the previews are made from: fixed for the life of the dialog.
struct Source {
    proxy: Arc<Vec<[f32; 4]>>,
    selection: Option<Arc<Vec<f32>>>,
    width: usize,
    height: usize,
    float: bool,
}

pub(crate) struct PreviewWorker {
    jobs: Arc<Latest<PreviewJob>>,
    results: Receiver<PreviewResult>,
}

impl PreviewWorker {
    pub fn new(ctx: &egui::Context, proxy: &[[f32; 4]], selection: Option<&[f32]>, width: usize, height: usize, float: bool) -> Self {
        let source = Source { proxy: Arc::new(proxy.to_vec()), selection: selection.map(|s| Arc::new(s.to_vec())), width, height, float };
        let jobs = Arc::new(Latest::new());
        let (sender, results) = channel();
        let (thread_jobs, ctx) = (jobs.clone(), ctx.clone());
        // A thread that cannot start leaves results that never come; the dialog notices (see `CameraRawDialog::submit_preview`).
        let _ = std::thread::Builder::new().name("camera-raw-preview".into()).spawn(move || run(thread_jobs, source, sender, ctx));
        PreviewWorker { jobs, results }
    }

    /// Replace whatever was waiting with `job`: a job not yet started is dropped, never run.
    pub fn submit(&self, job: PreviewJob) {
        self.jobs.put(job);
    }

    /// The newest finished preview, if one arrived since the last call (older ones that arrived with it are dropped).
    pub fn newest(&self) -> Option<PreviewResult> {
        let mut newest = None;
        while let Ok(result) = self.results.try_recv() {
            newest = Some(result);
        }
        newest
    }
}

impl Drop for PreviewWorker {
    fn drop(&mut self) {
        // The thread finishes the job it is in the middle of (it cannot be interrupted) and then exits; the UI does not wait for it.
        self.jobs.stop();
    }
}

fn run(jobs: Arc<Latest<PreviewJob>>, source: Source, sender: Sender<PreviewResult>, ctx: egui::Context) {
    while let Some(job) = jobs.take_blocking() {
        let result = make(&source, job);
        if jobs.is_stopped() || sender.send(result).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

/// One preview: the same steps, in the same order, as `CameraRawDialog::render`.
fn make(source: &Source, job: PreviewJob) -> PreviewResult {
    let t0 = crate::gpu_canvas::now_ms();
    let mut px: Vec<[f32; 4]> = source.proxy.as_ref().clone();
    crate::raw_recipe::apply(&mut px, source.width, source.height, &job.params, job.raw_curve.as_deref(), job.scale, source.float);
    if let Some(mask) = &source.selection {
        for ((out, original), coverage) in px.iter_mut().zip(source.proxy.iter()).zip(mask.iter()) {
            for (out, original) in out.iter_mut().zip(original) {
                *out = *original + (*out - *original) * coverage;
            }
        }
    }
    let histogram_start = crate::gpu_canvas::now_ms();
    let histogram = RgbHistogram::from_rgba(&px);
    let histogram_ms = crate::gpu_canvas::now_ms() - histogram_start;
    let image = CameraRawDialog::image(&px, source.width, source.height, &job.display);
    PreviewResult { id: job.id, pixels: px, histogram, image, render_ms: crate::gpu_canvas::now_ms() - t0, histogram_ms }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_slot_keeps_only_the_newest_value() {
        let slot = Latest::new();
        for value in 1..=3 {
            slot.put(value);
        }
        assert_eq!(slot.take_blocking(), Some(3), "1 and 2 were never run");
        slot.stop();
        assert_eq!(slot.take_blocking(), None);
    }

    #[test]
    fn a_waiting_taker_wakes_for_a_value_and_for_stop() {
        let slot = Arc::new(Latest::new());
        let taker = {
            let slot = slot.clone();
            std::thread::spawn(move || (slot.take_blocking(), slot.take_blocking()))
        };
        std::thread::sleep(Duration::from_millis(30));
        slot.put(7);
        std::thread::sleep(Duration::from_millis(30));
        slot.stop();
        assert_eq!(taker.join().unwrap(), (Some(7), None));
    }

    #[test]
    fn newest_keeps_only_the_latest_of_several_waiting_results() {
        let (sender, results) = channel();
        let worker = PreviewWorker { jobs: Arc::new(Latest::new()), results };
        assert!(worker.newest().is_none());
        for id in 1..=3 {
            sender
                .send(PreviewResult { id, pixels: Vec::new(), histogram: RgbHistogram::default(), image: egui::ColorImage::default(), render_ms: 0.0, histogram_ms: 0.0 })
                .unwrap();
        }
        assert_eq!(worker.newest().unwrap().id, 3);
        assert!(worker.newest().is_none());
    }
}
