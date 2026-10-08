//! Editorns paneler: verktygsrad, media och ytor (vänster), egenskaper (höger)
//! och förhandsvisningen av projektorn i mitten.

use crate::app::{LumaApp, Pending};
use crate::canvas::{fit, PtKind, ACCENT};
use eframe::egui::{self, Color32, RichText, Ui};
use lm_core::{BlendMode, Command, Mask, Pt, Shape, SourceKind, Surface, MESH_MAX, MESH_MIN, UNIT_QUAD};
use std::time::Duration;

const FILES: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "mpg", "mpeg", "ogv", "wmv", "flv", "ts", "mts", "gif", "png", "jpg",
    "jpeg", "bmp", "webp", "tif", "tiff",
];

impl LumaApp {
    pub fn editor_ui(&mut self, ui: &mut Ui) {
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("library").resizable(true).default_size(230.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.library(ui));
        });
        egui::Panel::right("properties").resizable(true).default_size(290.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.properties(ui));
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_gray(18)))
            .show(ui, |ui| self.preview(ui));
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let mode = if self.show_mode { "✏ Redigera" } else { "▶ Visa" };
            let mode_btn = egui::Button::new(RichText::new(mode).strong().size(15.0))
                .fill(if self.show_mode { Color32::from_rgb(40, 110, 50) } else { Color32::from_rgb(30, 90, 140) });
            if ui.add(mode_btn).on_hover_text("Växla mellan Redigera och Visa (Tab)").clicked() {
                self.show_mode = !self.show_mode;
            }
            ui.separator();
            if ui.button("➕ Media…").on_hover_text("Lägg till video eller bild (du kan också dra filer hit)").clicked() {
                if let Some(files) = rfd::FileDialog::new()
                    .set_title("Lägg till media")
                    .add_filter("Video och bild", FILES)
                    .pick_files()
                {
                    self.add_files(files, None);
                }
            }
            ui.menu_button("⬜ Ny yta ▾", |ui| {
                for (shape, label) in SHAPES {
                    if ui.button(label).clicked() {
                        let src = self.selected_source;
                        self.add_shaped(src, shape);
                    }
                }
            })
            .response
            .on_hover_text("Lägg till en yta");
            ui.separator();
            if ui.add_enabled(self.history.can_undo(), egui::Button::new("⮪")).on_hover_text("Ångra (Ctrl+Z)").clicked() {
                self.history.undo(&mut self.project);
            }
            if ui.add_enabled(self.history.can_redo(), egui::Button::new("⮫")).on_hover_text("Gör om (Ctrl+Shift+Z)").clicked() {
                self.history.redo(&mut self.project);
            }
            ui.separator();
            ui.toggle_value(&mut self.opts.output_test, "⊞ Testbild").on_hover_text("Testbild på hela projektorn (T)");
            ui.toggle_value(&mut self.opts.blackout, "⏹ Svart").on_hover_text("Svart på projektorn (B)");
            ui.separator();
            self.output_picker(ui);
            let out = self.current_output;
            if self.output_is_open(out) {
                if ui.button("⛶ Helskärm").on_hover_text("Projektorfönstret i helskärm (F i projektorfönstret)").clicked() {
                    let ctx = ui.ctx().clone();
                    self.set_output_fullscreen(&ctx, out, true);
                }
            } else if ui.button("🖵 Öppna projektorfönster").clicked() {
                self.open_output(out);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("☰", |ui| {
                    if ui.button("Nytt projekt   Ctrl+N").clicked() {
                        self.request(Pending::New);
                    }
                    if ui.button("Öppna…   Ctrl+O").clicked() {
                        self.request(Pending::Open(None));
                    }
                    if ui.button("Spara   Ctrl+S").clicked() {
                        self.save(false);
                    }
                    if ui.button("Spara som…   Ctrl+Shift+S").clicked() {
                        self.save(true);
                    }
                });
                if ui.button("💾").on_hover_text("Spara (Ctrl+S)").clicked() {
                    self.save(false);
                }
                let name = self
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Namnlöst projekt".into());
                let star = if self.dirty() { " •" } else { "" };
                ui.label(RichText::new(format!("{name}{star}")).color(Color32::from_gray(170)));
            });
        });
        ui.add_space(2.0);

        if let Some(path) = self.restore_offer.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Det finns ett osparat projekt från förra gången.").color(Color32::from_rgb(255, 200, 80)));
                if ui.button("Återställ").clicked() {
                    self.load(&path);
                    self.restore_offer = None;
                }
                if ui.button("Nej tack").clicked() {
                    let _ = std::fs::remove_file(&path);
                    self.restore_offer = None;
                }
            });
            ui.add_space(2.0);
        }
    }

    fn output_picker(&mut self, ui: &mut Ui) {
        ui.label("Utgång:");
        let current = self.project.output(self.current_output).map(|o| o.name.clone()).unwrap_or_default();
        let mut pick = self.current_output;
        let mut add = false;
        egui::ComboBox::from_id_salt("output_pick").selected_text(current).show_ui(ui, |ui| {
            for o in &self.project.outputs {
                ui.selectable_value(&mut pick, o.id, &o.name);
            }
            ui.separator();
            add = ui.button("➕ Ny utgång").on_hover_text("Lägg till en projektor till").clicked();
        });
        if add {
            self.add_output();
        } else {
            self.select_output(pick);
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let msg = self
                .status
                .as_ref()
                .filter(|(_, t)| t.elapsed() < Duration::from_secs(6))
                .map(|(m, _)| m.clone());
            match msg {
                Some(m) => ui.label(RichText::new(m).color(ACCENT)),
                None => ui.label(
                    RichText::new("Dra hörnen med musen  •  Pilar finjusterar (Shift = 10 px)  •  C = nästa hörn  •  Mellanslag = spela/pausa  •  Del = ta bort")
                        .color(Color32::from_gray(140)),
                ),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(o) = self.project.output(self.current_output) {
                    ui.label(RichText::new(format!("{}: {} × {}", o.name, o.resolution[0], o.resolution[1])).color(Color32::from_gray(140)));
                }
            });
        });
    }

    fn library(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.heading("Media");
        if self.project.sources.is_empty() {
            ui.label(RichText::new("Dra in videor eller bilder här.").color(Color32::from_gray(140)));
        }
        let mut remove = None;
        let mut assign = None;
        for src in self.project.sources.clone() {
            let icon = match src.kind {
                SourceKind::Video { .. } => "🎞",
                SourceKind::Image { .. } => "🖼",
                SourceKind::Color { .. } => "🎨",
                SourceKind::TestPattern => "⊞",
            };
            let error = self.media.get(src.id).and_then(|m| m.error()).map(str::to_owned);
            ui.horizontal(|ui| {
                let selected = self.selected_source == Some(src.id);
                let mut text = RichText::new(format!("{icon} {}", src.name));
                if error.is_some() {
                    text = text.color(Color32::from_rgb(255, 100, 100));
                }
                let r = ui.selectable_label(selected, text);
                let r = match &error {
                    Some(e) => r.on_hover_text(e),
                    None => r.on_hover_text("Dubbelklicka för att visa på markerad yta"),
                };
                if r.clicked() {
                    self.selected_source = Some(src.id);
                }
                if r.double_clicked() {
                    assign = Some(src.id);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("🗑").on_hover_text("Ta bort media").clicked() {
                        remove = Some(src.id);
                    }
                });
            });
        }
        if let Some(id) = remove {
            self.exec(Command::RemoveSource(id), None);
        }
        if let Some(sid) = assign {
            match self.selected.and_then(|id| self.project.surface(id).cloned()) {
                Some(mut s) => {
                    s.source = Some(sid);
                    self.exec(Command::ReplaceSurface(s), None);
                }
                None => {
                    self.add_surface(Some(sid));
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.small_button("+ Färg").clicked() {
                let s = self.project.make_source("Färg", SourceKind::Color { rgba: [1.0, 1.0, 1.0, 1.0] });
                self.selected_source = Some(s.id);
                let index = self.project.sources.len();
                self.exec(Command::AddSource { source: s, index }, None);
            }
            if ui.small_button("+ Testbild").clicked() {
                let s = self.project.make_source("Testbild", SourceKind::TestPattern);
                self.selected_source = Some(s.id);
                let index = self.project.sources.len();
                self.exec(Command::AddSource { source: s, index }, None);
            }
        });

        ui.add_space(14.0);
        ui.heading("Ytor");
        // Bara ytorna på vald utgång. `on_output` är deras index i hela listan.
        let out = self.current_output;
        let on_output: Vec<usize> = (0..self.project.surfaces.len()).filter(|&i| self.project.surfaces[i].output == out).collect();
        if on_output.is_empty() {
            ui.label(RichText::new("Inga ytor här än. Klicka ⬜ Ny yta.").color(Color32::from_gray(140)));
        }
        let mut action: Option<Command> = None;
        // Översta ytan visas först i listan.
        for (k, &index) in on_output.iter().enumerate().rev() {
            let s = self.project.surfaces[index].clone();
            let (below, above) = (k.checked_sub(1).map(|j| on_output[j]), on_output.get(k + 1).copied());
            ui.horizontal(|ui| {
                let mut visible = s.visible;
                if ui.checkbox(&mut visible, "").on_hover_text("Synlig").changed() {
                    let mut ns = s.clone();
                    ns.visible = visible;
                    action = Some(Command::ReplaceSurface(ns));
                }
                let lock = if s.locked { "🔒 " } else { "" };
                if ui.selectable_label(self.selected == Some(s.id), format!("{lock}{}", s.name)).clicked() {
                    self.select(Some(s.id));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(below.is_some(), egui::Button::new("⏷").small()).on_hover_text("Flytta bakåt").clicked() {
                        action = below.map(|index| Command::MoveSurfaceTo { id: s.id, index });
                    }
                    if ui.add_enabled(above.is_some(), egui::Button::new("⏶").small()).on_hover_text("Flytta framåt").clicked() {
                        action = above.map(|index| Command::MoveSurfaceTo { id: s.id, index });
                    }
                });
            });
        }
        if let Some(c) = action {
            self.exec(c, None);
        }
    }

    fn properties(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        let Some(mut s) = self.selected.and_then(|id| self.project.surface(id).cloned()) else {
            self.output_properties(ui);
            ui.add_space(14.0);
            ui.label(RichText::new("Markera en yta för att ändra den.").color(Color32::from_gray(140)));
            ui.add_space(10.0);
            self.quick_start(ui);
            return;
        };
        let before = s.clone();
        let mut gesture = None;

        let r = ui.add(egui::TextEdit::singleline(&mut s.name).font(egui::TextStyle::Heading));
        if r.changed() || r.gained_focus() {
            gesture = Some(self.field_gesture(&r));
        }
        ui.add_space(6.0);

        egui::Grid::new("surface_props").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            ui.label("Media");
            let current = s
                .source
                .and_then(|id| self.project.source(id))
                .map(|src| src.name.clone())
                .unwrap_or_else(|| "— ingen —".into());
            egui::ComboBox::from_id_salt("source_pick").selected_text(current).width(170.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.source, None, "— ingen —");
                for src in &self.project.sources {
                    ui.selectable_value(&mut s.source, Some(src.id), &src.name);
                }
            });
            ui.end_row();

            if self.project.outputs.len() > 1 {
                ui.label("Utgång");
                let current = self.project.output(s.output).map(|o| o.name.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("surface_output").selected_text(current).width(170.0).show_ui(ui, |ui| {
                    for o in &self.project.outputs {
                        ui.selectable_value(&mut s.output, o.id, &o.name);
                    }
                });
                ui.end_row();
            }

            ui.label("Form");
            ui.horizontal_wrapped(|ui| {
                let mut shape = s.shape;
                for (option, label) in SHAPES {
                    let same = std::mem::discriminant(&option) == std::mem::discriminant(&shape);
                    if ui.selectable_label(same, label).on_hover_text(shape_hint(option)).clicked() && !same {
                        shape = option;
                    }
                }
                if let Shape::Mesh { cols, rows } = &mut shape {
                    ui.add(egui::DragValue::new(cols).range(MESH_MIN..=MESH_MAX).suffix(" kol"))
                        .on_hover_text("Antal punkter på bredden");
                    ui.add(egui::DragValue::new(rows).range(MESH_MIN..=MESH_MAX).suffix(" rad"))
                        .on_hover_text("Antal punkter på höjden");
                }
                if shape != s.shape {
                    reshape(&mut s, shape);
                    self.selected_point = None;
                }
            });
            ui.end_row();

            ui.label("Blandning");
            egui::ComboBox::from_id_salt("blend_pick").selected_text(s.blend.label()).width(170.0).show_ui(ui, |ui| {
                for mode in BlendMode::ALL {
                    ui.selectable_value(&mut s.blend, mode, mode.label()).on_hover_text(blend_hint(mode));
                }
            });
            ui.end_row();

            ui.label("Opacitet");
            let r = ui.add(egui::Slider::new(&mut s.opacity, 0.0..=1.0).show_value(true));
            if r.changed() {
                gesture = Some(self.field_gesture(&r));
            }
            ui.end_row();

            ui.label("");
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.visible, "Synlig");
                ui.checkbox(&mut s.locked, "Låst");
            });
            ui.end_row();
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let mut test = self.opts.test_surfaces.contains(&s.id);
            if ui.toggle_value(&mut test, "⊞ Testbild").on_hover_text("Visa testbild på ytan medan du riktar in den").changed() {
                if test {
                    self.opts.test_surfaces.insert(s.id);
                } else {
                    self.opts.test_surfaces.remove(&s.id);
                }
            }
            if ui.button("⟲ Rak").on_hover_text("Gör ytan till en rak rektangel igen").clicked() {
                let xs = s.dst_pts.iter().map(|p| p[0]);
                let ys = s.dst_pts.iter().map(|p| p[1]);
                let (x0, x1) = (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max));
                let (y0, y1) = (ys.clone().fold(f32::MAX, f32::min), ys.fold(f32::MIN, f32::max));
                s.dst_pts = shape_points(s.shape, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1]]);
            }
            if ui.button("⛶ Fyll").on_hover_text("Fyll hela projektorbilden").clicked() {
                s.dst_pts = shape_points(s.shape, &UNIT_QUAD);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("🗐 Duplicera").clicked() {
                let mut copy = self.project.make_quad(s.source, s.output);
                copy.name = format!("{} kopia", s.name);
                copy.shape = s.shape;
                copy.mask = s.mask.clone();
                copy.blend = s.blend;
                copy.src_pts = s.src_pts.clone();
                copy.dst_pts = s.dst_pts.iter().map(|p| [p[0] + 0.03, p[1] + 0.03]).collect();
                copy.opacity = s.opacity;
                let index = self.project.surfaces.len();
                let id = copy.id;
                self.exec(Command::AddSurface { surface: copy, index }, None);
                self.select(Some(id));
                return;
            }
            if ui.button("🗑 Ta bort").clicked() {
                self.remove_selected();
            }
        });

        // Mask
        ui.add_space(14.0);
        ui.heading("Mask");
        match &mut s.mask {
            None => {
                ui.label(RichText::new("Dölj delar av ytan, t.ex. ett fönster eller en dörr.").color(Color32::from_gray(140)));
                if ui.button("➕ Mask").clicked() {
                    // Rektangel lite innanför ytan att börja dra i.
                    let xs = s.dst_pts.iter().map(|p| p[0]);
                    let ys = s.dst_pts.iter().map(|p| p[1]);
                    let (x0, x1) = (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max));
                    let (y0, y1) = (ys.clone().fold(f32::MAX, f32::min), ys.fold(f32::MIN, f32::max));
                    let (dx, dy) = ((x1 - x0) * 0.2, (y1 - y0) * 0.2);
                    let (x0, x1, y0, y1) = (x0 + dx, x1 - dx, y0 + dy, y1 - dy);
                    s.mask = Some(Mask {
                        points: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
                        feather: 0.0,
                        invert: true,
                    });
                    self.mask_edit = true;
                    self.selected_point = None;
                }
            }
            Some(m) => {
                ui.horizontal(|ui| {
                    if ui
                        .toggle_value(&mut self.mask_edit, "✏ Redigera mask")
                        .on_hover_text("Dra maskens punkter i stället för ytans hörn")
                        .changed()
                    {
                        self.selected_point = None;
                    }
                    ui.radio_value(&mut m.invert, true, "Dölj inuti");
                    ui.radio_value(&mut m.invert, false, "Visa inuti");
                });
                ui.horizontal(|ui| {
                    ui.label("Mjuk kant");
                    let r = ui.add(egui::Slider::new(&mut m.feather, 0.0..=200.0).suffix(" px"));
                    if r.changed() {
                        gesture = Some(self.field_gesture(&r));
                    }
                });
                if self.mask_edit {
                    ui.label(
                        RichText::new("Dubbelklicka på en kant för ny punkt, högerklicka på en punkt för att ta bort den.")
                            .color(Color32::from_gray(140)),
                    );
                }
                if ui.button("🗑 Ta bort mask").clicked() {
                    s.mask = None;
                    self.mask_edit = false;
                }
            }
        }

        if self.project.surface(before.id).is_some() && s != before && self.selected == Some(before.id) {
            self.exec(Command::ReplaceSurface(s.clone()), gesture);
            // Ytan flyttad till en annan utgång – följ med dit.
            self.current_output = s.output;
        }
        if self.selected != Some(before.id) {
            return;
        }

        // Utsnitt ur källan + uppspelning.
        if let Some(sid) = s.source {
            ui.add_space(14.0);
            ui.heading("Utsnitt ur media");
            ui.label(RichText::new("Dra hörnen för att välja vilken del som visas.").color(Color32::from_gray(140)));
            let (tex, size) = self
                .renderer
                .source_texture(sid)
                .map(|(t, s)| (Some(t), s))
                .unwrap_or((None, [16, 9]));
            let width = ui.available_width() - 16.0;
            let aspect = size[0] as f32 / size[1].max(1) as f32;
            let (outer, _) = ui.allocate_exact_size(egui::vec2(width + 16.0, width / aspect + 16.0), egui::Sense::hover());
            let rect = fit(outer.shrink(8.0), aspect);
            ui.painter().rect_filled(rect, 0.0, Color32::from_gray(30));
            if let Some(tex) = tex {
                ui.painter().image(
                    tex,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::from_gray(200),
                );
            }
            self.canvas(ui, rect, PtKind::Src, "source");
            if ui.small_button("⟲ Hela bilden").clicked() {
                self.set_points(s.id, PtKind::Src, UNIT_QUAD.to_vec(), None);
            }
            self.transport(ui, sid);
        }
    }

    fn transport(&mut self, ui: &mut Ui, sid: lm_core::SourceId) {
        let Some(src) = self.project.source(sid).cloned() else { return };
        let SourceKind::Video { path, looping, muted } = &src.kind else { return };
        ui.add_space(14.0);
        ui.heading("Uppspelning");
        let Some(media) = self.media.get_mut(sid) else { return };
        if let Some(e) = media.error() {
            ui.colored_label(Color32::from_rgb(255, 100, 100), e);
            return;
        }
        ui.horizontal(|ui| {
            let label = if media.is_playing() { "⏸ Paus" } else { "▶ Spela" };
            if ui.button(label).clicked() {
                if media.is_playing() {
                    media.pause();
                } else {
                    media.play();
                }
            }
            if ui.button("⏮ Början").clicked() {
                media.seek(0.0);
            }
        });
        if let (Some(pos), Some(dur)) = (media.position(), media.duration()) {
            let mut t = pos;
            let r = ui.add(
                egui::Slider::new(&mut t, 0.0..=dur.max(0.01))
                    .custom_formatter(|v, _| format!("{}:{:04.1}", (v / 60.0) as u32, v % 60.0))
                    .show_value(true),
            );
            if r.changed() {
                media.seek(t);
            }
        }
        let (mut l, mut m) = (*looping, *muted);
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut l, "🔁 Loopa").changed();
            changed |= ui.checkbox(&mut m, "🔇 Ljud av").changed();
        });
        if changed {
            let mut ns = src.clone();
            ns.kind = SourceKind::Video {
                path: path.clone(),
                looping: l,
                muted: m,
            };
            self.exec(Command::ReplaceSource(ns), None);
        }
    }

    fn output_properties(&mut self, ui: &mut Ui) {
        let Some(mut o) = self.project.output(self.current_output).cloned() else { return };
        let before = o.clone();
        ui.heading("Utgång");
        let r = ui.add(egui::TextEdit::singleline(&mut o.name).font(egui::TextStyle::Heading));
        let gesture = (r.changed() || r.gained_focus()).then(|| self.field_gesture(&r));
        ui.label(RichText::new(format!("{} × {} px", o.resolution[0], o.resolution[1])).color(Color32::from_gray(140)));
        ui.add_space(6.0);
        let mut remove = false;
        ui.horizontal(|ui| {
            if ui.button("➕ Ny utgång").clicked() {
                self.add_output();
            }
            let many = self.project.outputs.len() > 1;
            remove = ui
                .add_enabled(many, egui::Button::new("🗑 Ta bort"))
                .on_hover_text("Tar bort utgången och dess ytor")
                .clicked();
        });
        if remove {
            self.remove_output(before.id);
        } else if o != before {
            self.exec(Command::ReplaceOutput(o), gesture);
        }
    }

    fn quick_start(&mut self, ui: &mut Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(RichText::new("Kom igång").strong());
            ui.add_space(4.0);
            ui.label("1. Dra en video eller bild till fönstret.");
            ui.label("2. Dra ytans fyra hörn så att de passar väggen.");
            ui.label("3. Tryck ▶ Visa, och F i projektorfönstret för helskärm.");
        });
    }

    fn preview(&mut self, ui: &mut Ui) {
        let Some(out) = self.project.output(self.current_output).cloned() else { return };
        let avail = ui.max_rect().shrink(16.0);
        let aspect = out.resolution[0] as f32 / out.resolution[1].max(1) as f32;
        let rect = fit(avail, aspect);
        self.editor_canvas = Some(rect);
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, Color32::BLACK);
        if let Some(tex) = self.renderer.output_texture(out.id) {
            painter.image(
                tex,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, Color32::from_gray(70)), egui::StrokeKind::Outside);
        ui.painter().text(
            rect.left_top() - egui::vec2(0.0, 4.0),
            egui::Align2::LEFT_BOTTOM,
            &out.name,
            egui::FontId::proportional(12.0),
            Color32::from_gray(150),
        );

        let dragging_file = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        if self.project.surfaces.is_empty() || dragging_file {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if dragging_file { "Släpp här" } else { "Dra in en video eller bild hit" },
                egui::FontId::proportional(22.0),
                Color32::from_gray(if dragging_file { 230 } else { 110 }),
            );
        }
        let kind = self.edit_kind(out.id);
        self.canvas(ui, rect, kind, "editor");
    }
}

const SHAPES: [(Shape, &str); 4] = [
    (Shape::Quad, "Fyrhörn"),
    (Shape::Triangle, "Triangel"),
    (Shape::Ellipse, "Ellips"),
    (Shape::Mesh { cols: 4, rows: 4 }, "Mesh"),
];

fn shape_hint(shape: Shape) -> &'static str {
    match shape {
        Shape::Quad => "Fyra hörn, perspektivriktig",
        Shape::Triangle => "Tre hörn, t.ex. en gavel",
        Shape::Ellipse => "Rund yta inuti fyra hörn – blir en perspektivriktig ellips",
        Shape::Mesh { .. } => "Rutnät av punkter för böjda ytor, t.ex. pelare",
    }
}

fn blend_hint(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "Ytan täcker det som ligger under",
        BlendMode::Add => "Ljuset adderas – svart blir genomskinligt",
        BlendMode::Multiply => "Mörkar ner det som ligger under – vitt blir genomskinligt",
        BlendMode::Screen => "Ljusar upp mjukt – svart blir genomskinligt",
    }
}

/// Punkter för `shape` som täcker fyrhörningen `quad`.
pub(crate) fn shape_points(shape: Shape, quad: &[Pt; 4]) -> Vec<Pt> {
    match shape {
        Shape::Quad | Shape::Ellipse => quad.to_vec(),
        Shape::Triangle => lm_geom::triangle_from_quad(quad).to_vec(),
        Shape::Mesh { cols, rows } => lm_geom::mesh_from_quad(quad, cols as usize, rows as usize),
    }
}

/// Fyrhörningen som ytan fyller (för triangel: den den är inskriven i).
fn surface_quad(s: &Surface) -> Option<[Pt; 4]> {
    match s.shape {
        Shape::Quad | Shape::Ellipse => lm_geom::quad(&s.dst_pts),
        Shape::Triangle => Some(lm_geom::quad_from_triangle(s.dst_pts.get(..3)?.try_into().ok()?)),
        Shape::Mesh { cols, rows } => {
            let (c, r) = (cols as usize, rows as usize);
            let p = &s.dst_pts;
            Some([p[0], p[c - 1], p[c * r - 1], p[c * (r - 1)]])
        }
    }
}

/// Byter form (eller meshupplösning) utan att ytan hoppar: den nya formen
/// samplas från den gamla.
pub(crate) fn reshape(s: &mut Surface, to: Shape) {
    if s.shape == to {
        return;
    }
    s.dst_pts = match (s.shape, to) {
        (Shape::Mesh { cols, rows }, Shape::Mesh { cols: nc, rows: nr }) => {
            lm_geom::mesh_grid(&s.dst_pts, cols as usize, rows as usize, nc as usize, nr as usize)
        }
        _ => match surface_quad(s) {
            Some(q) => shape_points(to, &q),
            None => return,
        },
    };
    s.shape = to;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(dst: Vec<Pt>) -> Surface {
        let mut p = lm_core::Project::new();
        let mut s = p.make_quad(None, p.outputs[0].id);
        s.dst_pts = dst;
        s
    }

    fn close(a: Pt, b: Pt) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    #[test]
    fn every_shape_roundtrips_through_quad() {
        let corners = vec![[0.1, 0.2], [0.8, 0.2], [0.9, 0.9], [0.2, 0.9]];
        for (shape, _) in SHAPES {
            let mut s = surface(corners.clone());
            reshape(&mut s, shape);
            assert_eq!(s.dst_pts.len(), shape.point_count(), "{shape:?}");
            reshape(&mut s, Shape::Quad);
            for (a, b) in s.dst_pts.iter().zip(&corners) {
                assert!(close(*a, *b), "{shape:?}: {a:?} != {b:?}");
            }
        }
    }

    #[test]
    fn quad_to_mesh_and_back_keeps_corners() {
        let corners = vec![[0.1, 0.2], [0.8, 0.1], [0.9, 0.9], [0.2, 0.7]];
        let mut s = surface(corners.clone());
        reshape(&mut s, Shape::Mesh { cols: 4, rows: 3 });
        assert_eq!(s.dst_pts.len(), 12);
        reshape(&mut s, Shape::Mesh { cols: 6, rows: 6 });
        assert_eq!(s.dst_pts.len(), 36);
        reshape(&mut s, Shape::Quad);
        for (a, b) in s.dst_pts.iter().zip(&corners) {
            assert!(close(*a, *b), "{a:?} != {b:?}");
        }
    }
}
