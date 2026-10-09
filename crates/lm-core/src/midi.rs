//! Vad en MIDI-händelse ska göra med de kopplingar som finns. Rent och
//! testbart; appen utför effekterna.

use crate::model::{MidiAction, MidiBinding, MidiControl};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MidiEffect {
    /// Fader/ratt: sätt nivån (0..1).
    Level(f32),
    /// Knapp trycktes ned (stigande flank): utlös eller växla.
    Press,
    /// Fader på en på/av-åtgärd: av under mitten, på över.
    Switch(bool),
}

/// Kommer ihåg vilka knappar som är nedtryckta, så att ett tryck bara
/// räknas en gång även om kontrollen skickar flera meddelanden.
#[derive(Default)]
pub struct MidiState {
    pressed: HashSet<MidiControl>,
}

impl MidiState {
    pub fn handle(&mut self, bindings: &[MidiBinding], control: MidiControl, value: f32) -> Vec<(MidiAction, MidiEffect)> {
        let down = value >= 0.5;
        let rising = down && !self.pressed.contains(&control);
        if down {
            self.pressed.insert(control);
        } else {
            self.pressed.remove(&control);
        }
        let is_note = matches!(control, MidiControl::Note { .. });
        bindings
            .iter()
            .filter(|b| b.control == control)
            .filter_map(|b| {
                let effect = match b.action {
                    MidiAction::Master | MidiAction::SurfaceOpacity(_) | MidiAction::SourceSpeed(_) => MidiEffect::Level(value),
                    MidiAction::Blackout | MidiAction::SurfaceVisible(_) if !is_note => MidiEffect::Switch(down),
                    _ if rising => MidiEffect::Press,
                    _ => return None,
                };
                Some((b.action, effect))
            })
            .collect()
    }
}

/// Fader 0..1 till hastighet 0,25–4× (logaritmiskt, mitten = 1×).
pub fn speed_from_level(level: f32) -> f32 {
    0.25 * 16f32.powf(level.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CueId, SurfaceId};

    const FADER: MidiControl = MidiControl::Cc { channel: 0, number: 7 };
    const PAD: MidiControl = MidiControl::Note { channel: 9, number: 36 };

    fn bind(control: MidiControl, action: MidiAction) -> MidiBinding {
        MidiBinding { control, action }
    }

    #[test]
    fn fader_sets_level() {
        let b = [bind(FADER, MidiAction::Master)];
        let mut s = MidiState::default();
        assert_eq!(s.handle(&b, FADER, 0.25), vec![(MidiAction::Master, MidiEffect::Level(0.25))]);
    }

    #[test]
    fn button_triggers_once_per_press() {
        let b = [bind(PAD, MidiAction::CueNext)];
        let mut s = MidiState::default();
        assert_eq!(s.handle(&b, PAD, 0.8).len(), 1);
        // Tryck-känsliga knappar kan skicka igen medan de hålls ned.
        assert!(s.handle(&b, PAD, 0.9).is_empty());
        assert!(s.handle(&b, PAD, 0.0).is_empty());
        assert_eq!(s.handle(&b, PAD, 0.7), vec![(MidiAction::CueNext, MidiEffect::Press)]);
    }

    #[test]
    fn on_off_actions() {
        let surface = SurfaceId(3);
        let b = [bind(PAD, MidiAction::SurfaceVisible(surface)), bind(FADER, MidiAction::Blackout)];
        let mut s = MidiState::default();
        assert_eq!(s.handle(&b, PAD, 1.0), vec![(MidiAction::SurfaceVisible(surface), MidiEffect::Press)]);
        assert_eq!(s.handle(&b, FADER, 0.9), vec![(MidiAction::Blackout, MidiEffect::Switch(true))]);
        assert_eq!(s.handle(&b, FADER, 0.1), vec![(MidiAction::Blackout, MidiEffect::Switch(false))]);
    }

    #[test]
    fn only_matching_control() {
        let b = [bind(PAD, MidiAction::CueGo(CueId(1)))];
        let mut s = MidiState::default();
        assert!(s.handle(&b, FADER, 1.0).is_empty());
    }

    #[test]
    fn speed_curve() {
        assert!((speed_from_level(0.0) - 0.25).abs() < 1e-6);
        assert!((speed_from_level(0.5) - 1.0).abs() < 1e-6);
        assert!((speed_from_level(1.0) - 4.0).abs() < 1e-6);
    }
}
