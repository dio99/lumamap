//! Ljudanalys för ljudreaktiva effekter: basnivå och taktslag från
//! mikrofonen eller det datorn själv spelar.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::sync::{Arc, Mutex};

const RATE: f32 = 44100.0;

/// Varifrån ljudet kommer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioInputKind {
    /// Standardingången (mikrofon eller linjeingång).
    Microphone,
    /// Det datorn spelar (PipeWire/PulseAudio-monitor).
    Computer,
}

/// Senaste analysen.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AudioLevels {
    /// Basens nivå, 0..1 (anpassar sig efter volymen).
    pub bass: f32,
    /// Total nivå, 0..1.
    pub level: f32,
    /// Ökar med ett för varje taktslag som hörs.
    pub beats: u64,
}

/// Analyserar ljud i block: lågpassfiltrerad bas, automatisk nivå och taktslag.
pub struct AudioAnalyzer {
    lowpass: f32,
    alpha: f32,
    /// Basens kuvert: likriktad och utjämnad (30 ms), så att även toner
    /// lägre än blocklängden ger en jämn kurva.
    envelope: f32,
    env_alpha: f32,
    /// Långsamt sjunkande toppvärden för automatisk nivå.
    bass_peak: f32,
    level_peak: f32,
    /// Medelenergi i basen, att jämföra nya block mot.
    bass_average: f32,
    since_beat: f32,
    /// Basen ligger över tröskeln; ett nytt slag kräver att den först sjunker.
    above: bool,
    pub levels: AudioLevels,
}

impl Default for AudioAnalyzer {
    fn default() -> Self {
        // Enpoligt lågpassfilter runt 150 Hz.
        let alpha = 1.0 - (-std::f32::consts::TAU * 150.0 / RATE).exp();
        AudioAnalyzer {
            lowpass: 0.0,
            alpha,
            envelope: 0.0,
            env_alpha: 1.0 - (-1.0 / (0.03 * RATE)).exp(),
            bass_peak: 1e-4,
            level_peak: 1e-4,
            bass_average: 0.0,
            since_beat: 1.0,
            above: false,
            levels: AudioLevels::default(),
        }
    }
}

impl AudioAnalyzer {
    /// Ett block monosampel (−1..1) i 44,1 kHz.
    pub fn process(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let mut energy = 0.0f32;
        for &s in samples {
            self.lowpass += self.alpha * (s - self.lowpass);
            self.envelope += self.env_alpha * (self.lowpass.abs() - self.envelope);
            energy += s * s;
        }
        let n = samples.len() as f32;
        let bass = self.envelope;
        let level = (energy / n).sqrt();
        let dt = n / RATE;

        // Automatisk nivå: toppen sjunker sakta (halveras på ~4 s).
        let decay = 0.5f32.powf(dt / 4.0);
        self.bass_peak = (self.bass_peak * decay).max(bass).max(1e-4);
        self.level_peak = (self.level_peak * decay).max(level).max(1e-4);
        let bass_norm = (bass / self.bass_peak).clamp(0.0, 1.0);
        // Snabb attack, mjukare släpp – så att effekterna inte fladdrar.
        let release = 0.5f32.powf(dt / 0.12);
        self.levels.bass = if bass_norm > self.levels.bass { bass_norm } else { self.levels.bass * release + bass_norm * (1.0 - release) };
        self.levels.level = (level / self.level_peak).clamp(0.0, 1.0);

        // Taktslag: basen stiger över sitt medel (inte bara ligger över det),
        // minst 0,3 s sedan förra. Hysteres: sjunker under 1,2 × medel för att räknas igen.
        self.since_beat += dt;
        let loud_enough = bass > self.bass_peak * 0.3;
        if !self.above && loud_enough && bass > self.bass_average * 1.5 && self.since_beat > 0.3 {
            self.levels.beats += 1;
            self.since_beat = 0.0;
            self.above = true;
        } else if bass < self.bass_average * 1.2 {
            self.above = false;
        }
        let smoothing = 0.5f32.powf(dt / 0.5);
        self.bass_average = self.bass_average * smoothing + bass * (1.0 - smoothing);
    }
}

/// Lyssnar på ljud via GStreamer och analyserar det i bakgrunden.
pub struct AudioInput {
    pipeline: gst::Element,
    analyzer: Arc<Mutex<AudioAnalyzer>>,
    pub error: Option<String>,
}

impl AudioInput {
    pub fn start(kind: AudioInputKind) -> AudioInput {
        match kind {
            AudioInputKind::Microphone => Self::start_element("autoaudiosrc"),
            // PipeWire och PulseAudio känner igen monitorn för standardutgången.
            AudioInputKind::Computer => Self::start_device("@DEFAULT_MONITOR@"),
        }
    }

    /// En viss PulseAudio/PipeWire-källa, t.ex. för felsökning.
    pub fn start_device(device: &str) -> AudioInput {
        Self::start_element(&format!("pulsesrc device=\"{}\"", device.replace('"', "")))
    }

    fn start_element(src: &str) -> AudioInput {
        let analyzer = Arc::new(Mutex::new(AudioAnalyzer::default()));
        let desc = format!(
            "{src} ! audioconvert ! audioresample ! audio/x-raw,format=F32LE,channels=1,rate=44100 ! appsink name=sink sync=false max-buffers=4 drop=true"
        );
        let result = (|| -> Result<gst::Element, String> {
            let pipeline = gst::parse::launch(&desc).map_err(|e| e.to_string())?;
            let bin = pipeline.clone().downcast::<gst::Bin>().map_err(|_| "ingen pipeline")?;
            let sink = bin
                .by_name("sink")
                .and_then(|e| e.downcast::<gst_app::AppSink>().ok())
                .ok_or("appsink saknas")?;
            let a = analyzer.clone();
            sink.set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |sink| {
                        let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        if let Some(map) = sample.buffer().and_then(|b| b.map_readable().ok()) {
                            let samples: Vec<f32> = map.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
                            a.lock().unwrap().process(&samples);
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );
            pipeline.set_state(gst::State::Playing).map_err(|e| e.to_string())?;
            Ok(pipeline)
        })();
        match result {
            Ok(pipeline) => AudioInput { pipeline, analyzer, error: None },
            Err(e) => {
                log::warn!("Ljud: {e}");
                AudioInput {
                    pipeline: gst::ElementFactory::make("fakesink").build().expect("fakesink"),
                    analyzer,
                    error: Some(e),
                }
            }
        }
    }

    pub fn levels(&mut self) -> AudioLevels {
        // Fel som uppstår efter start (t.ex. ingen ljudingång) syns på bussen.
        if let Some(bus) = self.pipeline.bus() {
            while let Some(msg) = bus.pop() {
                if let gst::MessageView::Error(e) = msg.view() {
                    self.error = Some(e.error().to_string());
                }
            }
        }
        self.analyzer.lock().unwrap().levels
    }
}

impl Drop for AudioInput {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, amp: f32, seconds: f32) -> Vec<f32> {
        (0..(RATE * seconds) as usize).map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / RATE).sin()).collect()
    }

    fn feed(a: &mut AudioAnalyzer, signal: &[f32]) {
        for block in signal.chunks(512) {
            a.process(block);
        }
    }

    #[test]
    fn bass_tone_gives_high_bass_treble_does_not() {
        let mut a = AudioAnalyzer::default();
        feed(&mut a, &tone(60.0, 0.5, 1.0));
        assert!(a.levels.bass > 0.8, "bas: {}", a.levels.bass);
        let mut b = AudioAnalyzer::default();
        // Först lite bas så att den automatiska nivån har något att jämföra med.
        feed(&mut b, &tone(60.0, 0.5, 0.5));
        feed(&mut b, &tone(4000.0, 0.5, 1.0));
        assert!(b.levels.bass < 0.2, "diskant ska inte räknas som bas: {}", b.levels.bass);
    }

    #[test]
    fn kick_drum_beats_are_counted() {
        // 120 BPM: en kort basstöt var 0,5 s, tystnad emellan.
        let mut signal = Vec::new();
        for _ in 0..8 {
            signal.extend(tone(55.0, 0.8, 0.1));
            signal.extend(vec![0.0; (RATE * 0.4) as usize]);
        }
        let mut a = AudioAnalyzer::default();
        feed(&mut a, &signal);
        assert!((7..=8).contains(&a.levels.beats), "slag: {}", a.levels.beats);
    }

    #[test]
    fn steady_tone_is_not_a_beat_stream() {
        let mut a = AudioAnalyzer::default();
        feed(&mut a, &tone(60.0, 0.5, 4.0));
        assert!(a.levels.beats <= 1, "en jämn ton är inga taktslag: {}", a.levels.beats);
    }
}
