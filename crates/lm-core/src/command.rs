//! Alla ändringar av projektet går via `Command`. Varje kommando returnerar
//! sitt eget inverterade kommando när det appliceras, vilket ger ångra/gör om.

use crate::model::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    AddSource { source: Source, index: usize },
    RemoveSource(SourceId),
    ReplaceSource(Source),
    AddSurface { surface: Surface, index: usize },
    RemoveSurface(SurfaceId),
    /// Ersätter en yta helt (punkter, egenskaper). Används för alla redigeringar.
    ReplaceSurface(Surface),
    MoveSurfaceTo { id: SurfaceId, index: usize },
    ReplaceOutput(Output),
    /// Flera kommandon som ett ångra-steg.
    Batch(Vec<Command>),
}

impl Command {
    /// Applicerar kommandot och returnerar inversen. `None` om målet saknas.
    pub fn apply(self, p: &mut Project) -> Option<Command> {
        match self {
            Command::AddSource { source, index } => {
                let id = source.id;
                let index = index.min(p.sources.len());
                p.sources.insert(index, source);
                Some(Command::RemoveSource(id))
            }
            Command::RemoveSource(id) => {
                let index = p.source_index(id)?;
                let source = p.sources.remove(index);
                // Ytor som använde källan kopplas loss – och kopplas på igen vid ångra.
                let mut undo = vec![Command::AddSource { source, index }];
                for s in p.surfaces.iter_mut().filter(|s| s.source == Some(id)) {
                    undo.push(Command::ReplaceSurface(s.clone()));
                    s.source = None;
                }
                Some(batch(undo))
            }
            Command::ReplaceSource(new) => {
                let slot = p.source_mut(new.id)?;
                let old = std::mem::replace(slot, new);
                Some(Command::ReplaceSource(old))
            }
            Command::AddSurface { surface, index } => {
                let id = surface.id;
                let index = index.min(p.surfaces.len());
                p.surfaces.insert(index, surface);
                Some(Command::RemoveSurface(id))
            }
            Command::RemoveSurface(id) => {
                let index = p.surface_index(id)?;
                let surface = p.surfaces.remove(index);
                Some(Command::AddSurface { surface, index })
            }
            Command::ReplaceSurface(new) => {
                let slot = p.surface_mut(new.id)?;
                let old = std::mem::replace(slot, new);
                Some(Command::ReplaceSurface(old))
            }
            Command::MoveSurfaceTo { id, index } => {
                let from = p.surface_index(id)?;
                let s = p.surfaces.remove(from);
                let index = index.min(p.surfaces.len());
                p.surfaces.insert(index, s);
                Some(Command::MoveSurfaceTo { id, index: from })
            }
            Command::ReplaceOutput(new) => {
                let slot = p.outputs.iter_mut().find(|o| o.id == new.id)?;
                let old = std::mem::replace(slot, new);
                Some(Command::ReplaceOutput(old))
            }
            Command::Batch(cmds) => {
                let mut inv: Vec<Command> = cmds.into_iter().filter_map(|c| c.apply(p)).collect();
                inv.reverse();
                Some(Command::Batch(inv))
            }
        }
    }
}

fn batch(mut v: Vec<Command>) -> Command {
    if v.len() == 1 {
        v.pop().unwrap()
    } else {
        Command::Batch(v)
    }
}

/// Ångra/gör om-historik.
///
/// Kontinuerliga gester (t.ex. dra ett hörn) skickar ett kommando per
/// bildruta med samma `gesture`-nummer. Bara det första inversa kommandot
/// sparas, så hela draget blir ett enda ångra-steg.
#[derive(Default)]
pub struct History {
    undo: Vec<(Command, Option<u64>)>,
    redo: Vec<Command>,
    /// Ökar för varje ändring – används för "osparade ändringar" och autospara.
    revision: u64,
}

const MAX_UNDO: usize = 500;

impl History {
    pub fn exec(&mut self, p: &mut Project, cmd: Command, gesture: Option<u64>) {
        let Some(inv) = cmd.apply(p) else { return };
        self.revision += 1;
        self.redo.clear();
        if let (Some(g), Some((_, Some(last)))) = (gesture, self.undo.last()) {
            if g == *last {
                return;
            }
        }
        self.undo.push((inv, gesture));
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    pub fn undo(&mut self, p: &mut Project) -> bool {
        let Some((cmd, _)) = self.undo.pop() else { return false };
        if let Some(inv) = cmd.apply(p) {
            self.redo.push(inv);
        }
        self.revision += 1;
        true
    }

    pub fn redo(&mut self, p: &mut Project) -> bool {
        let Some(cmd) = self.redo.pop() else { return false };
        if let Some(inv) = cmd.apply(p) {
            self.undo.push((inv, None));
        }
        self.revision += 1;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(s: &Surface, dx: f32) -> Surface {
        let mut s = s.clone();
        for p in &mut s.dst_pts {
            p[0] += dx;
        }
        s
    }

    #[test]
    fn gesture_merges_into_one_undo_step() {
        let mut p = Project::new();
        let mut h = History::default();
        let s = p.make_quad(None);
        h.exec(&mut p, Command::AddSurface { surface: s.clone(), index: 0 }, None);
        let original = p.surfaces[0].clone();
        for i in 1..=10 {
            let next = moved(&original, i as f32 * 0.01);
            h.exec(&mut p, Command::ReplaceSurface(next), Some(7));
        }
        assert_ne!(p.surfaces[0], original);
        assert!(h.undo(&mut p));
        assert_eq!(p.surfaces[0], original);
        assert!(h.undo(&mut p));
        assert!(p.surfaces.is_empty());
        assert!(h.redo(&mut p));
        assert!(h.redo(&mut p));
        assert_eq!(p.surfaces[0], moved(&original, 0.1));
    }

    #[test]
    fn remove_source_detaches_and_restores() {
        let mut p = Project::new();
        let mut h = History::default();
        let src = p.make_source("c", SourceKind::Color { rgba: [1.0; 4] });
        let sid = src.id;
        h.exec(&mut p, Command::AddSource { source: src, index: 0 }, None);
        let s = p.make_quad(Some(sid));
        h.exec(&mut p, Command::AddSurface { surface: s, index: 0 }, None);
        h.exec(&mut p, Command::RemoveSource(sid), None);
        assert!(p.sources.is_empty());
        assert_eq!(p.surfaces[0].source, None);
        h.undo(&mut p);
        assert_eq!(p.sources.len(), 1);
        assert_eq!(p.surfaces[0].source, Some(sid));
    }

    #[test]
    fn reorder_undo() {
        let mut p = Project::new();
        let mut h = History::default();
        for _ in 0..3 {
            let s = p.make_quad(None);
            let n = p.surfaces.len();
            h.exec(&mut p, Command::AddSurface { surface: s, index: n }, None);
        }
        let ids: Vec<_> = p.surfaces.iter().map(|s| s.id).collect();
        h.exec(&mut p, Command::MoveSurfaceTo { id: ids[0], index: 2 }, None);
        assert_eq!(p.surfaces[2].id, ids[0]);
        h.undo(&mut p);
        let after: Vec<_> = p.surfaces.iter().map(|s| s.id).collect();
        assert_eq!(after, ids);
    }
}
