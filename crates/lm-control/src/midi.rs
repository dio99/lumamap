//! MIDI från Linux råa MIDI-enheter (`/dev/snd/midiC*D*`), utan ALSA-biblioteket.
//! Fungerar med USB-kontroller; enheter som kopplas in eller ur hittas automatiskt.

use std::collections::HashSet;
use std::io::Read;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use lm_core::MidiControl;

/// Ett meddelande från en kontroll, med värdet 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidiEvent {
    pub control: MidiControl,
    pub value: f32,
}

/// Tolkar en MIDI-byteström: statusbyte, löpande status och realtidsbyte.
#[derive(Default)]
pub struct MidiParser {
    status: u8,
    data: [u8; 2],
    count: usize,
}

impl MidiParser {
    pub fn feed(&mut self, byte: u8) -> Option<MidiEvent> {
        if byte >= 0xF8 {
            return None; // Realtid (klocka, start/stopp) – kan komma mitt i ett meddelande.
        }
        if byte & 0x80 != 0 {
            // Ny status. Systemmeddelanden (0xF0..) avbryter löpande status.
            self.status = if byte < 0xF0 { byte } else { 0 };
            self.count = 0;
            return None;
        }
        if self.status == 0 {
            return None;
        }
        let kind = self.status & 0xF0;
        let needed = if matches!(kind, 0xC0 | 0xD0) { 1 } else { 2 };
        self.data[self.count] = byte;
        self.count += 1;
        if self.count < needed {
            return None;
        }
        self.count = 0; // Löpande status: nästa databyte börjar ett nytt meddelande.
        let channel = self.status & 0x0F;
        let [a, b] = self.data;
        match kind {
            0x90 => Some(MidiEvent {
                control: MidiControl::Note { channel, number: a },
                value: b as f32 / 127.0, // Note On med hastighet 0 betyder släppt.
            }),
            0x80 => Some(MidiEvent {
                control: MidiControl::Note { channel, number: a },
                value: 0.0,
            }),
            0xB0 => Some(MidiEvent {
                control: MidiControl::Cc { channel, number: a },
                value: b as f32 / 127.0,
            }),
            _ => None,
        }
    }
}

/// Lyssnar på alla råa MIDI-enheter i bakgrunden.
pub struct MidiInput {
    rx: Receiver<MidiEvent>,
    devices: Arc<Mutex<Vec<String>>>,
}

impl MidiInput {
    pub fn start() -> MidiInput {
        let (tx, rx) = mpsc::channel();
        let devices = Arc::new(Mutex::new(Vec::new()));
        let list = devices.clone();
        let _ = std::thread::Builder::new().name("midi".into()).spawn(move || scan(tx, list));
        MidiInput { rx, devices }
    }

    pub fn poll(&self) -> Vec<MidiEvent> {
        self.rx.try_iter().collect()
    }

    /// Namn på anslutna enheter.
    pub fn devices(&self) -> Vec<String> {
        self.devices.lock().unwrap().clone()
    }
}

/// Letar efter nya enheter varannan sekund och startar en läsartråd per enhet.
/// Lever lika länge som programmet.
fn scan(tx: Sender<MidiEvent>, devices: Arc<Mutex<Vec<String>>>) {
    let open: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    loop {
        let mut found: Vec<String> = std::fs::read_dir("/dev/snd")
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("midiC"))
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        for node in &found {
            if open.lock().unwrap().contains(node) {
                continue;
            }
            let Ok(file) = std::fs::File::open(format!("/dev/snd/{node}")) else { continue };
            open.lock().unwrap().insert(node.clone());
            let (tx, open, node) = (tx.clone(), open.clone(), node.clone());
            let _ = std::thread::Builder::new().name(format!("midi {node}")).spawn(move || {
                read_device(file, &tx);
                // Enheten kopplades ur (eller kanalen stängdes).
                open.lock().unwrap().remove(&node);
            });
        }
        let names: Vec<String> = open.lock().unwrap().iter().map(|n| device_name(n)).collect();
        *devices.lock().unwrap() = names;
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn read_device(mut file: std::fs::File, tx: &Sender<MidiEvent>) {
    let mut parser = MidiParser::default();
    let mut buf = [0u8; 256];
    while let Ok(n) = file.read(&mut buf) {
        if n == 0 {
            return;
        }
        for &b in &buf[..n] {
            if let Some(e) = parser.feed(b) {
                if tx.send(e).is_err() {
                    return;
                }
            }
        }
    }
}

/// "midiC1D0" → kortets namn från /proc/asound, t.ex. "nanoKONTROL2".
fn device_name(node: &str) -> String {
    let card = node.trim_start_matches("midiC").split('D').next().unwrap_or("");
    std::fs::read_to_string(format!("/proc/asound/card{card}/id"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| node.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bytes: &[u8]) -> Vec<MidiEvent> {
        let mut p = MidiParser::default();
        bytes.iter().filter_map(|b| p.feed(*b)).collect()
    }

    #[test]
    fn control_change_and_notes() {
        let e = parse(&[0xB1, 7, 127, 0x90, 60, 100, 0x80, 60, 0]);
        assert_eq!(e[0], MidiEvent { control: MidiControl::Cc { channel: 1, number: 7 }, value: 1.0 });
        assert_eq!(e[1].control, MidiControl::Note { channel: 0, number: 60 });
        assert!((e[1].value - 100.0 / 127.0).abs() < 1e-6);
        assert_eq!(e[2].value, 0.0);
    }

    #[test]
    fn running_status_and_realtime() {
        // En fader som skickar flera värden med löpande status, med klockbyte inblandade.
        let e = parse(&[0xB0, 1, 0, 0xF8, 1, 64, 1, 0xF8, 127]);
        let values: Vec<f32> = e.iter().map(|e| (e.value * 127.0).round()).collect();
        assert_eq!(values, [0.0, 64.0, 127.0]);
    }

    #[test]
    fn note_on_zero_is_release_and_sysex_is_ignored() {
        let e = parse(&[0xF0, 1, 2, 3, 0xF7, 0x90, 36, 0]);
        assert_eq!(e, vec![MidiEvent { control: MidiControl::Note { channel: 0, number: 36 }, value: 0.0 }]);
    }

    #[test]
    fn program_change_takes_one_byte() {
        // Programbyte (1 databyte) följt av en CC – ska inte förskjuta tolkningen.
        let e = parse(&[0xC0, 5, 0xB0, 10, 20]);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].control, MidiControl::Cc { channel: 0, number: 10 });
    }
}
