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

/// Kommandot som körs när GO trycks, och stegen att tona (tomt om cuen
/// saknar övergångstid). Media byts direkt; ytor som ska tonas in görs
/// synliga med sin startopacitet.
///
/// Alla cuens ytor tas med i kommandot, även oförändrade: övergångens senare
/// steg slås ihop med det, så ångra-steget måste spara dem alla.
pub fn cue_start(p: &Project, cue: &Cue) -> (Command, Vec<FadeStep>) {
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
    (Command::Batch(cmds), steps)
}

/// Mjuk start och landning.
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Tillståndet vid `t` ∈ [0, 1] av övergången. Vid `t = 1` får ytorna
/// cuens synlighet och opacitet exakt.
pub fn fade_frame(p: &Project, steps: &[FadeStep], t: f32) -> Command {
    let t = t.clamp(0.0, 1.0);
    let e = ease(t);
    let mut cmds = Vec::new();
    for step in steps {
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
    fn instant_cue_has_no_steps() {
        let (mut p, mut h, mut cue) = setup();
        cue.fade = 0.0;
        let (start, steps) = cue_start(&p, &cue);
        assert!(steps.is_empty());
        h.exec(&mut p, start, None);
        assert!(p.surfaces[1].visible && !p.surfaces[0].visible);
    }
}
