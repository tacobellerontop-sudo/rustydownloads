//! The egui user interface: a dark dashboard in the style of the
//! reference design (sidebar + download hero + queue/history panels).
//!
//! This file only draws widgets and keeps UI state. It never downloads
//! anything itself: starting a job hands off to `download::start_job`,
//! and each frame it drains the progress updates that come back.
//!
//! One deliberate difference from the reference mock-up: the queue,
//! active download and history panels show only the user's real jobs.
//! The mock-up's example videos ("Chill Vibes", "Blinding Lights",
//! …) are not copied in as fake data.

use crate::download::{
    default_output_dir, find_program, is_app_managed, open_folder, start_job_with_args,
    start_yt_dlp_setup, FormatChoice, JobHandle, JobUpdate, SetupHandle, SetupTarget,
    SetupUpdate,
};
use egui::{Color32, RichText, Vec2};
use std::fs;

/// Dark dashboard palette (from the reference design).
const BG: Color32 = Color32::from_rgb(11, 14, 20);
const PANEL: Color32 = Color32::from_rgb(18, 20, 29);
const CARD: Color32 = Color32::from_rgb(23, 26, 36);
const ACCENT: Color32 = Color32::from_rgb(139, 124, 255);
const ACCENT_DARK: Color32 = Color32::from_rgb(108, 92, 231);
const TEXT: Color32 = Color32::from_rgb(230, 232, 240);
const MUTED: Color32 = Color32::from_rgb(154, 160, 176);

/// Which sidebar page is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nav {
    Home,
    Downloads,
    History,
    Settings,
}

impl Nav {
    fn label(self) -> &'static str {
        match self {
            Nav::Home => "Home",
            Nav::Downloads => "Downloads",
            Nav::History => "History",
            Nav::Settings => "Settings",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Nav::Home => "⌂",
            Nav::Downloads => "↓",
            Nav::History => "◷",
            Nav::Settings => "⚙",
        }
    }
}

/// Video + audio, or audio only. Quality (below) picks the cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormatKind {
    VideoAudio,
    AudioOnly,
}

impl FormatKind {
    fn label(self) -> &'static str {
        match self {
            FormatKind::VideoAudio => "Video + Audio (best quality)",
            FormatKind::AudioOnly => "Audio only (mp3)",
        }
    }
}

/// Resolution cap for video downloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Quality {
    Best,
    P1080,
    P720,
}

impl Quality {
    fn label(self) -> &'static str {
        match self {
            Quality::Best => "Best",
            Quality::P1080 => "1080p",
            Quality::P720 => "720p",
        }
    }
}

/// Quick-link category buttons under the hero. They only select a
/// category in the UI; every link still goes through yt-dlp, which
/// supports 1000+ sites, so no category is special-cased in downloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickLink {
    YouTube,
    Playlist,
    Channel,
    Url,
    MoreSites,
}

impl QuickLink {
    fn label(self) -> &'static str {
        match self {
            QuickLink::YouTube => "YouTube",
            QuickLink::Playlist => "Playlist",
            QuickLink::Channel => "Channel",
            QuickLink::Url => "URL",
            QuickLink::MoreSites => "More Sites",
        }
    }
}

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
            Status::Done => "Downloaded".to_string(),
            Status::Failed(msg) => format!("Failed: {}", msg),
            Status::Cancelled => "Cancelled".to_string(),
        }
    }

    fn is_running(&self) -> bool {
        matches!(self, Status::Queued | Status::Downloading)
    }

    fn is_finished(&self) -> bool {
        matches!(
            self,
            Status::Done | Status::Failed(_) | Status::Cancelled
        )
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

/// A short, readable title for a job. We have no video metadata
/// (no --dump-json call in v1), so the URL itself is the title,
/// stripped of its scheme/www prefix and truncated.
fn job_title(url: &str) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    let mut title: String = stripped.chars().take(42).collect();
    if stripped.chars().count() > 42 {
        title.push('…');
    }
    title
}

/// A stable thumbnail colour per URL, so each job gets a distinct
/// coloured placeholder tile (no images are fetched in v1).
fn thumb_color(url: &str) -> Color32 {
    let palette = [
        Color32::from_rgb(108, 92, 231),
        Color32::from_rgb(0, 150, 136),
        Color32::from_rgb(214, 48, 77),
        Color32::from_rgb(9, 132, 227),
        Color32::from_rgb(225, 112, 85),
        Color32::from_rgb(45, 52, 54),
    ];
    let hash: usize = url.bytes().fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
    palette[hash % palette.len()]
}

/// Draw a small coloured placeholder tile with the job's initial.
fn thumb(ui: &mut egui::Ui, url: &str, size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, 6.0, thumb_color(url));
    let initial = job_title(url).chars().next().unwrap_or('▶').to_string();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(20.0),
        Color32::WHITE,
    );
}

pub struct RustyDownloadsApp {
    nav: Nav,
    url_input: String,
    format_kind: FormatKind,
    quality: Quality,
    advanced_args: String,
    quick_link: Option<QuickLink>,
    output_folder: String,
    custom_yt_dlp_path: String,
    yt_dlp_path: Option<String>,
    ffmpeg_found: bool,
    error_message: String,
    jobs: Vec<QueueJob>,
    /// Present only while a built-in yt-dlp download/update is running.
    setup_handle: Option<SetupHandle>,
    /// Status line for the setup area: downloading progress, a done
    /// message, or an error. Empty when there is nothing to say.
    setup_message: String,
    setup_failed: bool,
}

impl Default for RustyDownloadsApp {
    fn default() -> Self {
        let mut app = Self {
            nav: Nav::Home,
            url_input: String::new(),
            format_kind: FormatKind::VideoAudio,
            quality: Quality::Best,
            advanced_args: String::new(),
            quick_link: None,
            output_folder: default_output_dir(),
            custom_yt_dlp_path: String::new(),
            yt_dlp_path: None,
            ffmpeg_found: false,
            error_message: String::new(),
            jobs: Vec::new(),
            setup_handle: None,
            setup_message: String::new(),
            setup_failed: false,
        };
        app.refresh_detection();
        app
    }
}

impl RustyDownloadsApp {
    /// The FormatChoice a new job gets from the Format + Quality rows.
    fn effective_format(&self) -> FormatChoice {
        match self.format_kind {
            FormatKind::AudioOnly => FormatChoice::AudioMp3,
            FormatKind::VideoAudio => match self.quality {
                Quality::Best => FormatChoice::BestVideo,
                Quality::P1080 => FormatChoice::Video1080p,
                Quality::P720 => FormatChoice::Video720p,
            },
        }
    }

    /// Extra yt-dlp arguments from Advanced Options, split on
    /// whitespace. Quoted strings are not supported in v1; the hint
    /// text in the UI says so by example only.
    fn extra_args(&self) -> Vec<String> {
        self.advanced_args
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    }

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

    /// Pull any built-in yt-dlp setup updates out of the running
    /// download. Called at the top of every frame, like `poll_jobs`.
    fn poll_setup(&mut self) {
        let Some(handle) = &self.setup_handle else {
            return;
        };
        for update in handle.drain_updates() {
            match update {
                SetupUpdate::Progress { downloaded, total } => {
                    let downloaded_mib = downloaded as f64 / 1_048_576.0;
                    self.setup_message = match total {
                        Some(total) => {
                            let total_mib = total as f64 / 1_048_576.0;
                            format!(
                                "Downloading yt-dlp… {:.1} MiB of {:.1} MiB",
                                downloaded_mib, total_mib
                            )
                        }
                        None => {
                            format!("Downloading yt-dlp… {:.1} MiB", downloaded_mib)
                        }
                    };
                    self.setup_failed = false;
                }
                SetupUpdate::Finished {
                    success,
                    message,
                    path,
                } => {
                    self.setup_handle = None;
                    // Prefer the verified path the download reported
                    // over the message text, so the field is actually
                    // used and the UI shows exactly where yt-dlp is.
                    self.setup_message = match (success, path) {
                        (true, Some(path)) => {
                            format!("yt-dlp is ready: {}", path.to_string_lossy())
                        }
                        _ => message,
                    };
                    self.setup_failed = !success;
                    if success {
                        // The status should flip to Found without the
                        // user having to restart the app or re-detect.
                        self.refresh_detection();
                    }
                }
            }
        }
    }

    /// Start a built-in yt-dlp download (or update) in the background.
    fn start_setup(&mut self, target: SetupTarget) {
        self.setup_handle = Some(start_yt_dlp_setup(target));
        self.setup_message = "Downloading yt-dlp…".to_string();
        self.setup_failed = false;
    }

    /// Validate the URL field and queue a job. Returns its index.
    fn add_job(&mut self) -> Option<usize> {
        self.error_message.clear();
        let url = self.url_input.trim().to_string();
        if url.is_empty() {
            self.error_message = "Paste a URL first.".to_string();
            return None;
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            self.error_message = "That does not look like a URL — it should start with https://".to_string();
            return None;
        }
        self.jobs.push(QueueJob {
            url,
            format: self.effective_format(),
            status: Status::Queued,
            percent: 0.0,
            speed: String::new(),
            eta: String::new(),
            handle: None,
        });
        self.url_input.clear();
        Some(self.jobs.len() - 1)
    }

    fn start_job_at(&mut self, index: usize) {
        self.error_message.clear();
        let Some(yt_dlp) = &self.yt_dlp_path else {
            self.error_message =
                "yt-dlp was not found. Use Download yt-dlp in Settings, then try again."
                    .to_string();
            return;
        };

        // Make sure the output folder exists before yt-dlp runs.
        if let Err(err) = fs::create_dir_all(&self.output_folder) {
            self.error_message = format!("Could not create output folder: {}", err);
            return;
        }

        let extra = self.extra_args();
        let job = &mut self.jobs[index];
        if job.status.is_running() {
            return; // Already running; the Start button is hidden then anyway.
        }
        let handle = start_job_with_args(
            yt_dlp,
            job.format,
            self.ffmpeg_found,
            &job.url,
            &self.output_folder,
            &extra,
        );
        job.handle = Some(handle);
        job.status = Status::Downloading;
        job.percent = 0.0;
        job.speed.clear();
        job.eta.clear();
    }

    /// The job the right panel calls the active download: a running
    /// one first, otherwise the next queued one.
    fn active_job_index(&self) -> Option<usize> {
        self.jobs
            .iter()
            .position(|j| j.status == Status::Downloading)
            .or_else(|| self.jobs.iter().position(|j| j.status == Status::Queued))
    }
}

/// Actions collected while drawing (applied afterwards, so drawing
/// never borrows `self.jobs` twice).
#[derive(Default)]
struct Actions {
    setup_target: Option<SetupTarget>,
    start_index: Option<usize>,
    remove_index: Option<usize>,
    clear_queue: bool,
    clear_history: bool,
    go_nav: Option<Nav>,
}

impl RustyDownloadsApp {
    /// One job row for the queue/history lists: thumbnail, title,
    /// format + status, progress bar when running, Start/Cancel/Remove.
    fn job_row(&mut self, ui: &mut egui::Ui, index: usize, actions: &mut Actions) {
        let job = &self.jobs[index];
        ui.horizontal(|ui| {
            thumb(ui, &job.url, Vec2::new(64.0, 48.0));
            ui.vertical(|ui| {
                ui.label(RichText::new(job_title(&job.url)).strong().color(TEXT));
                ui.label(
                    RichText::new(format!("{}  ·  {}", job.format.label(), job.status.label()))
                        .small()
                        .color(MUTED),
                );
                if job.status.is_running() {
                    let fraction = (job.percent / 100.0).clamp(0.0, 1.0);
                    let mut bar_text = format!("{:.1}%", job.percent);
                    if !job.speed.is_empty() || !job.eta.is_empty() {
                        bar_text.push_str(&format!("  {}  ETA {}", job.speed, job.eta));
                    }
                    ui.add(egui::ProgressBar::new(fraction).text(bar_text));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !job.status.is_running() && ui.button("Remove").clicked() {
                    actions.remove_index = Some(index);
                }
                if job.status.is_running() && ui.button("Cancel").clicked() {
                    if let Some(handle) = &job.handle {
                        handle.cancel();
                    }
                }
                if !job.status.is_running() && !matches!(job.status, Status::Done) && ui.button("Start").clicked() {
                    actions.start_index = Some(index);
                }
            });
        });
    }

    /// The Setup card, shared by the Home banner (when yt-dlp is
    /// missing) and the Settings page.
    fn setup_card(&mut self, ui: &mut egui::Ui, actions: &mut Actions) {
        ui.group(|ui| {
            ui.label(RichText::new("Setup").strong().color(TEXT));
            match &self.yt_dlp_path {
                Some(path) => {
                    ui.label(RichText::new(format!("yt-dlp found: {}", path)).color(MUTED));
                    // Only an app-managed copy may be replaced by
                    // the Update button. A copy on PATH, or a custom
                    // path the user typed in, is never overwritten.
                    let managed = std::path::Path::new(path).to_path_buf();
                    if is_app_managed(&managed) && self.setup_handle.is_none() {
                        if ui.button("Update yt-dlp").clicked() {
                            actions.setup_target = Some(SetupTarget::Replace(managed));
                        }
                    }
                }
                None => {
                    ui.label(RichText::new("yt-dlp not found.").color(TEXT));
                    if self.setup_handle.is_none() {
                        ui.label(RichText::new("Click Download yt-dlp and the app will fetch the official standalone binary from the yt-dlp releases page — nothing else to install by hand.").color(MUTED));
                        if ui.button("Download yt-dlp").clicked() {
                            actions.setup_target = Some(SetupTarget::Auto);
                        }
                    }
                }
            }
            if !self.setup_message.is_empty() {
                if self.setup_failed {
                    ui.colored_label(Color32::RED, &self.setup_message);
                } else {
                    ui.label(RichText::new(&self.setup_message).color(MUTED));
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
                ui.label("ffmpeg not found: audio-only downloads will be saved as-is (not converted to mp3). Install ffmpeg and put it on PATH, or use a release build, which embeds ffmpeg.");
            }
        });
    }

    fn home_view(&mut self, ui: &mut egui::Ui, actions: &mut Actions) {
        ui.label(RichText::new("Download anything").size(32.0).strong().color(TEXT));
        ui.label(
            RichText::new("Paste a YouTube link, playlist, or channel — and let yt-dlp do the rest.")
                .color(MUTED),
        );
        ui.add_space(12.0);

        // URL + Download (adds to the queue and starts it at once,
        // which is what the reference design's Download button does).
        let mut download_clicked = false;
        ui.horizontal(|ui| {
            let url_edit = egui::TextEdit::singleline(&mut self.url_input)
                .hint_text("https://www.youtube.com/watch?v=…")
                .desired_width(ui.available_width() - 130.0);
            ui.add(url_edit);
            if ui
                .add(egui::Button::new("↓  Download").fill(ACCENT))
                .clicked()
            {
                download_clicked = true;
            }
        });
        if download_clicked {
            if let Some(index) = self.add_job() {
                self.start_job_at(index);
            }
        }
        if !self.error_message.is_empty() {
            ui.colored_label(Color32::RED, &self.error_message);
        }
        ui.add_space(12.0);

        // Format / Quality / Output / Advanced card.
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Format").strong().color(TEXT));
                    ui.label(RichText::new("Video + Audio (best quality)").small().color(MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("format_kind")
                        .selected_text(self.format_kind.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.format_kind,
                                FormatKind::VideoAudio,
                                FormatKind::VideoAudio.label(),
                            );
                            ui.selectable_value(
                                &mut self.format_kind,
                                FormatKind::AudioOnly,
                                FormatKind::AudioOnly.label(),
                            );
                        });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Quality").strong().color(TEXT));
                    ui.label(RichText::new("Highest available (auto)").small().color(MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("quality")
                        .selected_text(self.quality.label())
                        .show_ui(ui, |ui| {
                            for q in [Quality::Best, Quality::P1080, Quality::P720] {
                                ui.selectable_value(&mut self.quality, q, q.label());
                            }
                        });
                });
            });
            if self.format_kind == FormatKind::AudioOnly {
                ui.label(RichText::new("Quality is ignored for audio-only downloads.").small().color(MUTED));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Output Folder").strong().color(TEXT));
                    ui.label(RichText::new(&self.output_folder).small().color(MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Browse").clicked() {
                        let _ = fs::create_dir_all(&self.output_folder);
                        open_folder(&self.output_folder);
                    }
                    ui.text_edit_singleline(&mut self.output_folder);
                });
            });
            ui.separator();
            egui::CollapsingHeader::new("Advanced Options (optional)")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(RichText::new("Add custom arguments (e.g. --cookies, --proxy, etc.)").small().color(MUTED));
                    ui.text_edit_singleline(&mut self.advanced_args);
                });
        });
        ui.add_space(12.0);

        // Quick Links.
        ui.label(RichText::new("Quick Links").small().color(MUTED));
        ui.horizontal(|ui| {
            for link in [
                QuickLink::YouTube,
                QuickLink::Playlist,
                QuickLink::Channel,
                QuickLink::Url,
                QuickLink::MoreSites,
            ] {
                let selected = self.quick_link == Some(link);
                let button = if selected {
                    egui::Button::new(link.label()).fill(ACCENT_DARK)
                } else {
                    egui::Button::new(link.label())
                };
                if ui.add(button).clicked() {
                    self.quick_link = Some(link);
                }
            }
        });
        ui.add_space(12.0);

        // Support banner.
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("✦  Support for 1000+ sites").strong().color(TEXT));
                    ui.label(RichText::new("YouTube, TikTok, Twitter, Instagram, SoundCloud, Twitch and more.").small().color(MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("→").size(20.0).color(ACCENT));
                });
            });
        });

        // If yt-dlp is missing, the hero cannot work — surface Setup
        // right here instead of hiding it on the Settings page.
        if self.yt_dlp_path.is_none() {
            ui.add_space(12.0);
            self.setup_card(ui, actions);
        }
    }

    fn downloads_view(&mut self, ui: &mut egui::Ui, actions: &mut Actions) {
        ui.label(RichText::new("Downloads").size(28.0).strong().color(TEXT));
        ui.label(RichText::new(format!("{} in the queue", self.jobs.len())).color(MUTED));
        ui.add_space(8.0);
        if self.jobs.is_empty() {
            ui.label("Nothing queued yet. Go to Home, paste a URL, and click Download.");
            return;
        }
        if ui.button("Clear queue").clicked() {
            actions.clear_queue = true;
        }
        ui.add_space(8.0);
        for index in 0..self.jobs.len() {
            ui.group(|ui| {
                self.job_row(ui, index, actions);
            });
            // Only one row's buttons can fire per frame in practice;
            // stop drawing after an action so indices stay valid.
            if actions.start_index.is_some() || actions.remove_index.is_some() {
                break;
            }
        }
    }

    fn history_view(&mut self, ui: &mut egui::Ui, actions: &mut Actions) {
        ui.label(RichText::new("History").size(28.0).strong().color(TEXT));
        let finished: Vec<usize> = self
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| j.status.is_finished())
            .map(|(i, _)| i)
            .collect();
        ui.label(RichText::new(format!("{} finished", finished.len())).color(MUTED));
        ui.add_space(8.0);
        if finished.is_empty() {
            ui.label("Nothing finished yet. Finished downloads will appear here.");
            return;
        }
        if ui.button("Clear history").clicked() {
            actions.clear_history = true;
        }
        ui.add_space(8.0);
        for index in finished {
            ui.group(|ui| {
                self.job_row(ui, index, actions);
            });
            if actions.remove_index.is_some() {
                break;
            }
        }
    }

    fn settings_view(&mut self, ui: &mut egui::Ui, actions: &mut Actions) {
        ui.label(RichText::new("Settings").size(28.0).strong().color(TEXT));
        ui.add_space(8.0);
        self.setup_card(ui, actions);
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(RichText::new("Output Folder").strong().color(TEXT));
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.output_folder);
                if ui.button("Open folder").clicked() {
                    let _ = fs::create_dir_all(&self.output_folder);
                    open_folder(&self.output_folder);
                }
            });
            ui.label(RichText::new("Advanced Options (extra yt-dlp arguments)").strong().color(TEXT));
            ui.text_edit_singleline(&mut self.advanced_args);
        });
    }

    /// Right panel for Home: active download, queue, history.
    fn right_panel(&mut self, ui: &mut egui::Ui, actions: &mut Actions) {
        // Active Download.
        ui.label(RichText::new("Active Download").strong().color(TEXT));
        match self.active_job_index() {
            Some(index) => {
                let job = &self.jobs[index];
                ui.horizontal(|ui| {
                    thumb(ui, &job.url, Vec2::new(64.0, 48.0));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(job_title(&job.url)).strong().color(TEXT));
                        ui.label(RichText::new(job.format.label()).small().color(MUTED));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let speed = if job.speed.is_empty() { "—".to_string() } else { job.speed.clone() };
                        ui.label(RichText::new(speed).strong().color(ACCENT));
                    });
                });
                let fraction = (job.percent / 100.0).clamp(0.0, 1.0);
                let mut bar_text = format!("{:.0}%", job.percent);
                if !job.eta.is_empty() {
                    bar_text.push_str(&format!("  ·  ~ {} left", job.eta));
                }
                ui.add(egui::ProgressBar::new(fraction).text(bar_text));
                if job.status.is_running() && ui.button("Cancel").clicked() {
                    if let Some(handle) = &job.handle {
                        handle.cancel();
                    }
                }
            }
            None => {
                ui.label(RichText::new("No active download.").color(MUTED));
            }
        }
        ui.add_space(16.0);

        // Queue.
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Queue  {}", self.jobs.len())).strong().color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    actions.clear_queue = true;
                }
            });
        });
        if self.jobs.is_empty() {
            ui.label(RichText::new("Queue is empty.").color(MUTED));
        }
        for index in 0..self.jobs.len() {
            ui.group(|ui| {
                self.job_row(ui, index, actions);
            });
            if actions.start_index.is_some() || actions.remove_index.is_some() {
                break;
            }
        }
        ui.add_space(16.0);

        // History.
        ui.horizontal(|ui| {
            ui.label(RichText::new("History").strong().color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("View all").clicked() {
                    actions.go_nav = Some(Nav::History);
                }
            });
        });
        let finished: Vec<usize> = self
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| j.status.is_finished())
            .map(|(i, _)| i)
            .collect();
        if finished.is_empty() {
            ui.label(RichText::new("No finished downloads yet.").color(MUTED));
        }
        for index in finished.iter().take(3) {
            ui.group(|ui| {
                self.job_row(ui, *index, actions);
            });
            if actions.remove_index.is_some() {
                break;
            }
        }
    }

    fn apply_actions(&mut self, actions: Actions) {
        if let Some(target) = actions.setup_target {
            self.start_setup(target);
        }
        if let Some(index) = actions.start_index {
            if index < self.jobs.len() {
                self.start_job_at(index);
            }
        }
        if let Some(index) = actions.remove_index {
            if index < self.jobs.len() {
                self.jobs.remove(index);
            }
        }
        if actions.clear_queue {
            // Only queued (not running) jobs are cleared; a running
            // download keeps its child process alive.
            self.jobs.retain(|j| j.status.is_running());
        }
        if actions.clear_history {
            self.jobs.retain(|j| !j.status.is_finished());
        }
        if let Some(nav) = actions.go_nav {
            self.nav = nav;
        }
    }
}

impl eframe::App for RustyDownloadsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_jobs();
        self.poll_setup();

        // Keep repainting while anything is running, so the progress
        // bars move even when the user is not touching the mouse.
        if self.jobs.iter().any(|j| j.status.is_running()) || self.setup_handle.is_some() {
            ctx.request_repaint();
        }

        let mut actions = Actions::default();

        // ---- Left sidebar --------------------------------------------
        egui::SidePanel::left("sidebar")
            .exact_width(190.0)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("▶  yt-dlp").size(20.0).strong().color(ACCENT));
                    ui.add_space(16.0);
                    for nav in [Nav::Home, Nav::Downloads, Nav::History, Nav::Settings] {
                        let selected = self.nav == nav;
                        let label = format!("{}  {}", nav.icon(), nav.label());
                        let button = if selected {
                            egui::Button::new(label).fill(ACCENT_DARK)
                        } else {
                            egui::Button::new(label)
                        };
                        if ui.add_sized(Vec2::new(ui.available_width(), 32.0), button).clicked() {
                            actions.go_nav = Some(nav);
                        }
                    }
                    ui.add_space(16.0);
                    ui.separator();
                    // Bottom status card, like the reference design's
                    // yt-dlp version box (without inventing a version:
                    // we show the detected path instead).
                    ui.label(RichText::new("yt-dlp").strong().color(TEXT));
                    match &self.yt_dlp_path {
                        Some(path) => {
                            ui.label(RichText::new(job_title(path)).small().color(MUTED));
                            ui.label(RichText::new("Ready").small().color(ACCENT));
                        }
                        None => {
                            ui.label(RichText::new("Not found").small().color(MUTED));
                            ui.label(RichText::new("See Settings").small().color(MUTED));
                        }
                    }
                    ui.label(RichText::new("Fast. Flexible. Powerful.").small().color(MUTED));
                });
            });

        // ---- Right panel (Home only) ----------------------------------
        if self.nav == Nav::Home {
            egui::SidePanel::right("right_panel")
                .exact_width(340.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.right_panel(ui, &mut actions);
                    });
                });
        }

        // ---- Centre ----------------------------------------------------
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                match self.nav {
                    Nav::Home => self.home_view(ui, &mut actions),
                    Nav::Downloads => self.downloads_view(ui, &mut actions),
                    Nav::History => self.history_view(ui, &mut actions),
                    Nav::Settings => self.settings_view(ui, &mut actions),
                }
            });
        });

        self.apply_actions(actions);
    }
}

/// Dark visuals for the dashboard. Called once from main.rs.
pub fn dark_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = BG;
    visuals.panel_fill = PANEL;
    visuals.extreme_bg_color = BG;
    visuals.widgets.noninteractive.bg_fill = CARD;
    visuals.widgets.inactive.bg_fill = CARD;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(32, 36, 52);
    visuals.widgets.active.bg_fill = Color32::from_rgb(32, 36, 52);
    visuals.selection.bg_fill = ACCENT;
    visuals
}
