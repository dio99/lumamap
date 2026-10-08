//! Projektets datamodell. Alla koordinater är normaliserade (0..1, y nedåt)
//! så att ett projekt överlever byte av upplösning eller projektor.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PROJECT_VERSION: u32 = 1;

pub type Pt = [f32; 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SurfaceId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OutputId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub sources: Vec<Source>,
    /// Ordning = lagerordning (första ritas längst bak).
    pub surfaces: Vec<Surface>,
    pub outputs: Vec<Output>,
    next_id: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub name: String,
    pub kind: SourceKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceKind {
    Video {
        path: PathBuf,
        #[serde(default = "yes")]
        looping: bool,
        #[serde(default)]
        muted: bool,
    },
    Image {
        path: PathBuf,
    },
    Color {
        rgba: [f32; 4],
    },
    TestPattern,
}

fn yes() -> bool {
    true
}

impl SourceKind {
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            SourceKind::Video { path, .. } | SourceKind::Image { path } => Some(path),
            _ => None,
        }
    }

    pub fn path_mut(&mut self) -> Option<&mut PathBuf> {
        match self {
            SourceKind::Video { path, .. } | SourceKind::Image { path } => Some(path),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Shape {
    /// Fyra hörn, perspektivriktig (homografi).
    #[default]
    Quad,
    /// Rutnät med `cols × rows` kontrollpunkter (radvis) för böjda ytor.
    Mesh { cols: u32, rows: u32 },
}

pub const MESH_MIN: u32 = 2;
pub const MESH_MAX: u32 = 16;

impl Shape {
    /// Antal punkter i `dst_pts` för formen.
    pub fn point_count(&self) -> usize {
        match *self {
            Shape::Quad => 4,
            Shape::Mesh { cols, rows } => (cols * rows) as usize,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    pub id: SurfaceId,
    pub name: String,
    pub source: Option<SourceId>,
    pub output: OutputId,
    #[serde(default)]
    pub shape: Shape,
    /// Utsnitt ur källan (ordning: övre vänster, övre höger, nedre höger, nedre vänster).
    pub src_pts: Vec<Pt>,
    /// Placering på utgången. Fyrhörn: samma ordning som `src_pts`.
    /// Mesh: `cols × rows` punkter radvis från övre vänster.
    pub dst_pts: Vec<Pt>,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub mask: Option<Mask>,
}

/// Polygonmask i utgångens koordinater – ligger kvar på väggen när ytan justeras.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub points: Vec<Pt>,
    /// Mjuk kant i projektorpixlar.
    #[serde(default)]
    pub feather: f32,
    /// `false` = visa bara inuti, `true` = dölj inuti (hål).
    #[serde(default)]
    pub invert: bool,
}

pub const MASK_MIN_POINTS: usize = 3;
pub const MASK_MAX_POINTS: usize = 32;

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub id: OutputId,
    pub name: String,
    pub resolution: [u32; 2],
    /// Var utgångsfönstret låg senast (skärmkoordinater), så rätt projektor hittas igen.
    #[serde(default)]
    pub window_pos: Option<[f32; 2]>,
    #[serde(default)]
    pub fullscreen: bool,
}

pub const UNIT_QUAD: [Pt; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Project {
    pub fn new() -> Self {
        let mut p = Project {
            version: PROJECT_VERSION,
            sources: Vec::new(),
            surfaces: Vec::new(),
            outputs: Vec::new(),
            next_id: 1,
        };
        let out = p.make_output();
        p.outputs.push(out);
        p
    }

    /// Skapar en ny utgång (läggs inte till – använd `Command::AddOutput`).
    pub fn make_output(&mut self) -> Output {
        Output {
            id: OutputId(self.alloc_id()),
            name: format!("Projektor {}", self.outputs.len() + 1),
            resolution: [1920, 1080],
            window_pos: None,
            fullscreen: false,
        }
    }

    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn source(&self, id: SourceId) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }

    pub fn source_mut(&mut self, id: SourceId) -> Option<&mut Source> {
        self.sources.iter_mut().find(|s| s.id == id)
    }

    pub fn surface(&self, id: SurfaceId) -> Option<&Surface> {
        self.surfaces.iter().find(|s| s.id == id)
    }

    pub fn surface_mut(&mut self, id: SurfaceId) -> Option<&mut Surface> {
        self.surfaces.iter_mut().find(|s| s.id == id)
    }

    pub fn surface_index(&self, id: SurfaceId) -> Option<usize> {
        self.surfaces.iter().position(|s| s.id == id)
    }

    pub fn source_index(&self, id: SourceId) -> Option<usize> {
        self.sources.iter().position(|s| s.id == id)
    }

    pub fn output(&self, id: OutputId) -> Option<&Output> {
        self.outputs.iter().find(|o| o.id == id)
    }

    /// Skapar en ny källa (läggs inte till – använd `Command::AddSource`).
    pub fn make_source(&mut self, name: impl Into<String>, kind: SourceKind) -> Source {
        Source {
            id: SourceId(self.alloc_id()),
            name: name.into(),
            kind,
        }
    }

    /// Skapar en ny fyrhörnsyta centrerad på utgången, lite förskjuten för
    /// varje ny yta så att de inte hamnar exakt ovanpå varandra.
    pub fn make_quad(&mut self, source: Option<SourceId>, output: OutputId) -> Surface {
        let id = SurfaceId(self.alloc_id());
        let n = self.surfaces.iter().filter(|s| s.output == output).count() as f32;
        let off = (n * 0.04) % 0.3;
        let (x0, y0, x1, y1) = (0.25 + off, 0.25 + off, 0.75 + off, 0.75 + off);
        Surface {
            id,
            name: format!("Yta {}", self.surfaces.len() + 1),
            source,
            output,
            shape: Shape::Quad,
            src_pts: UNIT_QUAD.to_vec(),
            dst_pts: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
            opacity: 1.0,
            visible: true,
            locked: false,
            mask: None,
        }
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    pub fn from_ron(s: &str) -> Result<Self, ron::error::SpannedError> {
        let mut p: Project = ron::from_str(s)?;
        p.migrate();
        Ok(p)
    }

    fn migrate(&mut self) {
        // Version 1 är första formatet. Framtida migreringar läggs här.
        self.version = PROJECT_VERSION;
        let max = self
            .sources
            .iter()
            .map(|s| s.id.0)
            .chain(self.surfaces.iter().map(|s| s.id.0))
            .chain(self.outputs.iter().map(|o| o.id.0))
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max + 1);

        // Det finns alltid minst en utgång, och varje yta ligger på en utgång som finns.
        if self.outputs.is_empty() {
            let out = self.make_output();
            self.outputs.push(out);
        }
        let first = self.outputs[0].id;
        let outputs: Vec<OutputId> = self.outputs.iter().map(|o| o.id).collect();
        for s in &mut self.surfaces {
            if !outputs.contains(&s.output) {
                s.output = first;
            }
            // Trasig geometri (t.ex. handredigerad fil) blir en rak fyrhörning i stället för en krasch.
            let bad_mesh = matches!(s.shape, Shape::Mesh { cols, rows } if !(MESH_MIN..=MESH_MAX).contains(&cols) || !(MESH_MIN..=MESH_MAX).contains(&rows));
            if bad_mesh || s.dst_pts.len() != s.shape.point_count() {
                s.shape = Shape::Quad;
                s.dst_pts = vec![[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]];
            }
            if s.src_pts.len() != 4 {
                s.src_pts = UNIT_QUAD.to_vec();
            }
            if let Some(m) = &mut s.mask {
                m.points.truncate(MASK_MAX_POINTS);
                m.feather = m.feather.max(0.0);
            }
            if s.mask.as_ref().is_some_and(|m| m.points.len() < MASK_MIN_POINTS) {
                s.mask = None;
            }
        }
    }

    /// Gör mediesökvägar relativa till `base` (projektfilens mapp) inför sparning.
    pub fn relativize_paths(&mut self, base: &std::path::Path) {
        for s in &mut self.sources {
            if let Some(p) = s.kind.path_mut() {
                if let Ok(rel) = p.strip_prefix(base) {
                    *p = rel.to_path_buf();
                }
            }
        }
    }

    /// Gör relativa mediesökvägar absoluta efter inläsning.
    pub fn absolutize_paths(&mut self, base: &std::path::Path) {
        for s in &mut self.sources {
            if let Some(p) = s.kind.path_mut() {
                if p.is_relative() {
                    *p = base.join(&*p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_ron() {
        let mut p = Project::new();
        let src = p.make_source(
            "intro",
            SourceKind::Video {
                path: "media/intro.mp4".into(),
                looping: true,
                muted: false,
            },
        );
        let sid = src.id;
        p.sources.push(src);
        let s = p.make_quad(Some(sid), p.outputs[0].id);
        p.surfaces.push(s);
        let text = p.to_ron().unwrap();
        let back = Project::from_ron(&text).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn ids_continue_after_load() {
        let mut p = Project::new();
        let s = p.make_quad(None, p.outputs[0].id);
        p.surfaces.push(s);
        let mut back = Project::from_ron(&p.to_ron().unwrap()).unwrap();
        let new_id = back.alloc_id();
        assert!(back.surfaces.iter().all(|s| s.id.0 < new_id));
    }

    #[test]
    fn example_project_loads() {
        let p = Project::from_ron(include_str!("../../../examples/demo.lmap")).unwrap();
        assert_eq!(p.surfaces.len(), 1);
        assert_eq!(p.surfaces[0].source, Some(p.sources[0].id));
    }

    #[test]
    fn broken_mesh_is_repaired() {
        let mut p = Project::new();
        let mut s = p.make_quad(None, p.outputs[0].id);
        s.shape = Shape::Mesh { cols: 3, rows: 3 };
        p.surfaces.push(s);
        let back = Project::from_ron(&p.to_ron().unwrap()).unwrap();
        assert_eq!(back.surfaces[0].shape, Shape::Quad);
        assert_eq!(back.surfaces[0].dst_pts.len(), 4);
    }

    #[test]
    fn relative_paths() {
        let mut p = Project::new();
        let src = p.make_source(
            "bild",
            SourceKind::Image {
                path: "/show/media/a.png".into(),
            },
        );
        p.sources.push(src);
        p.relativize_paths(std::path::Path::new("/show"));
        assert_eq!(
            p.sources[0].kind.path().unwrap(),
            std::path::Path::new("media/a.png")
        );
        p.absolutize_paths(std::path::Path::new("/other"));
        assert_eq!(
            p.sources[0].kind.path().unwrap(),
            std::path::Path::new("/other/media/a.png")
        );
    }
}
