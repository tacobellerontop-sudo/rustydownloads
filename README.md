# rustydownloads

A small Windows-first (but cross-platform) GUI downloader, written in Rust
with [egui](https://github.com/emilk/egui) / eframe.

Paste a video URL, pick a format, and download it. Rust handles the window,
the queue, and the progress bars. **yt-dlp does the actual downloading.**

## How yt-dlp is used

yt-dlp is a separate, feature-rich command-line audio/video downloader
written in Python, originally a fork of youtube-dl. It supports thousands
of sites. It is **not bundled with this app and not reimplemented here**.

When you click Start, rustydownloads:

1. Finds `yt-dlp` (see Setup below).
2. Runs it as a separate process with arguments built from your choices,
   for example `-f bv*[height<=1080]+ba/b[height<=1080]` for 1080p, or
   `-x --audio-format mp3` for audio-only when ffmpeg is available.
3. Reads yt-dlp's `--newline` progress lines, e.g.
   `[download]  42.1% of ~ 123.45MiB at  1.23MiB/s ETA 00:42`,
   and turns them into the progress bar, speed, and ETA you see.
4. Saves the file as `<video title>.<ext>` in your output folder.

yt-dlp is released under the Unlicense. See
https://github.com/yt-dlp/yt-dlp for its own documentation and license.

ffmpeg is only used by yt-dlp to convert audio to mp3 (and to merge
separate video/audio streams for some formats). It is also a separate
program, not bundled here.

## Setup (do this once)

1. **yt-dlp (required).** Download the Windows build `yt-dlp.exe` from
   https://github.com/yt-dlp/yt-dlp/releases and do one of:
   - put `yt-dlp.exe` in the same folder as `rustydownloads.exe`, or
   - put it somewhere on your PATH, or
   - paste its full path into the "Custom yt-dlp path" field in the app
     and click Detect.

   The app shows "yt-dlp found" with the path it will use, or clear
   instructions if it cannot find it. Nothing downloads until it is found.

2. **ffmpeg (optional, recommended for mp3).** Download a Windows build
   from https://ffmpeg.org/download.html (or install it with a package
   manager) and put `ffmpeg.exe` on PATH or next to the app.
   - With ffmpeg: Audio only downloads as mp3.
   - Without ffmpeg: the app warns you, and audio-only downloads are
     saved as-is (for example `.webm` / `.opus`) instead of failing.

3. **Build the app** (see below).

## Build and run

You need a Rust toolchain (https://rustup.rs). Then, from this folder:

```sh
cargo run --release
```

That builds `rustydownloads` and opens the window. On Windows, the
release binary is at `target\release\rustydownloads.exe` — copy
`yt-dlp.exe` next to it (see Setup) and run it from there.

To check the code without building a binary:

```sh
cargo check
cargo test
```

## What v1 does

- URL field, Add to queue, queue list with per-job status
- Formats: Best video, 1080p, 720p, Audio only (mp3 with ffmpeg, best
  audio as-is without it)
- Output folder (default `~/Downloads/rustydownloads`, on Windows
  `%USERPROFILE%\Downloads\rustydownloads`), with an Open folder button
- Start / Cancel per job, with a live progress bar, % / speed / ETA
  parsed from yt-dlp's real output — no fake progress
- yt-dlp detection (next to the exe, on PATH, or a custom path), with a
  clear missing message and setup instructions

## Intentionally left for v2

- Video title / thumbnail preview (could come from yt-dlp `--dump-json`,
  but v1 does not block on it)
- Playlists (v1 passes `--no-playlist`)
- Subtitles, SponsorBlock, and other yt-dlp options
- Download history saved between runs
- Auto-updating yt-dlp / ffmpeg from inside the app
