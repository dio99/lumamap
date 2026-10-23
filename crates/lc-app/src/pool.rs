//! Håller en levande mediekälla per källa i projektet och laddar upp nya
//! bildrutor till GPU:n varje bildruta.

use lc_core::{Project, SourceId, SourceKind};
use lc_media::{MediaSource, StillSource, VideoSource};
use lc_render::egui_wgpu::{self, wgpu};
use std::collections::HashMap;

struct Entry {
    kind: SourceKind,
    media: Box<dyn MediaSource>,
}

#[derive(Default)]
pub struct MediaPool {
    items: HashMap<SourceId, Entry>,
    /// Senaste medelfärgen per källa (för lampor som följer en källa).
    colors: HashMap<SourceId, [f32; 3]>,
    /// Rörelse i kameror och strömmar.
    motion: HashMap<SourceId, lc_media::motion::MotionDetector>,
}

fn open(kind: &SourceKind) -> Box<dyn MediaSource> {
    match kind {
        SourceKind::Video { path, looping, muted, speed } => {
            let mut v = VideoSource::open(path, *looping, *muted);
            v.set_speed(*speed as f64);
            Box::new(v)
        }
        SourceKind::Image { path } => Box::new(StillSource::image(path)),
        SourceKind::Color { rgba } => Box::new(StillSource::color(*rgba)),
        SourceKind::TestPattern => Box::new(StillSource::test_pattern()),
        SourceKind::Camera { device } => Box::new(VideoSource::camera(device)),
        SourceKind::Stream { uri, muted } => Box::new(VideoSource::stream(uri, *muted)),
        SourceKind::Generator { .. } => unreachable!("mönster ritas av renderaren"),
    }
}

impl MediaPool {
    /// Skapar, uppdaterar och tar bort mediekällor så att de matchar projektet.
    pub fn sync(&mut self, project: &Project, renderer: &mut lc_render::Renderer, egui: &mut egui_wgpu::Renderer) {
        // Mönsterkällor ritas av renderaren och har ingen media här.
        let is_media = |kind: &SourceKind| !matches!(kind, SourceKind::Generator { .. });
        self.items.retain(|id, _| {
            let keep = project.source(*id).is_some_and(|s| is_media(&s.kind));
            if !keep {
                renderer.remove_source(egui, *id);
            }
            keep
        });
        for src in project.sources.iter().filter(|s| is_media(&s.kind)) {
            match self.items.get_mut(&src.id) {
                Some(e) if e.kind == src.kind => {}
                Some(e) => match (&e.kind, &src.kind) {
                    // Bara flaggor ändrade – starta inte om videon.
                    (SourceKind::Video { path: a, .. }, SourceKind::Video { path: b, looping, muted, speed }) if a == b => {
                        e.media.set_looping(*looping);
                        e.media.set_muted(*muted);
                        e.media.set_speed(*speed as f64);
                        e.kind = src.kind.clone();
                    }
                    (SourceKind::Stream { uri: a, .. }, SourceKind::Stream { uri: b, muted }) if a == b => {
                        e.media.set_muted(*muted);
                        e.kind = src.kind.clone();
                    }
                    _ => {
                        renderer.remove_source(egui, src.id);
                        *e = Entry {
                            kind: src.kind.clone(),
                            media: open(&src.kind),
                        };
                    }
                },
                None => {
                    self.items.insert(
                        src.id,
                        Entry {
                            kind: src.kind.clone(),
                            media: open(&src.kind),
                        },
                    );
                }
            }
        }
    }

    pub fn poll(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut lc_render::Renderer,
        egui: &mut egui_wgpu::Renderer,
    ) {
        let (colors, motion) = (&mut self.colors, &mut self.motion);
        for (id, e) in &mut self.items {
            let live = matches!(e.kind, SourceKind::Camera { .. } | SourceKind::Stream { .. });
            e.media.poll(&mut |frame| {
                colors.insert(*id, frame.average_color());
                if live {
                    motion.entry(*id).or_default().feed(frame.luma_grid());
                }
                renderer.upload(device, queue, egui, *id, &frame)
            });
        }
        let items = &self.items;
        self.motion.retain(|id, _| items.contains_key(id));
    }

    /// Öppnar källan på nytt vid nästa `sync` (t.ex. efter tappad ström).
    pub fn restart(&mut self, id: SourceId) {
        self.items.remove(&id);
    }

    /// Rörelsen i en kamera eller ström (andel av bilden), om den visat bilder.
    pub fn motion(&self, id: SourceId) -> Option<f32> {
        self.motion.get(&id).map(|m| m.level())
    }

    /// Källans senaste medelfärg, om den har visat någon bild.
    pub fn average_color(&self, id: SourceId) -> Option<[f32; 3]> {
        self.colors.get(&id).copied()
    }

    pub fn get(&self, id: SourceId) -> Option<&dyn MediaSource> {
        self.items.get(&id).map(|e| e.media.as_ref())
    }

    pub fn get_mut(&mut self, id: SourceId) -> Option<&mut (dyn MediaSource + 'static)> {
        self.items.get_mut(&id).map(|e| e.media.as_mut())
    }

    pub fn for_each_video(&mut self, mut f: impl FnMut(&mut dyn MediaSource)) {
        for e in self.items.values_mut() {
            if e.media.is_video() {
                f(e.media.as_mut());
            }
        }
    }

    pub fn for_each_video_ref(&self, mut f: impl FnMut(&dyn MediaSource)) {
        for e in self.items.values() {
            if e.media.is_video() {
                f(e.media.as_ref());
            }
        }
    }
}
