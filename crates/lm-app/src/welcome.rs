//! Välkomstvyn när inget projekt är öppet: kom igång, exempelprojekt med
//! bilder och de senast öppnade projekten.

use crate::app::{LumaApp, Pending};
use crate::canvas::ACCENT;
use crate::library::DIM;
use eframe::egui::{self, Color32, RichText, Sense, Ui};
use lm_core::i18n::t;
use std::path::{Path, PathBuf};

/// Ett exempelprojekt i välkomstvyn.
pub struct Example {
    pub path: PathBuf,
    pub name: &'static str,
    pub thumb: Option<egui::TextureHandle>,
}

/// Exemplen och deras namn, i den ordning de visas.
fn example_names() -> [(&'static str, &'static str); 5] {
    [
        ("showcase", t("Översikt", "Showcase")),
        ("facade", t("Fasad", "Façade")),
        ("halloween", "Halloween"),
        ("christmas", t("Jul", "Christmas")),
        ("gallery", t("Galleri", "Gallery")),
    ]
}

/// Var exemplen ligger: bredvid programmet (AppImage, Flatpak, paket) eller i källkoden.
pub fn examples_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join("../share/lumamap/examples"));
        // target/release/lumamap i källkodsträdet.
        candidates.push(dir.join("../../examples"));
    }
    candidates.push(PathBuf::from("/app/share/lumamap/examples"));
    candidates.push(PathBuf::from("/usr/share/lumamap/examples"));
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples"));
    candidates.into_iter().find(|d| d.join("showcase.lmap").is_file()).map(|d| d.canonicalize().unwrap_or(d))
}

fn load_examples(ctx: &egui::Context) -> Vec<Example> {
    let Some(dir) = examples_dir() else { return Vec::new() };
    example_names()
        .into_iter()
        .filter_map(|(stem, name)| {
            let path = dir.join(format!("{stem}.lmap"));
            path.is_file().then(|| {
                let thumb = image::open(dir.join("thumbs").join(format!("{stem}.jpg"))).ok().map(|img| {
                    let img = img.to_rgba8();
                    let size = [img.width() as usize, img.height() as usize];
                    let colour = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    ctx.load_texture(format!("example-{stem}"), colour, egui::TextureOptions::LINEAR)
                });
                Example { path, name, thumb }
            })
        })
        .collect()
}

const CARD: egui::Vec2 = egui::vec2(192.0, 108.0);

impl LumaApp {
    /// Välkomstvyn visas tills något läggs till eller öppnas.
    pub fn show_welcome(&self) -> bool {
        !self.welcome_closed && self.path.is_none() && self.project.sources.is_empty() && self.project.surfaces.is_empty()
    }

    /// Öppnar ett exempel som ett namnlöst projekt, så att Spara frågar efter
    /// en ny plats i stället för att skriva över exemplet.
    pub fn open_example(&mut self, path: &Path) {
        self.load(path);
        if self.path.as_deref() == Some(path) {
            self.path = None;
        }
    }

    pub fn welcome(&mut self, ui: &mut Ui) {
        if self.examples.is_none() {
            self.examples = Some(load_examples(ui.ctx()));
        }
        let mut open: Option<(PathBuf, bool)> = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(36.0);
                ui.label(RichText::new("LumaMap").size(42.0).strong().color(ACCENT));
                ui.label(RichText::new(t("Projektionsmappning – enkelt.", "Projection mapping made simple.")).size(17.0).color(DIM));
                ui.add_space(26.0);

                let big = |text: &str| egui::Button::new(RichText::new(text).size(16.0)).min_size(egui::vec2(320.0, 40.0));
                if ui.add(big(t("➕  Lägg till video eller bild…", "➕  Add video or image…"))).clicked() {
                    self.pick_media_files();
                }
                ui.add_space(4.0);
                if ui.add(big(t("📂  Öppna projekt…", "📂  Open project…"))).clicked() {
                    self.request(Pending::Open(None));
                }
                ui.add_space(4.0);
                if ui.add(big(t("⬜  Börja med en tom yta", "⬜  Start with an empty surface"))).clicked() {
                    self.welcome_closed = true;
                    self.add_surface(None);
                }
                ui.add_space(8.0);
                ui.label(RichText::new(t("…eller dra in en videofil i fönstret.", "…or drag a video file into the window.")).color(DIM));

                let examples = self.examples.as_deref().unwrap_or_default();
                if !examples.is_empty() {
                    ui.add_space(34.0);
                    ui.label(RichText::new(t("Prova ett exempel", "Try an example")).size(18.0).strong());
                    ui.add_space(10.0);
                    let gap = 14.0;
                    let per_row = (((ui.available_width() + gap) / (CARD.x + gap)) as usize).clamp(1, examples.len());
                    for chunk in examples.chunks(per_row) {
                        let width = chunk.len() as f32 * (CARD.x + gap) - gap;
                        ui.allocate_ui(egui::vec2(width, CARD.y + 30.0), |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for ex in chunk {
                                    if example_card(ui, ex) {
                                        open = Some((ex.path.clone(), true));
                                    }
                                }
                            });
                        });
                        ui.add_space(gap);
                    }
                }

                let recent: Vec<PathBuf> = self.recent.iter().filter(|p| p.is_file()).take(6).cloned().collect();
                if !recent.is_empty() {
                    ui.add_space(24.0);
                    ui.label(RichText::new(t("Senaste projekt", "Recent projects")).size(18.0).strong());
                    ui.add_space(6.0);
                    for p in recent {
                        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
                        let r = ui
                            .add(egui::Button::new(RichText::new(format!("📄 {name}")).size(15.0)).frame(false))
                            .on_hover_text(&dir);
                        if r.clicked() {
                            open = Some((p.clone(), false));
                        }
                        ui.label(RichText::new(dir).small().color(Color32::from_gray(110)));
                        ui.add_space(4.0);
                    }
                }
                ui.add_space(30.0);
            });
        });
        match open {
            Some((p, true)) => self.request(Pending::OpenExample(p)),
            Some((p, false)) => self.request(Pending::Open(Some(p))),
            None => {}
        }
    }
}

/// Ett exempelkort: bild och namn. Returnerar true när det klickas.
fn example_card(ui: &mut Ui, ex: &Example) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(CARD.x, CARD.y + 28.0), Sense::click());
    let img = egui::Rect::from_min_size(rect.min, CARD);
    let painter = ui.painter_at(rect.expand(3.0));
    painter.rect_filled(img, 6.0, Color32::from_gray(30));
    if let Some(tex) = &ex.thumb {
        egui::Image::new((tex.id(), CARD)).corner_radius(6.0).paint_at(ui, img);
    }
    let hover = resp.hovered();
    if hover {
        painter.rect_stroke(img, 6.0, egui::Stroke::new(2.0, ACCENT), egui::StrokeKind::Outside);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    painter.text(
        egui::pos2(rect.center().x, img.max.y + 14.0),
        egui::Align2::CENTER_CENTER,
        ex.name,
        egui::FontId::proportional(15.0),
        if hover { ACCENT } else { Color32::from_gray(210) },
    );
    resp.on_hover_text(ex.path.display().to_string()).clicked()
}
