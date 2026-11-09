//! `.tonaltheme` files: a theme as TOML, plus its picture or video, in one zip to share.
use crate::scenes::{self, Preset, Scene, media};
use anyhow::{Context, bail};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

const THEME: &str = "theme.toml";
const MAX_MEDIA: u64 = 1 << 30;

pub fn export(look: &Preset, to: &Path) -> anyhow::Result<()> {
    let mut zip = zip::ZipWriter::new(File::create(to)?);
    let stored = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    zip.start_file(THEME, stored)?;
    zip.write_all(toml::to_string_pretty(look)?.as_bytes())?;
    if let Some(path) = media::path(&look.media).filter(|p| p.exists()) {
        zip.start_file(format!("media/{}", look.media), stored)?;
        std::io::copy(&mut File::open(path)?, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

/// Reads a `.tonaltheme` file or a plain `.toml` theme.
pub fn import(from: &Path) -> anyhow::Result<Preset> {
    let ext = from
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext == "toml" {
        return parse(&std::fs::read_to_string(from)?);
    }
    let mut zip =
        zip::ZipArchive::new(File::open(from)?).context("That isn't a Tonal theme file")?;
    let mut text = String::new();
    zip.by_name(THEME)
        .context("That isn't a Tonal theme file")?
        .take(1 << 20)
        .read_to_string(&mut text)?;
    let look: Preset = toml::from_str(&text).context("That theme file is broken")?;
    if media::is_safe_name(&look.media) {
        let target = media::dir().join(&look.media);
        if !target.exists() {
            let mut entry = zip
                .by_name(&format!("media/{}", look.media))
                .context("The theme's picture is missing from the file")?;
            if entry.size() > MAX_MEDIA {
                bail!("The theme's picture is over 1 GB");
            }
            std::fs::create_dir_all(media::dir())?;
            let part = target.with_extension("part");
            std::io::copy(&mut (&mut entry).take(MAX_MEDIA), &mut File::create(&part)?)?;
            std::fs::rename(&part, &target)?;
        }
    }
    finish(look)
}

fn parse(text: &str) -> anyhow::Result<Preset> {
    finish(toml::from_str(text).context("That theme file is broken")?)
}

/// Keeps every value in range, and copies in a picture a hand-written file names by its path.
fn finish(mut look: Preset) -> anyhow::Result<Preset> {
    look.name = look.name.trim().chars().take(40).collect();
    if look.name.is_empty() {
        look.name = "Imported".into();
    }
    if !look.media.is_empty() && !media::is_safe_name(&look.media) {
        let file = Path::new(&look.media).to_path_buf();
        let (name, rgb) = media::import(&file)?;
        look.media = name;
        look.media_rgb = Some(rgb);
    }
    if !look.media.is_empty() {
        let poster =
            media::poster(&look.media).context("Tonal couldn't open the theme's picture")?;
        look.media_rgb
            .get_or_insert_with(|| media::main_color(&poster));
    }
    if look.scene == Scene::Picture && look.media.is_empty() {
        look.scene = Scene::Classic;
    }
    if look.palette != scenes::CUSTOM && !scenes::PALETTES.iter().any(|p| p.name == look.palette) {
        look.palette = scenes::PALETTES[0].name.into();
    }
    look.speed = look.speed.clamp(0.1, 3.0);
    look.strength = look.strength.clamp(0.0, 1.0);
    look.dim = look.dim.clamp(0.0, 0.85);
    look.grain = look.grain.clamp(0.0, 1.0);
    if !scenes::FPS.contains(&look.fps) {
        look.fps = 15;
    }
    if !scenes::INTERVALS.contains(&look.rotate_minutes) {
        look.rotate_minutes = 15;
    }
    Ok(look)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_survives_a_file_and_bad_values_are_fixed() {
        let look = Preset {
            name: "Mine".into(),
            scene: Scene::Aurora,
            palette: scenes::CUSTOM.into(),
            custom: [0x101010, 0x203040, 0x406080, 0x80c0ff, 0xffffff],
            ..Preset::default()
        };
        let dir = std::env::temp_dir().join(format!("tonal-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mine.tonaltheme");
        export(&look, &file).unwrap();
        assert_eq!(import(&file).unwrap(), look);
        let text = toml::to_string_pretty(&look).unwrap();
        assert!(text.contains("\"#80c0ff\""), "{text}");
        let wild = text
            .replace("dim = 0.3", "dim = 9.0")
            .replace("scene = \"Aurora\"", "scene = \"Your own\"");
        let fixed = parse(&wild).unwrap();
        assert_eq!(fixed.dim, 0.85);
        assert_eq!(fixed.scene, Scene::Classic);
        assert!(parse("media = \"../../etc/passwd\"").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn share_codes_carry_custom_colours_and_old_codes_still_load() {
        let look = Preset {
            palette: scenes::CUSTOM.into(),
            custom: [0x010203, 0x040506, 0x070809, 0x0a0b0c, 0xfffefd],
            scene: Scene::Tide,
            ..Preset::default()
        };
        let back = Preset::from_code(&look.to_code()).unwrap();
        assert_eq!(
            (back.palette.as_str(), back.custom),
            (scenes::CUSTOM, look.custom)
        );
        let old =
            Preset::from_code("tonal1.1.1.1.1.11.100.60.28.35.15.0.15:Northern lights").unwrap();
        assert_eq!((old.scene, old.palette.as_str()), (Scene::Aurora, "Boreal"));
    }
}
