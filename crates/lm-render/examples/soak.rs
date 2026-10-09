//! Långtidstest utan fönster: spelar ett projekts videor och renderar dess
//! utgångar i 60 bilder/s, och skriver ut minnesanvändningen varje minut.
//! Växande minne tyder på en läcka i uppladdning, rendering eller GStreamer.
//!
//!   cargo run --release -p lm-render --example soak -- examples/showcase.lmap 600

use lm_core::{Project, SourceKind};
use lm_media::{MediaSource, StillSource, VideoSource};
use lm_render::{egui_wgpu, egui_wgpu::wgpu, RenderOptions, Renderer};
use std::time::{Duration, Instant};

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS")).map(str::to_owned))
        .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()))
        .map_or(0.0, |kb| kb / 1024.0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(args.first().map(String::as_str).unwrap_or("examples/showcase.lmap"));
    let seconds: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    lm_media::init().unwrap();

    let mut project = Project::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
    project.absolutize_paths(path.parent().unwrap());

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("inget grafikkort");
    println!("GPU: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let mut egui = egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, egui_wgpu::RendererOptions::default());
    let mut renderer = Renderer::new(&device, &queue, &mut egui);
    let opts = RenderOptions::default();

    let mut media: Vec<(lm_core::SourceId, Box<dyn MediaSource>)> = project
        .sources
        .iter()
        .map(|s| {
            let m: Box<dyn MediaSource> = match &s.kind {
                SourceKind::Video { path, .. } => Box::new(VideoSource::open(path, true, true)),
                SourceKind::Image { path } => Box::new(StillSource::image(path)),
                SourceKind::Color { rgba } => Box::new(StillSource::color(*rgba)),
                _ => Box::new(StillSource::test_pattern()),
            };
            (s.id, m)
        })
        .collect();

    let start = Instant::now();
    let mut next_report = Duration::ZERO;
    let (mut frames, mut uploads) = (0u64, 0u64);
    let mut first_rss = None;
    while start.elapsed() < Duration::from_secs(seconds) {
        let frame_start = Instant::now();
        for (id, m) in &mut media {
            m.poll(&mut |f| {
                renderer.upload(&device, &queue, &mut egui, *id, &f);
                uploads += 1;
            });
            if let Some(e) = m.error() {
                panic!("källa {id:?}: {e}");
            }
        }
        renderer.render(&device, &queue, &mut egui, &project, &opts);
        let _ = device.poll(wgpu::PollType::Poll);
        frames += 1;
        if start.elapsed() >= next_report {
            let rss = rss_mb();
            // Första minuten räknas som uppvärmning (buffrar, cacher).
            if first_rss.is_none() && start.elapsed() >= Duration::from_secs(60) {
                first_rss = Some(rss);
            }
            println!(
                "{:>4} s  {frames:>6} bildrutor  {uploads:>6} uppladdningar  RSS {rss:6.1} MB",
                start.elapsed().as_secs()
            );
            next_report += Duration::from_secs(60);
        }
        if let Some(rest) = Duration::from_millis(16).checked_sub(frame_start.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    let end = rss_mb();
    if let Some(base) = first_rss {
        println!("Efter uppvärmning: {base:.1} MB → {end:.1} MB ({:+.1} MB)", end - base);
    }
}
