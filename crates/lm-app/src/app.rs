//! Programmets tillstånd, bildrutsloop, filhantering och projektorfönstret.

use crate::canvas::{Drag, PtKind};
use crate::pool::MediaPool;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, ViewportCommand, ViewportId};
use lm_core::{Command, History, OutputId, Project, SourceKind, SurfaceId};
use lm_render::RenderOptions;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const AUTOSAVE_EVERY: Duration = Duration::from_secs(30);

pub struct Startup {
    pub file: Option<PathBuf>,
    pub play: bool,
}

/// Något som kräver att osparade ändringar hanteras först.
#[derive(Clone)]
pub enum Pending {
    New,
    Open(Option<PathBuf>),
    Quit,
}

pub struct LumaApp {
    pub project: Project,
    pub history: History,
    pub path: Option<PathBuf>,
    saved_revision: u64,
    pub media: MediaPool,
    pub renderer: lm_render::Renderer,
    pub opts: RenderOptions,

    pub selected: Option<SurfaceId>,
    pub selected_point: Option<usize>,
    pub selected_source: Option<lm_core::SourceId>,
    pub drag: Option<Drag>,
    /// Vyerna redigerar markerad ytas mask i stället för ytorna.
    pub mask_edit: bool,
    next_gesture: u64,
    field_gesture: Option<(egui::Id, u64)>,

    /// Visa-läge: inga handtag på projektorn.
    pub show_mode: bool,
    /// Utgången som visas i editorns förhandsvisning och får nya ytor.
    pub current_output: OutputId,
    /// Projektorfönster som användaren har stängt.
    pub closed_outputs: HashSet<OutputId>,
    /// Position och helskärm som varje projektorfönster öppnades med.
    output_spawn: HashMap<OutputId, ([f32; 2], bool)>,

    pub status: Option<(String, Instant)>,
    pub pending: Option<Pending>,
    allow_close: bool,
    autosave_at: Instant,
    autosave_revision: u64,
    pub restore_offer: Option<PathBuf>,

    // Show
    pub current_cue: Option<lm_core::CueId>,
    pub selected_cue: Option<lm_core::CueId>,
    pub fade: Option<crate::show::Fade>,
    pub osc: Option<lm_control::OscServer>,
    pub osc_error: Option<String>,
    /// Porten som senast försöktes (så att en upptagen port inte provas varje bildruta).
    pub osc_port_tried: Option<u16>,
    pub osc_last: Option<(String, Instant)>,
    pub osc_gestures: HashMap<SurfaceId, (u64, Instant)>,
    pub osc_window_open: bool,
    pub editor_canvas: Option<egui::Rect>,
}

impl LumaApp {
    pub fn new(cc: &eframe::CreationContext, startup: Startup) -> Result<Self, String> {
        let rs = cc.wgpu_render_state.as_ref().ok_or("wgpu saknas")?;
        let renderer = lm_render::Renderer::new(&rs.device, &rs.queue, &mut rs.renderer.write());
        cc.egui_ctx.set_theme(egui::Theme::Dark);

        let project = Project::new();
        let mut app = LumaApp {
            current_output: project.outputs[0].id,
            project,
            history: History::default(),
            path: None,
            saved_revision: 0,
            media: MediaPool::default(),
            renderer,
            opts: RenderOptions::default(),
            selected: None,
            selected_point: None,
            selected_source: None,
            drag: None,
            mask_edit: false,
            next_gesture: 0,
            field_gesture: None,
            show_mode: false,
            closed_outputs: HashSet::new(),
            output_spawn: HashMap::new(),
            status: None,
            pending: None,
            allow_close: false,
            autosave_at: Instant::now(),
            autosave_revision: 0,
            restore_offer: None,
            current_cue: None,
            selected_cue: None,
            fade: None,
            osc: None,
            osc_error: None,
            osc_port_tried: None,
            osc_last: None,
            osc_gestures: HashMap::new(),
            osc_window_open: false,
            editor_canvas: None,
        };

        match startup.file {
            Some(f) => app.load(&f),
            None => {
                let auto = autosave_path();
                if auto.exists() {
                    app.restore_offer = Some(auto);
                }
            }
        }
        if startup.play {
            app.show_mode = true;
            for o in &mut app.project.outputs {
                o.fullscreen = true;
            }
        }
        Ok(app)
    }

    // ---------- Ändringar ----------

    pub fn exec(&mut self, cmd: Command, gesture: Option<u64>) {
        self.history.exec(&mut self.project, cmd, gesture);
    }

    /// Ångra. Avbryter en pågående cue-övergång så att den inte skriver över.
    pub fn undo(&mut self) {
        self.fade = None;
        self.history.undo(&mut self.project);
    }

    pub fn redo(&mut self) {
        self.fade = None;
        self.history.redo(&mut self.project);
    }

    pub fn new_gesture(&mut self) -> u64 {
        self.next_gesture += 1;
        self.next_gesture
    }

    /// Ger samma gest-nummer så länge samma widget redigeras kontinuerligt,
    /// så att t.ex. ett helt reglagedrag blir ett ångra-steg.
    pub fn field_gesture(&mut self, resp: &egui::Response) -> u64 {
        let fresh = resp.drag_started() || resp.clicked() || resp.gained_focus();
        match self.field_gesture {
            Some((id, g)) if id == resp.id && !fresh => g,
            _ => {
                let g = self.new_gesture();
                self.field_gesture = Some((resp.id, g));
                g
            }
        }
    }

    pub fn select(&mut self, id: Option<SurfaceId>) {
        if self.selected != id {
            self.selected_point = None;
            self.mask_edit = false;
        }
        self.selected = id;
        if let Some(s) = id.and_then(|id| self.project.surface(id)) {
            self.current_output = s.output;
            if let Some(src) = s.source {
                self.selected_source = Some(src);
            }
        }
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    pub fn dirty(&self) -> bool {
        self.history.revision() != self.saved_revision
    }

    pub fn add_surface(&mut self, source: Option<lm_core::SourceId>) -> SurfaceId {
        self.add_shaped(source, lm_core::Shape::Quad)
    }

    pub fn add_shaped(&mut self, source: Option<lm_core::SourceId>, shape: lm_core::Shape) -> SurfaceId {
        let mut s = self.project.make_quad(source, self.current_output);
        crate::panels::reshape(&mut s, shape);
        let id = s.id;
        let index = self.project.surfaces.len();
        self.exec(Command::AddSurface { surface: s, index }, None);
        self.select(Some(id));
        id
    }

    pub fn remove_selected(&mut self) {
        if let Some(id) = self.selected.take() {
            self.exec(Command::RemoveSurface(id), None);
            self.opts.test_surfaces.remove(&id);
        }
    }

    /// Lägger till mediefiler. Släpps de på en yta får ytan filen,
    /// annars skapas en ny yta per fil.
    pub fn add_files(&mut self, files: Vec<PathBuf>, target: Option<SurfaceId>) {
        let mut target = target;
        for path in files {
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let kind = match lm_media::classify(&path) {
                Some(lm_media::MediaKind::Image) => SourceKind::Image { path: path.clone() },
                Some(lm_media::MediaKind::Video) => SourceKind::Video {
                    path: path.clone(),
                    looping: true,
                    muted: false,
                },
                None if path.extension().is_some_and(|e| e == "lmap") => {
                    self.request(Pending::Open(Some(path)));
                    return;
                }
                None => {
                    self.notify(format!("Okänt filformat: {}", path.display()));
                    continue;
                }
            };
            let src = self.project.make_source(name, kind);
            let sid = src.id;
            let index = self.project.sources.len();
            self.exec(Command::AddSource { source: src, index }, None);
            self.selected_source = Some(sid);
            match target.take().and_then(|id| self.project.surface(id).cloned()) {
                Some(mut s) => {
                    s.source = Some(sid);
                    self.select(Some(s.id));
                    self.exec(Command::ReplaceSurface(s), None);
                }
                None => {
                    self.add_surface(Some(sid));
                }
            }
        }
    }

    pub fn select_output(&mut self, id: OutputId) {
        if self.current_output != id {
            self.current_output = id;
            if self.selected.and_then(|s| self.project.surface(s)).is_some_and(|s| s.output != id) {
                self.select(None);
            }
        }
    }

    pub fn add_output(&mut self) {
        let mut out = self.project.make_output();
        // Nytt fönster bredvid de andra så att de inte hamnar ovanpå varandra.
        let n = self.project.outputs.len() as f32;
        out.window_pos = Some([80.0 + 60.0 * n, 80.0 + 60.0 * n]);
        let id = out.id;
        let index = self.project.outputs.len();
        self.exec(Command::AddOutput { output: out, index }, None);
        self.select_output(id);
    }

    /// Tar bort utgången och dess ytor. Den sista utgången går inte att ta bort.
    pub fn remove_output(&mut self, id: OutputId) {
        if self.project.outputs.len() <= 1 {
            return;
        }
        self.exec(Command::RemoveOutput(id), None);
        if self.current_output == id {
            self.select_output(self.project.outputs[0].id);
        }
    }

    // ---------- Filer ----------

    pub fn request(&mut self, action: Pending) {
        if self.dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    pub fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => {
                self.replace_project(Project::new(), None);
                self.notify("Nytt projekt");
            }
            Pending::Open(path) => {
                let path = path.or_else(|| {
                    rfd::FileDialog::new()
                        .set_title("Öppna projekt")
                        .add_filter("LumaMap-projekt", &["lmap"])
                        .pick_file()
                });
                if let Some(p) = path {
                    self.load(&p);
                }
            }
            Pending::Quit => {
                remove_autosave();
                self.allow_close = true;
            }
        }
    }

    fn replace_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.current_output = project.outputs[0].id;
        self.closed_outputs.clear();
        self.output_spawn.clear();
        self.project = project;
        self.history.clear();
        self.saved_revision = self.history.revision();
        self.autosave_revision = self.saved_revision;
        self.path = path;
        self.selected = None;
        self.selected_point = None;
        self.selected_source = None;
        self.drag = None;
        self.mask_edit = false;
        self.current_cue = None;
        self.selected_cue = None;
        self.fade = None;
        self.opts = RenderOptions::default();
    }

    pub fn load(&mut self, path: &Path) {
        let result = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|s| Project::from_ron(&s).map_err(|e| e.to_string()));
        match result {
            Ok(mut p) => {
                if let Some(dir) = path.parent() {
                    p.absolutize_paths(dir);
                }
                let is_autosave = path == autosave_path();
                // Projektorfönstren öppnas igen där de låg när projektet sparades.
                self.replace_project(p, (!is_autosave).then(|| path.to_path_buf()));
                if is_autosave {
                    // Återställt projekt räknas som osparat.
                    self.saved_revision = u64::MAX;
                }
                self.notify(format!("Öppnade {}", path.display()));
            }
            Err(e) => self.notify(format!("Kunde inte öppna {}: {e}", path.display())),
        }
    }

    pub fn save(&mut self, save_as: bool) -> bool {
        let path = match (&self.path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new()
                .set_title("Spara projekt")
                .add_filter("LumaMap-projekt", &["lmap"])
                .set_file_name("projekt.lmap")
                .save_file()
                .map(|p| if p.extension().is_none() { p.with_extension("lmap") } else { p }),
        };
        let Some(path) = path else { return false };
        let mut p = self.project.clone();
        if let Some(dir) = path.parent() {
            p.relativize_paths(dir);
        }
        match p.to_ron().map_err(|e| e.to_string()).and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string())) {
            Ok(()) => {
                self.saved_revision = self.history.revision();
                self.notify(format!("Sparade {}", path.display()));
                self.path = Some(path);
                remove_autosave();
                true
            }
            Err(e) => {
                self.notify(format!("Kunde inte spara: {e}"));
                false
            }
        }
    }

    fn autosave(&mut self) {
        if self.autosave_at.elapsed() < AUTOSAVE_EVERY {
            return;
        }
        self.autosave_at = Instant::now();
        if !self.dirty() || self.autosave_revision == self.history.revision() {
            return;
        }
        self.autosave_revision = self.history.revision();
        let path = autosave_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = self.project.to_ron() {
            if let Err(e) = std::fs::write(&path, s) {
                log::warn!("Autospara misslyckades: {e}");
            }
        }
    }

    // ---------- Tangentbord ----------

    /// Kortkommandon. Anropas från både editorn och projektorfönstret.
    pub fn shortcuts(&mut self, ui: &mut egui::Ui) {
        let typing = ui.ctx().egui_wants_keyboard_input();
        let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        let cmd_shift = |k| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
        let (undo, redo, redo2, save, save_as, open, new) = ui.input_mut(|i| {
            (
                i.consume_shortcut(&cmd(Key::Z)),
                i.consume_shortcut(&cmd_shift(Key::Z)),
                i.consume_shortcut(&cmd(Key::Y)),
                i.consume_shortcut(&cmd(Key::S)),
                i.consume_shortcut(&cmd_shift(Key::S)),
                i.consume_shortcut(&cmd(Key::O)),
                i.consume_shortcut(&cmd(Key::N)),
            )
        });
        if undo && !typing {
            self.undo();
        }
        if (redo || redo2) && !typing {
            self.redo();
        }
        if save_as {
            self.save(true);
        } else if save {
            self.save(false);
        }
        if open {
            self.request(Pending::Open(None));
        }
        if new {
            self.request(Pending::New);
        }
        if typing {
            return;
        }

        let (tab, del, t, b, space, arrows, shift, next_pt) = ui.input(|i| {
            let mut d = [0.0f32; 2];
            if i.key_pressed(Key::ArrowLeft) {
                d[0] -= 1.0;
            }
            if i.key_pressed(Key::ArrowRight) {
                d[0] += 1.0;
            }
            if i.key_pressed(Key::ArrowUp) {
                d[1] -= 1.0;
            }
            if i.key_pressed(Key::ArrowDown) {
                d[1] += 1.0;
            }
            (
                i.key_pressed(Key::Tab),
                i.key_pressed(Key::Delete),
                i.key_pressed(Key::T),
                i.key_pressed(Key::B),
                i.key_pressed(Key::Space),
                d,
                i.modifiers.shift,
                i.key_pressed(Key::C),
            )
        });
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            self.go_next_cue(1);
        }
        if tab {
            self.show_mode = !self.show_mode;
        }
        if del {
            self.remove_selected();
        }
        if t {
            self.opts.output_test = !self.opts.output_test;
        }
        if b {
            self.opts.blackout = !self.opts.blackout;
        }
        if space {
            self.toggle_play_all();
        }
        if next_pt {
            // C = välj nästa hörn på markerad yta (eller dess mask).
            let n = self.selected.and_then(|id| self.project.surface(id)).map(|s| match (&s.mask, self.mask_edit) {
                (Some(m), true) => m.points.len(),
                _ => s.dst_pts.len(),
            });
            if let Some(n) = n {
                self.selected_point = Some(self.selected_point.map_or(0, |p| (p + 1) % n));
            }
        }
        if arrows != [0.0, 0.0] {
            self.nudge(arrows, if shift { 10.0 } else { 1.0 });
        }
    }

    /// Flyttar markerat hörn (eller hela ytan) med ett antal projektorpixlar.
    fn nudge(&mut self, dir: [f32; 2], px: f32) {
        let Some(id) = self.selected else { return };
        let Some(s) = self.project.surface(id) else { return };
        if s.locked {
            return;
        }
        let res = self.project.output(s.output).map(|o| o.resolution).unwrap_or([1920, 1080]);
        let d = [dir[0] * px / res[0] as f32, dir[1] * px / res[1] as f32];
        let kind = self.edit_kind(s.output);
        let mut pts = match (kind, &s.mask) {
            (PtKind::Mask(_), Some(m)) => m.points.clone(),
            _ => s.dst_pts.clone(),
        };
        for (i, p) in pts.iter_mut().enumerate() {
            if self.selected_point.is_none_or(|sp| sp == i) {
                p[0] += d[0];
                p[1] += d[1];
            }
        }
        self.set_points(id, kind, pts, None);
    }

    pub fn toggle_play_all(&mut self) {
        let mut any_playing = false;
        self.media.for_each_video(|m| any_playing |= m.is_playing());
        self.media.for_each_video(|m| if any_playing { m.pause() } else { m.play() });
    }

    // ---------- Projektorfönster ----------

    fn output_windows(&mut self, ctx: &egui::Context) {
        let ids: Vec<OutputId> = self.project.outputs.iter().map(|o| o.id).collect();
        self.output_spawn.retain(|id, _| ids.contains(id));
        for (n, id) in ids.into_iter().enumerate() {
            if !self.closed_outputs.contains(&id) {
                self.output_window(ctx, id, n);
            }
        }
    }

    fn output_window(&mut self, ctx: &egui::Context, id: OutputId, n: usize) {
        let Some(out) = self.project.output(id).cloned() else { return };
        let default_pos = [80.0 + 60.0 * n as f32, 80.0 + 60.0 * n as f32];
        let (pos, fullscreen) = *self
            .output_spawn
            .entry(id)
            .or_insert((out.window_pos.unwrap_or(default_pos), out.fullscreen));
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("LumaMap – {}", out.name))
            .with_inner_size([960.0, 540.0])
            .with_position(pos)
            .with_fullscreen(fullscreen);
        let vid = output_viewport(id);

        ctx.show_viewport_immediate(vid, builder, |ui, _class| {
            let (close, info, ppp) = ui.input(|i| (i.viewport().close_requested(), i.viewport().clone(), i.pixels_per_point));
            if close {
                self.close_output(id);
                return;
            }
            // Kom ihåg var fönstret ligger och anpassa upplösningen till fönstret.
            if let Some(o) = self.project.outputs.iter_mut().find(|o| o.id == id) {
                if let Some(r) = info.outer_rect {
                    o.window_pos = Some([r.min.x, r.min.y]);
                }
                if let Some(f) = info.fullscreen {
                    o.fullscreen = f;
                }
                if let Some(r) = info.inner_rect {
                    let res = [(r.width() * ppp).round() as u32, (r.height() * ppp).round() as u32];
                    if res[0] >= 16 && res[1] >= 16 {
                        o.resolution = res;
                    }
                }
            }
            if info.focused == Some(true) && ui.input(|i| i.pointer.any_pressed()) {
                self.select_output(id);
            }

            let (toggle_fs, esc, dbl) = ui.input(|i| {
                (
                    i.key_pressed(Key::F) || i.key_pressed(Key::F11),
                    i.key_pressed(Key::Escape),
                    i.pointer.button_double_clicked(egui::PointerButton::Primary),
                )
            });
            let is_fs = info.fullscreen.unwrap_or(false);
            if (toggle_fs && !ui.ctx().egui_wants_keyboard_input()) || (dbl && self.show_mode) {
                ui.ctx().send_viewport_cmd(ViewportCommand::Fullscreen(!is_fs));
            } else if esc && is_fs {
                ui.ctx().send_viewport_cmd(ViewportCommand::Fullscreen(false));
            }
            self.shortcuts(ui);

            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
                .show(ui, |ui| {
                    let rect = ui.max_rect();
                    if let Some(tex) = self.renderer.output_texture(id) {
                        ui.painter().image(
                            tex,
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    // Testbilden visar utgångens namn så att man ser vilken projektor som är vilken.
                    if self.opts.output_test && !self.opts.blackout {
                        let galley = ui.painter().layout_no_wrap(
                            out.name.clone(),
                            egui::FontId::proportional(rect.height() / 9.0),
                            egui::Color32::WHITE,
                        );
                        let at = rect.center() - galley.size() / 2.0 + egui::vec2(0.0, rect.height() / 4.0);
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(at, galley.size()).expand(rect.height() / 60.0),
                            8.0,
                            egui::Color32::from_black_alpha(200),
                        );
                        ui.painter().galley(at, galley, egui::Color32::WHITE);
                    }
                    if self.show_mode {
                        if ui.rect_contains_pointer(rect) {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                        }
                    } else {
                        let kind = self.edit_kind(id);
                        self.canvas(ui, rect, kind, "output");
                        if !is_fs {
                            ui.painter().text(
                                rect.left_bottom() + egui::vec2(10.0, -10.0),
                                egui::Align2::LEFT_BOTTOM,
                                "Dra fönstret till projektorn och tryck F för helskärm  •  Tab = Visa/Redigera",
                                egui::FontId::proportional(13.0),
                                egui::Color32::from_gray(170),
                            );
                        }
                    }
                });
        });
    }

    pub fn output_is_open(&self, id: OutputId) -> bool {
        !self.closed_outputs.contains(&id)
    }

    pub fn open_output(&mut self, id: OutputId) {
        self.closed_outputs.remove(&id);
        self.output_spawn.remove(&id);
    }

    fn close_output(&mut self, id: OutputId) {
        self.closed_outputs.insert(id);
        self.output_spawn.remove(&id);
    }

    pub fn set_output_fullscreen(&mut self, ctx: &egui::Context, id: OutputId, on: bool) {
        ctx.send_viewport_cmd_to(output_viewport(id), ViewportCommand::Fullscreen(on));
    }

    // ---------- Dialoger ----------

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending.clone() else { return };
        let mut choice = None;
        egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading("Osparade ändringar");
            ui.add_space(6.0);
            ui.label("Vill du spara projektet innan du fortsätter?");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("💾 Spara").clicked() {
                    choice = Some(0);
                }
                if ui.button("Släng ändringar").clicked() {
                    choice = Some(1);
                }
                if ui.button("Avbryt").clicked() {
                    choice = Some(2);
                }
            });
        });
        match choice {
            Some(0) => {
                if self.save(false) {
                    self.pending = None;
                    self.perform(pending.clone());
                    if matches!(pending, Pending::Quit) {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                }
            }
            Some(1) => {
                self.pending = None;
                self.perform(pending.clone());
                if matches!(pending, Pending::Quit) {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
            Some(_) => self.pending = None,
            None => {}
        }
    }
}

impl eframe::App for LumaApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Vald utgång kan ha försvunnit (t.ex. ångrad "Ny utgång").
        if self.project.output(self.current_output).is_none() {
            self.current_output = self.project.outputs[0].id;
        }

        // 1. Media → GPU, och rendera alla utgångar.
        if let Some(rs) = frame.wgpu_render_state() {
            let mut egui_renderer = rs.renderer.write();
            self.media.sync(&self.project, &mut self.renderer, &mut egui_renderer);
            self.media.poll(&rs.device, &rs.queue, &mut self.renderer, &mut egui_renderer);
            self.renderer.render(&rs.device, &rs.queue, &mut egui_renderer, &self.project, &self.opts);
        }

        // 2. Stäng programmet – fråga först om något är osparat.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.dirty() {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.pending = Some(Pending::Quit);
            } else {
                remove_autosave();
            }
        }

        // 3. Filer som släpps på fönstret.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() {
            let hover = ctx.input(|i| i.pointer.hover_pos());
            let target = self.surface_at(hover);
            self.add_files(dropped, target);
        }

        self.poll_osc();
        self.tick_fade();
        self.shortcuts(ui);
        self.editor_ui(ui);
        self.output_windows(&ctx);
        self.osc_window(&ctx);
        self.dialogs(&ctx);
        self.autosave();

        ctx.request_repaint();
    }
}

impl LumaApp {
    /// Ytan under en punkt i editorns förhandsvisning.
    fn surface_at(&self, pos: Option<egui::Pos2>) -> Option<SurfaceId> {
        let (pos, rect) = (pos?, self.editor_canvas?);
        if !rect.contains(pos) {
            return None;
        }
        let p = [(pos.x - rect.min.x) / rect.width(), (pos.y - rect.min.y) / rect.height()];
        self.project
            .surfaces
            .iter()
            .rev()
            .filter(|s| s.output == self.current_output)
            .find(|s| lm_geom::point_in_polygon(p, &s.dst_pts))
            .map(|s| s.id)
    }
}

fn output_viewport(id: OutputId) -> ViewportId {
    ViewportId::from_hash_of(("output", id))
}

pub fn autosave_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("lumamap").join("autosave.lmap")
}

fn remove_autosave() {
    let _ = std::fs::remove_file(autosave_path());
}
