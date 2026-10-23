//! Live-fönstret: alla klipp som ett rutnät med miniatyrer. Ett klick byter
//! media på målytan – direkt eller på nästa taktslag – som i ett VJ-program.

use crate::app::LumaApp;
use crate::canvas::ACCENT;
use crate::library::{paint_thumb, source_icon, DIM};
use eframe::egui::{self, Color32, Key, RichText, Sense};
use lc_core::i18n::t;
use lc_core::{Command, SourceId, SurfaceId};

/// Ett klippbyte som väntar på nästa taktslag.
#[derive(Clone, Copy)]
pub struct PendingClip {
    pub surface: SurfaceId,
    pub source: SourceId,
    pub at_beat: f64,
}

const CARD: egui::Vec2 = egui::vec2(132.0, 74.0);
const KEYS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];

impl LumaApp {
    /// Ytan som klippen hamnar på: vald i fönstret, annars markerad yta.
    fn live_target(&self) -> Option<SurfaceId> {
        self.live_target.filter(|id| self.project.surface(*id).is_some()).or(self.selected)
    }

    /// Byter klipp på ytan nu, eller på nästa slag om det är valt.
    pub fn trigger_clip(&mut self, surface: SurfaceId, source: SourceId) {
        if self.live_quantize {
            self.clip_pending = Some(PendingClip { surface, source, at_beat: self.opts.beats.floor() + 1.0 });
        } else {
            self.apply_clip(surface, source);
        }
    }

    fn apply_clip(&mut self, surface: SurfaceId, source: SourceId) {
        let Some(mut s) = self.project.surface(surface).cloned() else { return };
        if let Some(old) = s.source.filter(|o| *o != source) {
            self.start_media_fade(surface, old, self.live_fade);
        }
        s.source = Some(source);
        s.visible = true;
        self.exec(Command::ReplaceSurface(s), None);
        // Ett klipp börjar från början, som i ett VJ-program.
        if let Some(m) = self.media.get_mut(source) {
            if m.is_video() {
                m.seek(0.0);
                m.play();
            }
        }
    }

    /// Utför ett väntande klippbyte när taktslaget kommer. Anropas varje bildruta.
    pub fn tick_clips(&mut self) {
        if let Some(p) = self.clip_pending {
            if self.opts.beats >= p.at_beat {
                self.clip_pending = None;
                self.apply_clip(p.surface, p.source);
            }
        }
    }

    pub fn live_window(&mut self, ctx: &egui::Context) {
        if !self.live_window_open {
            return;
        }
        let mut open = true;
        let mut trigger: Option<SourceId> = None;
        let target = self.live_target();
        egui::Window::new(t("⚡ Live", "⚡ Live")).open(&mut open).default_width(600.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(t("Till yta", "To surface"));
                let name = target.and_then(|id| self.project.surface(id)).map_or(t("— markera en yta —", "— select a surface —").to_string(), |s| s.name.clone());
                let mut pick = self.live_target;
                egui::ComboBox::from_id_salt("live_target").selected_text(name).width(150.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut pick, None, t("Markerad yta", "Selected surface"));
                    for s in self.project.surfaces.iter().filter(|s| s.output == self.current_output) {
                        ui.selectable_value(&mut pick, Some(s.id), &s.name);
                    }
                });
                self.live_target = pick;
                ui.checkbox(&mut self.live_quantize, t("⏱ På nästa slag", "⏱ On the next beat"))
                    .on_hover_text(t("Bytet väntar till nästa taktslag så att det landar i takt", "The change waits for the next beat so it lands in time"));
                ui.label(t("Övertoning", "Crossfade"));
                ui.add(egui::DragValue::new(&mut self.live_fade).range(0.0..=10.0).speed(0.05).suffix(" s"))
                    .on_hover_text(t("0 = hårt klipp", "0 = hard cut"));
            });
            ui.label(RichText::new(t("Klicka på ett klipp, eller tryck 1–9.", "Click a clip, or press 1–9.")).size(12.0).color(DIM));
            ui.add_space(6.0);
            let current = target.and_then(|id| self.project.surface(id)).and_then(|s| s.source);
            let pending = self.clip_pending.filter(|p| Some(p.surface) == target).map(|p| p.source);
            // Blinkar i takt medan bytet väntar.
            let blink = (self.opts.beats.fract() < 0.5) as u8 as f32;
            let sources: Vec<_> = self.project.sources.iter().map(|s| (s.id, s.name.clone(), source_icon(&s.kind))).collect();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for (i, (id, name, icon)) in sources.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(CARD.x, CARD.y + 20.0), Sense::click());
                    let img = egui::Rect::from_min_size(rect.min, CARD);
                    let painter = ui.painter_at(rect.expand(3.0));
                    paint_thumb(&painter, img, self.renderer.source_texture(*id), icon);
                    let stroke = if Some(*id) == pending {
                        egui::Stroke::new(3.0, Color32::from_rgb(255, 200, 80).gamma_multiply(0.4 + 0.6 * blink))
                    } else if Some(*id) == current {
                        egui::Stroke::new(3.0, ACCENT)
                    } else if resp.hovered() {
                        egui::Stroke::new(1.0, Color32::from_gray(200))
                    } else {
                        egui::Stroke::NONE
                    };
                    painter.rect_stroke(img, 3.0, stroke, egui::StrokeKind::Outside);
                    if i < KEYS.len() {
                        let badge = egui::Rect::from_min_size(img.min + egui::vec2(4.0, 4.0), egui::vec2(18.0, 18.0));
                        painter.rect_filled(badge, 4.0, Color32::from_black_alpha(170));
                        painter.text(badge.center(), egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(12.0), Color32::WHITE);
                    }
                    let color = if Some(*id) == current { ACCENT } else { Color32::from_gray(210) };
                    let galley = egui::WidgetText::from(RichText::new(name).size(13.0)).into_galley(ui, Some(egui::TextWrapMode::Truncate), CARD.x, egui::TextStyle::Body);
                    painter.galley(egui::pos2(img.min.x, img.max.y + 3.0), galley, color);
                    if resp.clicked() {
                        trigger = Some(*id);
                    }
                }
            });
            if sources.is_empty() {
                ui.label(RichText::new(t("Lägg till media först.", "Add some media first.")).color(DIM));
            }
        });
        if !ctx.egui_wants_keyboard_input() {
            for (i, key) in KEYS.iter().enumerate() {
                if ctx.input(|inp| inp.key_pressed(*key)) {
                    trigger = self.project.sources.get(i).map(|s| s.id).or(trigger);
                }
            }
        }
        match (trigger, target) {
            (Some(src), Some(surface)) => self.trigger_clip(surface, src),
            (Some(_), None) => self.notify(t("Markera en yta först", "Select a surface first")),
            _ => {}
        }
        self.live_window_open = open;
    }
}
