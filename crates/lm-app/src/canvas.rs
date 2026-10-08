//! Interaktiv mappningsvy: rita ytor och dra hörn med musen. Samma kod används
//! i editorns förhandsvisning, i projektorfönstret och i källans utsnittsvy.

use crate::app::LumaApp;
use eframe::egui::{self, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, Ui};
use lm_core::{Command, OutputId, Pt, SurfaceId};

/// Vilka punkter vyn redigerar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtKind {
    /// Var ytan hamnar på utgången. Vyn visar bara ytorna på den utgången.
    Dst(OutputId),
    /// Vilket utsnitt av källan som visas.
    Src,
}

pub struct Drag {
    surface: SurfaceId,
    kind: PtKind,
    salt: &'static str,
    point: Option<usize>,
    start_pts: Vec<Pt>,
    start: Pos2,
    gesture: u64,
}

pub const ACCENT: Color32 = Color32::from_rgb(0, 190, 255);
const HANDLE_HIT: f32 = 14.0;

pub fn to_screen(r: Rect, p: Pt) -> Pos2 {
    egui::pos2(r.min.x + p[0] * r.width(), r.min.y + p[1] * r.height())
}

/// Största rektangel med bildförhållandet `aspect` som ryms i `avail`, centrerad.
pub fn fit(avail: Rect, aspect: f32) -> Rect {
    let (w, h) = if avail.width() / avail.height() > aspect {
        (avail.height() * aspect, avail.height())
    } else {
        (avail.width(), avail.width() / aspect)
    };
    Rect::from_center_size(avail.center(), egui::vec2(w, h))
}

impl LumaApp {
    fn canvas_surfaces(&self, kind: PtKind) -> Vec<SurfaceId> {
        match kind {
            PtKind::Dst(out) => self.project.surfaces.iter().filter(|s| s.output == out).map(|s| s.id).collect(),
            PtKind::Src => self.selected.into_iter().collect(),
        }
    }

    fn points(&self, id: SurfaceId, kind: PtKind) -> Option<&[Pt]> {
        let s = self.project.surface(id)?;
        Some(match kind {
            PtKind::Dst(_) => &s.dst_pts,
            PtKind::Src => &s.src_pts,
        })
    }

    /// Hittar hörn (helst på markerad yta) eller annars översta yta under `pos`.
    fn hit(&self, rect: Rect, kind: PtKind, pos: Pos2) -> Option<(SurfaceId, Option<usize>)> {
        let ids = self.canvas_surfaces(kind);
        let screen = |id: SurfaceId| -> Vec<Pt> {
            self.points(id, kind)
                .unwrap_or_default()
                .iter()
                .map(|p| to_screen(rect, *p))
                .map(|p| [p.x, p.y])
                .collect()
        };
        let p = [pos.x, pos.y];
        let order = self
            .selected
            .filter(|s| ids.contains(s))
            .into_iter()
            .chain(ids.iter().rev().copied());
        for id in order.clone() {
            if let Some(i) = lm_geom::nearest_point(p, &screen(id), HANDLE_HIT) {
                return Some((id, Some(i)));
            }
        }
        for id in ids.iter().rev() {
            if lm_geom::point_in_polygon(p, &screen(*id)) {
                return Some((*id, None));
            }
        }
        None
    }

    /// Ritar ytornas konturer och handtag i `rect` och hanterar musen.
    /// `salt` skiljer vyerna åt (editor/projektor/källa).
    pub fn canvas(&mut self, ui: &mut Ui, rect: Rect, kind: PtKind, salt: &'static str) {
        let resp = ui.interact(rect, ui.id().with(("canvas", salt)), Sense::click_and_drag());
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());

        if resp.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).or(pointer);
            if let Some(origin) = origin {
                match self.hit(rect, kind, origin) {
                    Some((id, point)) => {
                        self.select(Some(id));
                        self.selected_point = point;
                        let locked = self.project.surface(id).is_some_and(|s| s.locked);
                        if !locked {
                            self.drag = Some(Drag {
                                surface: id,
                                kind,
                                salt,
                                point,
                                start_pts: self.points(id, kind).unwrap_or_default().to_vec(),
                                start: origin,
                                gesture: self.new_gesture(),
                            });
                        }
                    }
                    None if matches!(kind, PtKind::Dst(_)) => self.select(None),
                    None => {}
                }
            }
        }

        if resp.dragged() {
            if let (Some(d), Some(pos)) = (&self.drag, pointer) {
                if d.kind == kind && d.salt == salt {
                    let delta = (pos - d.start) / rect.size();
                    let mut pts = d.start_pts.clone();
                    match d.point {
                        Some(i) => {
                            pts[i][0] += delta.x;
                            pts[i][1] += delta.y;
                        }
                        None => {
                            for p in &mut pts {
                                p[0] += delta.x;
                                p[1] += delta.y;
                            }
                        }
                    }
                    let (id, g) = (d.surface, d.gesture);
                    self.set_points(id, kind, pts, Some(g));
                }
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }

        if resp.clicked() {
            match pointer.and_then(|p| self.hit(rect, kind, p)) {
                Some((id, point)) => {
                    self.select(Some(id));
                    self.selected_point = point;
                }
                None if matches!(kind, PtKind::Dst(_)) => self.select(None),
                None => {}
            }
        }

        if let Some(pos) = resp.hover_pos() {
            match self.hit(rect, kind, pos) {
                Some((_, Some(_))) => ui.ctx().set_cursor_icon(CursorIcon::Crosshair),
                Some((_, None)) => ui.ctx().set_cursor_icon(CursorIcon::Move),
                None => {}
            }
        }

        self.paint_overlay(ui, rect, kind);
    }

    fn paint_overlay(&self, ui: &Ui, rect: Rect, kind: PtKind) {
        let painter = ui.painter_at(rect.expand(8.0));
        for id in self.canvas_surfaces(kind) {
            let Some(s) = self.project.surface(id) else { continue };
            let pts: Vec<Pos2> = self.points(id, kind).unwrap_or_default().iter().map(|p| to_screen(rect, *p)).collect();
            let selected = self.selected == Some(id);
            let color = if !s.visible {
                Color32::from_gray(90)
            } else if selected {
                ACCENT
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 140)
            };
            if selected {
                painter.add(egui::Shape::convex_polygon(
                    pts.clone(),
                    Color32::from_rgba_unmultiplied(0, 190, 255, 18),
                    Stroke::NONE,
                ));
            }
            painter.add(egui::Shape::closed_line(pts.clone(), Stroke::new(if selected { 2.0 } else { 1.0 }, color)));

            if matches!(kind, PtKind::Dst(_)) {
                let c = lm_geom::centroid(self.points(id, kind).unwrap_or_default());
                let label = if s.source.is_none() { format!("{}\n(ingen media)", s.name) } else { s.name.clone() };
                painter.text(
                    to_screen(rect, c),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(14.0),
                    color,
                );
            }

            for (i, p) in pts.iter().enumerate() {
                if selected {
                    let active = self.selected_point == Some(i);
                    let r = if active { 8.0 } else { 6.0 };
                    let fill = if active { Color32::from_rgb(255, 170, 0) } else { Color32::WHITE };
                    painter.circle(*p, r, fill, Stroke::new(2.0, ACCENT));
                } else {
                    painter.circle_filled(*p, 3.5, color);
                }
            }
        }
    }

    pub fn set_points(&mut self, id: SurfaceId, kind: PtKind, pts: Vec<Pt>, gesture: Option<u64>) {
        let Some(mut s) = self.project.surface(id).cloned() else { return };
        match kind {
            PtKind::Dst(_) => s.dst_pts = pts,
            PtKind::Src => s.src_pts = pts,
        }
        self.exec(Command::ReplaceSurface(s), gesture);
    }
}
