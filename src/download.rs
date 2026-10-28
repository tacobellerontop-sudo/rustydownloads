//! Everything that talks to yt-dlp lives here.
//!
//! The app itself never downloads anything. It asks this module to start a
//! job, and this module spawns `yt-dlp` as a separate process, reads the
//! lines it prints, and turns the progress lines into numbers the UI can
//! show. If yt-dlp is missing, nothing is downloaded and the UI says so.

use std::env;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// The four format choices shown in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatChoice {
    BestVideo,
    Video1080p,
    Video720p,
    AudioMp3,
}

impl FormatChoice {
    /// Short label used in the UI and in the queue list.
    pub fn label(self) -> &'static str {
        match self {
            FormatChoice::BestVideo => "Best video",
            FormatChoice::Video1080p => "1080p",
            FormatChoice::Video720p => "720p",
            FormatChoice::AudioMp3 => "Audio only (mp3)",
        }
    }

    /// All four choices, in the order the UI shows them.
    pub fn all() -> [FormatChoice; 4] {
        [
            FormatChoice::BestVideo,
            FormatChoice::Video1080p,
            FormatChoice::Video720p,
            FormatChoice::AudioMp3,
        ]
    }
}

/// One parsed progress update from a yt-dlp output line.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgressInfo {
    /// 0.0 to 100.0
    pub percent: f32,
    /// e.g. "1.23MiB/s", or empty if yt-dlp did not print one.
    pub speed: String,
    /// e.g. "00:42", or empty if yt-dlp did not print one.
    pub eta: String,
}

/// Parse one line of yt-dlp output.
///
/// Real examples (with `--newline`, one line per update):
///   `[download]  42.1% of ~ 123.45MiB at  1.23MiB/s ETA 00:42`
///   `[download] 100% of 123.45MiB in 00:01:23`
///
/// Anything that is not a download-progress line returns `None` and is
/// ignored by the caller -- it must never crash the app.
pub fn parse_progress_line(line: &str) -> Option<ProgressInfo> {
    // Only lines from the downloader carry a percentage.
    if !line.contains("[download]") || !line.contains('%') {
        return None;
    }

    let tokens: Vec<&str> = line.split_whitespace().collect();

    // Find the token that ends in '%', e.g. "42.1%".
    let percent_token = tokens.iter().find(|t| t.ends_with('%'))?;
    let percent_text = percent_token.trim_end_matches('%');
    let percent: f32 = percent_text.parse().ok()?;

    // Speed is the token right after "at", ETA is the token after "ETA".
    // Both are optional: the 100% line has neither.
    let mut speed = String::new();
    let mut eta = String::new();
    for (i, token) in tokens.iter().enumerate() {
        if *token == "at" {
            if let Some(next) = tokens.get(i + 1) {
                speed = next.to_string();
            }
        }
        if *token == "ETA" {
            if let Some(next) = tokens.get(i + 1) {
                eta = next.to_string();
            }
        }
    }

    Some(ProgressInfo {
        percent,
        speed,
        eta,
    })
}

/// Build the yt-dlp arguments for one download.
///
/// This is the whole "mapping" between the UI choices and yt-dlp:
/// the format choice becomes `-f ...` (or `-x` for audio), and every
/// download gets `--newline` so progress arrives one line at a time,
/// plus `-o` so files land in the chosen folder with the video title
/// as the filename.
pub fn build_yt_dlp_args(
    format: FormatChoice,
    ffmpeg_available: bool,
    url: &str,
    output_dir: &str,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--newline".to_string(),
        "--no-playlist".to_string(),
        "-o".to_string(),
        // %(title)s is yt-dlp's placeholder for the video title.
        // %(ext)s is the file extension yt-dlp ends up using.
        format!("{}/%(title)s.%(ext)s", output_dir),
    ];

    match format {
        FormatChoice::BestVideo => {
            // No -f flag: yt-dlp picks its default "best" format.
        }
        FormatChoice::Video1080p => {
            args.push("-f".to_string());
            args.push("bv*[height<=1080]+ba/b[height<=1080]".to_string());
        }
        FormatChoice::Video720p => {
            args.push("-f".to_string());
            args.push("bv*[height<=720]+ba/b[height<=720]".to_string());
        }
        FormatChoice::AudioMp3 => {
            if ffmpeg_available {
                // -x extracts audio, --audio-format converts it to mp3.
                // Conversion is ffmpeg's job, not yt-dlp's.
                args.push("-x".to_string());
                args.push("--audio-format".to_string());
                args.push("mp3".to_string());
            } else {
                // No ffmpeg, so do not ask for a conversion that would
                // fail. Just take the best audio stream as it is
                // (often .webm / .opus). The UI warns about this.
                args.push("-f".to_string());
                args.push("bestaudio/best".to_string());
            }
        }
    }

    args.push(url.to_string());
    args
}

/// Updates a running job sends back to the UI thread.
#[derive(Debug)]
pub enum JobUpdate {
    Progress(ProgressInfo),
    Finished { success: bool, message: String },
}

/// The UI side of a running job.
///
/// Dropping this does not stop the download thread; use `cancel()` for that.
pub struct JobHandle {
    receiver: Receiver<JobUpdate>,
    cancel_flag: Arc<AtomicBool>,
    // Shared with the download thread so Cancel can kill the child process.
    child: Arc<Mutex<Option<Child>>>,
}

impl JobHandle {
    /// Drain all updates that have arrived so far (never blocks).
    pub fn drain_updates(&self) -> Vec<JobUpdate> {
        let mut out = Vec::new();
        while let Ok(update) = self.receiver.try_recv() {
            out.push(update);
        }
        out
    }

    /// Ask the download thread to stop, and kill yt-dlp if it is running.
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                // Best effort: killing may fail if it already exited.
                let _ = child.kill();
            }
        }
    }
}

/// Start a download in a background thread.
///
/// Returns a handle the UI polls for progress. The thread owns the
/// yt-dlp child process; the UI never blocks waiting for it.
pub fn start_job(
    yt_dlp_path: &str,
    format: FormatChoice,
    ffmpeg_available: bool,
    url: &str,
    output_dir: &str,
) -> JobHandle {
    let (sender, receiver) = channel::<JobUpdate>();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

    let url = url.to_string();
    let output_dir = output_dir.to_string();
    let yt_dlp_path = yt_dlp_path.to_string();
    let sender_for_thread = sender;
    let cancel_for_thread = Arc::clone(&cancel_flag);
    let child_for_thread = Arc::clone(&child);

    thread::spawn(move || {
        let args = build_yt_dlp_args(format, ffmpeg_available, &url, &output_dir);

        let spawned = Command::new(&yt_dlp_path)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child_process = match spawned {
            Ok(child) => child,
            Err(err) => {
                let _ = sender_for_thread.send(JobUpdate::Finished {
                    success: false,
                    message: format!("Could not start yt-dlp: {}", err),
                });
                return;
            }
        };

        // Feed both stdout and stderr lines into the same channel.
        // yt-dlp's progress lines can appear on either stream, and
        // reading both means we do not miss progress or deadlock if
        // one pipe fills up while nobody reads it.
        if let Some(stdout) = child_process.stdout.take() {
            let sender = sender_for_thread.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(progress) = parse_progress_line(&line) {
                        let _ = sender.send(JobUpdate::Progress(progress));
                    }
                }
            });
        }
        if let Some(stderr) = child_process.stderr.take() {
            let sender = sender_for_thread.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(progress) = parse_progress_line(&line) {
                        let _ = sender.send(JobUpdate::Progress(progress));
                    }
                }
            });
        }

        // Hand the child to the shared slot so Cancel can kill it,
        // then poll for exit without holding the lock the whole time.
        if let Ok(mut guard) = child_for_thread.lock() {
            *guard = Some(child_process);
        }

        loop {
            if cancel_for_thread.load(Ordering::SeqCst) {
                if let Ok(mut guard) = child_for_thread.lock() {
                    if let Some(child) = guard.as_mut() {
                        let _ = child.kill();
                    }
                }
                let _ = sender_for_thread.send(JobUpdate::Finished {
                    success: false,
                    message: "Cancelled".to_string(),
                });
                return;
            }

            let status = {
                let mut guard = match child_for_thread.lock() {
                    Ok(guard) => guard,
                    Err(_) => break,
                };
                match guard.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(status)) => Some(status),
                        Ok(None) => None,
                        Err(err) => {
                            let _ = sender_for_thread.send(JobUpdate::Finished {
                                success: false,
                                message: format!("Error waiting for yt-dlp: {}", err),
                            });
                            return;
                        }
                    },
                    None => break,
                }
            };

            if let Some(status) = status {
                let (success, message) = if status.success() {
                    (true, "Done".to_string())
                } else {
                    (
                        false,
                        format!(
                            "yt-dlp exited with status {}. \
                             Run yt-dlp with the same URL in a terminal for details.",
                            status
                        ),
                    )
                };
                let _ = sender_for_thread.send(JobUpdate::Finished { success, message });
                return;
            }

            thread::sleep(Duration::from_millis(100));
        }
    });

    JobHandle {
        receiver,
        cancel_flag,
        child,
    }
}

/// Default folder downloads go to: ~/Downloads/rustydownloads
/// (on Windows: %USERPROFILE%\Downloads\rustydownloads).
pub fn default_output_dir() -> String {
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let mut path = PathBuf::from(home);
    path.push("Downloads");
    path.push("rustydownloads");
    path.to_string_lossy().to_string()
}

/// Look for a program in, in order:
///   1. a custom path the user typed in (if it is a file),
///   2. next to this app's own executable (e.g. yt-dlp.exe beside it),
///   3. every folder on PATH.
pub fn find_program(custom_path: &str, program: &str) -> Option<PathBuf> {
    if !custom_path.trim().is_empty() {
        let candidate = PathBuf::from(custom_path.trim());
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // Next to our own exe.
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in program_names(program) {
                let candidate = dir.join(&name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    // On PATH.
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            for name in program_names(program) {
                let candidate = dir.join(&name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

/// On Windows, programs are usually called `yt-dlp.exe`; elsewhere just
/// `yt-dlp`. Check both spellings so a renamed copy is still found.
fn program_names(program: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            format!("{}.exe", program),
            program.to_string(),
        ]
    } else {
        vec![program.to_string()]
    }
}

/// Open a folder in the system's file manager. Best effort: failures
/// are ignored by the caller (the button just does nothing).
pub fn open_folder(path: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(path).spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_normal_download_line() {
        let line = "[download]  42.1% of ~ 123.45MiB at  1.23MiB/s ETA 00:42";
        let progress = parse_progress_line(line).expect("should parse");
        assert!((progress.percent - 42.1).abs() < 0.001);
        assert_eq!(progress.speed, "1.23MiB/s");
        assert_eq!(progress.eta, "00:42");
    }

    #[test]
    fn parses_a_100_percent_line() {
        let line = "[download] 100% of 123.45MiB in 00:01:23";
        let progress = parse_progress_line(line).expect("should parse");
        assert!((progress.percent - 100.0).abs() < 0.001);
        // The finished line carries no speed or ETA; empty is fine.
        assert_eq!(progress.speed, "");
        assert_eq!(progress.eta, "");
    }

    #[test]
    fn ignores_non_progress_lines() {
        assert_eq!(parse_progress_line("[download] Destination: video.mp4"), None);
        assert_eq!(parse_progress_line("[info] Downloading video"), None);
        assert_eq!(parse_progress_line(""), None);
        assert_eq!(parse_progress_line("just some text 50%"), None);
    }

    #[test]
    fn audio_args_use_mp3_only_when_ffmpeg_is_present() {
        let with_ffmpeg = build_yt_dlp_args(FormatChoice::AudioMp3, true, "URL", "OUT");
        assert!(with_ffmpeg.contains(&"-x".to_string()));
        assert!(with_ffmpeg.contains(&"mp3".to_string()));

        let without_ffmpeg =
            build_yt_dlp_args(FormatChoice::AudioMp3, false, "URL", "OUT");
        assert!(!without_ffmpeg.contains(&"-x".to_string()));
        assert!(without_ffmpeg.contains(&"bestaudio/best".to_string()));
    }
}
