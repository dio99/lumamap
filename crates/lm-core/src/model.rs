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
    /// Ljus som styrs med DMX (Art-Net).
    #[serde(default)]
    pub lamps: Vec<Lamp>,
    /// LED-slingor som tar sina färger från projektorbilden.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub led_strips: Vec<LedStrip>,
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
    /// Tempo för effekterna, slag per minut.
    #[serde(default = "default_bpm")]
    pub bpm: f32,
    /// Ljud som effekterna kan följa.
    #[serde(default)]
    pub audio: AudioSetting,
    /// DMX-ljus via Art-Net.
    #[serde(default)]
    pub artnet: ArtNetSettings,
    /// Fjärrkontroll i mobilen (webbsida i det lokala nätverket).
    #[serde(default)]
    pub remote_enabled: bool,
    #[serde(default = "default_remote_port")]
    pub remote_port: u16,
    /// Börja om från första cuen efter den sista (för automatiska cues).
    #[serde(default)]
    pub loop_cues: bool,
    /// Visa bara under vissa tider, t.ex. en installation 16–23.
    #[serde(default)]
    pub schedule: Schedule,
    /// Rörelse framför en kamera styr effekter och kan starta en cue.
    #[serde(default)]
    pub motion: MotionSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MotionSettings {
    /// Kamerakällan som tittar efter rörelse.
    pub source: Option<SourceId>,
    /// 0..1: högre = mindre rörelse räcker.
    pub sensitivity: f32,
    /// Cue som startar när någon rör sig.
    pub cue: Option<CueId>,
    /// Minsta tid mellan två starter, sekunder.
    pub cooldown: f32,
}

impl Default for MotionSettings {
    fn default() -> Self {
        MotionSettings { source: None, sensitivity: 0.5, cue: None, cooldown: 10.0 }
    }
}

impl MotionSettings {
    /// Rörelsenivån (andel av bilden som rört sig) omräknad till 0..1, där
    /// 1 betyder "tydlig rörelse" för den valda känsligheten.
    pub fn strength(&self, level: f32) -> f32 {
        let full = 0.25 - self.sensitivity.clamp(0.0, 1.0) * 0.23;
        (level / full).clamp(0.0, 1.0)
    }
}

/// Tider då showen är på. Utanför dem är projektorerna svarta och videorna pausade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub enabled: bool,
    /// Minuter efter midnatt.
    pub on: u16,
    pub off: u16,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule { enabled: false, on: 16 * 60, off: 23 * 60 }
    }
}

impl Schedule {
    /// Om showen ska vara på vid `minute` (minuter efter midnatt). Klarar
    /// tider över midnatt, t.ex. 20:00–02:00.
    pub fn is_on(&self, minute: u16) -> bool {
        if !self.enabled {
            return true;
        }
        let m = minute % (24 * 60);
        if self.on <= self.off {
            (self.on..self.off).contains(&m)
        } else {
            m >= self.on || m < self.off
        }
    }

    /// "16:00"
    pub fn format(minute: u16) -> String {
        format!("{:02}:{:02}", minute / 60 % 24, minute % 60)
    }
}

fn default_remote_port() -> u16 {
    8077
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtNetSettings {
    pub enabled: bool,
    /// Nodens IP-adress, eller 255.255.255.255 för alla på nätverket.
    pub target: String,
    pub universe: u16,
}

impl Default for ArtNetSettings {
    fn default() -> Self {
        ArtNetSettings {
            enabled: false,
            target: "255.255.255.255".into(),
            universe: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AudioSetting {
    #[default]
    Off,
    /// Mikrofon eller linjeingång.
    Microphone,
    /// Det datorn själv spelar.
    Computer,
}

impl AudioSetting {
    pub const ALL: [AudioSetting; 3] = [AudioSetting::Off, AudioSetting::Microphone, AudioSetting::Computer];

    pub fn label(self) -> &'static str {
        use crate::i18n::t;
        match self {
            AudioSetting::Off => t("Av", "Off"),
            AudioSetting::Microphone => t("Mikrofon", "Microphone"),
            AudioSetting::Computer => t("Datorns ljud", "Computer audio"),
        }
    }
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

fn default_bpm() -> f32 {
    120.0
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
            bpm: default_bpm(),
            audio: AudioSetting::Off,
            artnet: ArtNetSettings::default(),
            remote_enabled: false,
            remote_port: default_remote_port(),
            loop_cues: false,
            schedule: Schedule::default(),
            motion: MotionSettings::default(),
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
    /// Lampornas läge (lampor som inte finns med lämnas orörda).
    #[serde(default)]
    pub lamps: Vec<CueLamp>,
    /// Gå vidare till nästa cue automatiskt så här många sekunder efter GO.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CueLamp {
    pub lamp: LampId,
    pub level: f32,
    pub color: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LampId(pub u32);

/// En DMX-lampa.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lamp {
    pub id: LampId,
    pub name: String,
    /// Första DMX-kanalen, 1..512.
    pub address: u16,
    pub kind: LampKind,
    /// Ljusstyrka 0..1.
    pub level: f32,
    pub color: [f32; 3],
    /// Ta färgen från en källas medelfärg, så att ljuset matchar projektionen.
    #[serde(default)]
    pub follow: Option<SourceId>,
}

/// Lamptyper och hur deras kanaler ligger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LampKind {
    /// En kanal: ljusstyrka.
    Dimmer,
    /// Tre kanaler: röd, grön, blå.
    Rgb,
    /// Fyra kanaler: röd, grön, blå, vit.
    Rgbw,
    /// Fyra kanaler: ljusstyrka, röd, grön, blå.
    DimmerRgb,
}

impl LampKind {
    pub const ALL: [LampKind; 4] = [LampKind::Dimmer, LampKind::Rgb, LampKind::Rgbw, LampKind::DimmerRgb];

    pub fn channels(self) -> usize {
        match self {
            LampKind::Dimmer => 1,
            LampKind::Rgb => 3,
            LampKind::Rgbw | LampKind::DimmerRgb => 4,
        }
    }

    pub fn label(self) -> &'static str {
        use crate::i18n::t;
        match self {
            LampKind::Dimmer => t("Dimmer (1 kanal)", "Dimmer (1 channel)"),
            LampKind::Rgb => t("RGB (3 kanaler)", "RGB (3 channels)"),
            LampKind::Rgbw => t("RGBW (4 kanaler)", "RGBW (4 channels)"),
            LampKind::DimmerRgb => t("Dimmer + RGB (4 kanaler)", "Dimmer + RGB (4 channels)"),
        }
    }
}

impl Lamp {
    /// Kanalvärden 0..255, från lampans första kanal. `color` är den färg som
    /// gäller just nu (egen eller från media), `master` skalar allt.
    pub fn dmx(&self, color: [f32; 3], master: f32) -> Vec<u8> {
        let level = (self.level * master).clamp(0.0, 1.0);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let [r, g, b] = color.map(|c| c * level);
        match self.kind {
            LampKind::Dimmer => vec![byte(level)],
            LampKind::Rgb => vec![byte(r), byte(g), byte(b)],
            LampKind::Rgbw => {
                // Det gemensamma vita läggs på den vita kanalen.
                let w = r.min(g).min(b);
                vec![byte(r - w), byte(g - w), byte(b - w), byte(w)]
            }
            LampKind::DimmerRgb => {
                let [r, g, b] = color;
                vec![byte(level), byte(r), byte(g), byte(b)]
            }
        }
    }
}

/// Alla lampors kanaler i ett DMX-universum (512 kanaler). `color_of` ger den
/// färg lampan ska ha just nu.
pub fn dmx_universe(lamps: &[Lamp], master: f32, color_of: impl Fn(&Lamp) -> [f32; 3]) -> [u8; 512] {
    let mut out = [0u8; 512];
    for l in lamps {
        let start = (l.address.clamp(1, 512) - 1) as usize;
        for (i, v) in l.dmx(color_of(l), master).into_iter().enumerate() {
            if let Some(slot) = out.get_mut(start + i) {
                *slot = v;
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LedId(pub u32);

/// I vilken ordning slingans lysdioder vill ha färgerna.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorOrder {
    #[default]
    Rgb,
    Grb,
    Brg,
}

impl ColorOrder {
    pub const ALL: [ColorOrder; 3] = [ColorOrder::Rgb, ColorOrder::Grb, ColorOrder::Brg];

    pub fn label(self) -> &'static str {
        match self {
            ColorOrder::Rgb => "RGB",
            ColorOrder::Grb => "GRB",
            ColorOrder::Brg => "BRG",
        }
    }

    fn arrange(self, [r, g, b]: [u8; 3]) -> [u8; 3] {
        match self {
            ColorOrder::Rgb => [r, g, b],
            ColorOrder::Grb => [g, r, b],
            ColorOrder::Brg => [b, r, g],
        }
    }
}

/// En LED-slinga (pixlar som styrs via Art-Net), utlagd som en linje över
/// projektorbilden. Varje lysdiod får färgen i bilden där den ligger, så
/// slingan förlänger projektionen ut i rummet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedStrip {
    pub id: LedId,
    pub name: String,
    pub output: OutputId,
    /// Slingans ändar i utgångens koordinater (0..1).
    pub start: Pt,
    pub end: Pt,
    pub pixels: u16,
    /// Art-Net-universum och första kanal (1..512). Pixlar som inte ryms
    /// fortsätter i nästa universum (170 pixlar per universum).
    pub universe: u16,
    pub address: u16,
    #[serde(default)]
    pub order: ColorOrder,
    #[serde(default = "one")]
    pub level: f32,
}

pub const LED_PIXELS_MAX: u16 = 2048;

impl LedStrip {
    /// Var varje lysdiod ligger, jämnt fördelade från början till slut.
    pub fn positions(&self) -> Vec<Pt> {
        let n = self.pixels.clamp(1, LED_PIXELS_MAX) as usize;
        (0..n)
            .map(|i| {
                let f = if n == 1 { 0.5 } else { i as f32 / (n - 1) as f32 };
                [self.start[0] + (self.end[0] - self.start[0]) * f, self.start[1] + (self.end[1] - self.start[1]) * f]
            })
            .collect()
    }

    /// Universum och kanaler (universum, första kanal 0-baserad) för varje pixel.
    fn slots(&self) -> impl Iterator<Item = (u16, usize)> + '_ {
        let mut universe = self.universe;
        let mut ch = (self.address.clamp(1, 512) - 1) as usize;
        (0..self.pixels.clamp(1, LED_PIXELS_MAX)).map(move |_| {
            if ch + 3 > 512 {
                universe = universe.saturating_add(1);
                ch = 0;
            }
            let slot = (universe, ch);
            ch += 3;
            slot
        })
    }

    /// Sista universumet slingan använder.
    pub fn last_universe(&self) -> u16 {
        self.slots().last().map_or(self.universe, |s| s.0)
    }
}

/// Lägger LED-slingornas färger (0..1, en per pixel) i DMX-universum.
/// `base` innehåller redan lamporna; slingorna skriver över där de ligger.
pub fn led_universes(strips: &[LedStrip], colors: &[Vec<[f32; 3]>], master: f32, base: &mut std::collections::BTreeMap<u16, [u8; 512]>) {
    for (strip, cols) in strips.iter().zip(colors) {
        let k = strip.level.clamp(0.0, 1.0) * master.clamp(0.0, 1.0);
        for ((universe, ch), c) in strip.slots().zip(cols) {
            let rgb = c.map(|v| (v.clamp(0.0, 1.0) * k * 255.0).round() as u8);
            let out = base.entry(universe).or_insert([0u8; 512]);
            out[ch..ch + 3].copy_from_slice(&strip.order.arrange(rgb));
        }
    }
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
    /// Mönster som ritas av grafikkortet varje bildruta.
    Generator {
        pattern: Pattern,
        colors: [[f32; 3]; 2],
        /// Varv per taktslag.
        #[serde(default = "quarter")]
        speed: f32,
    },
    /// Nätverksström eller annan URI som GStreamer förstår (rtsp://, srt://, udp://, http://…).
    Stream {
        uri: String,
        #[serde(default)]
        muted: bool,
    },
}

fn quarter() -> f32 {
    0.25
}

/// Mönstren. Numreringen används i shadern – lägg bara till nya sist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pattern {
    Gradient = 0,
    Stripes = 1,
    Plasma = 2,
    Tunnel = 3,
    Clouds = 4,
    Rings = 5,
}

impl Pattern {
    pub const ALL: [Pattern; 6] = [Pattern::Gradient, Pattern::Stripes, Pattern::Plasma, Pattern::Tunnel, Pattern::Clouds, Pattern::Rings];

    pub fn label(self) -> &'static str {
        use crate::i18n::t;
        match self {
            Pattern::Gradient => t("Gradient", "Gradient"),
            Pattern::Stripes => t("Ränder", "Stripes"),
            Pattern::Plasma => t("Plasma", "Plasma"),
            Pattern::Tunnel => t("Tunnel", "Tunnel"),
            Pattern::Clouds => t("Moln", "Clouds"),
            Pattern::Rings => t("Ringar", "Rings"),
        }
    }

    /// Färger som passar mönstret från början.
    pub fn default_colors(self) -> [[f32; 3]; 2] {
        match self {
            Pattern::Gradient => [[1.0, 0.25, 0.6], [0.15, 0.6, 1.0]],
            Pattern::Stripes => [[0.02, 0.02, 0.05], [0.2, 0.9, 1.0]],
            Pattern::Plasma => [[0.55, 0.1, 0.9], [1.0, 0.65, 0.1]],
            Pattern::Tunnel => [[0.05, 0.0, 0.15], [1.0, 0.3, 0.8]],
            Pattern::Clouds => [[0.02, 0.05, 0.15], [0.6, 0.85, 1.0]],
            Pattern::Rings => [[0.0, 0.0, 0.0], [1.0, 0.85, 0.3]],
        }
    }
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
    /// Ett klot (boll) inskrivet i fyra hörn. Bilden lindas runt den synliga
    /// halvan så att den inte ser utdragen ut mot kanterna.
    Sphere,
    /// En cylinder (pelare, tunna) sedd från sidan, inskriven i fyra hörn.
    /// Bilden pressas ihop mot kanterna som på en rund yta.
    Cylinder,
    /// En kon (strut, julgran) med spetsen uppåt, tre hörn som en triangel.
    /// Bilden lindas runt konen.
    Cone,
    /// En ritad ljuslinje: `points` punkter längs en mjuk kurva, `width` pixlar bred.
    Line { points: u32, width: u32 },
}

pub const LINE_MAX_POINTS: u32 = 64;
pub const LINE_WIDTH_MAX: u32 = 400;

pub const MESH_MIN: u32 = 2;
pub const MESH_MAX: u32 = 16;

impl Shape {
    /// Antal punkter i `dst_pts` för formen.
    pub fn point_count(&self) -> usize {
        match *self {
            Shape::Quad | Shape::Ellipse | Shape::Sphere | Shape::Cylinder => 4,
            Shape::Triangle | Shape::Cone => 3,
            Shape::Mesh { cols, rows } => (cols * rows) as usize,
            Shape::Line { points, .. } => points as usize,
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
    #[serde(default)]
    pub effect: Effect,
}

/// En animerad effekt på ytan. Går i takt med projektets tempo.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    /// Hur mycket, 0..1.
    pub amount: f32,
    /// Varv per taktslag (0 = stilla).
    pub speed: f32,
    /// Ljudets bas förstärker effekten.
    #[serde(default)]
    pub follow_audio: bool,
    /// Rörelse framför kameran förstärker effekten.
    #[serde(default)]
    pub follow_motion: bool,
}

impl Default for Effect {
    fn default() -> Self {
        Effect {
            kind: EffectKind::None,
            amount: 0.5,
            speed: 0.25,
            follow_audio: false,
            follow_motion: false,
        }
    }
}

/// Effekterna. Numreringen används i shadern – lägg bara till nya sist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EffectKind {
    #[default]
    None = 0,
    Kaleidoscope = 1,
    Tile = 2,
    Scroll = 3,
    Rotate = 4,
    Pulse = 5,
    Blur = 6,
    Invert = 7,
    ColorCycle = 8,
    Strobe = 9,
    Outline = 10,
    Chase = 11,
    Neon = 12,
    Sweep = 13,
}

impl EffectKind {
    pub const ALL: [EffectKind; 14] = [
        EffectKind::None,
        EffectKind::Outline,
        EffectKind::Chase,
        EffectKind::Neon,
        EffectKind::Sweep,
        EffectKind::Kaleidoscope,
        EffectKind::Tile,
        EffectKind::Scroll,
        EffectKind::Rotate,
        EffectKind::Pulse,
        EffectKind::ColorCycle,
        EffectKind::Strobe,
        EffectKind::Blur,
        EffectKind::Invert,
    ];

    pub fn label(self) -> &'static str {
        use crate::i18n::t;
        match self {
            EffectKind::None => t("Ingen", "None"),
            EffectKind::Kaleidoscope => t("Kalejdoskop", "Kaleidoscope"),
            EffectKind::Tile => t("Kakel", "Tiles"),
            EffectKind::Scroll => t("Rulla", "Scroll"),
            EffectKind::Rotate => t("Rotera", "Rotate"),
            EffectKind::Pulse => t("Pulsera", "Pulse"),
            EffectKind::Blur => t("Oskärpa", "Blur"),
            EffectKind::Invert => t("Invertera", "Invert"),
            EffectKind::ColorCycle => t("Färgcykel", "Colour cycle"),
            EffectKind::Strobe => t("Blinka", "Strobe"),
            EffectKind::Outline => t("Konturglöd", "Edge glow"),
            EffectKind::Chase => t("Jagande ljus", "Chasing lights"),
            EffectKind::Neon => t("Neonram", "Neon frame"),
            EffectKind::Sweep => t("Ljussvep", "Light sweep"),
        }
    }

    pub fn hint(self) -> &'static str {
        use crate::i18n::t;
        match self {
            EffectKind::None => "",
            EffectKind::Kaleidoscope => t("Speglar bilden i tårtbitar runt mitten", "Mirrors the image in slices around the centre"),
            EffectKind::Tile => t("Upprepar bilden i ett rutnät", "Repeats the image in a grid"),
            EffectKind::Scroll => t("Bilden glider i sidled och går runt", "The image slides sideways and wraps around"),
            EffectKind::Rotate => t("Bilden snurrar inne i ytan", "The image spins inside the surface"),
            EffectKind::Pulse => t("Bilden zoomar in och ut i takt", "The image zooms in and out to the beat"),
            EffectKind::Blur => t("Mjukar upp bilden", "Softens the image"),
            EffectKind::Invert => t("Vänder färgerna", "Inverts the colours"),
            EffectKind::ColorCycle => t("Färgerna vandrar runt färghjulet", "Colours travel around the colour wheel"),
            EffectKind::Strobe => t("Ytan blinkar i takt", "The surface flashes to the beat"),
            EffectKind::Outline => t("Bara kanterna lyser, med ett ljus som springer runt", "Only the edges glow, with a light running around"),
            EffectKind::Chase => t("Ljuspunkter springer runt ytans kant – som en ljusslinga", "Dots of light run around the edge – like a light chain"),
            EffectKind::Neon => t("En skarp lysande ram runt ytan som pulserar i takt", "A sharp glowing frame around the surface that pulses to the beat"),
            EffectKind::Sweep => t("En ljusstrimma sveper över ytan i takt", "A bar of light sweeps across the surface to the beat"),
        }
    }
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
            lamps: Vec::new(),
            led_strips: Vec::new(),
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
            lamps: self
                .lamps
                .iter()
                .map(|l| CueLamp {
                    lamp: l.id,
                    level: l.level,
                    color: l.color,
                })
                .collect(),
            auto: None,
        }
    }

    /// Cuen efter `current` (index), eller den första om ingen körs. Efter
    /// den sista: den första om `loop_cues`, annars ingen.
    pub fn next_cue_index(&self, current: Option<usize>) -> Option<usize> {
        let n = self.cues.len();
        match current {
            _ if n == 0 => None,
            None => Some(0),
            Some(i) if i + 1 < n => Some(i + 1),
            Some(_) if self.settings.loop_cues => Some(0),
            Some(_) => None,
        }
    }

    /// En ny LED-slinga längs bildens nederkant (läggs inte till – använd `Command::SetLedStrips`).
    pub fn make_led_strip(&mut self, output: OutputId) -> LedStrip {
        // Efter de andra slingorna, i ett eget universum.
        let universe = self.led_strips.iter().map(|l| l.last_universe() + 1).max().unwrap_or(1);
        LedStrip {
            id: LedId(self.alloc_id()),
            name: format!("{} {}", crate::i18n::t("LED-slinga", "LED strip"), self.led_strips.len() + 1),
            output,
            start: [0.05, 0.95],
            end: [0.95, 0.95],
            pixels: 60,
            universe,
            address: 1,
            order: ColorOrder::Rgb,
            level: 1.0,
        }
    }

    /// Skapar en ny lampa efter de befintliga (läggs inte till – använd `Command::SetLamps`).
    pub fn make_lamp(&mut self, kind: LampKind) -> Lamp {
        let address = self.lamps.iter().map(|l| l.address + l.kind.channels() as u16).max().unwrap_or(1).min(512);
        Lamp {
            id: LampId(self.alloc_id()),
            name: format!("{} {}", crate::i18n::t("Lampa", "Lamp"), self.lamps.len() + 1),
            address,
            kind,
            level: 1.0,
            color: [1.0, 1.0, 1.0],
            follow: None,
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
            effect: Effect::default(),
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
            .chain(self.lamps.iter().map(|l| l.id.0))
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
            let bad_mesh = matches!(s.shape, Shape::Mesh { cols, rows } if !(MESH_MIN..=MESH_MAX).contains(&cols) || !(MESH_MIN..=MESH_MAX).contains(&rows))
                || matches!(s.shape, Shape::Line { points, width } if !(2..=LINE_MAX_POINTS).contains(&points) || !(1..=LINE_WIDTH_MAX).contains(&width));
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

    /// Alla exempelprojekt ska gå att öppna och peka på media som finns.
    #[test]
    fn seasonal_projects_load() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        for (name, text) in [
            ("christmas", include_str!("../../../examples/christmas.lmap")),
            ("halloween", include_str!("../../../examples/halloween.lmap")),
            ("facade", include_str!("../../../examples/facade.lmap")),
        ] {
            let p = Project::from_ron(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!p.cues.is_empty(), "{name}");
            for s in &p.sources {
                assert!(dir.join(s.kind.path().unwrap()).exists(), "{name}: {}", s.name);
            }
        }
    }

    #[test]
    fn led_strip_spills_into_next_universe() {
        let mut p = Project::new();
        let mut strip = p.make_led_strip(p.outputs[0].id);
        strip.pixels = 172;
        strip.universe = 3;
        strip.order = ColorOrder::Grb;
        assert_eq!(strip.positions().len(), 172);
        assert_eq!(strip.positions()[0], [0.05, 0.95]);
        assert_eq!(strip.positions()[171], [0.95, 0.95]);
        assert_eq!(strip.last_universe(), 4);
        let colors = vec![vec![[1.0, 0.5, 0.0]; 172]];
        let mut out = std::collections::BTreeMap::new();
        out.insert(3, [7u8; 512]);
        led_universes(&[strip], &colors, 1.0, &mut out);
        // GRB: grönt först. 170 pixlar i universum 3, resten i 4.
        assert_eq!(&out[&3][..3], &[128, 255, 0]);
        assert_eq!(&out[&3][507..510], &[128, 255, 0]);
        assert_eq!(&out[&3][510..], &[7, 7]);
        assert_eq!(&out[&4][..6], &[128, 255, 0, 128, 255, 0]);
        assert_eq!(out[&4][6], 0);
    }

    #[test]
    fn schedule_handles_midnight() {
        let day = Schedule { enabled: true, on: 16 * 60, off: 23 * 60 };
        assert!(!day.is_on(15 * 60 + 59));
        assert!(day.is_on(16 * 60));
        assert!(day.is_on(22 * 60 + 59));
        assert!(!day.is_on(23 * 60));
        let night = Schedule { enabled: true, on: 20 * 60, off: 2 * 60 };
        assert!(night.is_on(23 * 60));
        assert!(night.is_on(60));
        assert!(!night.is_on(12 * 60));
        assert!(Schedule { enabled: false, ..night }.is_on(12 * 60));
        assert_eq!(Schedule::format(16 * 60 + 5), "16:05");
    }

    #[test]
    fn next_cue_wraps_only_when_looping() {
        let mut p = Project::new();
        for i in 0..3 {
            let c = p.capture_cue(format!("{i}"), 0.0);
            p.cues.push(c);
        }
        assert_eq!(p.next_cue_index(None), Some(0));
        assert_eq!(p.next_cue_index(Some(1)), Some(2));
        assert_eq!(p.next_cue_index(Some(2)), None);
        p.settings.loop_cues = true;
        assert_eq!(p.next_cue_index(Some(2)), Some(0));
        // Cues utan `auto` sparas som förut.
        assert!(!p.to_ron().unwrap().contains("auto"));
    }

    #[test]
    fn geometry_project_uses_edge_effects() {
        let p = Project::from_ron(include_str!("../../../examples/geometry.lmap")).unwrap();
        assert_eq!(p.cues.len(), 4);
        // Bara mönster och färger – inga filer.
        assert!(p.sources.iter().all(|s| s.kind.path().is_none()));
        for kind in [EffectKind::Chase, EffectKind::Neon, EffectKind::Sweep] {
            assert!(p.surfaces.iter().any(|s| s.effect.kind == kind), "{kind:?}");
        }
    }

    #[test]
    fn objects_project_uses_3d_shapes() {
        let p = Project::from_ron(include_str!("../../../examples/objects.lmap")).unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        for shape in [Shape::Sphere, Shape::Cylinder, Shape::Cone] {
            let s = p.surfaces.iter().find(|s| s.shape == shape).unwrap_or_else(|| panic!("{shape:?}"));
            assert_eq!(s.dst_pts.len(), shape.point_count());
        }
        for s in &p.sources {
            assert!(dir.join(s.kind.path().unwrap()).exists(), "{}", s.name);
        }
    }

    #[test]
    fn effect_kinds_are_listed_once() {
        for (i, a) in EffectKind::ALL.iter().enumerate() {
            assert!(!EffectKind::ALL[i + 1..].contains(a), "{a:?}");
            assert!(*a == EffectKind::None || !a.hint().is_empty(), "{a:?}");
        }
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
    fn lamp_channels() {
        let mut p = Project::new();
        let mut rgbw = p.make_lamp(LampKind::Rgbw);
        rgbw.address = 10;
        rgbw.color = [1.0, 0.5, 0.25];
        assert_eq!(rgbw.dmx(rgbw.color, 1.0), vec![191, 64, 0, 64]);
        let mut dim = p.make_lamp(LampKind::DimmerRgb);
        dim.level = 0.5;
        dim.color = [1.0, 0.0, 0.0];
        assert_eq!(dim.dmx(dim.color, 1.0), vec![128, 255, 0, 0]);
        assert_eq!(dim.dmx(dim.color, 0.0)[0], 0, "master/svart släcker");
        let u = dmx_universe(&[rgbw.clone()], 1.0, |l| l.color);
        assert_eq!(&u[9..13], &[191, 64, 0, 64]);
        assert_eq!(u[8], 0);
        // Nästa lampa hamnar efter den förra.
        p.lamps.push(rgbw);
        assert_eq!(p.make_lamp(LampKind::Rgb).address, 14);
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
