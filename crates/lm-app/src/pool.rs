//! Håller en levande mediekälla per källa i projektet och laddar upp nya
//! bildrutor till GPU:n varje bildruta.

use lm_core::{Project, SourceId, SourceKind};
use lm_media::{MediaSource, StillSource, VideoSource};
use lm_render::egui_wgpu::{self, wgpu};
use std::collections::HashMap;

struct Entry {
    kind: SourceKind,
    media: Box<dyn MediaSource>,
}

#[derive(Default)]
pub struct MediaPool {
    items: HashMap<SourceId, Entry>,
}

fn open(kind: &SourceKind) -> Box<dyn MediaSource> {
    match kind {
        SourceKind::Video { path, looping, muted } => Box::new(VideoSource::open(path, *looping, *muted)),
        SourceKind::Image { path } => Box::new(StillSource::image(path)),
        SourceKind::Color { rgba } => Box::new(StillSource::color(*rgba)),
        SourceKind::TestPattern => Box::new(StillSource::test_pattern()),
        SourceKind::Camera { device } => Box::new(VideoSource::camera(device)),
        SourceKind::Stream { uri, muted } => Box::new(VideoSource::stream(uri, *muted)),
    }
}

impl MediaPool {
    /// Skapar, uppdaterar och tar bort mediekällor så att de matchar projektet.
    pub fn sync(&mut self, project: &Project, renderer: &mut lm_render::Renderer, egui: &mut egui_wgpu::Renderer) {
        self.items.retain(|id, _| {
            let keep = project.source(*id).is_some();
            if !keep {
                renderer.remove_source(egui, *id);
            }
            keep
        });
        for src in &project.sources {
            match self.items.get_mut(&src.id) {
                Some(e) if e.kind == src.kind => {}
                Some(e) => match (&e.kind, &src.kind) {
                    // Bara flaggor ändrade – starta inte om videon.
                    (SourceKind::Video { path: a, .. }, SourceKind::Video { path: b, looping, muted }) if a == b => {
                        e.media.set_looping(*looping);
                        e.media.set_muted(*muted);
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
        renderer: &mut lm_render::Renderer,
        egui: &mut egui_wgpu::Renderer,
    ) {
        for (id, e) in &mut self.items {
            e.media.poll(&mut |frame| renderer.upload(device, queue, egui, *id, &frame));
        }
    }

    /// Öppnar källan på nytt vid nästa `sync` (t.ex. efter tappad ström).
    pub fn restart(&mut self, id: SourceId) {
        self.items.remove(&id);
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
}
