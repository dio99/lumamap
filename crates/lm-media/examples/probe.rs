//! Provar en mediekälla utan gränssnitt och skriver ut när första bildrutan kommer.
//!
//!   cargo run -p lm-media --example probe -- camera /dev/video0
//!   cargo run -p lm-media --example probe -- stream udp://127.0.0.1:5000
//!   cargo run -p lm-media --example probe -- file video.mp4
//!   cargo run -p lm-media --example probe -- file video.mp4 2.0   (med hastighet)
//!   cargo run -p lm-media --example probe -- cameras

use lm_media::{MediaSource, VideoSource};
use std::time::{Duration, Instant};

fn main() {
    lm_media::init().unwrap();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (kind, target) = (args.first().map(String::as_str), args.get(1).map(String::as_str).unwrap_or(""));
    let mut src = match kind {
        Some("cameras") => {
            for c in lm_media::list_cameras() {
                println!("{}  {}", c.device, c.name);
            }
            return;
        }
        Some("camera") => VideoSource::camera(target),
        Some("stream") => VideoSource::stream(target, true),
        Some("file") => {
            let mut v = VideoSource::open(std::path::Path::new(target), false, true);
            if let Some(speed) = args.get(2).and_then(|s| s.parse().ok()) {
                v.set_speed(speed);
            }
            v
        }
        _ => {
            eprintln!("Användning: probe camera|stream|file <adress>  eller  probe cameras");
            std::process::exit(2);
        }
    };
    let start = Instant::now();
    let mut frames = 0;
    let mut measure = None;
    while start.elapsed() < Duration::from_secs(20) {
        let mut got = None;
        src.poll(&mut |f| got = Some((f.width, f.height)));
        if let Some((w, h)) = got {
            frames += 1;
            if frames == 1 {
                println!("Första bildrutan efter {:.2} s: {w} × {h}", start.elapsed().as_secs_f32());
            }
            // Takten mäts efter uppstarten (buffring, autoexponering).
            if frames == 30 {
                measure = Some(Instant::now());
            }
            if frames == 120 {
                let fps = 90.0 / measure.unwrap().elapsed().as_secs_f32();
                println!("{fps:.1} bildrutor/s");
                if let Some(pos) = src.position() {
                    println!("Spelat {pos:.2} s video på {:.2} s", start.elapsed().as_secs_f32());
                }
                return;
            }
        }
        if let Some(e) = src.error() {
            println!("Fel: {e}");
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("Timeout: {frames} bildrutor på 20 s");
    std::process::exit(1);
}
