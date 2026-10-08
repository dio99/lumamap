//! LumaMap – videomappning för Linux.
//!
//! Användning:
//!   lumamap                 starta tomt
//!   lumamap show.lmap       öppna projekt
//!   lumamap --play show.lmap  starta direkt i Visa-läge med projektorn i helskärm

mod app;
mod canvas;
mod panels;
mod pool;
mod show;

use app::{LumaApp, Startup};
use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    app::init_language();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let mut startup = Startup { file: None, play: false };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--play" | "-p" => startup.play = true,
            "--help" | "-h" => {
                println!("{}", lm_core::i18n::t("Användning: lumamap [--play] [projekt.lmap]", "Usage: lumamap [--play] [project.lmap]"));
                return Ok(());
            }
            _ => startup.file = Some(PathBuf::from(arg)),
        }
    }

    if let Err(e) = lm_media::init() {
        eprintln!("{e}");
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LumaMap")
            .with_app_id("lumamap")
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "LumaMap",
        options,
        Box::new(|cc| Ok(Box::new(LumaApp::new(cc, startup).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?))),
    )
}
