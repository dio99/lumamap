//! Interaktiv mappningsvy: rita ytor och dra hörn med musen. Samma kod används
//! i editorns förhandsvisning, i projektorfönstret och i källans utsnittsvy.

use crate::app::LumaApp;
use eframe::egui::{self, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, Ui};
use lm_core::i18n::t;
use lm_core::{Command, OutputId, Pt, Shape, SurfaceId, MASK_MAX_POINTS, MASK_MIN_POINTS};

/// Vilka punkter vyn redigerar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtKind {
    /// Var ytan hamnar på utgången. Vyn visar bara ytorna på den utgången.
    Dst(OutputId),
    /// Vilket utsnitt av källan som visas.
    Src,
    /// Markerad ytas mask på utgången.
    Mask(OutputId),
    /// Utgångens keystone-hörn.
    Keystone(OutputId),
}

/// Pågående drag i ett keystone-hörn.
pub struct KeystoneDrag {
    output: OutputId,
    corner: usize,
    salt: &'static str,
    start_pts: [Pt; 4],
    start: Pos2,
    gesture: u64,
}

/// Hur normaliserade utgångskoordinater ritas i en vy: genom utgångens
/// keystone, så att handtagen hamnar där bilden faktiskt syns.
pub struct View {
    rect: Rect,
    warp: Option<lm_geom::Homography>,
    inverse: Option<lm_geom::Homography>,
}

impl View {
    pub fn pt(&self, p: Pt) -> Pos2 {
        let p = self.warp.and_then(|h| h.apply(p)).unwrap_or(p);
        to_screen(self.rect, p)
    }

    /// Skärmposition tillbaka till normaliserade (oförvrängda) koordinater.
    pub fn norm(&self, pos: Pos2) -> Pt {
        let p = [(pos.x - self.rect.min.x) / self.rect.width(), (pos.y - self.rect.min.y) / self.rect.height()];
        self.inverse.and_then(|h| h.apply(p)).unwrap_or(p)
    }
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
pub const MASK_COLOR: Color32 = Color32::from_rgb(255, 150, 40);
const HANDLE_HIT: f32 = 14.0;
/// Antal linjesegment mellan två kontrollpunkter när meshkurvor ritas.
const CURVE_STEPS: usize = 8;

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
            PtKind::Mask(out) => self
                .selected
                .and_then(|id| self.project.surface(id))
                .filter(|s| s.output == out && s.mask.is_some())
                .map(|s| s.id)
                .into_iter()
                .collect(),
            PtKind::Keystone(_) => Vec::new(),
        }
    }

    /// Vyn för `kind` i `rect`: utgångens vyer går genom dess keystone.
    pub fn view(&self, rect: Rect, kind: PtKind) -> View {
        let keystone = match kind {
            PtKind::Dst(out) | PtKind::Mask(out) => self.project.output(out).map(|o| o.keystone),
            _ => None,
        };
        let warp = keystone
            .filter(|k| *k != lm_core::UNIT_QUAD)
            .and_then(|k| lm_geom::Homography::from_points(&lm_core::UNIT_QUAD, &k));
        View {
            rect,
            inverse: warp.and_then(|h| h.inverse()),
            warp,
        }
    }

    /// Vad en vy av utgången `out` ska redigera: ytor, eller markerad ytas mask.
    pub fn edit_kind(&self, out: OutputId) -> PtKind {
        if self.keystone_edit {
            return PtKind::Keystone(out);
        }
        let has_mask = self.selected.and_then(|id| self.project.surface(id)).is_some_and(|s| s.mask.is_some());
        if self.mask_edit && has_mask {
            PtKind::Mask(out)
        } else {
            PtKind::Dst(out)
        }
    }

    fn points(&self, id: SurfaceId, kind: PtKind) -> Option<&[Pt]> {
        let s = self.project.surface(id)?;
        Some(match kind {
            PtKind::Dst(_) => &s.dst_pts,
            PtKind::Src => &s.src_pts,
            PtKind::Mask(_) => &s.mask.as_ref()?.points,
            PtKind::Keystone(_) => return None,
        })
    }

    /// Meshens storlek om vyn visar ytans mesh, annars `None` (rak polygon).
    fn mesh_size(&self, id: SurfaceId, kind: PtKind) -> Option<(usize, usize)> {
        match (kind, self.project.surface(id)?.shape) {
            (PtKind::Dst(_), Shape::Mesh { cols, rows }) => Some((cols as usize, rows as usize)),
            _ => None,
        }
    }

    /// Om vyn visar ytan som ellips (hörnen är då en ram runt ellipsen).
    fn is_ellipse(&self, id: SurfaceId, kind: PtKind) -> bool {
        matches!(kind, PtKind::Dst(_)) && self.project.surface(id).is_some_and(|s| s.shape == Shape::Ellipse)
    }

    /// Ytans kontur (normaliserad). För mesh och ellips följer den kurvorna.
    fn outline(&self, id: SurfaceId, kind: PtKind) -> Vec<Pt> {
        let pts = self.points(id, kind).unwrap_or_default();
        if let Some((c, r)) = self.mesh_size(id, kind) {
            return lm_geom::mesh_outline(pts, c, r, CURVE_STEPS);
        }
        match lm_geom::quad(pts) {
            Some(q) if self.is_ellipse(id, kind) => lm_geom::ellipse_outline(&q, 64),
            _ => pts.to_vec(),
        }
    }

    /// Hittar hörn (helst på markerad yta) eller annars översta yta under `pos`.
    fn hit(&self, rect: Rect, kind: PtKind, pos: Pos2) -> Option<(SurfaceId, Option<usize>)> {
        let ids = self.canvas_surfaces(kind);
        let v = self.view(rect, kind);
        let screen = |id: SurfaceId| -> Vec<Pt> {
            self.points(id, kind)
                .unwrap_or_default()
                .iter()
                .map(|p| v.pt(*p))
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
            let outline: Vec<Pt> = self.outline(*id, kind).iter().map(|q| v.pt(*q)).map(|q| [q.x, q.y]).collect();
            if lm_geom::point_in_polygon(p, &outline) {
                return Some((*id, None));
            }
        }
        None
    }

    /// Ritar ytornas konturer och handtag i `rect` och hanterar musen.
    /// `salt` skiljer vyerna åt (editor/projektor/källa).
    pub fn canvas(&mut self, ui: &mut Ui, rect: Rect, kind: PtKind, salt: &'static str) {
        if let PtKind::Keystone(out) = kind {
            self.keystone_canvas(ui, rect, out, salt);
            return;
        }
        let v = self.view(rect, kind);
        let resp = ui.interact(rect, ui.id().with(("canvas", salt)), Sense::click_and_drag());
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());

        if resp.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).or(pointer);
            if let Some(origin) = origin {
                match self.hit(rect, kind, origin) {
                    Some((id, point)) => {
                        self.select(Some(id));
                        self.selected_point = point.filter(|_| kind != PtKind::Src);
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
                    // Förflyttningen räknas i oförvrängda koordinater.
                    let (now, then) = (v.norm(pos), v.norm(d.start));
                    let delta = [now[0] - then[0], now[1] - then[1]];
                    let mut pts = d.start_pts.clone();
                    match d.point {
                        Some(i) => {
                            pts[i][0] += delta[0];
                            pts[i][1] += delta[1];
                        }
                        None => {
                            for p in &mut pts {
                                p[0] += delta[0];
                                p[1] += delta[1];
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

        if let (PtKind::Mask(_), Some(pos)) = (kind, pointer) {
            self.mask_click(&resp, rect, pos);
        }

        if resp.clicked() {
            match pointer.and_then(|p| self.hit(rect, kind, p)) {
                Some((id, point)) => {
                    self.select(Some(id));
                    self.selected_point = point.filter(|_| kind != PtKind::Src);
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
        let v = self.view(rect, kind);
        // Kantblandningens zoner, så att man ser var överlappet ska ligga.
        if let Some(e) = match kind {
            PtKind::Dst(out) => self.project.output(out).map(|o| o.edge_blend),
            _ => None,
        } {
            let stroke = Stroke::new(1.0, Color32::from_rgb(255, 220, 80));
            let dash = |a: Pos2, b: Pos2| painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 6.0, 4.0));
            let (x0, x1) = (rect.min.x + e.left * rect.width(), rect.max.x - e.right * rect.width());
            let (y0, y1) = (rect.min.y + e.top * rect.height(), rect.max.y - e.bottom * rect.height());
            if e.left > 0.0 {
                dash(egui::pos2(x0, rect.min.y), egui::pos2(x0, rect.max.y));
            }
            if e.right > 0.0 {
                dash(egui::pos2(x1, rect.min.y), egui::pos2(x1, rect.max.y));
            }
            if e.top > 0.0 {
                dash(egui::pos2(rect.min.x, y0), egui::pos2(rect.max.x, y0));
            }
            if e.bottom > 0.0 {
                dash(egui::pos2(rect.min.x, y1), egui::pos2(rect.max.x, y1));
            }
        }
        for id in self.canvas_surfaces(kind) {
            let Some(s) = self.project.surface(id) else { continue };
            let pts: Vec<Pos2> = self.points(id, kind).unwrap_or_default().iter().map(|p| v.pt(*p)).collect();
            let outline: Vec<Pos2> = self.outline(id, kind).iter().map(|p| v.pt(*p)).collect();
            let mesh = self.mesh_size(id, kind);
            let selected = self.selected == Some(id);
            let color = if matches!(kind, PtKind::Mask(_)) {
                MASK_COLOR
            } else if !s.visible {
                Color32::from_gray(90)
            } else if selected {
                ACCENT
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 140)
            };
            if selected && matches!(kind, PtKind::Dst(_)) {
                if let Some(m) = &s.mask {
                    let pts = m.points.iter().map(|p| v.pt(*p)).collect();
                    painter.add(egui::Shape::closed_line(pts, Stroke::new(1.0, MASK_COLOR.gamma_multiply(0.7))));
                }
            }
            let ellipse = self.is_ellipse(id, kind);
            if selected && ellipse {
                // Ramen som ellipsen är inskriven i.
                painter.add(egui::Shape::closed_line(pts.clone(), Stroke::new(1.0, color.gamma_multiply(0.4))));
            }
            if selected && mesh.is_none() && !ellipse && !matches!(kind, PtKind::Mask(_)) {
                painter.add(egui::Shape::convex_polygon(
                    pts.clone(),
                    Color32::from_rgba_unmultiplied(0, 190, 255, 18),
                    Stroke::NONE,
                ));
            }
            // Meshens inre rutnät som tunna kurvor.
            if let (Some((c, r)), true) = (mesh, selected) {
                let ctrl = self.points(id, kind).unwrap_or_default();
                let thin = Stroke::new(1.0, color.gamma_multiply(0.5));
                let n = (c - 1) * CURVE_STEPS + 1;
                let along = lm_geom::mesh_grid(ctrl, c, r, n, r);
                for row in 1..r - 1 {
                    let line = along[row * n..(row + 1) * n].iter().map(|p| v.pt(*p)).collect();
                    painter.add(egui::Shape::line(line, thin));
                }
                let n = (r - 1) * CURVE_STEPS + 1;
                let down = lm_geom::mesh_grid(ctrl, c, r, c, n);
                for col in 1..c - 1 {
                    let line = (0..n).map(|j| v.pt(down[j * c + col])).collect();
                    painter.add(egui::Shape::line(line, thin));
                }
            }
            painter.add(egui::Shape::closed_line(outline, Stroke::new(if selected { 2.0 } else { 1.0 }, color)));

            if matches!(kind, PtKind::Dst(_)) {
                let c = lm_geom::centroid(self.points(id, kind).unwrap_or_default());
                let label = if s.source.is_none() { format!("{}\n({})", s.name, t("ingen media", "no media")) } else { s.name.clone() };
                painter.text(
                    v.pt(c),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(14.0),
                    color,
                );
            }

            for (i, p) in pts.iter().enumerate() {
                if selected {
                    // Valt hörn gäller ytans eller maskens punkter, aldrig utsnittet.
                    let active = kind != PtKind::Src && self.selected_point == Some(i);
                    let r = match (active, mesh.is_some()) {
                        (true, _) => 8.0,
                        (false, true) => 5.0,
                        (false, false) => 6.0,
                    };
                    let fill = if active { Color32::from_rgb(255, 170, 0) } else { Color32::WHITE };
                    painter.circle(*p, r, fill, Stroke::new(2.0, if matches!(kind, PtKind::Mask(_)) { MASK_COLOR } else { ACCENT }));
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
            PtKind::Mask(_) => match &mut s.mask {
                Some(m) => m.points = pts,
                None => return,
            },
            PtKind::Keystone(_) => return,
        }
        self.exec(Command::ReplaceSurface(s), gesture);
    }

    /// Maskredigering: dubbelklick på en kant lägger till en punkt,
    /// högerklick på en punkt tar bort den.
    fn mask_click(&mut self, resp: &egui::Response, rect: Rect, pos: Pos2) {
        let Some(id) = self.selected else { return };
        let Some(s) = self.project.surface(id).cloned() else { return };
        let (Some(mut m), false) = (s.mask.clone(), s.locked) else { return };
        let v = self.view(rect, PtKind::Mask(s.output));
        let screen: Vec<Pt> = m.points.iter().map(|p| v.pt(*p)).map(|p| [p.x, p.y]).collect();
        let p = [pos.x, pos.y];
        if resp.double_clicked() && m.points.len() < MASK_MAX_POINTS {
            if let Some((i, q)) = lm_geom::nearest_edge(p, &screen, HANDLE_HIT) {
                m.points.insert(i, v.norm(egui::pos2(q[0], q[1])));
                self.selected_point = Some(i);
            }
        } else if resp.secondary_clicked() && m.points.len() > MASK_MIN_POINTS {
            if let Some(i) = lm_geom::nearest_point(p, &screen, HANDLE_HIT) {
                m.points.remove(i);
                self.selected_point = None;
            }
        }
        if Some(&m) != s.mask.as_ref() {
            let mut ns = s;
            ns.mask = Some(m);
            self.exec(Command::ReplaceSurface(ns), None);
        }
    }

    /// Keystone-läget: utgångens fyra hörn som handtag. Hela projektorbilden
    /// följer med när de dras.
    fn keystone_canvas(&mut self, ui: &mut Ui, rect: Rect, out: OutputId, salt: &'static str) {
        let Some(o) = self.project.output(out).cloned() else { return };
        let resp = ui.interact(rect, ui.id().with(("keystone", salt)), Sense::click_and_drag());
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        let screen: Vec<Pt> = o.keystone.iter().map(|p| to_screen(rect, *p)).map(|p| [p.x, p.y]).collect();
        let hit = |pos: Pos2| lm_geom::nearest_point([pos.x, pos.y], &screen, HANDLE_HIT * 1.5);

        if resp.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).or(pointer);
            if let Some((origin, corner)) = origin.and_then(|p| hit(p).map(|c| (p, c))) {
                self.selected_point = Some(corner);
                self.keystone_drag = Some(KeystoneDrag {
                    output: out,
                    corner,
                    salt,
                    start_pts: o.keystone,
                    start: origin,
                    gesture: self.new_gesture(),
                });
            }
        }
        if resp.dragged() {
            if let (Some(d), Some(pos)) = (&self.keystone_drag, pointer) {
                if d.output == out && d.salt == salt {
                    let delta = (pos - d.start) / rect.size();
                    let mut o = o.clone();
                    o.keystone = d.start_pts;
                    o.keystone[d.corner][0] += delta.x;
                    o.keystone[d.corner][1] += delta.y;
                    let g = d.gesture;
                    self.exec(Command::ReplaceOutput(o), Some(g));
                }
            }
        }
        if resp.drag_stopped() {
            self.keystone_drag = None;
        }
        if resp.clicked() {
            self.selected_point = pointer.and_then(hit);
        }
        if resp.hover_pos().and_then(hit).is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }

        let painter = ui.painter_at(rect.expand(10.0));
        let colour = Color32::from_rgb(255, 220, 80);
        let pts: Vec<Pos2> = screen.iter().map(|p| egui::pos2(p[0], p[1])).collect();
        painter.add(egui::Shape::closed_line(pts.clone(), Stroke::new(2.0, colour)));
        // Diagonaler och mittkors gör det lättare att se om bilden är rak.
        painter.line_segment([pts[0], pts[2]], Stroke::new(1.0, colour.gamma_multiply(0.4)));
        painter.line_segment([pts[1], pts[3]], Stroke::new(1.0, colour.gamma_multiply(0.4)));
        for (i, p) in pts.iter().enumerate() {
            let active = self.selected_point == Some(i);
            let fill = if active { Color32::from_rgb(255, 170, 0) } else { Color32::WHITE };
            painter.circle(*p, if active { 10.0 } else { 8.0 }, fill, Stroke::new(2.0, colour));
        }
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            t("Keystone: dra hörnen tills bilden är rak på väggen", "Keystone: drag the corners until the image is square on the wall"),
            egui::FontId::proportional(16.0),
            colour,
        );
    }
}
