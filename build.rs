//! Build-time embedding for yt-dlp and ffmpeg.
//!
//! Neither binary is committed to this repo (see .gitignore, and the
//! release workflow, which fetches both at build time). If a binary
//! is present in `assets/` when the app is built, it is baked into
//! the executable: `src/download.rs` extracts it to the app-managed
//! bin folder on first run, so the release exe is fully self-contained
//! and needs no sidecar files or download step.
//!
//! If an asset is missing -- the normal case for a local dev build --
//! the corresponding constant is `None` and the app falls back to its
//! existing behaviour: find a copy on disk, or download yt-dlp
//! in-app. A missing asset is never a build error, so `cargo check`
//! for another target (e.g. Windows from Linux) keeps working.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let is_windows = target_os == "windows";

    // (asset filename, constant name)
    let binaries: [(&str, &str); 2] = [
        (if is_windows { "yt-dlp.exe" } else { "yt-dlp" }, "EMBEDDED_YTDLP"),
        (if is_windows { "ffmpeg.exe" } else { "ffmpeg" }, "EMBEDDED_FFMPEG"),
    ];

    let mut generated = String::new();
    for (asset_name, const_name) in binaries {
        let asset = manifest_dir.join("assets").join(asset_name);
        println!("cargo:rerun-if-changed={}", asset.display());
        if asset.is_file() {
            // Copy into OUT_DIR so the generated include_bytes! path
            // is stable and independent of the source tree layout.
            let embedded_copy = out_dir.join(format!("embedded_{}", asset_name));
            fs::copy(&asset, &embedded_copy)
                .unwrap_or_else(|err| panic!("could not copy {}: {}", asset.display(), err));
            generated.push_str(&format!(
                "pub const {}: Option<&[u8]> = Some(include_bytes!({:?}));\n",
                const_name, embedded_copy
            ));
        } else {
            generated.push_str(&format!(
                "pub const {}: Option<&[u8]> = None;\n",
                const_name
            ));
        }
    }

    fs::write(out_dir.join("embedded.rs"), generated)
        .expect("could not write embedded.rs");
}
