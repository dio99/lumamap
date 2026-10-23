//! LED-slingor: läggs ut som linjer över projektorbilden, får färgerna där
//! de ligger och skickas med Art-Net. Ändarna dras i förhandsvisningen när
//! ljusfönstret är öppet.

use crate::app::LumaApp;
use crate::canvas::View;
use eframe::egui::{self, Color32, Pos2, RichText, Stroke};
use lc_core::i18n::t;
use lc_core::{ColorOrder, Command, LedId, OutputId, Pt, LED_PIXELS_MAX};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const LED_COLOR: Color32 = Color32::from_rgb(255, 150, 40);
const HIT: f32 = 14.0;

/// Pågående drag i en slingas ände.
pub struct LedDrag {
    id: LedId,
    end: bool,
    start_pt: Pt,
    start: Pos2,
    gesture: u64,
}

impl LumaApp {
    /// Läser slingornas färger ur utgångarna (ca 30 gånger per sekund).
    pub fn tick_leds(&mut self, device: &eframe::wgpu::Device, queue: &eframe::wgpu::Queue) {
        if self.project.led_strips.is_empty() {
            if !self.led_colors.is_empty() {
                for o in &self.project.outputs {
                    self.renderer.sample_leds(device, queue, o.id, &[]);
                }
                self.led_colors.clear();
            }
            return;
        }
        if self.led_sampled.elapsed() < Duration::from_millis(30) {
            return;
        }
        self.led_sampled = Instant::now();
        let mut by_output: HashMap<OutputId, Vec<(LedId, usize)>> = HashMap::new();
        let mut points: HashMap<OutputId, Vec<Pt>> = HashMap::new();
        for s in &self.project.led_strips {
            let pos = s.positions();
            by_output.entry(s.output).or_default().push((s.id, pos.len()));
            points.entry(s.output).or_default().extend(pos);
        }
        for (out, strips) in by_output {
            // Förra läsningen först (om den är klar), sedan en ny.
            if let Some(colors) = self.renderer.led_colors(device, out) {
                let mut at = 0;
                for (id, n) in &strips {
                    if at + n <= colors.len() {
                        self.led_colors.insert(*id, colors[at..at + n].to_vec());
                    }
                    at += n;
                }
            }
            self.renderer.sample_leds(device, queue, out, &points[&out]);
        }
    }

    /// LED-slingorna i ljusfönstret.
    pub fn led_section(&mut self, ui: &mut egui::Ui, gesture: &mut Option<u64>) {
        ui.add_space(10.0);
        ui.label(RichText::new(t("LED-slingor", "LED strips")).strong());
        let mut strips = self.project.led_strips.clone();
        if strips.is_empty() {
            ui.label(RichText::new(t(
                "En LED-slinga får färgerna från projektorbilden där den ligger – dra ändarna i förhandsvisningen.",
                "An LED strip takes its colours from the projector image where it lies – drag its ends in the preview.",
            )).color(Color32::from_gray(150)));
        }
        let mut remove = None;
        egui::Grid::new("led_strips").num_columns(6).spacing([8.0, 6.0]).striped(true).show(ui, |ui| {
            if !strips.is_empty() {
                for h in [t("Namn", "Name"), t("Pixlar", "Pixels"), t("Universum", "Universe"), t("Kanal", "Channel"), t("Ordning", "Order"), ""] {
                    ui.label(RichText::new(h).color(Color32::from_gray(150)));
                }
                ui.end_row();
            }
            for (i, s) in strips.iter_mut().enumerate() {
                let r = ui.add(egui::TextEdit::singleline(&mut s.name).desired_width(90.0));
                if r.changed() {
                    *gesture = Some(self.field_gesture(&r));
                }
                ui.add(egui::DragValue::new(&mut s.pixels).range(1..=LED_PIXELS_MAX));
                ui.add(egui::DragValue::new(&mut s.universe).range(0..=32767))
                    .on_hover_text(format!("{} {}–{}", t("Universum", "Universe"), s.universe, s.last_universe()));
                ui.add(egui::DragValue::new(&mut s.address).range(1..=510));
                egui::ComboBox::from_id_salt(("led_order", s.id)).selected_text(s.order.label()).width(60.0).show_ui(ui, |ui| {
                    for o in ColorOrder::ALL {
                        ui.selectable_value(&mut s.order, o, o.label());
                    }
                });
                if ui.small_button("🗑").on_hover_text(t("Ta bort slingan", "Remove the strip")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
                // Slingans färger just nu.
                if let Some(colors) = self.led_colors.get(&s.id) {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(90.0, 10.0), egui::Sense::hover());
                    let w = rect.width() / colors.len().max(1) as f32;
                    for (k, c) in colors.iter().enumerate() {
                        let x = rect.min.x + k as f32 * w;
                        let r = egui::Rect::from_min_max(egui::pos2(x, rect.min.y), egui::pos2(x + w.max(1.0), rect.max.y));
                        ui.painter().rect_filled(r, 0.0, Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
                    }
                    ui.end_row();
                }
            }
        });
        if let Some(i) = remove {
            strips.remove(i);
        }
        if ui
            .button(t("➕ LED-slinga", "➕ LED strip"))
            .on_hover_text(t("Pixel-slinga (t.ex. WS2812 via en Art-Net-nod), 3 kanaler per pixel", "Pixel strip (e.g. WS2812 via an Art-Net node), 3 channels per pixel"))
            .clicked()
        {
            let strip = self.project.make_led_strip(self.current_output);
            strips.push(strip);
        }
        if strips != self.project.led_strips {
            self.exec(Command::SetLedStrips(strips), *gesture);
        }
    }

    /// Slingans ände under `pos` i förhandsvisningen.
    fn led_hit(&self, v: &View, out: OutputId, pos: Pos2) -> Option<(LedId, bool)> {
        self.project
            .led_strips
            .iter()
            .filter(|s| s.output == out)
            .flat_map(|s| [(s.id, false, s.start), (s.id, true, s.end)])
            .map(|(id, end, p)| (id, end, v.pt(p).distance(pos)))
            .filter(|(_, _, d)| *d <= HIT)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(id, end, _)| (id, end))
    }

    /// Drag i slingornas ändar. Returnerar true om musen tog en ände.
    pub fn led_drag(&mut self, resp: &egui::Response, v: &View, out: OutputId, origin: Option<Pos2>, pointer: Option<Pos2>) -> bool {
        if !self.dmx_window_open {
            return false;
        }
        if resp.drag_started() {
            if let Some((origin, (id, end))) = origin.and_then(|o| self.led_hit(v, out, o).map(|h| (o, h))) {
                let s = self.project.led_strips.iter().find(|s| s.id == id).expect("träffad");
                self.led_drag = Some(LedDrag {
                    id,
                    end,
                    start_pt: if end { s.end } else { s.start },
                    start: origin,
                    gesture: self.new_gesture(),
                });
            }
        }
        let Some(d) = &self.led_drag else { return false };
        if let (true, Some(pos)) = (resp.dragged(), pointer) {
            let (now, then) = (v.norm(pos), v.norm(d.start));
            let p = [d.start_pt[0] + now[0] - then[0], d.start_pt[1] + now[1] - then[1]];
            let (id, end, g) = (d.id, d.end, d.gesture);
            let mut strips = self.project.led_strips.clone();
            if let Some(s) = strips.iter_mut().find(|s| s.id == id) {
                if end {
                    s.end = p;
                } else {
                    s.start = p;
                }
            }
            self.exec(Command::SetLedStrips(strips), Some(g));
        }
        if resp.drag_stopped() {
            self.led_drag = None;
        }
        true
    }

    /// Ritar slingorna med sina färger (bara när ljusfönstret är öppet).
    pub fn paint_leds(&self, painter: &egui::Painter, v: &View, out: OutputId) {
        if !self.dmx_window_open {
            return;
        }
        for s in self.project.led_strips.iter().filter(|s| s.output == out) {
            let (a, b) = (v.pt(s.start), v.pt(s.end));
            painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.0, LED_COLOR), 4.0, 3.0));
            let colors = self.led_colors.get(&s.id);
            let pos = s.positions();
            // Högst ~80 prickar så att långa slingor inte blir en klump.
            let step = (pos.len() / 80).max(1);
            for (i, p) in pos.iter().enumerate().step_by(step) {
                let c = colors.and_then(|c| c.get(i)).map_or(LED_COLOR, |c| Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
                painter.circle(v.pt(*p), 3.5, c, Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 110)));
            }
            for p in [a, b] {
                painter.circle(p, 6.0, Color32::WHITE, Stroke::new(2.0, LED_COLOR));
            }
            painter.text(a + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, &s.name, egui::FontId::proportional(13.0), LED_COLOR);
        }
    }
}
