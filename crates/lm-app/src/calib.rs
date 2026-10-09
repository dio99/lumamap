//! Inriktning med kamera: projektorn visar en ljuspunkt i taget, kameran
//! hittar dem, och sedan räcker det att klicka på väggens hörn i kamerabilden.

use crate::app::LumaApp;
use eframe::egui::{self, Color32, RichText};
use lm_core::i18n::t;
use lm_core::{Command, OutputId, Pt};
use lm_geom::calibrate::{calibration_points, find_dot, Gray};
use lm_geom::Homography;
use lm_media::{MediaSource, PixelFormat, VideoSource};
use std::time::{Duration, Instant};

/// Kamerabilder skalas ned till högst den här bredden (räcker för punkterna).
const MAX_WIDTH: usize = 640;
/// Tid för projektor och kamera att visa en ny bild (fördröjning, exponering).
const SETTLE: Duration = Duration::from_millis(800);
const WARMUP: Duration = Duration::from_millis(2000);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Step {
    Warmup,
    Dark,
    Dot(usize),
    Done,
    Failed,
}

struct GrayFrame {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl GrayFrame {
    fn gray(&self) -> Gray<'_> {
        Gray { width: self.width, height: self.height, data: &self.data }
    }
}

pub struct Calibration {
    pub output: OutputId,
    pub device: String,
    camera: VideoSource,
    pub step: Step,
    step_started: Instant,
    latest: Option<GrayFrame>,
    dark: Option<GrayFrame>,
    /// Hittade punkter: (projektor, kamera).
    found: Vec<(Pt, Pt)>,
    /// Kamera → utgångens koordinater.
    pub homography: Option<Homography>,
    /// Klick i kamerabilden (normaliserade).
    pub clicks: Vec<Pt>,
    texture: Option<egui::TextureHandle>,
    aspect: f32,
}

impl Calibration {
    pub fn start(output: OutputId, device: &str) -> Calibration {
        Calibration {
            output,
            device: device.to_string(),
            camera: VideoSource::camera(device),
            step: Step::Warmup,
            step_started: Instant::now(),
            latest: None,
            dark: None,
            found: Vec::new(),
            homography: None,
            clicks: Vec::new(),
            texture: None,
            aspect: 16.0 / 9.0,
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.camera.error()
    }

    /// Punkten som projektorn ska visa just nu (`Some(None)` = svart bild).
    pub fn pattern(&self) -> Option<Option<Pt>> {
        match self.step {
            Step::Dark => Some(None),
            Step::Dot(i) => Some(Some(calibration_points()[i])),
            _ => None,
        }
    }

    /// Hämtar kamerabilder och går vidare i stegen. Anropas varje bildruta.
    fn tick(&mut self, ctx: &egui::Context) {
        let mut frame = None;
        self.camera.poll(&mut |f| frame = Some(to_gray(&f)));
        if let Some(g) = frame {
            self.aspect = g.width as f32 / g.height.max(1) as f32;
            let rgb: Vec<u8> = g.data.iter().flat_map(|&v| [v, v, v]).collect();
            let image = egui::ColorImage::from_rgb([g.width, g.height], &rgb);
            match &mut self.texture {
                Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture("calibration", image, egui::TextureOptions::LINEAR)),
            }
            self.latest = Some(g);
        }
        if self.step_started.elapsed() < if self.step == Step::Warmup { WARMUP } else { SETTLE } {
            return;
        }
        let next = match self.step {
            Step::Warmup if self.latest.is_some() => Step::Dark,
            Step::Dark => {
                self.dark = self.latest.take();
                Step::Dot(0)
            }
            Step::Dot(i) => {
                if let (Some(dark), Some(lit)) = (&self.dark, &self.latest) {
                    if let Some(cam) = find_dot(&dark.gray(), &lit.gray()) {
                        self.found.push((calibration_points()[i], cam));
                    }
                }
                if i + 1 < calibration_points().len() {
                    Step::Dot(i + 1)
                } else {
                    let (proj, cam): (Vec<Pt>, Vec<Pt>) = self.found.iter().copied().unzip();
                    self.homography = Homography::fit(&cam, &proj).filter(|_| self.found.len() >= 6);
                    if self.homography.is_some() {
                        Step::Done
                    } else {
                        Step::Failed
                    }
                }
            }
            other => other,
        };
        if next != self.step {
            self.step = next;
            self.step_started = Instant::now();
        }
    }

    /// De fyra klicken som utgångens koordinater, i ordningen övre vänster,
    /// övre höger, nedre höger, nedre vänster (oavsett klickordning).
    pub fn corners(&self) -> Option<[Pt; 4]> {
        let h = self.homography?;
        if self.clicks.len() != 4 {
            return None;
        }
        let pts: Vec<Pt> = self.clicks.iter().map(|c| h.apply(*c)).collect::<Option<_>>()?;
        Some(order_corners(&pts))
    }
}

/// Sorterar fyra punkter till övre vänster, övre höger, nedre höger, nedre vänster.
pub fn order_corners(pts: &[Pt]) -> [Pt; 4] {
    let c = lm_geom::centroid(pts);
    let mut sorted = pts.to_vec();
    // Vinkel runt mitten, vriden ett halvt varv så att övre vänster får 45°,
    // övre höger 135°, nedre höger 225° och nedre vänster 315° (y nedåt).
    sorted.sort_by(|a, b| {
        let angle = |p: &Pt| ((p[1] - c[1]).atan2(p[0] - c[0]) + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU);
        angle(a).total_cmp(&angle(b))
    });
    [sorted[0], sorted[1], sorted[2], sorted[3]]
}

/// Gråskala från en kamerabild: YUV-bildernas Y-plan är redan gråskala.
fn to_gray(f: &lm_media::FrameView) -> GrayFrame {
    let step = (f.width as usize).div_ceil(MAX_WIDTH).max(1);
    let (w, h) = (f.width as usize / step, f.height as usize / step);
    let mut data = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = y * step * f.stride as usize;
        for x in 0..w {
            let v = match f.format {
                PixelFormat::Rgba => {
                    let i = row + x * step * 4;
                    let (r, g, b) = (f.data[i] as u32, f.data[i + 1] as u32, f.data[i + 2] as u32);
                    ((r * 54 + g * 183 + b * 19) >> 8) as u8
                }
                _ => f.data[row + x * step],
            };
            data.push(v);
        }
    }
    GrayFrame { width: w, height: h, data }
}

impl LumaApp {
    pub fn tick_calibration(&mut self, ctx: &egui::Context) {
        let Some(c) = &mut self.calibration else {
            self.opts.calibration = None;
            return;
        };
        c.tick(ctx);
        self.opts.calibration = c.pattern().map(|dot| (c.output, dot));
    }

    pub fn calibration_window(&mut self, ctx: &egui::Context) {
        let Some(output) = self.calibration_window_for else { return };
        let mut open = true;
        let mut action = None;
        egui::Window::new(t("📷 Rikta in med kamera", "📷 Align with camera")).open(&mut open).default_width(520.0).show(ctx, |ui| {
            match &self.calibration {
                None => {
                    ui.label(t(
                        "1. Ställ webbkameran så att den ser hela väggen som projektorn lyser på.",
                        "1. Place the webcam so it sees the whole wall the projector lights up.",
                    ));
                    ui.label(t(
                        "2. Klicka Starta. Projektorn visar nio punkter, en i taget (ca 10 s).",
                        "2. Click Start. The projector shows nine dots, one at a time (about 10 s).",
                    ));
                    ui.label(t(
                        "3. Klicka på väggens fyra hörn i kamerabilden – ytan hamnar där.",
                        "3. Click the wall's four corners in the camera picture – the surface lands there.",
                    ));
                    ui.add_space(8.0);
                    let cameras = self.cameras.get_or_insert_with(lm_media::list_cameras).clone();
                    if cameras.is_empty() {
                        ui.colored_label(Color32::from_rgb(255, 120, 100), t("Ingen kamera hittades.", "No camera found."));
                    }
                    ui.horizontal(|ui| {
                        let current = self.calibration_camera.clone().or_else(|| cameras.first().map(|c| c.device.clone()));
                        let mut pick = current.clone();
                        egui::ComboBox::from_id_salt("calib_camera")
                            .selected_text(current.clone().unwrap_or_default())
                            .show_ui(ui, |ui| {
                                for c in &cameras {
                                    ui.selectable_value(&mut pick, Some(c.device.clone()), format!("{} – {}", c.name, c.device));
                                }
                            });
                        self.calibration_camera = pick.clone();
                        if ui.add_enabled(pick.is_some(), egui::Button::new(t("▶ Starta", "▶ Start"))).clicked() {
                            action = Some(CalibAction::Start(pick.unwrap()));
                        }
                    });
                }
                Some(c) => {
                    if let Some(e) = c.error() {
                        ui.colored_label(Color32::from_rgb(255, 120, 100), e);
                    }
                    let status = match c.step {
                        Step::Warmup => t("Startar kameran …", "Starting the camera …").to_string(),
                        Step::Dark => t("Mäter mörkret …", "Measuring the dark …").to_string(),
                        Step::Dot(i) => format!("{} {} / {}", t("Punkt", "Dot"), i + 1, calibration_points().len()),
                        Step::Done => match c.clicks.len() {
                            4 => t("Klart! Skapa ytan eller klicka om.", "Done! Create the surface or click again.").to_string(),
                            n => format!("{} ({n}/4)", t("Klicka på väggens fyra hörn i kamerabilden", "Click the wall's four corners in the camera picture")),
                        },
                        Step::Failed => format!(
                            "{} ({} / {}). {}",
                            t("Kameran såg för få punkter", "The camera saw too few dots"),
                            c.found.len(),
                            calibration_points().len(),
                            t("Se till att kameran ser hela projektorbilden och att rummet är mörkt.", "Make sure the camera sees the whole projection and the room is dark.")
                        ),
                    };
                    ui.label(RichText::new(status).strong());
                    // Kamerabilden, med hittade punkter och klickade hörn.
                    if let Some(tex) = &c.texture {
                        let width = ui.available_width().min(640.0);
                        let size = egui::vec2(width, width / c.aspect);
                        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                        let painter = ui.painter_at(rect);
                        painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                        let at = |p: Pt| egui::pos2(rect.min.x + p[0] * rect.width(), rect.min.y + p[1] * rect.height());
                        for (_, cam) in &c.found {
                            painter.circle_stroke(at(*cam), 6.0, egui::Stroke::new(2.0, Color32::from_rgb(120, 220, 120)));
                        }
                        let clicks: Vec<egui::Pos2> = c.clicks.iter().map(|p| at(*p)).collect();
                        if clicks.len() == 4 {
                            let ordered = order_corners(&c.clicks).map(at);
                            painter.add(egui::Shape::closed_line(ordered.to_vec(), egui::Stroke::new(2.0, crate::canvas::ACCENT)));
                        }
                        for p in &clicks {
                            painter.circle(*p, 6.0, Color32::WHITE, egui::Stroke::new(2.0, crate::canvas::ACCENT));
                        }
                        if c.step == Step::Done && resp.clicked() {
                            if let Some(pos) = resp.interact_pointer_pos() {
                                let p = [(pos.x - rect.min.x) / rect.width(), (pos.y - rect.min.y) / rect.height()];
                                action = Some(CalibAction::Click(p));
                            }
                        }
                    }
                    ui.horizontal(|ui| {
                        if c.step == Step::Done && c.clicks.len() == 4 {
                            if ui.button(t("➕ Skapa yta här", "➕ Create surface here")).clicked() {
                                action = Some(CalibAction::Create);
                            }
                            let can_move = self.selected.and_then(|id| self.project.surface(id)).is_some_and(|s| s.output == c.output);
                            if can_move && ui.button(t("Flytta markerad yta hit", "Move selected surface here")).clicked() {
                                action = Some(CalibAction::MoveSelected);
                            }
                        }
                        if !c.clicks.is_empty() && ui.button(t("Klicka om", "Click again")).clicked() {
                            action = Some(CalibAction::ClearClicks);
                        }
                        if ui.button(t("⟳ Börja om", "⟳ Start over")).clicked() {
                            action = Some(CalibAction::Restart);
                        }
                    });
                }
            }
        });
        match action {
            Some(CalibAction::Start(device)) => self.calibration = Some(Calibration::start(output, &device)),
            Some(CalibAction::Restart) => {
                let device = self.calibration.as_ref().map(|c| c.device.clone());
                self.calibration = device.map(|d| Calibration::start(output, &d));
            }
            Some(CalibAction::Click(p)) => {
                if let Some(c) = &mut self.calibration {
                    if c.clicks.len() >= 4 {
                        c.clicks.clear();
                    }
                    c.clicks.push(p);
                }
            }
            Some(CalibAction::ClearClicks) => {
                if let Some(c) = &mut self.calibration {
                    c.clicks.clear();
                }
            }
            Some(CalibAction::Create) => {
                if let Some(corners) = self.calibration.as_ref().and_then(|c| c.corners()) {
                    self.select_output(output);
                    let id = self.add_surface(self.selected_source);
                    self.place_surface(id, corners);
                }
            }
            Some(CalibAction::MoveSelected) => {
                if let (Some(corners), Some(id)) = (self.calibration.as_ref().and_then(|c| c.corners()), self.selected) {
                    self.place_surface(id, corners);
                }
            }
            None => {}
        }
        if !open {
            self.calibration = None;
            self.calibration_window_for = None;
        }
    }

    /// Lägger ytan (vilken form som helst) i fyrhörningen `corners`.
    fn place_surface(&mut self, id: lm_core::SurfaceId, corners: [Pt; 4]) {
        let Some(mut s) = self.project.surface(id).cloned() else { return };
        s.dst_pts = crate::panels::shape_points(s.shape, &corners);
        self.exec(Command::ReplaceSurface(s), None);
        self.select(Some(id));
        self.notify(t("Ytan placerad efter kameran", "Surface placed from the camera"));
    }
}

enum CalibAction {
    Start(String),
    Restart,
    Click(Pt),
    ClearClicks,
    Create,
    MoveSelected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_ordered_whatever_the_click_order() {
        let tl = [0.1, 0.1];
        let tr = [0.9, 0.15];
        let br = [0.85, 0.9];
        let bl = [0.15, 0.85];
        assert_eq!(order_corners(&[br, tl, bl, tr]), [tl, tr, br, bl]);
        assert_eq!(order_corners(&[bl, br, tr, tl]), [tl, tr, br, bl]);
    }
}
