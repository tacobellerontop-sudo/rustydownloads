mod app;
mod download;

use app::{dark_visuals, RustyDownloadsApp};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 760.0])
            .with_title("rustydownloads"),
        ..Default::default()
    };

    eframe::run_native(
        "rustydownloads",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(dark_visuals());
            Ok(Box::new(RustyDownloadsApp::default()))
        }),
    )
}
