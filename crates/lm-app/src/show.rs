//! Showkörning: cues med mjuka övergångar, OSC-fjärrstyrning och master-nivå.
//! Allt som ändrar projektet går via samma kommandon som musen.

use crate::app::LumaApp;
use lm_control::{name_matches, ControlMsg, OscServer};
use lm_core::{cue_start, fade_frame, Command, CueId, FadeStep, SourceId, SurfaceId};
use std::time::{Duration, Instant};

/// En pågående övergång till en cue.
pub struct Fade {
    start: Instant,
    duration: f32,
    steps: Vec<FadeStep>,
    gesture: u64,
}

/// OSC-fadrar som rör samma yta inom den här tiden blir ett ångra-steg.
const OSC_GESTURE_GAP: Duration = Duration::from_secs(1);

impl LumaApp {
    // ---------- Cues ----------

    /// Går till en cue. Media byts direkt; synlighet och opacitet tonas över
    /// cuens övergångstid. Hela GO:t blir ett ångra-steg.
    pub fn go_cue(&mut self, id: CueId) {
        let Some(cue) = self.project.cue(id).cloned() else { return };
        self.fade = None;
        self.current_cue = Some(id);
        let gesture = self.new_gesture();
        let (start, steps) = cue_start(&self.project, &cue);
        self.exec(start, Some(gesture));
        if !steps.is_empty() {
            self.fade = Some(Fade {
                start: Instant::now(),
                duration: cue.fade,
                steps,
                gesture,
            });
        }
        self.notify(format!("Cue: {}", cue.name));
    }

    pub fn go_next_cue(&mut self, step: isize) {
        let n = self.project.cues.len() as isize;
        if n == 0 {
            return;
        }
        let current = self.current_cue.and_then(|id| self.project.cues.iter().position(|c| c.id == id));
        let next = match current {
            Some(i) => (i as isize + step).clamp(0, n - 1),
            None => 0,
        };
        if current != Some(next as usize) {
            self.go_cue(self.project.cues[next as usize].id);
        }
    }

    /// Driver en pågående övergång ett steg. Anropas varje bildruta.
    pub fn tick_fade(&mut self) {
        let Some(f) = &self.fade else { return };
        let t = (f.start.elapsed().as_secs_f32() / f.duration).min(1.0);
        let (frame, gesture) = (fade_frame(&self.project, &f.steps, t), f.gesture);
        if t >= 1.0 {
            self.fade = None;
        }
        if !matches!(&frame, Command::Batch(c) if c.is_empty()) {
            self.exec(frame, Some(gesture));
        }
    }

    /// Cue från en OSC-adress: nummer i listan (1, 2, …) eller namn.
    fn find_cue(&self, key: &str) -> Option<CueId> {
        if let Ok(n) = key.parse::<usize>() {
            if let Some(c) = n.checked_sub(1).and_then(|i| self.project.cues.get(i)) {
                return Some(c.id);
            }
        }
        self.project.cues.iter().find(|c| name_matches(key, &c.name)).map(|c| c.id)
    }

    fn find_surface(&self, key: &str) -> Option<SurfaceId> {
        let s = &self.project.surfaces;
        s.iter()
            .find(|s| name_matches(key, &s.name))
            .or_else(|| key.parse::<u32>().ok().and_then(|id| s.iter().find(|s| s.id.0 == id)))
            .map(|s| s.id)
    }

    fn find_source(&self, key: &str) -> Option<SourceId> {
        let s = &self.project.sources;
        s.iter()
            .find(|s| name_matches(key, &s.name))
            .or_else(|| key.parse::<u32>().ok().and_then(|id| s.iter().find(|s| s.id.0 == id)))
            .map(|s| s.id)
    }

    // ---------- OSC ----------

    /// Startar om OSC-servern om inställningarna ändrats och tar hand om nya meddelanden.
    pub fn poll_osc(&mut self) {
        let settings = &self.project.settings;
        let want = settings.osc_enabled.then_some(settings.osc_port);
        if want != self.osc_port_tried {
            self.osc_port_tried = want;
            self.osc = None;
            self.osc_error = None;
            if let Some(port) = want {
                match OscServer::start(port) {
                    Ok(s) => self.osc = Some(s),
                    Err(e) => {
                        log::warn!("{e}");
                        self.osc_error = Some(e);
                    }
                }
            }
        }
        let msgs = self.osc.as_ref().map(|s| s.poll()).unwrap_or_default();
        for msg in msgs {
            self.osc_last = Some((format!("{msg:?}"), Instant::now()));
            self.handle_control(msg);
        }
    }

    fn handle_control(&mut self, msg: ControlMsg) {
        match msg {
            ControlMsg::SourcePlay(n) | ControlMsg::SourcePause(n) | ControlMsg::SourceSeek(n, _) if self.find_source(&n).is_none() => {
                log::warn!("OSC: okänd källa {n}");
            }
            ControlMsg::SourcePlay(n) => {
                let id = self.find_source(&n);
                if let Some(m) = id.and_then(|id| self.media.get_mut(id)) {
                    m.play();
                }
            }
            ControlMsg::SourcePause(n) => {
                let id = self.find_source(&n);
                if let Some(m) = id.and_then(|id| self.media.get_mut(id)) {
                    m.pause();
                }
            }
            ControlMsg::SourceSeek(n, t) => {
                let id = self.find_source(&n);
                if let Some(m) = id.and_then(|id| self.media.get_mut(id)) {
                    m.seek(t);
                }
            }
            ControlMsg::SurfaceOpacity(n, v) => self.osc_surface(&n, |s| s.opacity = v),
            ControlMsg::SurfaceVisible(n, v) => self.osc_surface(&n, |s| s.visible = v),
            ControlMsg::CueGo(key) => match self.find_cue(&key) {
                Some(id) => self.go_cue(id),
                None => log::warn!("OSC: okänd cue {key}"),
            },
            ControlMsg::CueNext => self.go_next_cue(1),
            ControlMsg::CuePrev => self.go_next_cue(-1),
            ControlMsg::MasterOpacity(v) => self.opts.master = v,
            ControlMsg::Blackout(on) => self.opts.blackout = on,
        }
    }

    /// Ändrar en yta från OSC. Snabba fadrar på samma yta blir ett ångra-steg.
    fn osc_surface(&mut self, key: &str, f: impl FnOnce(&mut lm_core::Surface)) {
        let Some(id) = self.find_surface(key) else {
            log::warn!("OSC: okänd yta {key}");
            return;
        };
        let Some(mut s) = self.project.surface(id).cloned() else { return };
        f(&mut s);
        let gesture = match self.osc_gestures.get(&id) {
            Some((g, at)) if at.elapsed() < OSC_GESTURE_GAP => *g,
            _ => self.new_gesture(),
        };
        self.osc_gestures.insert(id, (gesture, Instant::now()));
        self.exec(Command::ReplaceSurface(s), Some(gesture));
    }
}
