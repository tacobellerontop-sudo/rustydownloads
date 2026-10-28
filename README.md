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

1. **yt-dlp (required, but built in).** You do not need to install
   yt-dlp by hand. On first run, click **Download yt-dlp** in the app:
   it downloads the official standalone binary from
   https://github.com/yt-dlp/yt-dlp/releases (Windows: `yt-dlp.exe`,
   other systems: `yt-dlp`) in a background thread, verifies it by
   running `yt-dlp --version`, and then shows "yt-dlp found" — no
   restart needed. Nothing else downloads until yt-dlp is ready.

   Where it is stored: next to the app's own executable if that
   folder is writable, otherwise in a per-user app folder —
   `%LOCALAPPDATA%\rustydownloads\bin` on Windows, or
   `~/.local/share/rustydownloads/bin` elsewhere. The app looks for
   yt-dlp in this order: a custom path you typed in, next to the
   app exe, the app-managed folder, then PATH.

   If yt-dlp is already found in an app-managed folder, the app
   shows **Update yt-dlp** instead, which re-downloads and replaces
   only that app-managed copy. The app never overwrites a copy found
   on PATH or a custom path you typed in.

   Manual setup still works if you prefer it: put `yt-dlp.exe` in
   the same folder as `rustydownloads.exe`, put it somewhere on
   your PATH, or paste its full path into the "Custom yt-dlp path"
   field in the app and click Detect.

   **yt-dlp is not bundled in this repo and is not reimplemented
   here.** It is a separate program, released under the Unlicense,
   and the downloaded binary is gitignored — never commit it. See
   "Release zip" below for the packaged version, which does include
   a downloaded yt-dlp.exe alongside the app.

2. **ffmpeg (optional, recommended for mp3).** Download a Windows build
   from https://ffmpeg.org/download.html (or install it with a package
   manager) and put `ffmpeg.exe` on PATH or next to the app.
   - With ffmpeg: Audio only downloads as mp3.
   - Without ffmpeg: the app warns you, and audio-only downloads are
     saved as-is (for example `.webm` / `.opus`) instead of failing.
   - ffmpeg is not bundled and is not downloaded by the app.

3. **Build the app** (see below).

## Build and run

You need a Rust toolchain (https://rustup.rs). Then, from this folder:

```sh
cargo run --release
```

That builds `rustydownloads` and opens the window. On Windows, the
release binary is at `target\release\rustydownloads.exe` — run it,
then use the in-app **Download yt-dlp** button (see Setup) instead
of copying `yt-dlp.exe` by hand.

## Release zip (packaged version)

For end users who do not want to build from source, tagged releases
are packaged by `.github/workflows/release.yml`: on a `v*` tag (or a
manual workflow run), the workflow builds the app on Windows, downloads
`yt-dlp.exe` from the official yt-dlp releases **at build time**, and
zips `rustydownloads.exe` + `yt-dlp.exe` + `README.md` into a single
release zip. An end user just unzips it and runs `rustydownloads.exe`
— yt-dlp is already beside it, so no setup step is needed.

The repo itself still contains no yt-dlp binary: the binary only
exists inside the release zip (and, for source builds, in the
per-user/app folder described in Setup). The in-app
Download/Update yt-dlp buttons remain as the fallback and update path
for people who build from source.

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
- yt-dlp detection (custom path, next to the exe, the app-managed
  folder, or on PATH), with a built-in Download yt-dlp button when
  it is missing and an Update yt-dlp button for app-managed copies

## Intentionally left for v2

- Video title / thumbnail preview (could come from yt-dlp `--dump-json`,
  but v1 does not block on it)
- Playlists (v1 passes `--no-playlist`)
- Subtitles, SponsorBlock, and other yt-dlp options
- Download history saved between runs
- Auto-updating ffmpeg from inside the app (yt-dlp already has
  in-app Download/Update; ffmpeg is still manual)
