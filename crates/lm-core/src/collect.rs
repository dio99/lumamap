//! "Samla projekt": kopierar projektet och all media det använder till en
//! mapp, så att den kan flyttas till showdatorn som den är.

use crate::model::Project;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

pub struct Collected {
    /// Den sparade projektfilen.
    pub project_file: PathBuf,
    /// Projektet som det sparades (med sökvägar relativa till mappen).
    pub project: Project,
    /// Antal mediefiler som kopierades.
    pub copied: usize,
}

/// Kopierar `project` och dess media till `dir`: projektfilen som
/// `dir/<file_name>` och media till `dir/media/`. Mediesökvägarna i
/// `project` ska vara absoluta. Samma fil kopieras bara en gång; olika filer
/// med samma namn får ett nummer.
pub fn collect(project: &Project, dir: &Path, file_name: &str) -> io::Result<Collected> {
    let media = dir.join("media");
    std::fs::create_dir_all(&media)?;
    let mut out = project.clone();
    let mut done: HashMap<PathBuf, String> = HashMap::new();
    let mut taken: Vec<String> = Vec::new();
    for s in &mut out.sources {
        let Some(path) = s.kind.path_mut() else { continue };
        let name = match done.get(path.as_path()) {
            Some(n) => n.clone(),
            None => {
                let name = unique_name(path, &taken);
                std::fs::copy(&*path, media.join(&name))
                    .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
                taken.push(name.clone());
                done.insert(path.clone(), name.clone());
                name
            }
        };
        *path = PathBuf::from("media").join(name);
    }
    let project_file = dir.join(file_name);
    let text = out.to_ron().map_err(io::Error::other)?;
    std::fs::write(&project_file, text)?;
    Ok(Collected {
        project_file,
        project: out,
        copied: taken.len(),
    })
}

fn unique_name(path: &Path, taken: &[String]) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "media".into());
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut name = format!("{stem}{ext}");
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{stem}_{n}{ext}");
        n += 1;
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceKind;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumamap-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn copies_media_and_rewrites_paths() {
        let src = temp_dir("collect-src");
        std::fs::create_dir_all(src.join("a")).unwrap();
        std::fs::create_dir_all(src.join("b")).unwrap();
        std::fs::write(src.join("a/clip.mp4"), b"first").unwrap();
        std::fs::write(src.join("b/clip.mp4"), b"second").unwrap();
        std::fs::write(src.join("bild.png"), b"image").unwrap();

        let mut p = Project::new();
        let video = |path: PathBuf| SourceKind::Video { path, looping: true, muted: false, speed: 1.0 };
        for (name, kind) in [
            ("a", video(src.join("a/clip.mp4"))),
            ("b", video(src.join("b/clip.mp4"))),
            ("a igen", video(src.join("a/clip.mp4"))),
            ("bild", SourceKind::Image { path: src.join("bild.png") }),
            ("färg", SourceKind::Color { rgba: [1.0; 4] }),
        ] {
            let s = p.make_source(name, kind);
            p.sources.push(s);
        }

        let out = temp_dir("collect-out");
        let c = collect(&p, &out, "show.lmap").unwrap();
        assert_eq!(c.copied, 3, "samma fil kopieras bara en gång");
        let paths: Vec<_> = c.project.sources.iter().filter_map(|s| s.kind.path()).map(|p| p.to_path_buf()).collect();
        assert_eq!(
            paths,
            ["media/clip.mp4", "media/clip_2.mp4", "media/clip.mp4", "media/bild.png"].map(PathBuf::from)
        );
        assert_eq!(std::fs::read(out.join("media/clip_2.mp4")).unwrap(), b"second");
        // Projektfilen öppnas och pekar på kopiorna.
        let mut back = Project::from_ron(&std::fs::read_to_string(&c.project_file).unwrap()).unwrap();
        back.absolutize_paths(&out);
        assert!(back.sources.iter().filter_map(|s| s.kind.path()).all(|p| p.exists()));

        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn missing_media_is_reported() {
        let mut p = Project::new();
        let s = p.make_source("borta", SourceKind::Image { path: "/finns/inte.png".into() });
        p.sources.push(s);
        let out = temp_dir("collect-missing");
        let err = collect(&p, &out, "show.lmap").err().unwrap();
        assert!(err.to_string().contains("/finns/inte.png"));
        let _ = std::fs::remove_dir_all(&out);
    }
}
