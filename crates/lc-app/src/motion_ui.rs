//! Rörelse framför en kamera: driver effekter som "följer rörelse" och kan
//! starta en cue när någon går förbi (t.ex. en skrämsel på halloween).

use crate::app::LumaApp;
use eframe::egui::{self, Color32, RichText};
use lc_core::i18n::t;
use lc_core::{Command, SourceKind};
use std::time::{Duration, Instant};

impl LumaApp {
    /// Räknar fram rörelsen och startar cuen när den blir tydlig. Varje bildruta.
    pub fn tick_motion(&mut self) {
        let m = self.project.settings.motion;
        let level = m.source.and_then(|id| self.media.motion(id)).unwrap_or(0.0);
        let strength = m.strength(level);
        self.opts.motion = strength;
        // Starta på den stigande flanken, och inte oftare än vänttiden.
        let rising = strength >= 1.0 && !self.motion_high;
        self.motion_high = strength >= 0.6 && (self.motion_high || strength >= 1.0);
        let rested = self.motion_fired.is_none_or(|t| t.elapsed() >= Duration::from_secs_f32(m.cooldown.max(0.0)));
        if let (true, true, Some(cue)) = (rising, rested, m.cue) {
            if self.project.cue(cue).is_some() && self.schedule_on != Some(false) {
                self.motion_fired = Some(Instant::now());
                self.go_cue(cue);
                self.notify(t("Rörelse – cuen startar", "Motion – the cue starts"));
            }
        }
    }

    pub fn motion_window(&mut self, ctx: &egui::Context) {
        if !self.motion_window_open {
            return;
        }
        let mut open = true;
        let mut settings = self.project.settings.clone();
        let mut add_camera = false;
        egui::Window::new(t("🏃 Rörelse", "🏃 Motion")).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new(t(
                "En kamera som ser publiken: rörelse kan förstärka effekter och starta en cue, t.ex. en skrämsel när någon går förbi.",
                "A camera that sees the audience: motion can boost effects and start a cue, e.g. a scare when someone walks past.",
            )).color(Color32::from_gray(150)));
            ui.add_space(6.0);
            let m = &mut settings.motion;
            let cameras: Vec<(lc_core::SourceId, String)> = self
                .project
                .sources
                .iter()
                .filter(|s| matches!(s.kind, SourceKind::Camera { .. } | SourceKind::Stream { .. }))
                .map(|s| (s.id, s.name.clone()))
                .collect();
            egui::Grid::new("motion").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(t("Kamera", "Camera"));
                ui.horizontal(|ui| {
                    let name = m.source.and_then(|id| cameras.iter().find(|c| c.0 == id)).map_or(t("— av —", "— off —").to_string(), |c| c.1.clone());
                    egui::ComboBox::from_id_salt("motion_source").selected_text(name).width(170.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut m.source, None, t("— av —", "— off —"));
                        for (id, name) in &cameras {
                            ui.selectable_value(&mut m.source, Some(*id), format!("📷 {name}"));
                        }
                    });
                    if cameras.is_empty() {
                        add_camera = ui.button(t("➕ Kamera", "➕ Camera")).on_hover_text(t("Lägg till en kamera under ➕ Lägg till i medielistan", "Add a camera under ➕ Add in the media list")).clicked();
                    }
                });
                ui.end_row();

                ui.label(t("Rörelse nu", "Motion now"));
                let strength = self.opts.motion;
                let colour = if strength >= 1.0 { Color32::from_rgb(255, 120, 80) } else { Color32::from_rgb(120, 220, 120) };
                ui.add(egui::ProgressBar::new(strength).desired_width(200.0).fill(colour));
                ui.end_row();

                ui.label(t("Känslighet", "Sensitivity"));
                ui.add(egui::Slider::new(&mut m.sensitivity, 0.0..=1.0).show_value(false))
                    .on_hover_text(t("Höger = mindre rörelse räcker. Stapeln ska nå slutet när någon går förbi.", "Right = less motion is enough. The bar should reach the end when someone walks past."));
                ui.end_row();

                ui.label(t("Starta cue", "Start cue"));
                let name = m.cue.and_then(|id| self.project.cue(id)).map_or(t("— ingen —", "— none —").to_string(), |c| c.name.clone());
                egui::ComboBox::from_id_salt("motion_cue").selected_text(name).width(170.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut m.cue, None, t("— ingen —", "— none —"));
                    for c in &self.project.cues {
                        ui.selectable_value(&mut m.cue, Some(c.id), &c.name);
                    }
                });
                ui.end_row();

                ui.label(t("Vänta minst", "Wait at least"));
                ui.add(egui::DragValue::new(&mut m.cooldown).range(0.0..=600.0).speed(0.2).suffix(" s"))
                    .on_hover_text(t("Tid mellan två starter", "Time between two starts"));
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.label(RichText::new(t(
                "Tips: ge cuen ⏱ Nästa efter, så går showen tillbaka av sig själv. Bocka i 🏃 Följ rörelse på en effekt för att låta rörelsen styra den.",
                "Tip: give the cue ⏱ Next after so the show goes back by itself. Tick 🏃 Follow motion on an effect to let motion drive it.",
            )).size(12.0).color(Color32::from_gray(150)));
        });
        if add_camera {
            self.notify(t("Klicka ➕ Lägg till → 📷 Kamera i medielistan", "Click ➕ Add → 📷 Camera in the media list"));
        }
        self.motion_window_open = open;
        if settings != self.project.settings {
            self.exec(Command::ReplaceSettings(settings), None);
        }
    }
}
