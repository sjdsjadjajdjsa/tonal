use super::*;

#[test]
fn share_codes_round_trip_and_reject_junk() {
    for look in gallery() {
        let back = Preset::from_code(&look.to_code()).expect("code reads back");
        assert_eq!(back, look, "{}", look.name);
    }
    assert!(Preset::from_code("hello").is_none());
    assert!(Preset::from_code("tonal1.99.0.0.0.1.100.60.30.35.15.0.15:x").is_none());
}

#[test]
fn album_palette_follows_the_cover() {
    let look = Preset {
        palette: ALBUM.into(),
        scene: Scene::Bloom,
        ..Preset::default()
    };
    let red = Preset {
        tint: Some([200, 30, 30]),
        ..look.clone()
    }
    .colors();
    let blue = Preset {
        tint: Some([30, 60, 200]),
        ..look
    }
    .colors();
    assert!(red[3][0] > red[3][2], "red cover gives red tones");
    assert!(blue[3][2] > blue[3][0], "blue cover gives blue tones");
    assert!(red[0].iter().all(|c| *c < 0.2), "darkest step stays dark");
}

#[test]
fn gallery_paints_every_scene_with_variety() {
    let (w, h) = (480u32, 300u32);
    let looks = gallery();
    let cols = 4u32;
    let rows = (looks.len() as u32).div_ceil(cols);
    let mut sheet = image::RgbImage::new(w * cols, h * rows);
    let mut renderer = Renderer::default();
    let mut slowest = (0.0f64, "");
    for (i, look) in looks.iter().enumerate() {
        let audio = Audio {
            pulse: 0.4,
            sparkle: 0.4,
            flow: 0.0,
        };
        let start = std::time::Instant::now();
        let frame = renderer.frame(look, None, 37.0, audio, (1240, 780));
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if ms > slowest.0 {
            slowest = (ms, look.scene.label());
        }
        let small = renderer.frame(look, None, 37.0, audio, (w, h));
        let bytes = small.image.as_bytes();
        let lumas: Vec<u32> = bytes
            .chunks(3)
            .map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32)
            .collect();
        let (lo, hi) = (lumas.iter().min().unwrap(), lumas.iter().max().unwrap());
        assert!(
            hi - lo > 40 || look.scene == Scene::Classic,
            "{} is flat",
            look.name
        );
        let other = Preset {
            seed: look.seed + 1,
            ..look.clone()
        };
        let varied = renderer.frame(&other, None, 37.0, audio, (w, h));
        assert!(
            look.scene == Scene::Classic || varied.image.as_bytes() != bytes,
            "{} ignores its seed",
            look.name
        );
        assert_eq!(frame.image.width(), 1240);
        let (ox, oy) = ((i as u32 % cols) * w, (i as u32 / cols) * h);
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 3) as usize;
                sheet.put_pixel(
                    ox + x,
                    oy + y,
                    image::Rgb([bytes[o], bytes[o + 1], bytes[o + 2]]),
                );
            }
        }
    }
    eprintln!("slowest full frame: {:.1} ms ({})", slowest.0, slowest.1);
    if let Ok(path) = std::env::var("SCENES_SHEET") {
        sheet.save(path).unwrap();
    }
}
