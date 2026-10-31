#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod dev;
mod engine;
mod media;
mod net;
mod platform;
mod process;
mod session;
mod streamable;
mod tools;
mod update;

use eframe::egui;

fn main() -> eframe::Result {
    if std::env::args().skip(1).any(|arg| arg == "--check-tools") {
        if let Err(error) = tools::check() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        println!("All bundled download tools are ready.");
        return Ok(());
    }
    dev::errors::record_panics();
    let dev_mode = std::env::args().skip(1).any(|arg| arg == "--dev-mode");
    std::thread::spawn(update::remove_stale_installers);
    eframe::run_native(
        "stream-dlp",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1040., 840.])
                .with_min_inner_size([880., 700.])
                .with_icon(app_icon()),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, dev_mode)))),
    )
}

fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/app.png"))
        .expect("the bundled app icon is valid PNG")
}
