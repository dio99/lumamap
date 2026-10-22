//! Fönstren för mobil-fjärrkontrollen (med QR-kod) och schemat.

use crate::app::LumaApp;
use eframe::egui::{self, Color32, RichText};
use lm_core::i18n::t;
use lm_core::{Command, Schedule};

/// Nyckeln i fjärrkontrollens adress. Sparas per dator så att ett bokmärke i
/// mobilen fortsätter fungera.
pub fn load_token() -> String {
    let path = crate::app::config_dir().join("remote-token");
    if let Some(t) = std::fs::read_to_string(&path).ok().map(|s| s.trim().to_string()).filter(|s| s.len() >= 8) {
        return t;
    }
    let token = lm_control::remote::new_token();
    save_token(&token);
    token
}

fn save_token(token: &str) {
    let dir = crate::app::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = std::fs::write(dir.join("remote-token"), token) {
        log::warn!("{e}");
    }
}

/// Ritar en QR-kod för `text` i en ruta av storleken `size`.
fn qr(ui: &mut egui::Ui, text: &str, size: f32) {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else { return };
    let n = code.width();
    let quiet = 3;
    let cells = (n + 2 * quiet) as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, Color32::WHITE);
    let cell = size / cells;
    for (i, c) in code.to_colors().iter().enumerate() {
        if *c == qrcode::Color::Dark {
            let (x, y) = ((i % n + quiet) as f32, (i / n + quiet) as f32);
            let r = egui::Rect::from_min_size(rect.min + egui::vec2(x * cell, y * cell), egui::vec2(cell + 0.5, cell + 0.5));
            painter.rect_filled(r, 0.0, Color32::BLACK);
        }
    }
}

impl LumaApp {
    pub fn remote_url(&self) -> Option<String> {
        let server = self.remote.as_ref()?;
        let host = lm_control::remote::local_ip().map_or("localhost".to_string(), |ip| ip.to_string());
        Some(format!("http://{host}:{}/?k={}", server.port(), self.remote_token))
    }

    pub fn remote_window(&mut self, ctx: &egui::Context) {
        let mut open = self.remote_window_open;
        let mut settings = self.project.settings.clone();
        let mut new_code = false;
        egui::Window::new(t("📱 Fjärrkontroll i mobilen", "📱 Phone remote")).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(&mut settings.remote_enabled, t("Styr showen från mobilen", "Control the show from a phone"));
            ui.label(RichText::new(t(
                "GO, Svart, ljusstyrka och cue-listan på en webbsida. Ingen app behövs.",
                "GO, Black, brightness and the cue list on a web page. No app needed.",
            )).color(Color32::from_gray(150)));
            ui.add_space(6.0);
            match (&self.remote_error, self.remote_url()) {
                (Some(e), _) if settings.remote_enabled => {
                    ui.colored_label(Color32::from_rgb(255, 120, 100), e);
                }
                (_, Some(url)) => {
                    ui.vertical_centered(|ui| {
                        qr(ui, &url, 220.0);
                        ui.add_space(6.0);
                        ui.label(RichText::new(t("Skanna med mobilens kamera", "Scan with the phone's camera")).strong());
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut url.as_str()).desired_width(260.0));
                        if ui.small_button("📋").on_hover_text(t("Kopiera adressen", "Copy the address")).clicked() {
                            ui.ctx().copy_text(url.clone());
                        }
                    });
                    ui.label(RichText::new(t(
                        "Mobilen måste vara på samma wifi som datorn.",
                        "The phone must be on the same Wi-Fi as the computer.",
                    )).color(Color32::from_gray(150)));
                    if let Some(at) = self.remote_last {
                        ui.label(RichText::new(format!("{} {:.0} s {}", t("Senaste tryck för", "Last tap"), at.elapsed().as_secs_f32(), t("sedan", "ago"))).color(Color32::from_gray(150)));
                    }
                }
                _ => {}
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(t("Port", "Port"));
                ui.add(egui::DragValue::new(&mut settings.remote_port).range(1024..=65535));
                new_code = ui
                    .button(t("🔄 Ny kod", "🔄 New code"))
                    .on_hover_text(t("Byt nyckel – gamla länkar och QR-koder slutar fungera", "Change the key – old links and QR codes stop working"))
                    .clicked();
            });
        });
        self.remote_window_open = open;
        if new_code {
            self.remote_token = lm_control::remote::new_token();
            save_token(&self.remote_token);
        }
        if settings != self.project.settings {
            self.exec(Command::ReplaceSettings(settings), None);
        }
    }

    pub fn schedule_window(&mut self, ctx: &egui::Context) {
        let mut open = self.schedule_window_open;
        let mut settings = self.project.settings.clone();
        egui::Window::new(t("⏰ Schema", "⏰ Schedule")).open(&mut open).resizable(false).show(ctx, |ui| {
            let s = &mut settings.schedule;
            ui.checkbox(&mut s.enabled, t("Visa bara under vissa tider", "Only show at certain times"));
            ui.label(RichText::new(t(
                "För installationer: utanför tiderna är projektorerna svarta och videorna pausade. När det blir dags startar första cuen.",
                "For installations: outside these times the projectors are black and the videos paused. When it's time, the first cue starts.",
            )).color(Color32::from_gray(150)));
            ui.add_space(6.0);
            ui.add_enabled_ui(s.enabled, |ui| {
                egui::Grid::new("schedule").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(t("Tänd", "On"));
                    time_field(ui, &mut s.on);
                    ui.end_row();
                    ui.label(t("Släck", "Off"));
                    time_field(ui, &mut s.off);
                    ui.end_row();
                });
            });
            if s.enabled {
                let now = jiff::Zoned::now();
                let on = s.is_on(now.hour() as u16 * 60 + now.minute() as u16);
                let text = if on {
                    format!("{} {}", t("På nu – släcks", "On now – goes off at"), Schedule::format(s.off))
                } else {
                    format!("{} {}", t("Svart nu – tänds", "Black now – comes on at"), Schedule::format(s.on))
                };
                ui.add_space(4.0);
                ui.colored_label(if on { Color32::from_rgb(120, 220, 120) } else { Color32::from_rgb(255, 200, 80) }, text);
            }
            ui.add_space(4.0);
            ui.label(RichText::new(t(
                "Starta datorn med  lumamap --play show.lmap  så rullar allt av sig självt.",
                "Start the computer with  lumamap --play show.lmap  and everything runs by itself.",
            )).color(Color32::from_gray(150)));
        });
        self.schedule_window_open = open;
        if settings != self.project.settings {
            self.exec(Command::ReplaceSettings(settings), None);
        }
    }
}

/// Klockslag som timmar och minuter.
fn time_field(ui: &mut egui::Ui, minutes: &mut u16) {
    let (mut h, mut m) = (*minutes / 60, *minutes % 60);
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut h).range(0..=23).custom_formatter(|v, _| format!("{v:02}")));
        ui.label(":");
        ui.add(egui::DragValue::new(&mut m).range(0..=59).speed(0.5).custom_formatter(|v, _| format!("{v:02}")));
    });
    *minutes = h * 60 + m;
}
