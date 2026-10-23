//! Spelar in ett projekt till en videofil, exakt som projektorn visar det –
//! utan fönster. För demofilmer och skärmdumpar.
//!
//!   cargo run --release -p lc-render --example record -- examples/halloween.lmap ut.mp4 8
//!   cargo run --release -p lc-render --example record -- examples/halloween.lmap ut.mp4 8 --cue 6
//!
//! `--cue N` går till cue nummer N (1 = första) direkt, utan övergång.
//! Inspelningen går i realtid (30 bilder/s) så att videorna hinner med.

use lc_core::{cue_start, fade_frame, Project, SourceKind};
use lc_media::{MediaSource, StillSource, VideoSource};
use lc_render::{egui_wgpu, egui_wgpu::wgpu, RenderOptions, Renderer};
use std::io::Write;
use std::process::{Command as Process, Stdio};
use std::time::{Duration, Instant};

const FPS: u32 = 30;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("Användning: record <projekt.lmap> <ut.mp4> <sekunder> [--cue N] [--warmup S]");
        std::process::exit(2);
    }
    let path = std::path::PathBuf::from(&args[0]);
    let out_file = &args[1];
    let seconds: f64 = args[2].parse().expect("sekunder");
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str);
    let cue: Option<usize> = flag("--cue").map(|v| v.parse::<usize>().expect("cue-nummer") - 1);
    let warmup: f64 = flag("--warmup").map_or(0.6, |v| v.parse().expect("sekunder"));
    lc_media::init().unwrap();

    let mut project = Project::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
    project.absolutize_paths(path.parent().unwrap());
    if let Some(i) = cue {
        let c = project.cues.get(i).cloned().expect("cuen finns inte");
        let (start, steps) = cue_start(&project, &c);
        start.apply(&mut project);
        fade_frame(&project, &steps, 1.0).apply(&mut project);
    }

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("inget grafikkort");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let mut egui = egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, egui_wgpu::RendererOptions::default());
    let mut renderer = Renderer::new(&device, &queue, &mut egui);
    let mut opts = RenderOptions::default();
    let output = project.outputs[0].id;
    let [w, h] = project.outputs[0].resolution;

    let mut media: Vec<(lc_core::SourceId, Box<dyn MediaSource>)> = project
        .sources
        .iter()
        .filter_map(|s| {
            let m: Box<dyn MediaSource> = match &s.kind {
                SourceKind::Video { path, .. } => Box::new(VideoSource::open(path, true, true)),
                SourceKind::Image { path } => Box::new(StillSource::image(path)),
                SourceKind::Color { rgba } => Box::new(StillSource::color(*rgba)),
                SourceKind::TestPattern => Box::new(StillSource::test_pattern()),
                // Mönster ritas av renderaren; kameror och strömmar spelas inte in.
                _ => return None,
            };
            Some((s.id, m))
        })
        .collect();

    let mut ffmpeg = Process::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"), "-r", &FPS.to_string(), "-i", "-"])
        .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "14", "-pix_fmt", "yuv420p", out_file])
        .stdin(Stdio::piped())
        .spawn()
        .expect("ffmpeg saknas");
    let mut pipe = ffmpeg.stdin.take().unwrap();

    let bpm = project.settings.bpm as f64;
    let start = Instant::now();
    let total = ((seconds + warmup) * FPS as f64) as u64;
    for frame in 0..total {
        // Vänta in realtid så att videorna hinner spela.
        let due = start + Duration::from_secs_f64(frame as f64 / FPS as f64);
        if let Some(rest) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(rest);
        }
        for (id, m) in &mut media {
            m.poll(&mut |f| renderer.upload(&device, &queue, &mut egui, *id, &f));
        }
        opts.beats = frame as f64 / FPS as f64 * bpm / 60.0;
        renderer.render(&device, &queue, &mut egui, &project, &opts);
        // Första stunden hoppas över medan videorna startar.
        if (frame as f64) < warmup * FPS as f64 {
            continue;
        }
        let (rgba, _) = renderer.read_output(&device, &queue, output).expect("utgången finns inte");
        pipe.write_all(&rgba).expect("ffmpeg avbröts");
    }
    drop(pipe);
    ffmpeg.wait().unwrap();
    println!("{out_file}: {seconds} s");
}
