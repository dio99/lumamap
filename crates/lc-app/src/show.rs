//! Showkörning: cues med mjuka övergångar, OSC-fjärrstyrning och master-nivå.
//! Allt som ändrar projektet går via samma kommandon som musen.

use crate::app::LumaApp;
use lc_core::i18n::t;
use lc_control::{name_matches, ControlMsg, OscServer};
use lc_core::midi::MidiEffect;
use lc_core::{cue_start, fade_frame, Command, CueFade, CueId, MidiAction, MidiBinding, MidiControl, SourceId, SourceKind, SurfaceId};
use std::time::{Duration, Instant};

/// En pågående övergång till en cue.
pub struct Fade {
    start: Instant,
    duration: f32,
    steps: CueFade,
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
        self.cue_started = Some(Instant::now());
        let gesture = self.new_gesture();
        // Ytor som byter media tonar över under cuens övergångstid.
        for cs in &cue.surfaces {
            let old = self.project.surface(cs.surface).and_then(|s| s.source);
            if let Some(old) = old.filter(|o| Some(*o) != cs.source) {
                self.start_media_fade(cs.surface, old, cue.fade);
            }
        }
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
            // Framåt följer "börja om efter sista".
            Some(i) if step == 1 => self.project.next_cue_index(Some(i)).map_or(i as isize, |j| j as isize),
            Some(i) => (i as isize + step).clamp(0, n - 1),
            None => 0,
        };
        if current != Some(next as usize) {
            self.go_cue(self.project.cues[next as usize].id);
        }
    }

    /// Låter ytan tona över från `old` till sitt nya media på `seconds` sekunder.
    pub fn start_media_fade(&mut self, surface: SurfaceId, old: SourceId, seconds: f32) {
        if seconds > 0.01 {
            self.media_fades.insert(surface, (old, Instant::now(), seconds));
        } else {
            self.media_fades.remove(&surface);
        }
    }

    /// Räknar fram övertoningarnas läge till renderaren. Anropas varje bildruta.
    pub fn tick_media_fades(&mut self) {
        self.media_fades.retain(|_, (_, start, secs)| start.elapsed().as_secs_f32() < *secs);
        self.opts.crossfades = self
            .media_fades
            .iter()
            .map(|(id, (old, start, secs))| {
                let t = (start.elapsed().as_secs_f32() / secs).clamp(0.0, 1.0);
                // Mjuk start och slut.
                (*id, (*old, t * t * (3.0 - 2.0 * t)))
            })
            .collect();
    }

    /// Cues med "gå vidare efter" går till nästa när tiden är ute.
    pub fn tick_auto_cue(&mut self) {
        let Some(i) = self.current_cue.and_then(|id| self.project.cues.iter().position(|c| c.id == id)) else { return };
        let (Some(auto), Some(started)) = (self.project.cues[i].auto, self.cue_started) else { return };
        // Inte medan schemat håller showen släckt.
        if self.schedule_on == Some(false) || started.elapsed().as_secs_f32() < auto {
            return;
        }
        match self.project.next_cue_index(Some(i)) {
            Some(j) => {
                let id = self.project.cues[j].id;
                self.go_cue(id);
            }
            None => self.cue_started = None,
        }
    }

    /// Sekunder kvar tills en automatisk cue går vidare.
    pub fn auto_remaining(&self, id: CueId) -> Option<f32> {
        let cue = self.project.cue(id)?;
        let started = self.cue_started.filter(|_| self.current_cue == Some(id))?;
        Some((cue.auto? - started.elapsed().as_secs_f32()).max(0.0))
    }

    /// Schemat: släcker och pausar utanför tiderna, tänder och startar första
    /// cuen när det blir dags. Kollas en gång per sekund.
    pub fn tick_schedule(&mut self) {
        if self.schedule_checked.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.schedule_checked = Instant::now();
        let schedule = self.project.settings.schedule;
        if !schedule.enabled {
            if self.schedule_on == Some(false) {
                self.opts.blackout = false;
            }
            self.schedule_on = None;
            return;
        }
        let now = jiff::Zoned::now();
        let on = schedule.is_on(now.hour() as u16 * 60 + now.minute() as u16);
        if self.schedule_on == Some(on) {
            return;
        }
        let first = self.schedule_on.is_none();
        self.schedule_on = Some(on);
        if on {
            if first {
                return;
            }
            self.opts.blackout = false;
            self.media.for_each_video(|m| m.play());
            if let Some(c) = self.project.cues.first() {
                let id = c.id;
                self.go_cue(id);
            }
            self.notify(t("Schemat: showen startar", "Schedule: the show starts"));
        } else {
            self.opts.blackout = true;
            self.media.for_each_video(|m| m.pause());
            self.notify(format!("{} {}", t("Schemat: svart till", "Schedule: black until"), lc_core::Schedule::format(schedule.on)));
        }
    }

    /// Mobil-fjärrkontrollen: startar/stoppar servern efter inställningarna,
    /// utför det mobilen ber om och skickar läget dit.
    pub fn poll_remote(&mut self) {
        let settings = &self.project.settings;
        let want = settings.remote_enabled.then(|| (settings.remote_port, self.remote_token.clone()));
        if want != self.remote_tried {
            self.remote_tried = want.clone();
            self.remote = None;
            self.remote_error = None;
            if let Some((port, token)) = want {
                match lc_control::remote::RemoteServer::start(port, token) {
                    Ok(s) => self.remote = Some(s),
                    Err(e) => {
                        log::warn!("{e}");
                        self.remote_error = Some(e);
                    }
                }
            }
        }
        let cmds = self.remote.as_ref().map(|s| s.poll()).unwrap_or_default();
        for cmd in cmds {
            self.remote_last = Some(Instant::now());
            use lc_control::remote::RemoteCmd;
            match cmd {
                RemoteCmd::Go => self.go_next_cue(1),
                RemoteCmd::Prev => self.go_next_cue(-1),
                RemoteCmd::Cue(i) => {
                    if let Some(id) = self.project.cues.get(i).map(|c| c.id) {
                        self.go_cue(id);
                    }
                }
                RemoteCmd::Blackout(on) => self.opts.blackout = on,
                RemoteCmd::Master(v) => self.opts.master = v,
                RemoteCmd::PlayPause => self.toggle_play_all(),
            }
            // Svara snabbt med det nya läget.
            self.remote_state_at = Instant::now() - Duration::from_secs(1);
        }
        if self.remote.is_some() && self.remote_state_at.elapsed() >= Duration::from_millis(250) {
            self.remote_state_at = Instant::now();
            let json = self.remote_state();
            if let Some(s) = &self.remote {
                s.set_state(json);
            }
        }
    }

    fn remote_state(&self) -> String {
        use lc_control::remote::{state_json, CueInfo};
        let cues: Vec<CueInfo> = self.project.cues.iter().map(|c| CueInfo { name: &c.name, fade: c.fade, auto: c.auto }).collect();
        let current = self.current_cue.and_then(|id| self.project.cues.iter().position(|c| c.id == id));
        let next = self.project.next_cue_index(current).filter(|n| Some(*n) != current);
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| t("Namnlöst projekt", "Untitled project").into());
        let mut playing = false;
        self.media.for_each_video_ref(|m| playing |= m.is_playing());
        state_json(&name, &cues, current, next, self.opts.blackout, self.opts.master, playing)
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
            ControlMsg::SourcePlay(n) | ControlMsg::SourcePause(n) | ControlMsg::SourceSeek(n, _) | ControlMsg::SourceSpeed(n, _)
                if self.find_source(&n).is_none() =>
            {
                log::warn!("OSC: {} {n}", t("okänd källa", "unknown source"));
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
            ControlMsg::SourceSpeed(n, v) => {
                if let Some(id) = self.find_source(&n) {
                    self.set_source_speed(id, v);
                }
            }
            ControlMsg::SurfaceOpacity(n, v) => self.osc_surface(&n, |s| s.opacity = v),
            ControlMsg::SurfaceVisible(n, v) => self.osc_surface(&n, |s| s.visible = v),
            ControlMsg::CueGo(key) => match self.find_cue(&key) {
                Some(id) => self.go_cue(id),
                None => log::warn!("OSC: {} {key}", t("okänd cue", "unknown cue")),
            },
            ControlMsg::CueNext => self.go_next_cue(1),
            ControlMsg::CuePrev => self.go_next_cue(-1),
            ControlMsg::MasterOpacity(v) => self.opts.master = v,
            ControlMsg::Blackout(on) => self.opts.blackout = on,
        }
    }

    /// Ändrar en yta från OSC.
    fn osc_surface(&mut self, key: &str, f: impl FnOnce(&mut lc_core::Surface)) {
        match self.find_surface(key) {
            Some(id) => self.remote_surface(id, f),
            None => log::warn!("OSC: {} {key}", t("okänd yta", "unknown surface")),
        }
    }

    /// Ändrar en yta från OSC eller MIDI. Snabba fadrar på samma yta blir ett ångra-steg.
    fn remote_surface(&mut self, id: SurfaceId, f: impl FnOnce(&mut lc_core::Surface)) {
        let Some(mut s) = self.project.surface(id).cloned() else { return };
        f(&mut s);
        let gesture = self.osc_gesture(id.0);
        self.exec(Command::ReplaceSurface(s), Some(gesture));
    }

    fn set_source_speed(&mut self, id: SourceId, v: f32) {
        let Some(mut src) = self.project.source(id).cloned() else { return };
        if let SourceKind::Video { speed, .. } = &mut src.kind {
            *speed = v.clamp(lc_core::SPEED_MIN, lc_core::SPEED_MAX);
            let gesture = self.osc_gesture(id.0);
            self.exec(Command::ReplaceSource(src), Some(gesture));
        }
    }

    // ---------- DMX ----------

    /// Skickar lampornas kanaler med Art-Net, 30 gånger per sekund.
    pub fn send_dmx(&mut self) {
        let settings = &self.project.settings.artnet;
        if !settings.enabled || self.dmx_sent.elapsed() < Duration::from_millis(33) {
            return;
        }
        self.dmx_sent = Instant::now();
        let master = if self.opts.blackout { 0.0 } else { self.opts.master };
        let media = &self.media;
        let universe = lc_core::dmx_universe(&self.project.lamps, master, |l| {
            l.follow.and_then(|id| media.average_color(id)).unwrap_or(l.color)
        });
        let mut universes = std::collections::BTreeMap::new();
        universes.insert(settings.universe, universe);
        // LED-slingorna: bilden innehåller redan master och Svart.
        let colors: Vec<Vec<[f32; 3]>> = self.project.led_strips.iter().map(|s| self.led_colors.get(&s.id).cloned().unwrap_or_default()).collect();
        lc_core::led_universes(&self.project.led_strips, &colors, 1.0, &mut universes);
        if self.artnet.is_none() {
            match lc_control::artnet::ArtNetSender::new() {
                Ok(s) => self.artnet = Some(s),
                Err(e) => {
                    self.dmx_error = Some(e.to_string());
                    return;
                }
            }
        }
        if let Some(sender) = &mut self.artnet {
            self.dmx_error = None;
            for (u, data) in &universes {
                if let Err(e) = sender.send(&settings.target, *u, data) {
                    self.dmx_error = Some(e.to_string());
                }
            }
        }
    }

    // ---------- Ljud ----------

    /// Startar om ljudingången när inställningen ändras, och låter basen och
    /// taktslagen driva effekterna.
    pub fn poll_audio(&mut self) {
        use lc_media::audio::{AudioInput, AudioInputKind};
        let want = self.project.settings.audio;
        if want != self.audio_running {
            self.audio_running = want;
            self.audio = match want {
                lc_core::AudioSetting::Off => None,
                lc_core::AudioSetting::Microphone => Some(AudioInput::start(AudioInputKind::Microphone)),
                lc_core::AudioSetting::Computer => Some(AudioInput::start(AudioInputKind::Computer)),
            };
            self.audio_beats_seen = 0;
            self.beat_times.clear();
            self.audio_bpm = None;
        }
        let Some(input) = &mut self.audio else {
            self.opts.audio = 0.0;
            self.audio_level = 0.0;
            return;
        };
        let levels = input.levels();
        self.opts.audio = levels.bass;
        self.audio_level = levels.level;
        if levels.beats > self.audio_beats_seen {
            self.audio_beats_seen = levels.beats;
            // Lägg effekternas takt på slaget.
            self.opts.beats = self.opts.beats.round();
            let now = Instant::now();
            self.beat_times.push_back(now);
            while self.beat_times.len() > 9 {
                self.beat_times.pop_front();
            }
            self.audio_bpm = tempo_from_beats(&self.beat_times);
        }
        // Inga slag på ett tag: tillbaka till det inställda tempot.
        if self.beat_times.back().is_some_and(|t| t.elapsed() > Duration::from_secs(4)) {
            self.beat_times.clear();
            self.audio_bpm = None;
        }
    }

    // ---------- MIDI ----------

    /// Tar hand om MIDI-händelser: inlärning av en ny koppling, eller
    /// utför de kopplingar som finns.
    pub fn poll_midi(&mut self) {
        for e in self.midi.poll() {
            self.midi_last = Some((e, Instant::now()));
            if let Some(action) = self.midi_learn {
                // Att släppa en knapp räknas inte som att röra den.
                if matches!(e.control, MidiControl::Note { .. }) && e.value < 0.5 {
                    continue;
                }
                self.midi_learn = None;
                let mut settings = self.project.settings.clone();
                settings.midi.retain(|b| b.control != e.control);
                settings.midi.push(MidiBinding { control: e.control, action });
                self.exec(Command::ReplaceSettings(settings), None);
                // Trycket som lärde in kopplingen ska inte också utlösa den.
                self.midi_state.handle(&[], e.control, e.value);
                self.notify(format!("{}: {} → {}", t("MIDI kopplad", "MIDI mapped"), e.control.label(), self.midi_action_label(action)));
                continue;
            }
            let effects = self.midi_state.handle(&self.project.settings.midi, e.control, e.value);
            for (action, effect) in effects {
                self.apply_midi(action, effect);
            }
        }
    }

    fn apply_midi(&mut self, action: MidiAction, effect: MidiEffect) {
        use MidiEffect::{Level, Press, Switch};
        match (action, effect) {
            (MidiAction::Master, Level(v)) => self.opts.master = v,
            (MidiAction::Blackout, Press) => self.opts.blackout = !self.opts.blackout,
            (MidiAction::Blackout, Switch(on)) => self.opts.blackout = on,
            (MidiAction::CueNext, Press) => self.go_next_cue(1),
            (MidiAction::CuePrev, Press) => self.go_next_cue(-1),
            (MidiAction::CueGo(id), Press) => self.go_cue(id),
            (MidiAction::SurfaceOpacity(id), Level(v)) => self.remote_surface(id, |s| s.opacity = v),
            (MidiAction::SurfaceVisible(id), Press) => self.remote_surface(id, |s| s.visible = !s.visible),
            (MidiAction::SurfaceVisible(id), Switch(on)) => self.remote_surface(id, |s| s.visible = on),
            (MidiAction::SourcePlayPause(id), Press) => {
                if let Some(m) = self.media.get_mut(id) {
                    if m.is_playing() {
                        m.pause();
                    } else {
                        m.play();
                    }
                }
            }
            (MidiAction::SourceSpeed(id), Level(v)) => self.set_source_speed(id, lc_core::midi::speed_from_level(v)),
            _ => {}
        }
    }

    /// Läsbar beskrivning av en MIDI-åtgärd, med namn ur projektet.
    pub fn midi_action_label(&self, action: MidiAction) -> String {
        let gone = || t("(borttagen)", "(deleted)").to_string();
        let surface = |id: SurfaceId| self.project.surface(id).map_or_else(gone, |s| s.name.clone());
        let source = |id: SourceId| self.project.source(id).map_or_else(gone, |s| s.name.clone());
        match action {
            MidiAction::Master => "Master".into(),
            MidiAction::Blackout => t("Svart", "Black").into(),
            MidiAction::CueNext => t("Nästa cue", "Next cue").into(),
            MidiAction::CuePrev => t("Föregående cue", "Previous cue").into(),
            MidiAction::CueGo(id) => format!("GO {}", self.project.cue(id).map_or_else(gone, |c| c.name.clone())),
            MidiAction::SurfaceOpacity(id) => format!("{}: {}", surface(id), t("opacitet", "opacity")),
            MidiAction::SurfaceVisible(id) => format!("{}: {}", surface(id), t("synlig", "visible")),
            MidiAction::SourcePlayPause(id) => format!("{}: {}", source(id), t("spela/paus", "play/pause")),
            MidiAction::SourceSpeed(id) => format!("{}: {}", source(id), t("hastighet", "speed")),
        }
    }

    /// Samma gest-nummer så länge OSC rör samma yta eller källa i en följd.
    fn osc_gesture(&mut self, id: u32) -> u64 {
        let gesture = match self.osc_gestures.get(&id) {
            Some((g, at)) if at.elapsed() < OSC_GESTURE_GAP => *g,
            _ => self.new_gesture(),
        };
        self.osc_gestures.insert(id, (gesture, Instant::now()));
        gesture
    }
}

/// Tempo ur avstånden mellan de senaste slagen (medianen, så att enstaka
/// missade eller extra slag inte stör). `None` tills det finns ett stabilt.
pub fn tempo_from_beats(times: &std::collections::VecDeque<Instant>) -> Option<f32> {
    if times.len() < 5 {
        return None;
    }
    let mut gaps: Vec<f32> = times.iter().zip(times.iter().skip(1)).map(|(a, b)| b.duration_since(*a).as_secs_f32()).collect();
    gaps.sort_by(f32::total_cmp);
    let median = gaps[gaps.len() / 2];
    let bpm = 60.0 / median;
    // Ett vanligt dansgolvsområde; utanför det är det troligen halv/dubbel takt.
    (60.0..=200.0).contains(&bpm).then(|| bpm.round())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn tempo_from_steady_and_noisy_beats() {
        let start = Instant::now();
        let at = |s: f32| start + Duration::from_secs_f32(s);
        let steady: VecDeque<_> = (0..8).map(|i| at(i as f32 * 0.5)).collect();
        assert_eq!(tempo_from_beats(&steady), Some(120.0));
        // Ett extra slag mitt emellan ändrar inte medianen.
        let mut noisy: Vec<_> = (0..8).map(|i| at(i as f32 * 0.5)).collect();
        noisy.insert(4, at(1.75));
        assert_eq!(tempo_from_beats(&noisy.into_iter().collect()), Some(120.0));
        let few: VecDeque<_> = (0..3).map(|i| at(i as f32 * 0.5)).collect();
        assert_eq!(tempo_from_beats(&few), None);
    }
}
