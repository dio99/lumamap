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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CueId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub sources: Vec<Source>,
    /// Ordning = lagerordning (första ritas längst bak).
    pub surfaces: Vec<Surface>,
    pub outputs: Vec<Output>,
    /// Sparade tillstånd att växla mellan under en show.
    #[serde(default)]
    pub cues: Vec<Cue>,
    #[serde(default)]
    pub settings: Settings,
    next_id: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Ta emot OSC-fjärrstyrning.
    #[serde(default = "yes")]
    pub osc_enabled: bool,
    #[serde(default = "default_osc_port")]
    pub osc_port: u16,
    /// MIDI-kontroller kopplade till åtgärder.
    #[serde(default)]
    pub midi: Vec<MidiBinding>,
}

/// En kontroll på en MIDI-enhet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MidiControl {
    /// Ratt eller fader (Control Change).
    Cc { channel: u8, number: u8 },
    /// Tangent eller knapp (Note).
    Note { channel: u8, number: u8 },
}

impl MidiControl {
    pub fn label(&self) -> String {
        match self {
            MidiControl::Cc { channel, number } => format!("CC {number} ({} {})", crate::i18n::t("kanal", "channel"), channel + 1),
            MidiControl::Note { channel, number } => {
                format!("{} {number} ({} {})", crate::i18n::t("Ton", "Note"), crate::i18n::t("kanal", "channel"), channel + 1)
            }
        }
    }
}

/// Vad en MIDI-kontroll gör.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidiAction {
    /// Fader: master-nivå.
    Master,
    /// Knapp växlar, fader: av under mitten, på över.
    Blackout,
    CueNext,
    CuePrev,
    CueGo(CueId),
    /// Fader: ytans opacitet.
    SurfaceOpacity(SurfaceId),
    /// Knapp växlar, fader: av under mitten, på över.
    SurfaceVisible(SurfaceId),
    SourcePlayPause(SourceId),
    /// Fader: hastighet 0,25–4×.
    SourceSpeed(SourceId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiBinding {
    pub control: MidiControl,
    pub action: MidiAction,
}

fn default_osc_port() -> u16 {
    12345
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            osc_enabled: true,
            osc_port: default_osc_port(),
            midi: Vec::new(),
        }
    }
}

/// Ett sparat tillstånd: vilka ytor som syns, hur starkt och med vilket media.
/// Ytor som inte finns med i cuen lämnas orörda.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub id: CueId,
    pub name: String,
    /// Övergångstid i sekunder.
    #[serde(default)]
    pub fade: f32,
    pub surfaces: Vec<CueSurface>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CueSurface {
    pub surface: SurfaceId,
    pub visible: bool,
    pub opacity: f32,
    pub source: Option<SourceId>,
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
        /// Uppspelningshastighet, 1 = normal.
        #[serde(default = "one")]
        speed: f32,
    },
    Image {
        path: PathBuf,
    },
    Color {
        rgba: [f32; 4],
    },
    TestPattern,
    /// Kamera via V4L2, t.ex. `/dev/video0`.
    Camera {
        device: String,
    },
    /// Nätverksström eller annan URI som GStreamer förstår (rtsp://, srt://, udp://, http://…).
    Stream {
        uri: String,
        #[serde(default)]
        muted: bool,
    },
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
    /// Tre hörn: spets, nedre höger, nedre vänster. Visar motsvarande triangel ur källan.
    Triangle,
    /// Ellips inskriven i fyra hörn (samma ordning som fyrhörn), perspektivriktig.
    Ellipse,
    /// Rutnät med `cols × rows` kontrollpunkter (radvis) för böjda ytor.
    Mesh { cols: u32, rows: u32 },
}

pub const MESH_MIN: u32 = 2;
pub const MESH_MAX: u32 = 16;

impl Shape {
    /// Antal punkter i `dst_pts` för formen.
    pub fn point_count(&self) -> usize {
        match *self {
            Shape::Quad | Shape::Ellipse => 4,
            Shape::Triangle => 3,
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
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default)]
    pub color: ColorAdjust,
}

/// Färgjustering av en yta, t.ex. för att matcha två projektorer eller en färgad vägg.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorAdjust {
    /// −1..1, 0 = oförändrad.
    pub brightness: f32,
    /// 0..2, 1 = oförändrad.
    pub contrast: f32,
    /// 0.2..3, 1 = oförändrad.
    pub gamma: f32,
    /// 0..2, 1 = oförändrad, 0 = gråskala.
    pub saturation: f32,
    /// Nyansvridning i grader, −180..180.
    pub hue: f32,
}

impl Default for ColorAdjust {
    fn default() -> Self {
        ColorAdjust {
            brightness: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            saturation: 1.0,
            hue: 0.0,
        }
    }
}

impl ColorAdjust {
    pub fn is_identity(&self) -> bool {
        *self == ColorAdjust::default()
    }

    /// Samma beräkning som i `surface.wgsl` (för tester och förhandsvisning).
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut c = rgb.map(|v| ((v - 0.5) * self.contrast + 0.5 + self.brightness).max(0.0).powf(1.0 / self.gamma));
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        c = c.map(|v| luma + (v - luma) * self.saturation);
        // Vrid färgen runt gråaxeln (Rodrigues).
        let (s, co) = self.hue.to_radians().sin_cos();
        let k = 1.0 / 3f32.sqrt();
        let dot = k * (c[0] + c[1] + c[2]);
        let cross = [k * (c[2] - c[1]), k * (c[0] - c[2]), k * (c[1] - c[0])];
        [0, 1, 2].map(|i| (c[i] * co + cross[i] * s + k * dot * (1.0 - co)).clamp(0.0, 1.0))
    }
}

/// Hur ytan blandas med det som ligger under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    /// Ljuset adderas – bra för ljuseffekter och överlapp.
    Add,
    /// Mörkar ner det under – bra för skuggor och texturer.
    Multiply,
    /// Ljusar upp mjukt utan att bränna ut.
    Screen,
}

impl BlendMode {
    pub const ALL: [BlendMode; 4] = [BlendMode::Normal, BlendMode::Add, BlendMode::Multiply, BlendMode::Screen];

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Add => crate::i18n::t("Addera", "Add"),
            BlendMode::Multiply => crate::i18n::t("Multiplicera", "Multiply"),
            BlendMode::Screen => "Screen",
        }
    }
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

/// Tillåten uppspelningshastighet.
pub const SPEED_MIN: f32 = 0.1;
pub const SPEED_MAX: f32 = 4.0;

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
    #[serde(default)]
    pub edge_blend: EdgeBlend,
    /// Hörnkorrigering för hela utgången (övre vänster, övre höger, nedre
    /// höger, nedre vänster). Rätar upp bilden från en snett ställd projektor.
    #[serde(default = "unit_quad")]
    pub keystone: [Pt; 4],
}

fn unit_quad() -> [Pt; 4] {
    UNIT_QUAD
}

/// Mjuk övergång mot kanterna där två projektorer överlappar, så att
/// överlappet inte blir dubbelt så ljust.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeBlend {
    /// Bredd på blandningszonen som andel av bilden (0..0.5): vänster, höger, topp, botten.
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    /// Projektorns gamma (vanligen 2.2). Rampen görs i linjärt ljus och kodas med den.
    pub gamma: f32,
    /// Svartnivåkompensation (0..0.2): lyfter svärtan utanför överlappet så att
    /// den blir lika över hela väggen (två projektorers svärta adderas i överlappet).
    #[serde(default)]
    pub black_level: f32,
}

pub const EDGE_BLEND_MAX: f32 = 0.5;
pub const BLACK_LEVEL_MAX: f32 = 0.2;

impl Default for EdgeBlend {
    fn default() -> Self {
        EdgeBlend {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
            gamma: 2.2,
            black_level: 0.0,
        }
    }
}

impl EdgeBlend {
    pub fn is_active(&self) -> bool {
        self.left > 0.0 || self.right > 0.0 || self.top > 0.0 || self.bottom > 0.0
    }

    /// Hur mycket av bilden som släpps igenom vid (x, y) ∈ [0, 1]², i linjärt ljus.
    /// Samma beräkning som i `edge_blend.wgsl`.
    pub fn linear_factor(&self, x: f32, y: f32) -> f32 {
        fn ramp(d: f32, w: f32) -> f32 {
            if w <= 0.0 {
                return 1.0;
            }
            let t = (d / w).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }
        ramp(x, self.left) * ramp(1.0 - x, self.right) * ramp(y, self.top) * ramp(1.0 - y, self.bottom)
    }
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
            cues: Vec::new(),
            settings: Settings::default(),
            next_id: 1,
        };
        let out = p.make_output();
        p.outputs.push(out);
        p
    }

    /// Skapar en ny utgång (läggs inte till – använd `Command::AddOutput`).
    /// Sparar nuvarande tillstånd som en ny cue (läggs inte till – använd `Command::AddCue`).
    pub fn capture_cue(&mut self, name: impl Into<String>, fade: f32) -> Cue {
        Cue {
            id: CueId(self.alloc_id()),
            name: name.into(),
            fade,
            surfaces: self
                .surfaces
                .iter()
                .map(|s| CueSurface {
                    surface: s.id,
                    visible: s.visible,
                    opacity: s.opacity,
                    source: s.source,
                })
                .collect(),
        }
    }

    pub fn cue(&self, id: CueId) -> Option<&Cue> {
        self.cues.iter().find(|c| c.id == id)
    }

    pub fn make_output(&mut self) -> Output {
        Output {
            id: OutputId(self.alloc_id()),
            name: format!("{} {}", crate::i18n::t("Projektor", "Projector"), self.outputs.len() + 1),
            resolution: [1920, 1080],
            window_pos: None,
            fullscreen: false,
            edge_blend: EdgeBlend::default(),
            keystone: UNIT_QUAD,
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
            name: format!("{} {}", crate::i18n::t("Yta", "Surface"), self.surfaces.len() + 1),
            source,
            output,
            shape: Shape::Quad,
            src_pts: UNIT_QUAD.to_vec(),
            dst_pts: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
            opacity: 1.0,
            visible: true,
            locked: false,
            mask: None,
            blend: BlendMode::Normal,
            color: ColorAdjust::default(),
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
            .chain(self.cues.iter().map(|c| c.id.0))
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max + 1);

        // Det finns alltid minst en utgång, och varje yta ligger på en utgång som finns.
        if self.outputs.is_empty() {
            let out = self.make_output();
            self.outputs.push(out);
        }
        for o in &mut self.outputs {
            let e = &mut o.edge_blend;
            for w in [&mut e.left, &mut e.right, &mut e.top, &mut e.bottom] {
                *w = w.clamp(0.0, EDGE_BLEND_MAX);
            }
            if !(1.0..=4.0).contains(&e.gamma) {
                e.gamma = 2.2;
            }
            e.black_level = e.black_level.clamp(0.0, BLACK_LEVEL_MAX);
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
                speed: 1.0,
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
    fn gallery_project_loads() {
        let p = Project::from_ron(include_str!("../../../examples/gallery.lmap")).unwrap();
        assert_eq!(p.cues.len(), 10);
        assert_eq!(p.surfaces.iter().filter(|s| s.visible).count(), 1);
        // Varje video som galleriet pekar på finns.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        for s in &p.sources {
            assert!(dir.join(s.kind.path().unwrap()).exists(), "{}", s.name);
        }
    }

    #[test]
    fn showcase_project_loads() {
        let p = Project::from_ron(include_str!("../../../examples/showcase.lmap")).unwrap();
        assert_eq!(p.surfaces.len(), 5);
        assert_eq!(p.cues.len(), 4);
        // Inget repareras bort vid inläsning: alla former och masker är giltiga.
        assert!(p.surfaces.iter().all(|s| s.dst_pts.len() == s.shape.point_count()));
        assert!(p.surfaces.iter().any(|s| s.mask.is_some()));
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

    /// Två projektorer som överlappar: höger kant på den ena och vänster
    /// kant på den andra ska tillsammans ge full ljusstyrka genom hela överlappet.
    #[test]
    fn edge_blend_overlap_sums_to_one() {
        let overlap = 0.2;
        let a = EdgeBlend { right: overlap, ..EdgeBlend::default() };
        let b = EdgeBlend { left: overlap, ..EdgeBlend::default() };
        for i in 0..=20 {
            let t = i as f32 / 20.0 * overlap;
            // Samma punkt på väggen: x = 1 − overlap + t på A, x = t på B.
            let sum = a.linear_factor(1.0 - overlap + t, 0.5) + b.linear_factor(t, 0.5);
            assert!((sum - 1.0).abs() < 1e-5, "t={t}: {sum}");
        }
        assert_eq!(a.linear_factor(0.5, 0.5), 1.0);
    }

    #[test]
    fn color_adjust() {
        let id = ColorAdjust::default();
        let same = id.apply([0.2, 0.5, 0.8]);
        assert!(same.iter().zip([0.2, 0.5, 0.8]).all(|(a, b)| (a - b).abs() < 1e-5), "{same:?}");
        let grey = ColorAdjust { saturation: 0.0, ..id };
        let g = grey.apply([1.0, 0.0, 0.0]);
        assert!((g[0] - g[1]).abs() < 1e-6 && (g[1] - g[2]).abs() < 1e-6);
        // 120° vrider rött till grönt.
        let turned = ColorAdjust { hue: 120.0, ..id }.apply([1.0, 0.0, 0.0]);
        assert!(turned[1] > 0.99 && turned[0] < 0.01 && turned[2] < 0.01, "{turned:?}");
        let brighter = ColorAdjust { brightness: 0.2, ..id }.apply([0.5, 0.5, 0.5]);
        assert!((brighter[0] - 0.7).abs() < 1e-6);
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
