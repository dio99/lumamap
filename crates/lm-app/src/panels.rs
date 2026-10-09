//! Editorns paneler: verktygsrad, media och ytor (vänster), egenskaper (höger)
//! och förhandsvisningen av projektorn i mitten.

use crate::app::{LumaApp, Pending};
use crate::canvas::{fit, PtKind, ACCENT};
use eframe::egui::{self, Color32, RichText, Ui};
use lm_core::{BlendMode, Command, Mask, BLACK_LEVEL_MAX, EDGE_BLEND_MAX, Pt, Shape, SourceKind, Surface, MESH_MAX, MESH_MIN, UNIT_QUAD};
use lm_core::i18n::{self, t, Language};
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
            let mode = if self.show_mode { t("✏ Redigera", "✏ Edit") } else { t("▶ Visa", "▶ Show") };
            let mode_btn = egui::Button::new(RichText::new(mode).strong().size(15.0))
                .fill(if self.show_mode { Color32::from_rgb(40, 110, 50) } else { Color32::from_rgb(30, 90, 140) });
            if ui.add(mode_btn).on_hover_text(t("Växla mellan Redigera och Visa (Tab)", "Switch between Edit and Show (Tab)")).clicked() {
                self.show_mode = !self.show_mode;
            }
            ui.separator();
            if ui.button("➕ Media…").on_hover_text(t("Lägg till video eller bild (du kan också dra filer hit)", "Add video or image (you can also drag files here)")).clicked() {
                if let Some(files) = rfd::FileDialog::new()
                    .set_title(t("Lägg till media", "Add media"))
                    .add_filter(t("Video och bild", "Video and images"), FILES)
                    .pick_files()
                {
                    self.add_files(files, None);
                }
            }
            ui.menu_button(t("⬜ Ny yta ⏷", "⬜ New surface ⏷"), |ui| {
                for shape in SHAPES {
                    if ui.button(shape_label(shape)).clicked() {
                        let src = self.selected_source;
                        self.add_shaped(src, shape);
                    }
                }
            })
            .response
            .on_hover_text(t("Lägg till en yta", "Add a surface"));
            ui.separator();
            if ui.add_enabled(self.history.can_undo(), egui::Button::new("⮪")).on_hover_text(t("Ångra (Ctrl+Z)", "Undo (Ctrl+Z)")).clicked() {
                self.undo();
            }
            if ui.add_enabled(self.history.can_redo(), egui::Button::new("⮫")).on_hover_text(t("Gör om (Ctrl+Shift+Z)", "Redo (Ctrl+Shift+Z)")).clicked() {
                self.redo();
            }
            ui.separator();
            ui.toggle_value(&mut self.opts.output_test, t("⊞ Testbild", "⊞ Test pattern")).on_hover_text(t("Testbild på hela projektorn (T)", "Test pattern on the whole projector (T)"));
            ui.toggle_value(&mut self.opts.blackout, t("⏹ Svart", "⏹ Black")).on_hover_text(t("Svart på projektorn (B)", "Black out the projector (B)"));
            ui.label("Master");
            ui.add(egui::Slider::new(&mut self.opts.master, 0.0..=1.0).show_value(false))
                .on_hover_text(t("Ljusstyrka för alla ytor (sparas inte)", "Brightness of all surfaces (not saved)"));
            ui.separator();
            let mut bpm = self.project.settings.bpm;
            let r = ui
                .add(egui::DragValue::new(&mut bpm).range(30.0..=300.0).speed(0.5).fixed_decimals(0).prefix("♩ "))
                .on_hover_text(t("Tempo för effekterna (slag per minut)", "Tempo for the effects (beats per minute)"));
            if r.changed() {
                let gesture = self.field_gesture(&r);
                let mut settings = self.project.settings.clone();
                settings.bpm = bpm;
                self.exec(Command::ReplaceSettings(settings), Some(gesture));
            }
            if ui
                .button("Tap")
                .on_hover_text(t("Tryck i takt med musiken för att ställa tempot", "Tap along with the music to set the tempo"))
                .clicked()
            {
                self.tap_tempo();
            }
            ui.separator();
            self.output_picker(ui);
            let out = self.current_output;
            if self.output_is_open(out) {
                if ui.button(t("⛶ Helskärm", "⛶ Fullscreen")).on_hover_text(t("Projektorfönstret i helskärm (F i projektorfönstret)", "Projector window fullscreen (F in the projector window)")).clicked() {
                    let ctx = ui.ctx().clone();
                    self.set_output_fullscreen(&ctx, out, true);
                }
            } else if ui.button(t("🖵 Öppna projektorfönster", "🖵 Open projector window")).clicked() {
                self.open_output(out);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("☰", |ui| {
                    if ui.button(t("Nytt projekt   Ctrl+N", "New project   Ctrl+N")).clicked() {
                        self.request(Pending::New);
                    }
                    if ui.button(t("Öppna…   Ctrl+O", "Open…   Ctrl+O")).clicked() {
                        self.request(Pending::Open(None));
                    }
                    if ui.button(t("Spara   Ctrl+S", "Save   Ctrl+S")).clicked() {
                        self.save(false);
                    }
                    if ui.button(t("Spara som…   Ctrl+Shift+S", "Save as…   Ctrl+Shift+S")).clicked() {
                        self.save(true);
                    }
                    if ui
                        .button(t("📦 Samla projekt…", "📦 Collect project…"))
                        .on_hover_text(t(
                            "Kopiera projektet och all media till en mapp, t.ex. för att flytta till showdatorn",
                            "Copy the project and all its media into one folder, e.g. to move it to the show computer",
                        ))
                        .clicked()
                    {
                        self.collect_project();
                    }
                    ui.separator();
                    if ui.button(t("OSC-fjärrstyrning…", "OSC remote control…")).clicked() {
                        self.osc_window_open = true;
                    }
                    if ui.button("🎹 MIDI…").clicked() {
                        self.midi_window_open = true;
                    }
                    ui.separator();
                    ui.menu_button("🌐 Language / Språk", |ui| {
                        for lang in Language::ALL {
                            if ui.selectable_label(i18n::language() == lang, lang.native_name()).clicked() {
                                crate::app::set_language_saved(lang);
                            }
                        }
                    });
                });
                if ui.button("💾").on_hover_text(t("Spara (Ctrl+S)", "Save (Ctrl+S)")).clicked() {
                    self.save(false);
                }
                let name = self
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| t("Namnlöst projekt", "Untitled project").into());
                let star = if self.dirty() { " •" } else { "" };
                ui.label(RichText::new(format!("{name}{star}")).color(Color32::from_gray(170)));
            });
        });
        ui.add_space(2.0);

        if let Some(path) = self.restore_offer.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Det finns ett osparat projekt från förra gången.", "There is an unsaved project from last time.")).color(Color32::from_rgb(255, 200, 80)));
                if ui.button(t("Återställ", "Restore")).clicked() {
                    self.load(&path);
                    self.restore_offer = None;
                }
                if ui.button(t("Nej tack", "No thanks")).clicked() {
                    let _ = std::fs::remove_file(&path);
                    self.restore_offer = None;
                }
            });
            ui.add_space(2.0);
        }
    }

    fn output_picker(&mut self, ui: &mut Ui) {
        ui.label(t("Utgång:", "Output:"));
        let current = self.project.output(self.current_output).map(|o| o.name.clone()).unwrap_or_default();
        let mut pick = self.current_output;
        let mut add = false;
        egui::ComboBox::from_id_salt("output_pick").selected_text(current).show_ui(ui, |ui| {
            for o in &self.project.outputs {
                ui.selectable_value(&mut pick, o.id, &o.name);
            }
            ui.separator();
            add = ui.button(t("➕ Ny utgång", "➕ New output")).on_hover_text(t("Lägg till en projektor till", "Add another projector")).clicked();
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
                    RichText::new(t("Dra hörnen med musen  •  Pilar finjusterar (Shift = 10 px)  •  C = nästa hörn  •  Mellanslag = spela/pausa  •  Del = ta bort", "Drag the corners with the mouse  •  Arrows nudge (Shift = 10 px)  •  C = next corner  •  Space = play/pause  •  Del = delete"))
                        .color(Color32::from_gray(140)),
                ),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, color) = match (&self.osc, &self.osc_error) {
                    (Some(s), _) => (format!("OSC :{}", s.port()), Color32::from_rgb(120, 200, 120)),
                    (None, Some(_)) => ("OSC ✖".to_string(), Color32::from_rgb(255, 100, 100)),
                    _ => (t("OSC av", "OSC off").to_string(), Color32::from_gray(120)),
                };
                // Blinkar till när ett meddelande kommer in.
                let fresh = self.osc_last.as_ref().is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(300));
                let color = if fresh { ACCENT } else { color };
                if ui.add(egui::Button::new(RichText::new(text).color(color)).frame(false)).on_hover_text(t("OSC-inställningar", "OSC settings")).clicked() {
                    self.osc_window_open = true;
                }
                let devices = self.midi.devices().len();
                let fresh = self.midi_last.as_ref().is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(300));
                let color = if fresh {
                    ACCENT
                } else if devices > 0 {
                    Color32::from_rgb(120, 200, 120)
                } else {
                    Color32::from_gray(120)
                };
                if ui
                    .add(egui::Button::new(RichText::new(format!("MIDI {devices}")).color(color)).frame(false))
                    .on_hover_text(t("MIDI-kopplingar", "MIDI mappings"))
                    .clicked()
                {
                    self.midi_window_open = true;
                }
                ui.separator();
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
            ui.label(RichText::new(t("Dra in videor eller bilder här.", "Drag videos or images here.")).color(Color32::from_gray(140)));
        }
        let mut remove = None;
        let mut assign = None;
        for src in self.project.sources.clone() {
            let icon = match src.kind {
                SourceKind::Video { .. } => "🎞",
                SourceKind::Image { .. } => "🖼",
                SourceKind::Color { .. } => "🎨",
                SourceKind::TestPattern => "⊞",
                SourceKind::Camera { .. } => "📷",
                SourceKind::Stream { .. } => "📡",
                SourceKind::Generator { .. } => "✨",
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
                    None => r.on_hover_text(t("Dubbelklicka för att visa på markerad yta", "Double-click to show on the selected surface")),
                };
                if r.clicked() {
                    self.selected_source = Some(src.id);
                }
                if r.double_clicked() {
                    assign = Some(src.id);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("🗑").on_hover_text(t("Ta bort media", "Remove media")).clicked() {
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
            if ui.small_button(t("+ Färg", "+ Color")).clicked() {
                let s = self.project.make_source(t("Färg", "Color"), SourceKind::Color { rgba: [1.0, 1.0, 1.0, 1.0] });
                self.selected_source = Some(s.id);
                let index = self.project.sources.len();
                self.exec(Command::AddSource { source: s, index }, None);
            }
            if ui.small_button(t("+ Testbild", "+ Test pattern")).clicked() {
                let s = self.project.make_source(t("Testbild", "Test pattern"), SourceKind::TestPattern);
                self.selected_source = Some(s.id);
                let index = self.project.sources.len();
                self.exec(Command::AddSource { source: s, index }, None);
            }
        });
        ui.menu_button(t("+ Mönster ⏷", "+ Pattern ⏷"), |ui| {
            for pattern in lm_core::Pattern::ALL {
                if ui.button(format!("✨ {}", pattern.label())).clicked() {
                    let kind = SourceKind::Generator { pattern, colors: pattern.default_colors(), speed: 0.25 };
                    self.add_source_shown(pattern.label(), kind);
                }
            }
        })
        .response
        .on_hover_text(t("Rörligt mönster som ritas direkt – ingen videofil behövs", "Moving pattern drawn live – no video file needed"));
        ui.horizontal(|ui| {
            let r = ui.menu_button(t("+ Kamera ⏷", "+ Camera ⏷"), |ui| {
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
            r.response.on_hover_text(t("Webbkamera eller annan videoenhet", "Webcam or other video device"));
            if ui
                .small_button(t("+ Ström…", "+ Stream…"))
                .on_hover_text(t("RTSP, SRT, UDP, HTTP eller NDI via GStreamer", "RTSP, SRT, UDP, HTTP or NDI via GStreamer"))
                .clicked()
            {
                self.stream_input = Some(self.stream_input.take().unwrap_or_default());
            }
        });
        if let Some(mut uri) = self.stream_input.take() {
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

        ui.add_space(14.0);
        ui.heading(t("Ytor", "Surfaces"));
        // Bara ytorna på vald utgång. `on_output` är deras index i hela listan.
        let out = self.current_output;
        let on_output: Vec<usize> = (0..self.project.surfaces.len()).filter(|&i| self.project.surfaces[i].output == out).collect();
        if on_output.is_empty() {
            ui.label(RichText::new(t("Inga ytor här än. Klicka ⬜ Ny yta.", "No surfaces here yet. Click ⬜ New surface.")).color(Color32::from_gray(140)));
        }
        let mut action: Option<Command> = None;
        // Översta ytan visas först i listan.
        for (k, &index) in on_output.iter().enumerate().rev() {
            let s = self.project.surfaces[index].clone();
            let (below, above) = (k.checked_sub(1).map(|j| on_output[j]), on_output.get(k + 1).copied());
            ui.horizontal(|ui| {
                let mut visible = s.visible;
                if ui.checkbox(&mut visible, "").on_hover_text(t("Synlig", "Visible")).changed() {
                    let mut ns = s.clone();
                    ns.visible = visible;
                    action = Some(Command::ReplaceSurface(ns));
                }
                let lock = if s.locked { "🔒 " } else { "" };
                // Källan går inte att spela (fil saknas, ström tappad …): ytan är svart.
                let broken = s.source.and_then(|id| self.media.get(id)).and_then(|m| m.error()).map(str::to_owned);
                let mut text = RichText::new(format!("{}{lock}{}", if broken.is_some() { "⚠ " } else { "" }, s.name));
                if broken.is_some() {
                    text = text.color(Color32::from_rgb(255, 120, 100));
                }
                let r = ui.selectable_label(self.selected == Some(s.id), text);
                let r = match &broken {
                    Some(e) => r.on_hover_text(format!("{}: {e}", t("Media fungerar inte", "Media is not working"))),
                    None => r,
                };
                if r.clicked() {
                    self.select(Some(s.id));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(below.is_some(), egui::Button::new("⏷").small()).on_hover_text(t("Flytta bakåt", "Move back")).clicked() {
                        action = below.map(|index| Command::MoveSurfaceTo { id: s.id, index });
                    }
                    if ui.add_enabled(above.is_some(), egui::Button::new("⏶").small()).on_hover_text(t("Flytta framåt", "Move forward")).clicked() {
                        action = above.map(|index| Command::MoveSurfaceTo { id: s.id, index });
                    }
                });
            });
        }
        if let Some(c) = action {
            self.exec(c, None);
        }

        ui.add_space(14.0);
        self.cue_list(ui);
    }

    fn cue_list(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Cues");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let go = egui::Button::new(RichText::new("GO ▶").strong()).fill(Color32::from_rgb(40, 110, 50));
                if ui.add_enabled(!self.project.cues.is_empty(), go).on_hover_text(t("Nästa cue (Enter)", "Next cue (Enter)")).clicked() {
                    self.go_next_cue(1);
                }
            });
        });
        if self.project.cues.is_empty() {
            ui.label(RichText::new(t("Ställ in ytorna och klicka + Spara cue.", "Set up the surfaces and click + Save cue.")).color(Color32::from_gray(140)));
        }
        let (mut go, mut remove) = (None, None);
        for (i, c) in self.project.cues.iter().enumerate() {
            ui.horizontal(|ui| {
                let current = self.current_cue == Some(c.id);
                let mut text = RichText::new(format!("{}. {}", i + 1, c.name));
                if current {
                    text = text.color(Color32::from_rgb(120, 220, 120)).strong();
                }
                let r = ui
                    .selectable_label(self.selected_cue == Some(c.id), text)
                    .on_hover_text(format!("{} {:.1} s  •  {}", t("Övergång", "Fade"), c.fade, t("dubbelklicka för GO", "double-click for GO")));
                if r.clicked() {
                    self.selected_cue = Some(c.id);
                }
                if r.double_clicked() {
                    go = Some(c.id);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("🗑").on_hover_text(t("Ta bort cue", "Delete cue")).clicked() {
                        remove = Some(c.id);
                    }
                    if ui.small_button("▶").on_hover_text(t("Gå till cuen", "Go to this cue")).clicked() {
                        go = Some(c.id);
                    }
                });
            });
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
                if ui
                    .button(t("⟳ Uppdatera", "⟳ Update"))
                    .on_hover_text(t("Spara nuvarande läge i den här cuen", "Store the current state in this cue"))
                    .clicked()
                {
                    c.surfaces = self.project.capture_cue("", 0.0).surfaces;
                }
                let index = self.project.cues.iter().position(|x| x.id == c.id).unwrap_or(0);
                let n = self.project.cues.len();
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

    pub fn osc_window(&mut self, ctx: &egui::Context) {
        let mut open = self.osc_window_open;
        let mut settings = self.project.settings.clone();
        egui::Window::new(t("OSC-fjärrstyrning", "OSC remote control")).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(&mut settings.osc_enabled, t("Ta emot OSC", "Receive OSC"));
            ui.horizontal(|ui| {
                ui.label(t("UDP-port", "UDP port"));
                ui.add(egui::DragValue::new(&mut settings.osc_port).range(1024..=65535));
            });
            match (&self.osc, &self.osc_error) {
                (Some(s), _) => ui.colored_label(Color32::from_rgb(120, 220, 120), format!("{} {}", t("Lyssnar på port", "Listening on port"), s.port())),
                (None, Some(e)) => ui.colored_label(Color32::from_rgb(255, 100, 100), e),
                _ => ui.label(t("Avstängd", "Off")),
            };
            if let Some((m, t)) = &self.osc_last {
                ui.label(RichText::new(format!("{} ({:.0} s): {m}", i18n::t("Senast", "Last"), t.elapsed().as_secs_f32())).color(Color32::from_gray(150)));
            }
            ui.add_space(8.0);
            ui.label(RichText::new(t("Adresser (mellanslag i namn skrivs som _):", "Addresses (write spaces in names as _):")).strong());
            let addresses = "/lumamap/cue/<nr>/go\n/lumamap/cue/next\n/lumamap/cue/prev\n\
                 /lumamap/surface/<name>/opacity  f\n/lumamap/surface/<name>/visible  i\n\
                 /lumamap/source/<name>/play\n/lumamap/source/<name>/pause\n/lumamap/source/<name>/seek  f\n\
                 /lumamap/master/opacity  f\n/lumamap/blackout  i";
            let addresses = addresses.replace("<name>", t("<namn>", "<name>"));
            ui.label(RichText::new(addresses).monospace());
        });
        self.osc_window_open = open;
        if settings != self.project.settings {
            self.exec(Command::ReplaceSettings(settings), None);
        }
    }

    pub fn midi_window(&mut self, ctx: &egui::Context) {
        let mut open = self.midi_window_open;
        let mut remove = None;
        egui::Window::new("MIDI").open(&mut open).resizable(false).show(ctx, |ui| {
            let devices = self.midi.devices();
            if devices.is_empty() {
                ui.label(RichText::new(t("Ingen MIDI-enhet hittades. Anslut en USB-kontroller.", "No MIDI device found. Connect a USB controller.")).color(Color32::from_gray(150)));
            } else {
                ui.label(format!("{}: {}", t("Anslutna", "Connected"), devices.join(", ")));
            }
            if let Some((e, at)) = &self.midi_last {
                let fresh = at.elapsed() < Duration::from_millis(400);
                let text = RichText::new(format!("{}: {} = {:.2}", t("Senast", "Last"), e.control.label(), e.value));
                ui.label(if fresh { text.color(ACCENT) } else { text.color(Color32::from_gray(150)) });
            }
            ui.add_space(8.0);
            ui.label(RichText::new(t("Kopplingar", "Mappings")).strong());
            if self.project.settings.midi.is_empty() {
                ui.label(RichText::new(t("Inga än.", "None yet.")).color(Color32::from_gray(150)));
            }
            egui::Grid::new("midi_bindings").num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
                for (i, b) in self.project.settings.midi.iter().enumerate() {
                    ui.label(self.midi_action_label(b.action));
                    ui.label(RichText::new(b.control.label()).monospace());
                    if ui.small_button("🗑").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            match self.midi_learn {
                Some(action) => {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} {}", t("Rör en kontroll för:", "Move a control for:"), self.midi_action_label(action)))
                                .color(Color32::from_rgb(255, 200, 80)),
                        );
                        if ui.button(t("Avbryt", "Cancel")).clicked() {
                            self.midi_learn = None;
                        }
                    });
                }
                None => {
                    ui.menu_button(t("➕ Ny koppling ⏷", "➕ New mapping ⏷"), |ui| {
                        let mut pick = None;
                        for action in [lm_core::MidiAction::Master, lm_core::MidiAction::Blackout, lm_core::MidiAction::CueNext, lm_core::MidiAction::CuePrev] {
                            if ui.button(self.midi_action_label(action)).clicked() {
                                pick = Some(action);
                            }
                        }
                        ui.menu_button("Cue ▸", |ui| {
                            for c in &self.project.cues {
                                if ui.button(format!("GO {}", c.name)).clicked() {
                                    pick = Some(lm_core::MidiAction::CueGo(c.id));
                                }
                            }
                        });
                        ui.menu_button(t("Yta ▸", "Surface ▸"), |ui| {
                            for s in &self.project.surfaces {
                                ui.menu_button(&s.name, |ui| {
                                    if ui.button(t("Opacitet (fader)", "Opacity (fader)")).clicked() {
                                        pick = Some(lm_core::MidiAction::SurfaceOpacity(s.id));
                                    }
                                    if ui.button(t("Synlig (knapp)", "Visible (button)")).clicked() {
                                        pick = Some(lm_core::MidiAction::SurfaceVisible(s.id));
                                    }
                                });
                            }
                        });
                        ui.menu_button(t("Video ▸", "Video ▸"), |ui| {
                            for s in self.project.sources.iter().filter(|s| matches!(s.kind, SourceKind::Video { .. })) {
                                ui.menu_button(&s.name, |ui| {
                                    if ui.button(t("Spela/paus (knapp)", "Play/pause (button)")).clicked() {
                                        pick = Some(lm_core::MidiAction::SourcePlayPause(s.id));
                                    }
                                    if ui.button(t("Hastighet (fader)", "Speed (fader)")).clicked() {
                                        pick = Some(lm_core::MidiAction::SourceSpeed(s.id));
                                    }
                                });
                            }
                        });
                        if pick.is_some() {
                            self.midi_learn = pick;
                            ui.close();
                        }
                    });
                }
            }
        });
        if !open {
            self.midi_learn = None;
        }
        self.midi_window_open = open;
        if let Some(i) = remove {
            let mut settings = self.project.settings.clone();
            settings.midi.remove(i);
            self.exec(Command::ReplaceSettings(settings), None);
        }
    }

    fn properties(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        let Some(mut s) = self.selected.and_then(|id| self.project.surface(id).cloned()) else {
            self.output_properties(ui);
            ui.add_space(14.0);
            ui.label(RichText::new(t("Markera en yta för att ändra den.", "Select a surface to edit it.")).color(Color32::from_gray(140)));
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
                .unwrap_or_else(|| t("— ingen —", "— none —").into());
            egui::ComboBox::from_id_salt("source_pick").selected_text(current).width(170.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.source, None, t("— ingen —", "— none —"));
                for src in &self.project.sources {
                    ui.selectable_value(&mut s.source, Some(src.id), &src.name);
                }
            });
            ui.end_row();

            if self.project.outputs.len() > 1 {
                ui.label(t("Utgång", "Output"));
                let current = self.project.output(s.output).map(|o| o.name.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("surface_output").selected_text(current).width(170.0).show_ui(ui, |ui| {
                    for o in &self.project.outputs {
                        ui.selectable_value(&mut s.output, o.id, &o.name);
                    }
                });
                ui.end_row();
            }

            ui.label(t("Form", "Shape"));
            ui.horizontal(|ui| {
                let mut shape = s.shape;
                let same = |a: &Shape, b: &Shape| std::mem::discriminant(a) == std::mem::discriminant(b);
                let current = shape_label(shape);
                egui::ComboBox::from_id_salt("shape_pick").selected_text(current).width(90.0).show_ui(ui, |ui| {
                    for option in SHAPES {
                        if ui.selectable_label(same(&option, &shape), shape_label(option)).on_hover_text(shape_hint(option)).clicked()
                            && !same(&option, &shape)
                        {
                            shape = option;
                        }
                    }
                });
                if let Shape::Mesh { cols, rows } = &mut shape {
                    ui.add(egui::DragValue::new(cols).range(MESH_MIN..=MESH_MAX).suffix(t(" kol", " col")))
                        .on_hover_text(t("Antal punkter på bredden", "Number of points across"));
                    ui.add(egui::DragValue::new(rows).range(MESH_MIN..=MESH_MAX).suffix(t(" rad", " row")))
                        .on_hover_text(t("Antal punkter på höjden", "Number of points down"));
                }
                if shape != s.shape {
                    reshape(&mut s, shape);
                    self.selected_point = None;
                }
            });
            ui.end_row();

            ui.label(t("Blandning", "Blending"));
            egui::ComboBox::from_id_salt("blend_pick").selected_text(s.blend.label()).width(170.0).show_ui(ui, |ui| {
                for mode in BlendMode::ALL {
                    ui.selectable_value(&mut s.blend, mode, mode.label()).on_hover_text(blend_hint(mode));
                }
            });
            ui.end_row();

            ui.label(t("Opacitet", "Opacity"));
            let r = ui.add(egui::Slider::new(&mut s.opacity, 0.0..=1.0).show_value(true));
            if r.changed() {
                gesture = Some(self.field_gesture(&r));
            }
            ui.end_row();

            ui.label("");
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.visible, t("Synlig", "Visible"));
                ui.checkbox(&mut s.locked, t("Låst", "Locked"));
            });
            ui.end_row();
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let mut test = self.opts.test_surfaces.contains(&s.id);
            if ui.toggle_value(&mut test, t("⊞ Testbild", "⊞ Test pattern")).on_hover_text(t("Visa testbild på ytan medan du riktar in den", "Show a test pattern on the surface while aligning it")).changed() {
                if test {
                    self.opts.test_surfaces.insert(s.id);
                } else {
                    self.opts.test_surfaces.remove(&s.id);
                }
            }
            if ui.button(t("⟲ Rak", "⟲ Straighten")).on_hover_text(t("Gör ytan till en rak rektangel igen", "Make the surface a straight rectangle again")).clicked() {
                let xs = s.dst_pts.iter().map(|p| p[0]);
                let ys = s.dst_pts.iter().map(|p| p[1]);
                let (x0, x1) = (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max));
                let (y0, y1) = (ys.clone().fold(f32::MAX, f32::min), ys.fold(f32::MIN, f32::max));
                s.dst_pts = shape_points(s.shape, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1]]);
            }
            if ui.button(t("⛶ Fyll", "⛶ Fill")).on_hover_text(t("Fyll hela projektorbilden", "Fill the whole projector image")).clicked() {
                s.dst_pts = shape_points(s.shape, &UNIT_QUAD);
            }
        });
        ui.horizontal(|ui| {
            if ui.button(t("🗐 Duplicera", "🗐 Duplicate")).clicked() {
                let mut copy = self.project.make_quad(s.source, s.output);
                copy.name = format!("{} {}", s.name, t("kopia", "copy"));
                copy.shape = s.shape;
                copy.mask = s.mask.clone();
                copy.blend = s.blend;
                copy.color = s.color;
                copy.effect = s.effect;
                copy.src_pts = s.src_pts.clone();
                copy.dst_pts = s.dst_pts.iter().map(|p| [p[0] + 0.03, p[1] + 0.03]).collect();
                copy.opacity = s.opacity;
                let index = self.project.surfaces.len();
                let id = copy.id;
                self.exec(Command::AddSurface { surface: copy, index }, None);
                self.select(Some(id));
                return;
            }
            if ui.button(t("🗑 Ta bort", "🗑 Delete")).clicked() {
                self.remove_selected();
            }
        });

        // Färg
        ui.add_space(8.0);
        egui::CollapsingHeader::new(t("🎨 Färg", "🎨 Colour"))
            .default_open(!s.color.is_identity())
            .show(ui, |ui| {
                egui::Grid::new("color_adjust").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                    let c = &mut s.color;
                    let rows: [(&str, &mut f32, std::ops::RangeInclusive<f32>); 5] = [
                        (t("Ljusstyrka", "Brightness"), &mut c.brightness, -1.0..=1.0),
                        (t("Kontrast", "Contrast"), &mut c.contrast, 0.0..=2.0),
                        (t("Gamma", "Gamma"), &mut c.gamma, 0.2..=3.0),
                        (t("Mättnad", "Saturation"), &mut c.saturation, 0.0..=2.0),
                        (t("Nyans", "Hue"), &mut c.hue, -180.0..=180.0),
                    ];
                    for (label, value, range) in rows {
                        ui.label(label);
                        let r = ui.add(egui::Slider::new(value, range).fixed_decimals(2));
                        if r.changed() {
                            gesture = Some(self.field_gesture(&r));
                        }
                        ui.end_row();
                    }
                });
                if ui.add_enabled(!s.color.is_identity(), egui::Button::new(t("⟲ Återställ färg", "⟲ Reset colour"))).clicked() {
                    s.color = lm_core::ColorAdjust::default();
                }
            });

        // Effekt
        egui::CollapsingHeader::new(t("✨ Effekt", "✨ Effect"))
            .default_open(s.effect.kind != lm_core::EffectKind::None)
            .show(ui, |ui| {
                let e = &mut s.effect;
                egui::ComboBox::from_id_salt("effect_pick").selected_text(e.kind.label()).width(170.0).show_ui(ui, |ui| {
                    for kind in lm_core::EffectKind::ALL {
                        ui.selectable_value(&mut e.kind, kind, kind.label()).on_hover_text(kind.hint());
                    }
                });
                if e.kind != lm_core::EffectKind::None {
                    egui::Grid::new("effect_params").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                        ui.label(t("Styrka", "Amount"));
                        let r = ui.add(egui::Slider::new(&mut e.amount, 0.0..=1.0).show_value(false));
                        if r.changed() {
                            gesture = Some(self.field_gesture(&r));
                        }
                        ui.end_row();
                        ui.label(t("Fart", "Speed"));
                        let r = ui
                            .add(egui::Slider::new(&mut e.speed, 0.0..=2.0).custom_formatter(|v, _| speed_text(v)))
                            .on_hover_text(t("I takt med tempot i verktygsraden", "In time with the tempo in the toolbar"));
                        if r.changed() {
                            gesture = Some(self.field_gesture(&r));
                        }
                        ui.end_row();
                    });
                }
            });

        // Mask
        ui.add_space(14.0);
        ui.heading("Mask");
        match &mut s.mask {
            None => {
                ui.label(RichText::new(t("Dölj delar av ytan, t.ex. ett fönster eller en dörr.", "Hide parts of the surface, e.g. a window or a door.")).color(Color32::from_gray(140)));
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
                        .toggle_value(&mut self.mask_edit, t("✏ Redigera mask", "✏ Edit mask"))
                        .on_hover_text(t("Dra maskens punkter i stället för ytans hörn", "Drag the mask's points instead of the surface corners"))
                        .changed()
                    {
                        self.selected_point = None;
                    }
                    ui.radio_value(&mut m.invert, true, t("Dölj inuti", "Hide inside"));
                    ui.radio_value(&mut m.invert, false, t("Visa inuti", "Show inside"));
                });
                ui.horizontal(|ui| {
                    ui.label(t("Mjuk kant", "Feather"));
                    let r = ui.add(egui::Slider::new(&mut m.feather, 0.0..=200.0).suffix(" px"));
                    if r.changed() {
                        gesture = Some(self.field_gesture(&r));
                    }
                });
                if self.mask_edit {
                    ui.label(
                        RichText::new(t("Dubbelklicka på en kant för ny punkt, högerklicka på en punkt för att ta bort den.", "Double-click an edge to add a point, right-click a point to remove it."))
                            .color(Color32::from_gray(140)),
                    );
                }
                if ui.button(t("🗑 Ta bort mask", "🗑 Remove mask")).clicked() {
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
            ui.heading(t("Utsnitt ur media", "Media crop"));
            ui.label(RichText::new(t("Dra hörnen för att välja vilken del som visas.", "Drag the corners to choose which part is shown.")).color(Color32::from_gray(140)));
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
            if ui.small_button(t("⟲ Hela bilden", "⟲ Whole image")).clicked() {
                self.set_points(s.id, PtKind::Src, UNIT_QUAD.to_vec(), None);
            }
            self.transport(ui, sid);
        }
    }

    /// Kamera och ström: status, paus, försök igen, ljud.
    fn live_controls(&mut self, ui: &mut Ui, src: &lm_core::Source) {
        let (title, muted) = match &src.kind {
            SourceKind::Camera { .. } => (t("Kamera", "Camera"), None),
            SourceKind::Stream { muted, .. } => (t("Ström", "Stream"), Some(*muted)),
            _ => return,
        };
        ui.add_space(14.0);
        ui.heading(title);
        match &src.kind {
            SourceKind::Camera { device } => ui.label(RichText::new(device).color(Color32::from_gray(150))),
            SourceKind::Stream { uri, .. } => ui.label(RichText::new(uri).color(Color32::from_gray(150))),
            _ => unreachable!(),
        };
        let Some(media) = self.media.get_mut(src.id) else { return };
        let mut restart = false;
        match media.error() {
            Some(e) => {
                ui.colored_label(Color32::from_rgb(255, 100, 100), e);
                restart = ui.button(t("⟳ Försök igen", "⟳ Try again")).clicked();
            }
            None => {
                let size = media.size().map_or(t("väntar på bild…", "waiting for picture…").to_string(), |s| format!("{} × {}", s[0], s[1]));
                ui.horizontal(|ui| {
                    let label = if media.is_playing() { t("⏸ Paus", "⏸ Pause") } else { t("▶ Spela", "▶ Play") };
                    if ui.button(label).clicked() {
                        if media.is_playing() {
                            media.pause();
                        } else {
                            media.play();
                        }
                    }
                    ui.label(RichText::new(size).color(Color32::from_gray(150)));
                });
            }
        }
        if restart {
            self.media.restart(src.id);
        }
        if let (Some(mut m), SourceKind::Stream { uri, .. }) = (muted, &src.kind) {
            if ui.checkbox(&mut m, t("🔇 Ljud av", "🔇 Mute")).changed() {
                let mut ns = src.clone();
                ns.kind = SourceKind::Stream { uri: uri.clone(), muted: m };
                self.exec(Command::ReplaceSource(ns), None);
            }
        }
    }

    fn transport(&mut self, ui: &mut Ui, sid: lm_core::SourceId) {
        let Some(src) = self.project.source(sid).cloned() else { return };
        if let SourceKind::Generator { pattern, colors, speed } = src.kind {
            ui.add_space(14.0);
            ui.heading(t("Mönster", "Pattern"));
            let (mut p, mut c, mut sp) = (pattern, colors, speed);
            let mut gesture = None;
            egui::Grid::new("pattern_props").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(t("Mönster", "Pattern"));
                egui::ComboBox::from_id_salt("pattern_pick").selected_text(p.label()).show_ui(ui, |ui| {
                    for option in lm_core::Pattern::ALL {
                        ui.selectable_value(&mut p, option, option.label());
                    }
                });
                ui.end_row();
                ui.label(t("Färger", "Colours"));
                ui.horizontal(|ui| {
                    for colour in &mut c {
                        let r = ui.color_edit_button_rgb(colour);
                        if r.changed() {
                            gesture = Some(self.field_gesture(&r));
                        }
                    }
                    if ui.small_button("⟲").on_hover_text(t("Mönstrets egna färger", "The pattern's own colours")).clicked() {
                        c = p.default_colors();
                    }
                });
                ui.end_row();
                ui.label(t("Fart", "Speed"));
                let r = ui.add(egui::Slider::new(&mut sp, 0.0..=2.0).custom_formatter(|v, _| speed_text(v)));
                if r.changed() {
                    gesture = Some(self.field_gesture(&r));
                }
                ui.end_row();
            });
            if p != pattern {
                // Nytt mönster får sina egna färger, om färgerna inte ändrats.
                if c == pattern.default_colors() {
                    c = p.default_colors();
                }
            }
            if (p, c, sp) != (pattern, colors, speed) {
                let mut ns = src.clone();
                ns.kind = SourceKind::Generator { pattern: p, colors: c, speed: sp };
                if ns.name == pattern.label() {
                    ns.name = p.label().to_string();
                }
                self.exec(Command::ReplaceSource(ns), gesture);
            }
            return;
        }
        if let SourceKind::Color { rgba } = src.kind {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.label(t("Färg", "Colour"));
                let mut c = egui::Rgba::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                let r = egui::color_picker::color_edit_button_rgba(ui, &mut c, egui::color_picker::Alpha::OnlyBlend);
                if r.changed() {
                    let gesture = self.field_gesture(&r);
                    let mut ns = src.clone();
                    let [red, green, blue, alpha] = c.to_rgba_unmultiplied();
                    ns.kind = SourceKind::Color { rgba: [red, green, blue, alpha] };
                    self.exec(Command::ReplaceSource(ns), Some(gesture));
                }
            });
            return;
        }
        if matches!(src.kind, SourceKind::Camera { .. } | SourceKind::Stream { .. }) {
            self.live_controls(ui, &src);
            return;
        }
        let SourceKind::Video { path, looping, muted, speed } = &src.kind else { return };
        ui.add_space(14.0);
        ui.heading(t("Uppspelning", "Playback"));
        let Some(media) = self.media.get_mut(sid) else { return };
        if let Some(e) = media.error() {
            ui.colored_label(Color32::from_rgb(255, 100, 100), e);
            return;
        }
        ui.horizontal(|ui| {
            let label = if media.is_playing() { t("⏸ Paus", "⏸ Pause") } else { t("▶ Spela", "▶ Play") };
            if ui.button(label).clicked() {
                if media.is_playing() {
                    media.pause();
                } else {
                    media.play();
                }
            }
            if ui.button(t("⏮ Början", "⏮ Start")).clicked() {
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
        let (mut l, mut m, mut sp) = (*looping, *muted, *speed);
        let mut changed = false;
        let mut gesture = None;
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut l, t("🔁 Loopa", "🔁 Loop")).changed();
            changed |= ui.checkbox(&mut m, t("🔇 Ljud av", "🔇 Mute")).changed();
        });
        ui.horizontal(|ui| {
            ui.label(t("Hastighet", "Speed"));
            let r = ui.add(
                egui::Slider::new(&mut sp, lm_core::SPEED_MIN..=lm_core::SPEED_MAX)
                    .logarithmic(true)
                    .fixed_decimals(2)
                    .suffix("×"),
            );
            if r.changed() {
                changed = true;
                gesture = Some(self.field_gesture(&r));
            }
            if (sp - 1.0).abs() > 1e-3 && ui.small_button("1×").clicked() {
                sp = 1.0;
                changed = true;
            }
        });
        if changed {
            let mut ns = src.clone();
            ns.kind = SourceKind::Video {
                path: path.clone(),
                looping: l,
                muted: m,
                speed: sp,
            };
            self.exec(Command::ReplaceSource(ns), gesture);
        }
    }

    fn output_properties(&mut self, ui: &mut Ui) {
        let Some(mut o) = self.project.output(self.current_output).cloned() else { return };
        let before = o.clone();
        ui.heading(t("Utgång", "Output"));
        let r = ui.add(egui::TextEdit::singleline(&mut o.name).font(egui::TextStyle::Heading));
        let mut gesture = (r.changed() || r.gained_focus()).then(|| self.field_gesture(&r));
        ui.label(RichText::new(format!("{} × {} px", o.resolution[0], o.resolution[1])).color(Color32::from_gray(140)));

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Keystone").strong());
            if ui
                .toggle_value(&mut self.keystone_edit, t("✏ Justera", "✏ Adjust"))
                .on_hover_text(t(
                    "Dra projektorbildens fyra hörn tills den är rak på väggen",
                    "Drag the four corners of the projector image until it is square on the wall",
                ))
                .changed()
            {
                self.selected_point = None;
                self.mask_edit = false;
            }
            if ui.add_enabled(o.keystone != UNIT_QUAD, egui::Button::new(t("⟲ Nollställ", "⟲ Reset"))).clicked() {
                o.keystone = UNIT_QUAD;
            }
        });

        ui.add_space(8.0);
        ui.label(RichText::new(t("Kantblandning", "Edge blending")).strong())
            .on_hover_text(t("Där två projektorer överlappar tonas bilden ut mot kanten så att överlappet inte blir dubbelt så ljust.", "Where two projectors overlap, the image fades out towards the edge so the overlap isn't twice as bright."));
        egui::Grid::new("edge_blend").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            let e = &mut o.edge_blend;
            for (label, w) in [(t("Vänster", "Left"), &mut e.left), (t("Höger", "Right"), &mut e.right), (t("Topp", "Top"), &mut e.top), (t("Botten", "Bottom"), &mut e.bottom)] {
                ui.label(label);
                let r = ui.add(
                    egui::Slider::new(w, 0.0..=EDGE_BLEND_MAX)
                        .custom_formatter(|v, _| format!("{:.0} %", v * 100.0))
                        .custom_parser(|t| t.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0)),
                );
                if r.changed() {
                    gesture = Some(self.field_gesture(&r));
                }
                ui.end_row();
            }
            ui.label("Gamma");
            let r = ui
                .add(egui::Slider::new(&mut e.gamma, 1.0..=3.0).fixed_decimals(1))
                .on_hover_text(t("Projektorns gamma, oftast 2.2. Justera om överlappet ser ljusare eller mörkare ut.", "The projector's gamma, usually 2.2. Adjust if the overlap looks brighter or darker."));
            if r.changed() {
                gesture = Some(self.field_gesture(&r));
            }
            ui.end_row();
            ui.label(t("Svartnivå", "Black level"));
            let r = ui
                .add(egui::Slider::new(&mut e.black_level, 0.0..=BLACK_LEVEL_MAX).fixed_decimals(3))
                .on_hover_text(t(
                    "Svart blir ljusare där två projektorer överlappar. Höj tills resten av bilden är lika grå som överlappet när allt är svart.",
                    "Black gets brighter where two projectors overlap. Raise until the rest of the image is as grey as the overlap when everything is black.",
                ));
            if r.changed() {
                gesture = Some(self.field_gesture(&r));
            }
            ui.end_row();
        });
        if o.edge_blend.is_active() {
            ui.label(
                RichText::new(t("Bredden ska vara lika stor som överlappet. Slå på ⊞ Testbild för att rikta in.", "The width should match the overlap. Turn on ⊞ Test pattern to align."))
                    .color(Color32::from_gray(140)),
            );
        }
        ui.add_space(6.0);
        let mut remove = false;
        ui.horizontal(|ui| {
            if ui.button(t("➕ Ny utgång", "➕ New output")).clicked() {
                self.add_output();
            }
            let many = self.project.outputs.len() > 1;
            remove = ui
                .add_enabled(many, egui::Button::new(t("🗑 Ta bort", "🗑 Delete")))
                .on_hover_text(t("Tar bort utgången och dess ytor", "Removes the output and its surfaces"))
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
            ui.label(RichText::new(t("Kom igång", "Getting started")).strong());
            ui.add_space(4.0);
            ui.label(t("1. Dra en video eller bild till fönstret.", "1. Drag a video or image into the window."));
            ui.label(t("2. Dra ytans fyra hörn så att de passar väggen.", "2. Drag the surface's four corners to fit the wall."));
            ui.label(t("3. Tryck ▶ Visa, och F i projektorfönstret för helskärm.", "3. Press ▶ Show, and F in the projector window for fullscreen."));
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
                if dragging_file { t("Släpp här", "Drop here") } else { t("Dra in en video eller bild hit", "Drag a video or image here") },
                egui::FontId::proportional(22.0),
                Color32::from_gray(if dragging_file { 230 } else { 110 }),
            );
        }
        let kind = self.edit_kind(out.id);
        self.canvas(ui, rect, kind, "editor");
    }
}

const SHAPES: [Shape; 4] = [Shape::Quad, Shape::Triangle, Shape::Ellipse, Shape::Mesh { cols: 4, rows: 4 }];

fn shape_label(shape: Shape) -> &'static str {
    match shape {
        Shape::Quad => t("Fyrhörn", "Quad"),
        Shape::Triangle => t("Triangel", "Triangle"),
        Shape::Ellipse => t("Ellips", "Ellipse"),
        Shape::Mesh { .. } => t("Mesh", "Mesh"),
    }
}

/// "1 varv / 4 slag" – effektens fart i förhållande till tempot.
fn speed_text(v: f64) -> String {
    if v < 0.01 {
        t("stilla", "still").into()
    } else if v < 0.99 {
        format!("1 {} / {:.0} {}", t("varv", "turn"), 1.0 / v, t("slag", "beats"))
    } else {
        format!("{:.1} {} / {}", v, t("varv", "turns"), t("slag", "beat"))
    }
}

fn shape_hint(shape: Shape) -> &'static str {
    match shape {
        Shape::Quad => t("Fyra hörn, perspektivriktig", "Four corners, perspective-correct"),
        Shape::Triangle => t("Tre hörn, t.ex. en gavel", "Three corners, e.g. a gable"),
        Shape::Ellipse => t("Rund yta inuti fyra hörn – blir en perspektivriktig ellips", "Round surface inside four corners – becomes a perspective-correct ellipse"),
        Shape::Mesh { .. } => t("Rutnät av punkter för böjda ytor, t.ex. pelare", "Grid of points for curved surfaces, e.g. pillars"),
    }
}

fn blend_hint(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => t("Ytan täcker det som ligger under", "The surface covers what is underneath"),
        BlendMode::Add => t("Ljuset adderas – svart blir genomskinligt", "Light is added – black becomes transparent"),
        BlendMode::Multiply => t("Mörkar ner det som ligger under – vitt blir genomskinligt", "Darkens what is underneath – white becomes transparent"),
        BlendMode::Screen => t("Ljusar upp mjukt – svart blir genomskinligt", "Brightens softly – black becomes transparent"),
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
        for shape in SHAPES {
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
