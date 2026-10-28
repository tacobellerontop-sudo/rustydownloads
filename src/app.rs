//! The egui user interface.
//!
//! This file only draws widgets and keeps UI state. It never downloads
//! anything itself: starting a job hands off to `download::start_job`,
//! and each frame it drains the progress updates that come back.

use crate::download::{
    default_output_dir, find_program, open_folder, start_job, FormatChoice, JobHandle,
    JobUpdate,
};
use std::fs;

/// What a job is doing, in plain words for the queue list.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    Queued,
    Downloading,
    Done,
    Failed(String),
    Cancelled,
}

impl Status {
    fn label(&self) -> String {
        match self {
            Status::Queued => "Queued".to_string(),
            Status::Downloading => "Downloading…".to_string(),
            Status::Done => "Done".to_string(),
            Status::Failed(msg) => format!("Failed: {}", msg),
            Status::Cancelled => "Cancelled".to_string(),
        }
    }

    fn is_running(&self) -> bool {
        matches!(self, Status::Queued | Status::Downloading)
    }
}

/// One row in the queue list.
struct QueueJob {
    url: String,
    format: FormatChoice,
    status: Status,
    percent: f32, // 0.0 to 100.0
    speed: String,
    eta: String,
    // Present only while the job is actually running.
    handle: Option<JobHandle>,
}

pub struct RustyDownloadsApp {
    url_input: String,
    format: FormatChoice,
    output_folder: String,
    custom_yt_dlp_path: String,
    yt_dlp_path: Option<String>,
    ffmpeg_found: bool,
    error_message: String,
    jobs: Vec<QueueJob>,
}

impl Default for RustyDownloadsApp {
    fn default() -> Self {
        let mut app = Self {
            url_input: String::new(),
            format: FormatChoice::BestVideo,
            output_folder: default_output_dir(),
            custom_yt_dlp_path: String::new(),
            yt_dlp_path: None,
            ffmpeg_found: false,
            error_message: String::new(),
            jobs: Vec::new(),
        };
        app.refresh_detection();
        app
    }
}

impl RustyDownloadsApp {
    /// Re-check for yt-dlp and ffmpeg, and store the results for the UI.
    fn refresh_detection(&mut self) {
        self.yt_dlp_path = find_program(&self.custom_yt_dlp_path, "yt-dlp")
            .map(|p| p.to_string_lossy().to_string());
        self.ffmpeg_found = find_program("", "ffmpeg").is_some();
    }

    /// Pull any progress/finished updates out of running jobs.
    /// Called at the top of every frame, before drawing.
    fn poll_jobs(&mut self) {
        for job in &mut self.jobs {
            let Some(handle) = &job.handle else {
                continue;
            };
            for update in handle.drain_updates() {
                match update {
                    JobUpdate::Progress(progress) => {
                        job.percent = progress.percent;
                        job.speed = progress.speed;
                        job.eta = progress.eta;
                        job.status = Status::Downloading;
                    }
                    JobUpdate::Finished { success, message } => {
                        job.handle = None;
                        if success {
                            job.percent = 100.0;
                            job.status = Status::Done;
                        } else if message == "Cancelled" {
                            job.status = Status::Cancelled;
                        } else {
                            job.status = Status::Failed(message);
                        }
                    }
                }
            }
        }
    }

    fn add_job(&mut self) {
        self.error_message.clear();
        let url = self.url_input.trim().to_string();
        if url.is_empty() {
            self.error_message = "Paste a URL first.".to_string();
            return;
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            self.error_message = "That does not look like a URL — it should start with https://".to_string();
            return;
        }
        self.jobs.push(QueueJob {
            url,
            format: self.format,
            status: Status::Queued,
            percent: 0.0,
            speed: String::new(),
            eta: String::new(),
            handle: None,
        });
        self.url_input.clear();
    }

    fn start_job_at(&mut self, index: usize) {
        self.error_message.clear();
        let Some(yt_dlp) = &self.yt_dlp_path else {
            self.error_message =
                "yt-dlp was not found. Follow the setup steps above, then click Detect again."
                    .to_string();
            return;
        };

        // Make sure the output folder exists before yt-dlp runs.
        if let Err(err) = fs::create_dir_all(&self.output_folder) {
            self.error_message = format!("Could not create output folder: {}", err);
            return;
        }

        let job = &mut self.jobs[index];
        if job.status.is_running() {
            return; // Already running; the Start button is hidden then anyway.
        }
        let handle = start_job(
            yt_dlp,
            job.format,
            self.ffmpeg_found,
            &job.url,
            &self.output_folder,
        );
        job.handle = Some(handle);
        job.status = Status::Downloading;
        job.percent = 0.0;
        job.speed.clear();
        job.eta.clear();
    }
}

impl eframe::App for RustyDownloadsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_jobs();

        // Keep repainting while anything is running, so the progress
        // bars move even when the user is not touching the mouse.
        if self.jobs.iter().any(|j| j.status.is_running()) {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("rustydownloads");
            ui.label("A small GUI wrapper around yt-dlp. Rust handles the window and the queue; yt-dlp does the downloading.");
            ui.add_space(8.0);

            // ---- yt-dlp / ffmpeg status --------------------------------
            ui.group(|ui| {
                ui.label("Setup");
                match &self.yt_dlp_path {
                    Some(path) => {
                        ui.label(format!("yt-dlp found: {}", path));
                    }
                    None => {
                        ui.label("yt-dlp not found.");
                        ui.label("Download yt-dlp.exe from https://github.com/yt-dlp/yt-dlp/releases and either put it next to this app, put it on PATH, or paste its path below.");
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Custom yt-dlp path:");
                    ui.text_edit_singleline(&mut self.custom_yt_dlp_path);
                    if ui.button("Detect").clicked() {
                        self.refresh_detection();
                    }
                });
                if self.ffmpeg_found {
                    ui.label("ffmpeg found (mp3 conversion available).");
                } else {
                    ui.label("ffmpeg not found: audio-only downloads will be saved as-is (not converted to mp3). Install ffmpeg and put it on PATH to enable mp3 conversion.");
                }
            });
            ui.add_space(8.0);

            // ---- New download ------------------------------------------
            ui.group(|ui| {
                ui.label("New download");
                ui.horizontal(|ui| {
                    ui.label("URL:");
                    ui.text_edit_singleline(&mut self.url_input);
                    if ui.button("Add to queue").clicked() {
                        self.add_job();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Format:");
                    for choice in FormatChoice::all() {
                        ui.radio_value(&mut self.format, choice, choice.label());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Output folder:");
                    ui.text_edit_singleline(&mut self.output_folder);
                    if ui.button("Open folder").clicked() {
                        let _ = fs::create_dir_all(&self.output_folder);
                        open_folder(&self.output_folder);
                    }
                });
            });

            if !self.error_message.is_empty() {
                ui.colored_label(egui::Color32::RED, &self.error_message);
            }
            ui.add_space(8.0);

            // ---- Queue --------------------------------------------------
            ui.label("Queue");
            if self.jobs.is_empty() {
                ui.label("Nothing queued yet. Paste a URL above and click Add to queue.");
            }

            // We collect button clicks first and act on them after the
            // row is drawn, to avoid borrowing the jobs list twice.
            let mut start_index: Option<usize> = None;
            let mut remove_index: Option<usize> = None;

            for (index, job) in self.jobs.iter_mut().enumerate() {
                ui.group(|ui| {
                    ui.label(&job.url);
                    ui.label(format!(
                        "{}  ·  {}",
                        job.format.label(),
                        job.status.label()
                    ));
                    if job.status.is_running() {
                        let fraction = (job.percent / 100.0).clamp(0.0, 1.0);
                        let mut bar_text = format!("{:.1}%", job.percent);
                        if !job.speed.is_empty() || !job.eta.is_empty() {
                            bar_text.push_str(&format!("  {}  ETA {}", job.speed, job.eta));
                        }
                        ui.add(egui::ProgressBar::new(fraction).text(bar_text));
                    }
                    ui.horizontal(|ui| {
                        if !job.status.is_running()
                            && !matches!(job.status, Status::Done)
                        {
                            if ui.button("Start").clicked() {
                                start_index = Some(index);
                            }
                        }
                        if job.status.is_running() {
                            if ui.button("Cancel").clicked() {
                                if let Some(handle) = &job.handle {
                                    handle.cancel();
                                }
                            }
                        }
                        if !job.status.is_running() && ui.button("Remove").clicked() {
                            remove_index = Some(index);
                        }
                    });
                });
            }

            if let Some(index) = start_index {
                self.start_job_at(index);
            }
            if let Some(index) = remove_index {
                self.jobs.remove(index);
            }
        });
    }
}
