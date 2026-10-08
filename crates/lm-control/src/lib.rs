//! Fjärrstyrning via OSC över UDP. En bakgrundstråd tar emot paket, tolkar
//! dem till `ControlMsg` och skickar dem via en kanal till huvudtråden, som
//! översätter dem till samma kommandon som musen ger.
//!
//! Adresser (namn får skrivas med `_` i stället för mellanslag, eller som ID):
//!
//! ```text
//! /lumamap/source/<namn>/play
//! /lumamap/source/<namn>/pause
//! /lumamap/source/<namn>/seek        f   (sekunder)
//! /lumamap/surface/<namn>/opacity    f   (0..1)
//! /lumamap/surface/<namn>/visible    i
//! /lumamap/cue/<nummer eller namn>/go
//! /lumamap/cue/next
//! /lumamap/cue/prev
//! /lumamap/master/opacity            f   (0 = svart)
//! /lumamap/blackout                  i
//! ```

use rosc::{OscMessage, OscPacket, OscType};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 12345;

#[derive(Debug, Clone, PartialEq)]
pub enum ControlMsg {
    SourcePlay(String),
    SourcePause(String),
    SourceSeek(String, f64),
    SurfaceOpacity(String, f32),
    SurfaceVisible(String, bool),
    CueGo(String),
    CueNext,
    CuePrev,
    MasterOpacity(f32),
    Blackout(bool),
}

/// Tolkar ett OSC-paket (meddelande eller bunt). Okända adresser ignoreras.
pub fn parse(packet: &OscPacket) -> Vec<ControlMsg> {
    match packet {
        OscPacket::Message(m) => parse_message(m).into_iter().collect(),
        OscPacket::Bundle(b) => b.content.iter().flat_map(parse).collect(),
    }
}

fn number(m: &OscMessage) -> Option<f64> {
    match m.args.first()? {
        OscType::Float(f) => Some(*f as f64),
        OscType::Double(f) => Some(*f),
        OscType::Int(i) => Some(*i as f64),
        OscType::Long(i) => Some(*i as f64),
        OscType::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Knappar (t.ex. i TouchOSC) skickar 1 när de trycks och 0 när de släpps.
/// Bara tryckningen räknas; ett meddelande utan argument räknas också.
fn pressed(m: &OscMessage) -> bool {
    number(m).is_none_or(|v| v > 0.5)
}

fn parse_message(m: &OscMessage) -> Option<ControlMsg> {
    let parts: Vec<&str> = m.addr.trim_matches('/').split('/').collect();
    let name = |s: &str| s.to_string();
    let msg = match parts.as_slice() {
        ["lumamap", "source", n, "play"] if pressed(m) => ControlMsg::SourcePlay(name(n)),
        ["lumamap", "source", n, "pause"] if pressed(m) => ControlMsg::SourcePause(name(n)),
        ["lumamap", "source", n, "seek"] => ControlMsg::SourceSeek(name(n), number(m)?.max(0.0)),
        ["lumamap", "surface", n, "opacity"] => ControlMsg::SurfaceOpacity(name(n), number(m)?.clamp(0.0, 1.0) as f32),
        ["lumamap", "surface", n, "visible"] => ControlMsg::SurfaceVisible(name(n), number(m)? > 0.5),
        ["lumamap", "cue", "next"] if pressed(m) => ControlMsg::CueNext,
        ["lumamap", "cue", "prev"] if pressed(m) => ControlMsg::CuePrev,
        ["lumamap", "cue", n, "go"] if pressed(m) => ControlMsg::CueGo(name(n)),
        ["lumamap", "master", "opacity"] => ControlMsg::MasterOpacity(number(m)?.clamp(0.0, 1.0) as f32),
        ["lumamap", "blackout"] => ControlMsg::Blackout(number(m).is_none_or(|v| v > 0.5)),
        _ => return None,
    };
    Some(msg)
}

/// Jämför ett namn från en OSC-adress med ett namn i projektet:
/// skiftlägesokänsligt och med `_` som mellanslag.
pub fn name_matches(osc: &str, name: &str) -> bool {
    let norm = |s: &str| s.trim().to_lowercase().replace(' ', "_");
    norm(osc) == norm(name)
}

/// Lyssnar på en UDP-port i en bakgrundstråd. Tråden stängs när servern släpps.
pub struct OscServer {
    port: u16,
    rx: Receiver<ControlMsg>,
    stop: Arc<AtomicBool>,
}

impl OscServer {
    pub fn start(port: u16) -> Result<Self, String> {
        let socket = UdpSocket::bind(("0.0.0.0", port)).map_err(|e| format!("{} {port}: {e}", lm_core::i18n::t("Kunde inte öppna OSC-porten", "Could not open OSC port")))?;
        socket
            .set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::Builder::new()
            .name("osc".into())
            .spawn(move || {
                let mut buf = [0u8; rosc::decoder::MTU];
                while !stopped.load(Ordering::Relaxed) {
                    let Ok((n, from)) = socket.recv_from(&mut buf) else { continue };
                    match rosc::decoder::decode_udp(&buf[..n]) {
                        Ok((_, packet)) => {
                            for msg in parse(&packet) {
                                if tx.send(msg).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(e) => log::warn!("{} {from}: {e:?}", lm_core::i18n::t("Ogiltigt OSC-paket från", "Invalid OSC packet from")),
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(OscServer { port, rx, stop })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Alla meddelanden som kommit sedan förra anropet.
    pub fn poll(&self) -> Vec<ControlMsg> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(m) => out.push(m),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
            }
        }
    }
}

impl Drop for OscServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::OscBundle;

    fn msg(addr: &str, args: Vec<OscType>) -> OscPacket {
        OscPacket::Message(OscMessage { addr: addr.into(), args })
    }

    #[test]
    fn parses_addresses() {
        assert_eq!(parse(&msg("/lumamap/source/intro/play", vec![])), vec![ControlMsg::SourcePlay("intro".into())]);
        assert_eq!(
            parse(&msg("/lumamap/surface/Vägg_A/opacity", vec![OscType::Float(0.5)])),
            vec![ControlMsg::SurfaceOpacity("Vägg_A".into(), 0.5)]
        );
        assert_eq!(
            parse(&msg("/lumamap/surface/3/visible", vec![OscType::Int(0)])),
            vec![ControlMsg::SurfaceVisible("3".into(), false)]
        );
        assert_eq!(parse(&msg("/lumamap/cue/2/go", vec![])), vec![ControlMsg::CueGo("2".into())]);
        assert_eq!(parse(&msg("/lumamap/cue/next", vec![OscType::Float(1.0)])), vec![ControlMsg::CueNext]);
        assert_eq!(
            parse(&msg("/lumamap/master/opacity", vec![OscType::Double(2.0)])),
            vec![ControlMsg::MasterOpacity(1.0)]
        );
        assert!(parse(&msg("/annat/program", vec![])).is_empty());
        assert!(parse(&msg("/lumamap/surface/a/opacity", vec![])).is_empty());
    }

    #[test]
    fn button_release_is_ignored() {
        assert!(parse(&msg("/lumamap/cue/next", vec![OscType::Float(0.0)])).is_empty());
        assert!(parse(&msg("/lumamap/source/a/play", vec![OscType::Int(0)])).is_empty());
    }

    #[test]
    fn bundles_are_unpacked() {
        let b = OscPacket::Bundle(OscBundle {
            timetag: (0, 1).into(),
            content: vec![msg("/lumamap/blackout", vec![OscType::Int(1)]), msg("/lumamap/cue/prev", vec![])],
        });
        assert_eq!(parse(&b), vec![ControlMsg::Blackout(true), ControlMsg::CuePrev]);
    }

    #[test]
    fn names() {
        assert!(name_matches("vägg_a", "Vägg A"));
        assert!(!name_matches("vägg_b", "Vägg A"));
    }

    #[test]
    fn receives_over_udp() {
        // Hitta en ledig port.
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let server = OscServer::start(port).unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let bytes = rosc::encoder::encode(&msg("/lumamap/cue/1/go", vec![])).unwrap();
        sender.send_to(&bytes, ("127.0.0.1", port)).unwrap();
        let mut got = Vec::new();
        for _ in 0..50 {
            got.extend(server.poll());
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(got, vec![ControlMsg::CueGo("1".into())]);
    }
}
