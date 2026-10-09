//! Att gå till en cue: vilka ändringar som görs direkt och hur övergången
//! tonar. Rena funktioner – appen kör dem som kommandon med ett gest-nummer
//! så att hela GO:t blir ett ångra-steg.

use crate::command::Command;
use crate::model::*;

/// En yta som tonas under en övergång.
#[derive(Debug, Clone, PartialEq)]
pub struct FadeStep {
    pub surface: SurfaceId,
    /// Opaciteten ytan startade på (0 om den var dold).
    pub from: f32,
    pub to: CueSurface,
}

/// En lampa som tonas under en övergång.
#[derive(Debug, Clone, PartialEq)]
pub struct LampFade {
    pub from_level: f32,
    pub from_color: [f32; 3],
    pub to: CueLamp,
}

/// Allt som tonas under en övergång.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CueFade {
    pub surfaces: Vec<FadeStep>,
    pub lamps: Vec<LampFade>,
}

impl CueFade {
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty() && self.lamps.is_empty()
    }
}

/// Kommandot som körs när GO trycks, och stegen att tona (tomt om cuen
/// saknar övergångstid). Media byts direkt; ytor som ska tonas in görs
/// synliga med sin startopacitet.
///
/// Alla cuens ytor tas med i kommandot, även oförändrade: övergångens senare
/// steg slås ihop med det, så ångra-steget måste spara dem alla.
pub fn cue_start(p: &Project, cue: &Cue) -> (Command, CueFade) {
    let instant = cue.fade <= 0.0;
    let mut cmds = Vec::new();
    let mut steps = Vec::new();
    for cs in &cue.surfaces {
        let Some(s) = p.surface(cs.surface) else { continue };
        let from = if s.visible { s.opacity } else { 0.0 };
        let mut ns = s.clone();
        ns.source = cs.source;
        if instant {
            ns.visible = cs.visible;
            ns.opacity = cs.opacity;
        } else {
            ns.visible = s.visible || cs.visible;
            ns.opacity = from;
            steps.push(FadeStep {
                surface: s.id,
                from,
                to: cs.clone(),
            });
        }
        cmds.push(Command::ReplaceSurface(ns));
    }
    // Lamporna: direkt, eller tonas från där de är.
    let mut lamps = p.lamps.clone();
    let mut lamp_steps = Vec::new();
    for cl in &cue.lamps {
        let Some(l) = lamps.iter_mut().find(|l| l.id == cl.lamp) else { continue };
        if instant {
            l.level = cl.level;
            l.color = cl.color;
        } else {
            lamp_steps.push(LampFade {
                from_level: l.level,
                from_color: l.color,
                to: cl.clone(),
            });
        }
    }
    if !cue.lamps.is_empty() {
        cmds.push(Command::SetLamps(lamps));
    }
    (Command::Batch(cmds), CueFade { surfaces: steps, lamps: lamp_steps })
}

/// Mjuk start och landning.
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Tillståndet vid `t` ∈ [0, 1] av övergången. Vid `t = 1` får ytorna
/// cuens synlighet och opacitet exakt.
pub fn fade_frame(p: &Project, fade: &CueFade, t: f32) -> Command {
    let t = t.clamp(0.0, 1.0);
    let e = ease(t);
    let mut cmds = Vec::new();
    if !fade.lamps.is_empty() {
        let mut lamps = p.lamps.clone();
        for step in &fade.lamps {
            let Some(l) = lamps.iter_mut().find(|l| l.id == step.to.lamp) else { continue };
            l.level = step.from_level + (step.to.level - step.from_level) * e;
            l.color = [0, 1, 2].map(|i| step.from_color[i] + (step.to.color[i] - step.from_color[i]) * e);
        }
        if lamps != p.lamps {
            cmds.push(Command::SetLamps(lamps));
        }
    }
    for step in &fade.surfaces {
        let Some(s) = p.surface(step.surface) else { continue };
        let mut ns = s.clone();
        if t >= 1.0 {
            ns.visible = step.to.visible;
            ns.opacity = step.to.opacity;
        } else {
            let target = if step.to.visible { step.to.opacity } else { 0.0 };
            ns.opacity = step.from + (target - step.from) * e;
        }
        if ns != *s {
            cmds.push(Command::ReplaceSurface(ns));
        }
    }
    Command::Batch(cmds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::History;

    /// Två ytor: A synlig, B dold. Cuen visar B och döljer A.
    fn setup() -> (Project, History, Cue) {
        let mut p = Project::new();
        let mut h = History::default();
        let out = p.outputs[0].id;
        for _ in 0..2 {
            let s = p.make_quad(None, out);
            let n = p.surfaces.len();
            h.exec(&mut p, Command::AddSurface { surface: s, index: n }, None);
        }
        let mut cue = p.capture_cue("B", 1.0);
        cue.surfaces[0].visible = false;
        cue.surfaces[1].visible = true;
        cue.surfaces[1].opacity = 0.8;
        p.surfaces[1].visible = false;
        (p, h, cue)
    }

    fn run(p: &mut Project, h: &mut History, cue: &Cue) {
        let (start, steps) = cue_start(p, cue);
        h.exec(p, start, Some(42));
        for i in 1..=10 {
            let frame = fade_frame(p, &steps, i as f32 / 10.0);
            h.exec(p, frame, Some(42));
        }
    }

    #[test]
    fn fade_reaches_cue_state() {
        let (mut p, mut h, cue) = setup();
        run(&mut p, &mut h, &cue);
        assert!(!p.surfaces[0].visible);
        assert_eq!(p.surfaces[0].opacity, 1.0, "dold yta behåller sin opacitet");
        assert!(p.surfaces[1].visible);
        assert_eq!(p.surfaces[1].opacity, 0.8);
    }

    #[test]
    fn fade_midway() {
        let (p, _, cue) = setup();
        let (_, steps) = cue_start(&p, &cue);
        let Command::Batch(cmds) = fade_frame(&p, &steps, 0.5) else { panic!() };
        let Command::ReplaceSurface(a) = &cmds[0] else { panic!() };
        assert!((a.opacity - 0.5).abs() < 1e-6);
    }

    #[test]
    fn whole_go_is_one_undo_step() {
        let (mut p, mut h, cue) = setup();
        let before = p.clone();
        run(&mut p, &mut h, &cue);
        assert_ne!(p, before);
        h.undo(&mut p);
        assert_eq!(p, before);
        h.redo(&mut p);
        assert!(p.surfaces[1].visible && !p.surfaces[0].visible);
    }

    #[test]
    fn lamps_fade_with_the_cue_and_undo() {
        let (mut p, mut h, _) = setup();
        let lamp = p.make_lamp(LampKind::Rgb);
        h.exec(&mut p, Command::SetLamps(vec![lamp]), None);
        let mut cue = p.capture_cue("Rött", 1.0);
        cue.lamps[0].color = [1.0, 0.0, 0.0];
        cue.lamps[0].level = 0.5;
        let before = p.clone();
        let (start, fade) = cue_start(&p, &cue);
        h.exec(&mut p, start, Some(7));
        let mid = fade_frame(&p, &fade, 0.5);
        h.exec(&mut p, mid, Some(7));
        assert!((p.lamps[0].level - 0.75).abs() < 1e-5, "halvvägs: {}", p.lamps[0].level);
        let end = fade_frame(&p, &fade, 1.0);
        h.exec(&mut p, end, Some(7));
        assert_eq!(p.lamps[0].color, [1.0, 0.0, 0.0]);
        h.undo(&mut p);
        assert_eq!(p, before);
    }

    #[test]
    fn instant_cue_has_no_steps() {
        let (mut p, mut h, mut cue) = setup();
        cue.fade = 0.0;
        let (start, steps) = cue_start(&p, &cue);
        assert!(steps.is_empty());
        h.exec(&mut p, start, None);
        assert!(p.surfaces[1].visible && !p.surfaces[0].visible);
    }
}
