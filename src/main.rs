mod app;
mod download;

use app::RustyDownloadsApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 620.0])
            .with_title("rustydownloads"),
        ..Default::default()
    };

    eframe::run_native(
        "rustydownloads",
        options,
        Box::new(|_cc| Ok(Box::new(RustyDownloadsApp::default()))),
    )
}
