//! LuxCanvas – videomappning för Linux.
//!
//! Användning:
//!   luxcanvas                 starta tomt
//!   luxcanvas show.lmap       öppna projekt
//!   luxcanvas --play show.lmap  starta direkt i Visa-läge med projektorn i helskärm

mod app;
mod calib;
mod canvas;
mod leds;
mod library;
mod live;
mod motion_ui;
mod panels;
mod pool;
mod remote_ui;
mod show;
mod welcome;

use app::{LumaApp, Startup};
use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    app::migrate_old_dirs();
    app::init_language();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let mut startup = Startup { file: None, play: false };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--play" | "-p" => startup.play = true,
            "--version" | "-V" => {
                println!("LuxCanvas {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{}", lc_core::i18n::t("Användning: luxcanvas [--play] [--version] [projekt.lmap]", "Usage: luxcanvas [--play] [--version] [project.lmap]"));
                return Ok(());
            }
            _ => startup.file = Some(PathBuf::from(arg)),
        }
    }

    if let Err(e) = lc_media::init() {
        eprintln!("{e}");
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LuxCanvas")
            // Samma id som .desktop-filen, så att skrivbordet visar rätt ikon.
            .with_app_id("io.github.dio99.LuxCanvas")
            .with_icon(std::sync::Arc::new(icon()))
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "LuxCanvas",
        options,
        Box::new(|cc| Ok(Box::new(LumaApp::new(cc, startup).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?))),
    )
}

/// Fönsterikonen (samma som i packaging/linux).
fn icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../../../packaging/linux/io.github.dio99.LuxCanvas.png"))
        .map(|i| i.to_rgba8())
        .expect("ikonen ska gå att läsa");
    let (width, height) = img.dimensions();
    egui::IconData { rgba: img.into_raw(), width, height }
}
