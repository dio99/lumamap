//! Vänsterpanelen: media, ytor och cues. Raderna har miniatyrer och kan dras:
//! media till en yta (i listan eller i bilden), ytor upp och ner i lagerordningen.
//! Högerklick ger en meny med det man kan göra med raden.

use crate::app::LumaApp;
use crate::canvas::{fit, ACCENT};
use eframe::egui::{self, Color32, Id, Rect, RichText, Sense, TextureId, Ui};
use lm_core::i18n::t;
use lm_core::{Command, SourceId, SourceKind, SurfaceId};

/// Media som dras från listan.
#[derive(Clone, Copy)]
pub struct DragMedia(pub SourceId);

/// Yta som dras i ytlistan.
#[derive(Clone, Copy)]
pub struct DragSurface(pub SurfaceId);

const ROW_H: f32 = 30.0;
const THUMB: egui::Vec2 = egui::vec2(44.0, 25.0);
pub const DIM: Color32 = Color32::from_gray(140);

pub fn source_icon(kind: &SourceKind) -> &'static str {
    match kind {
        SourceKind::Video { .. } => "🎞",
        SourceKind::Image { .. } => "🖼",
        SourceKind::Color { .. } => "🎨",
        SourceKind::TestPattern => "⊞",
        SourceKind::Camera { .. } => "📷",
        SourceKind::Stream { .. } => "📡",
        SourceKind::Generator { .. } => "✨",
    }
}

/// Ritar en miniatyr av `tex` (eller ikonen) i `rect`.
pub fn paint_thumb(painter: &egui::Painter, rect: Rect, tex: Option<(TextureId, [u32; 2])>, icon: &str) {
    painter.rect_filled(rect, 3.0, Color32::from_gray(28));
    match tex {
        Some((tex, size)) => {
            let r = fit(rect, size[0] as f32 / size[1].max(1) as f32);
            painter.image(tex, r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(14.0), DIM);
        }
    }
}

/// En listrad: miniatyr + text, klickbar och dragbar. `pad` lämnar plats
/// till vänster (för ögat i ytlistan).
fn row(ui: &mut Ui, id: Id, selected: bool, pad: f32, thumb: Option<(TextureId, [u32; 2])>, icon: &str, text: RichText) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let v = ui.visuals();
    if selected {
        painter.rect_filled(rect, 4.0, v.selection.bg_fill);
    } else if ui.rect_contains_pointer(rect) {
        painter.rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let thumb_rect = Rect::from_min_size(egui::pos2(rect.min.x + pad + 2.0, rect.center().y - THUMB.y / 2.0), THUMB);
    paint_thumb(&painter, thumb_rect, thumb, icon);
    let color = if selected { v.selection.stroke.color } else { v.text_color() };
    let x = thumb_rect.max.x + 8.0;
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Truncate), rect.max.x - x - 26.0, egui::TextStyle::Body);
    painter.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    resp
}

/// Det som dras följer med muspekaren.
fn ghost(ctx: &egui::Context, thumb: Option<(TextureId, [u32; 2])>, icon: &str, name: &str) {
    let Some(p) = ctx.pointer_interact_pos() else { return };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("lm_drag_ghost")));
    let galley = painter.layout_no_wrap(name.to_string(), egui::FontId::proportional(14.0), Color32::WHITE);
    let rect = Rect::from_min_size(p + egui::vec2(14.0, 10.0), egui::vec2(THUMB.x + galley.size().x + 20.0, ROW_H));
    painter.rect_filled(rect, 5.0, Color32::from_rgba_unmultiplied(30, 40, 50, 230));
    painter.rect_stroke(rect, 5.0, egui::Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
    let thumb_rect = Rect::from_min_size(egui::pos2(rect.min.x + 4.0, rect.center().y - THUMB.y / 2.0), THUMB);
    paint_thumb(&painter, thumb_rect, thumb, icon);
    painter.galley(egui::pos2(thumb_rect.max.x + 8.0, rect.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
}

/// En liten knapp på raden, t.ex. ögat eller papperskorgen.
fn row_button(ui: &mut Ui, rect: Rect, at_x: f32, text: RichText, hint: &str) -> bool {
    let r = Rect::from_center_size(egui::pos2(at_x, rect.center().y), egui::vec2(22.0, 22.0));
    ui.place(r, egui::Button::new(text).frame(false)).on_hover_text(hint).clicked()
}

impl LumaApp {
    pub fn library(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        self.media_list(ui);
        ui.add_space(14.0);
        self.surface_list(ui);
        ui.add_space(14.0);
        self.cue_list(ui);
    }

    /// "➕ Lägg till": alla sorters källor på ett ställe.
    fn add_menu(&mut self, ui: &mut Ui) {
        ui.menu_button(t("➕ Lägg till ⏷", "➕ Add ⏷"), |ui| {
            if ui.button(t("🎞 Video eller bild…", "🎞 Video or image…")).clicked() {
                self.pick_media_files();
            }
            ui.menu_button(t("✨ Mönster", "✨ Pattern"), |ui| {
                for pattern in lm_core::Pattern::ALL {
                    if ui.button(format!("✨ {}", pattern.label())).clicked() {
                        let kind = SourceKind::Generator { pattern, colors: pattern.default_colors(), speed: 0.25 };
                        self.add_source_shown(pattern.label(), kind);
                    }
                }
            })
            .response
            .on_hover_text(t("Rörligt mönster som ritas direkt – ingen videofil behövs", "Moving pattern drawn live – no video file needed"));
            if ui.button(t("🎨 Färg", "🎨 Colour")).clicked() {
                self.add_source_shown(t("Färg", "Colour"), SourceKind::Color { rgba: [1.0, 1.0, 1.0, 1.0] });
            }
            if ui.button(t("⊞ Testbild", "⊞ Test pattern")).clicked() {
                self.add_source_shown(t("Testbild", "Test pattern"), SourceKind::TestPattern);
            }
            ui.menu_button(t("📷 Kamera", "📷 Camera"), |ui| {
                let cameras = self.cameras.get_or_insert_with(lm_media::list_cameras).clone();
                if cameras.is_empty() {
                    ui.label(t("Ingen kamera hittades", "No camera found"));
                }
                for c in &cameras {
                    // Samma namn på flera enheter (t.ex. vanlig kamera och IR) – visa enheten också.
                    let twin = cameras.iter().filter(|o| o.name == c.name).count() > 1;
                    let label = if twin { format!("📷 {} – {}", c.name, c.device) } else { format!("📷 {}", c.name) };
                    if ui.button(label).on_hover_text(&c.device).clicked() {
                        self.add_source_shown(c.name.clone(), SourceKind::Camera { device: c.device.clone() });
                    }
                }
                ui.separator();
                if ui.button(t("⟳ Sök igen", "⟳ Search again")).clicked() {
                    self.cameras = None;
                }
            });
            if ui
                .button(t("📡 Ström…", "📡 Stream…"))
                .on_hover_text(t("RTSP, SRT, UDP, HTTP eller NDI via GStreamer", "RTSP, SRT, UDP, HTTP or NDI via GStreamer"))
                .clicked()
            {
                self.stream_input = Some(self.stream_input.take().unwrap_or_default());
            }
        })
        .response
        .on_hover_text(t("Lägg till video, bild, mönster, färg, kamera eller ström", "Add video, image, pattern, colour, camera or stream"));
    }

    pub fn pick_media_files(&mut self) {
        if let Some(files) = rfd::FileDialog::new()
            .set_title(t("Lägg till media", "Add media"))
            .add_filter(t("Video och bild", "Video and images"), crate::panels::FILES)
            .pick_files()
        {
            self.add_files(files, None);
        }
    }

    fn media_list(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Media");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.add_menu(ui));
        });
        if self.project.sources.is_empty() {
            ui.label(RichText::new(t("Dra in videor eller bilder här, eller klicka ➕ Lägg till.", "Drag videos or images here, or click ➕ Add.")).color(DIM));
        }
        let (mut remove, mut assign, mut new_surface, mut rename) = (None, None, None, None);
        for src in self.project.sources.clone() {
            let icon = source_icon(&src.kind);
            let error = self.media.get(src.id).and_then(|m| m.error()).map(str::to_owned);
            let thumb = self.renderer.source_texture(src.id);
            let selected = self.selected_source == Some(src.id);
            let mut text = RichText::new(&src.name);
            if error.is_some() {
                text = text.color(Color32::from_rgb(255, 100, 100));
            }
            let resp = row(ui, Id::new(("media_row", src.id)), selected, 0.0, thumb, icon, text);
            if (ui.rect_contains_pointer(resp.rect) || selected)
                && row_button(ui, resp.rect, resp.rect.max.x - 12.0, RichText::new("🗑").color(DIM), t("Ta bort media", "Remove media"))
            {
                remove = Some(src.id);
            }
            let resp = match &error {
                Some(e) => resp.on_hover_text(e),
                None => resp.on_hover_text(t(
                    "Dra till en yta, eller dubbelklicka för att visa på markerad yta",
                    "Drag onto a surface, or double-click to show on the selected surface",
                )),
            };
            if resp.clicked() {
                self.selected_source = Some(src.id);
            }
            if resp.double_clicked() {
                assign = Some(src.id);
            }
            resp.dnd_set_drag_payload(DragMedia(src.id));
            if resp.dragged() {
                ghost(ui.ctx(), thumb, icon, &src.name);
            }
            resp.context_menu(|ui| {
                let mut name = src.name.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0));
                if r.changed() {
                    rename = Some((src.id, name, self.field_gesture(&r)));
                }
                ui.separator();
                if ui.add_enabled(self.selected.is_some(), egui::Button::new(t("Visa på markerad yta", "Show on the selected surface"))).clicked() {
                    assign = Some(src.id);
                }
                if ui.button(t("⬜ Ny yta med den här", "⬜ New surface with this")).clicked() {
                    new_surface = Some(src.id);
                }
                ui.separator();
                if ui.button(t("🗑 Ta bort", "🗑 Remove")).clicked() {
                    remove = Some(src.id);
                }
            });
        }
        if let Some((id, name, gesture)) = rename {
            if let Some(mut s) = self.project.source(id).cloned() {
                s.name = name;
                self.exec(Command::ReplaceSource(s), Some(gesture));
            }
        }
        if let Some(id) = remove {
            self.exec(Command::RemoveSource(id), None);
        }
        if let Some(sid) = new_surface {
            self.add_surface(Some(sid));
        }
        if let Some(sid) = assign {
            self.show_source_on(sid, self.selected);
        }
        self.stream_field(ui);
    }

    /// Visar källan på ytan, eller på en ny yta om ingen anges.
    pub fn show_source_on(&mut self, sid: SourceId, surface: Option<SurfaceId>) {
        self.selected_source = Some(sid);
        match surface.and_then(|id| self.project.surface(id).cloned()) {
            Some(mut s) => {
                s.source = Some(sid);
                let id = s.id;
                self.exec(Command::ReplaceSurface(s), None);
                self.select(Some(id));
            }
            None => {
                self.add_surface(Some(sid));
            }
        }
    }

    fn stream_field(&mut self, ui: &mut Ui) {
        let Some(mut uri) = self.stream_input.take() else { return };
        let mut keep = true;
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut uri).hint_text(t("rtsp://kamera.local/stream", "rtsp://camera.local/stream")).desired_width(150.0));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.small_button(t("Lägg till", "Add")).clicked() || enter) && !uri.trim().is_empty() {
                let uri = uri.trim().to_string();
                let name = uri.split("://").nth(1).unwrap_or(&uri).split('/').next().unwrap_or(t("Ström", "Stream")).to_string();
                self.add_source_shown(name, SourceKind::Stream { uri, muted: false });
                keep = false;
            }
            if ui.small_button("✖").clicked() {
                keep = false;
            }
        });
        if keep {
            self.stream_input = Some(uri);
        }
    }

    fn surface_list(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading(t("Ytor", "Surfaces"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(t("➕ Ny ⏷", "➕ New ⏷"), |ui| self.new_surface_buttons(ui))
                    .response
                    .on_hover_text(t("Lägg till en yta", "Add a surface"));
            });
        });
        // Bara ytorna på vald utgång. `on_output` är deras index i hela listan.
        let out = self.current_output;
        let on_output: Vec<usize> = (0..self.project.surfaces.len()).filter(|&i| self.project.surfaces[i].output == out).collect();
        if on_output.is_empty() {
            ui.label(RichText::new(t("Inga ytor här än. Klicka ➕ Ny.", "No surfaces here yet. Click ➕ New.")).color(DIM));
        } else {
            ui.label(RichText::new(t("Överst = fram · Shift = flera", "Top = front · Shift = several")).size(12.0).color(DIM));
        }
        let ctx = ui.ctx().clone();
        let mut action: Option<Command> = None;
        let mut select = None;
        let mut toggle = None;
        for &index in on_output.iter().rev() {
            let s = self.project.surfaces[index].clone();
            let lock = if s.locked { " 🔒" } else { "" };
            // Källan går inte att spela (fil saknas, ström tappad …): ytan är svart.
            let broken = s.source.and_then(|id| self.media.get(id)).and_then(|m| m.error()).map(str::to_owned);
            let mut text = RichText::new(format!("{}{}{lock}", if broken.is_some() { "⚠ " } else { "" }, s.name));
            if broken.is_some() {
                text = text.color(Color32::from_rgb(255, 120, 100));
            } else if !s.visible {
                text = text.color(Color32::from_gray(110));
            }
            let thumb = s.source.and_then(|id| self.renderer.source_texture(id));
            let icon = s.source.and_then(|id| self.project.source(id)).map_or("—", |src| source_icon(&src.kind));
            let marked = self.selected == Some(s.id) || self.group.contains(&s.id);
            let resp = row(ui, Id::new(("surface_row", s.id)), marked, 24.0, thumb, icon, text);
            let eye = if s.visible { RichText::new("👁").size(15.0).color(Color32::from_gray(235)) } else { RichText::new("👁").size(15.0).color(Color32::from_gray(55)) };
            if row_button(ui, resp.rect, resp.rect.min.x + 12.0, eye, if s.visible { t("Synlig – klicka för att dölja", "Visible – click to hide") } else { t("Dold – klicka för att visa", "Hidden – click to show") }) {
                let mut ns = s.clone();
                ns.visible = !s.visible;
                action = Some(Command::ReplaceSurface(ns));
            }
            let resp = match &broken {
                Some(e) => resp.on_hover_text(format!("{}: {e}", t("Media fungerar inte", "Media is not working"))),
                None => resp,
            };
            if resp.clicked() {
                if ui.input(|i| i.modifiers.shift) {
                    toggle = Some(s.id);
                } else {
                    select = Some(s.id);
                }
            }
            resp.dnd_set_drag_payload(DragSurface(s.id));
            if resp.dragged() {
                ghost(&ctx, thumb, icon, &s.name);
            }
            if resp.secondary_clicked() {
                select = Some(s.id);
            }
            let revision = self.history.revision();
            resp.context_menu(|ui| self.surface_menu(ui, s.id));
            if self.history.revision() != revision {
                // Menyn ändrade listan (t.ex. tog bort ytan) – rita om nästa bildruta.
                break;
            }

            // Släpp en annan yta här: före eller efter, beroende på halvan.
            // (Pekarens läge kollas direkt – egui räknar inte raden som
            // träffad medan något annat dras.)
            let pointer = ctx.pointer_interact_pos().filter(|p| resp.rect.contains(*p));
            let released = ctx.input(|i| i.pointer.any_released());
            if let (Some(p), Some(pos)) = (egui::DragAndDrop::payload::<DragSurface>(&ctx), pointer) {
                if p.0 != s.id {
                    let above = pos.y < resp.rect.center().y;
                    let y = if above { resp.rect.min.y } else { resp.rect.max.y };
                    ui.painter().hline(resp.rect.x_range(), y, egui::Stroke::new(2.0, ACCENT));
                    if released {
                        egui::DragAndDrop::clear_payload(&ctx);
                        if let Some(from) = self.project.surface_index(p.0) {
                            let target = if from < index { index - 1 } else { index };
                            // Listan visas med översta ytan först: "ovanför" = framför.
                            action = Some(Command::MoveSurfaceTo { id: p.0, index: if above { target + 1 } else { target } });
                        }
                    }
                }
            }
            // Släpp media här: ytan får mediet.
            if let (Some(m), Some(_)) = (egui::DragAndDrop::payload::<DragMedia>(&ctx), pointer) {
                ui.painter().rect_stroke(resp.rect, 4.0, egui::Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
                if released {
                    egui::DragAndDrop::clear_payload(&ctx);
                    self.show_source_on(m.0, Some(s.id));
                }
            }
        }
        if let Some(id) = select {
            self.select(Some(id));
        }
        if let Some(id) = toggle {
            self.toggle_in_group(id);
        }
        if let Some(c) = action {
            self.exec(c, None);
        }
    }

    pub fn new_surface_buttons(&mut self, ui: &mut Ui) {
        for shape in crate::panels::SHAPES {
            if ui.button(crate::panels::shape_label(shape)).on_hover_text(crate::panels::shape_hint(shape)).clicked() {
                let src = self.selected_source;
                self.add_shaped(src, shape);
            }
        }
        ui.separator();
        if ui
            .button(t("✏ Rita linje   D", "✏ Draw line   D"))
            .on_hover_text(t("Rita ljuslinjer med musen direkt i bilden", "Draw lines of light with the mouse, right in the picture"))
            .clicked()
        {
            self.draw_mode = true;
        }
    }

    /// Högerklicksmenyn för en yta – samma i ytlistan och i bilden.
    pub fn surface_menu(&mut self, ui: &mut Ui, id: SurfaceId) {
        let Some(s) = self.project.surface(id).cloned() else {
            ui.close();
            return;
        };
        let mut ns = s.clone();
        ui.label(RichText::new(&s.name).strong());
        ui.separator();
        ui.menu_button(t("🎞 Media", "🎞 Media"), |ui| {
            ui.selectable_value(&mut ns.source, None, t("— ingen —", "— none —"));
            for src in &self.project.sources {
                ui.selectable_value(&mut ns.source, Some(src.id), format!("{} {}", source_icon(&src.kind), src.name));
            }
        });
        ui.menu_button(t("⬜ Form", "⬜ Shape"), |ui| {
            for shape in crate::panels::SHAPES {
                let same = std::mem::discriminant(&shape) == std::mem::discriminant(&s.shape);
                if ui.selectable_label(same, crate::panels::shape_label(shape)).clicked() && !same {
                    crate::panels::reshape(&mut ns, shape);
                }
            }
        });
        ui.checkbox(&mut ns.visible, t("👁 Synlig", "👁 Visible"));
        ui.checkbox(&mut ns.locked, t("🔒 Låst", "🔒 Locked"));
        let mut test = self.opts.test_surfaces.contains(&id);
        if ui.checkbox(&mut test, t("⊞ Testbild", "⊞ Test pattern")).changed() {
            if test {
                self.opts.test_surfaces.insert(id);
            } else {
                self.opts.test_surfaces.remove(&id);
            }
        }
        ui.separator();
        if ui.button(t("⟲ Rak", "⟲ Straighten")).clicked() {
            ns.dst_pts = crate::panels::straightened(&s);
        }
        if ui.button(t("⛶ Fyll bilden", "⛶ Fill the image")).clicked() {
            ns.dst_pts = crate::panels::shape_points(s.shape, &lm_core::UNIT_QUAD);
        }
        if ui.button(t("⏶ Längst fram", "⏶ Bring to front")).clicked() {
            self.exec(Command::MoveSurfaceTo { id, index: self.project.surfaces.len() }, None);
        }
        if ui.button(t("⏷ Längst bak", "⏷ Send to back")).clicked() {
            self.exec(Command::MoveSurfaceTo { id, index: 0 }, None);
        }
        ui.separator();
        if ui.button(t("🗐 Duplicera", "🗐 Duplicate")).clicked() {
            self.duplicate_surface(id);
            return;
        }
        if ui.button(t("🗑 Ta bort", "🗑 Delete")).clicked() {
            self.select(Some(id));
            self.remove_selected();
            return;
        }
        if ns != s {
            self.exec(Command::ReplaceSurface(ns), None);
        }
    }

    fn cue_list(&mut self, ui: &mut Ui) {
        let n = self.project.cues.len();
        let current = self.current_cue.and_then(|id| self.project.cues.iter().position(|c| c.id == id));
        let next = self.project.next_cue_index(current).filter(|j| Some(*j) != current);
        ui.horizontal(|ui| {
            ui.heading("Cues");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let go = egui::Button::new(RichText::new("GO ▶").strong()).fill(Color32::from_rgb(40, 110, 50));
                let hint = match next {
                    Some(i) => format!("{}: {} (Enter)", t("Nästa", "Next"), self.project.cues[i].name),
                    None => t("Sista cuen är igång", "The last cue is running").to_string(),
                };
                if ui.add_enabled(next.is_some(), go).on_hover_text(hint).clicked() {
                    self.go_next_cue(1);
                }
                let mut looping = self.project.settings.loop_cues;
                if ui
                    .toggle_value(&mut looping, "🔁")
                    .on_hover_text(t("Börja om från första cuen efter den sista", "Start over from the first cue after the last"))
                    .changed()
                {
                    let mut settings = self.project.settings.clone();
                    settings.loop_cues = looping;
                    self.exec(Command::ReplaceSettings(settings), None);
                }
            });
        });
        if n == 0 {
            ui.label(RichText::new(t("Ställ in ytorna och klicka + Spara cue.", "Set up the surfaces and click + Save cue.")).color(DIM));
        }
        let (mut go, mut remove) = (None, None);
        for (i, c) in self.project.cues.iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::hover());
            let resp = ui.interact(rect, Id::new(("cue_row", c.id)), Sense::click());
            let painter = ui.painter_at(rect);
            let v = ui.visuals();
            let selected = self.selected_cue == Some(c.id);
            if Some(i) == current {
                painter.rect_filled(rect, 4.0, Color32::from_rgb(30, 80, 40));
            } else if selected {
                painter.rect_filled(rect, 4.0, v.selection.bg_fill);
            } else if resp.hovered() {
                painter.rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
            }
            // Spelas nu: ▶, nästa på tur: ›.
            let (mark, mark_color) = if Some(i) == current {
                ("▶", Color32::from_rgb(120, 230, 120))
            } else if Some(i) == next {
                ("›", Color32::from_rgb(255, 200, 80))
            } else {
                ("", DIM)
            };
            let y = rect.center().y;
            painter.text(egui::pos2(rect.min.x + 10.0, y), egui::Align2::CENTER_CENTER, mark, egui::FontId::proportional(14.0), mark_color);
            let color = if Some(i) == current { Color32::WHITE } else { v.text_color() };
            painter.text(egui::pos2(rect.min.x + 22.0, y), egui::Align2::LEFT_CENTER, format!("{}. {}", i + 1, c.name), egui::FontId::proportional(14.0), color);
            // Automatisk cue: nedräkning när den körs, annars ⏱ och tiden.
            let meta = match (self.auto_remaining(c.id), c.auto) {
                (Some(left), _) => format!("⏱ {left:.0} s"),
                (None, Some(a)) => format!("⏱ {a:.0} s"),
                (None, None) => format!("{:.1} s", c.fade),
            };
            let meta_color = if c.auto.is_some() { Color32::from_rgb(255, 200, 80) } else { DIM };
            painter.text(egui::pos2(rect.max.x - 30.0, y), egui::Align2::RIGHT_CENTER, meta, egui::FontId::proportional(12.0), meta_color);
            if (ui.rect_contains_pointer(rect) || selected)
                && row_button(ui, rect, rect.max.x - 12.0, RichText::new("🗑").color(DIM), t("Ta bort cue", "Delete cue"))
            {
                remove = Some(c.id);
            }
            let resp = resp.on_hover_text(format!("{} {:.1} s  •  {}", t("Övergång", "Fade"), c.fade, t("dubbelklicka för GO", "double-click for GO")));
            if resp.clicked() {
                self.selected_cue = Some(c.id);
            }
            if resp.double_clicked() {
                go = Some(c.id);
            }
        }
        if let Some(id) = go {
            self.go_cue(id);
        }
        if let Some(id) = remove {
            self.exec(Command::RemoveCue(id), None);
        }
        if ui
            .button(t("+ Spara cue", "+ Save cue"))
            .on_hover_text(t("Sparar vilka ytor som syns, deras opacitet och media", "Saves which surfaces are visible, their opacity and media"))
            .clicked()
        {
            let cue = self.project.capture_cue(format!("Cue {}", self.project.cues.len() + 1), 1.0);
            let id = cue.id;
            // Efter vald cue, annars sist.
            let index = self
                .selected_cue
                .and_then(|s| self.project.cues.iter().position(|c| c.id == s))
                .map_or(self.project.cues.len(), |i| i + 1);
            self.exec(Command::AddCue { cue, index }, None);
            self.selected_cue = Some(id);
            self.current_cue = Some(id);
        }

        // Vald cue
        let Some(mut c) = self.selected_cue.and_then(|id| self.project.cue(id)).cloned() else { return };
        let before = c.clone();
        let mut gesture = None;
        ui.add_space(6.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            let r = ui.text_edit_singleline(&mut c.name);
            if r.changed() || r.gained_focus() {
                gesture = Some(self.field_gesture(&r));
            }
            ui.horizontal(|ui| {
                ui.label(t("Övergång", "Fade"));
                let r = ui.add(egui::DragValue::new(&mut c.fade).range(0.0..=60.0).speed(0.05).suffix(" s"));
                if r.changed() {
                    gesture = Some(self.field_gesture(&r));
                }
            });
            ui.horizontal(|ui| {
                let mut auto = c.auto.is_some();
                if ui
                    .checkbox(&mut auto, t("⏱ Nästa efter", "⏱ Next after"))
                    .on_hover_text(t("Gå vidare till nästa cue automatiskt – för shower som rullar av sig själva", "Go on to the next cue automatically – for shows that run by themselves"))
                    .changed()
                {
                    c.auto = auto.then_some(c.auto.unwrap_or(10.0).max(c.fade));
                }
                if let Some(a) = &mut c.auto {
                    let r = ui.add(egui::DragValue::new(a).range(0.5..=3600.0).speed(0.2).suffix(" s"));
                    if r.changed() {
                        gesture = Some(self.field_gesture(&r));
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("▶ GO").on_hover_text(t("Gå till cuen", "Go to this cue")).clicked() {
                    self.go_cue(c.id);
                }
                if ui
                    .button(t("⟳ Uppdatera", "⟳ Update"))
                    .on_hover_text(t("Spara nuvarande läge i den här cuen", "Store the current state in this cue"))
                    .clicked()
                {
                    let now = self.project.capture_cue("", 0.0);
                    c.surfaces = now.surfaces;
                    c.lamps = now.lamps;
                }
                let index = self.project.cues.iter().position(|x| x.id == c.id).unwrap_or(0);
                if ui.add_enabled(index > 0, egui::Button::new("⏶").small()).on_hover_text(t("Flytta upp", "Move up")).clicked() {
                    self.exec(Command::MoveCueTo { id: c.id, index: index - 1 }, None);
                }
                if ui.add_enabled(index + 1 < n, egui::Button::new("⏷").small()).on_hover_text(t("Flytta ner", "Move down")).clicked() {
                    self.exec(Command::MoveCueTo { id: c.id, index: index + 1 }, None);
                }
            });
        });
        if c != before {
            self.exec(Command::ReplaceCue(c), gesture);
        }
    }
}
