//! Tonal's own scene painter (src/scenes) for tonalplayer.com, so the site shows the
//! same living themes the app paints. The page reads pixels straight from memory.
#[allow(dead_code)]
#[path = "../../src/scenes/mod.rs"]
mod scenes;

use scenes::{Audio, Preset, Scene};
use std::cell::RefCell;

thread_local! {
    static OUT: RefCell<(Vec<u8>, u32, u32, u32)> = const { RefCell::new((Vec::new(), 0, 0, 0)) };
    static INFO: RefCell<String> = const { RefCell::new(String::new()) };
    static CUSTOM: RefCell<[u32; 5]> = RefCell::new(Preset::default().custom);
}

fn look(index: u32) -> Option<Preset> {
    let gallery = scenes::gallery();
    let mut look = match gallery.get(index as usize) {
        Some(look) => look.clone(),
        None if index as usize == gallery.len() => Preset {
            name: scenes::CUSTOM.into(),
            scene: Scene::Aurora,
            palette: scenes::CUSTOM.into(),
            seed: 11,
            ..Preset::default()
        },
        None => return None,
    };
    look.custom = CUSTOM.with(|c| *c.borrow());
    if look.palette == scenes::ALBUM {
        look.tint = Some([140, 120, 220]);
    }
    Some(look)
}

fn hex(c: [f32; 3]) -> String {
    let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The gallery as JSON, one line per theme: name, scene, style, five colours and the accent.
#[unsafe(no_mangle)]
pub extern "C" fn info() -> *const u8 {
    let gallery = scenes::gallery();
    let rows: Vec<String> = (0..=gallery.len() as u32)
        .filter_map(look)
        .map(|l| {
            let colors: Vec<String> = l.colors().iter().map(|c| format!("\"{}\"", hex(*c))).collect();
            let [r, g, b] = l.accent();
            format!(
                "{{\"name\":\"{}\",\"scene\":\"{}\",\"style\":\"{}\",\"palette\":\"{}\",\"colors\":[{}],\"accent\":\"#{r:02x}{g:02x}{b:02x}\"}}",
                l.name,
                l.scene.label(),
                l.style.label(),
                l.palette,
                colors.join(",")
            )
        })
        .collect();
    INFO.with(|i| {
        let mut i = i.borrow_mut();
        *i = format!("[{}]", rows.join(","));
        i.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn info_len() -> u32 {
    INFO.with(|i| i.borrow().len() as u32)
}

/// Sets one of the five "Your colors" colours, as 0xRRGGBB.
#[unsafe(no_mangle)]
pub extern "C" fn set_custom(slot: u32, rgb: u32) {
    CUSTOM.with(|c| {
        if let Some(v) = c.borrow_mut().get_mut(slot as usize) {
            *v = rgb & 0xff_ffff;
        }
    });
}

/// Paints theme `index` for a `w` by `h` view at `time` seconds, and returns RGBA pixels at
/// the size the app paints at (about half), read with `width()` and `height()`.
/// `bass`, `high` and `energy` are the music's levels from 0 to 1.
#[unsafe(no_mangle)]
pub extern "C" fn paint(index: u32, w: u32, h: u32, time: f32, bass: f32, high: f32, energy: f32) -> *const u8 {
    let Some(look) = look(index) else {
        return std::ptr::null();
    };
    let (cw, ch) = scenes::canvas_size(w.max(1), h.max(1), scenes::detail(look.scene));
    let audio = Audio::shaped([bass, 0.0, high, energy], look.react, look.strength);
    let mut canvas = scenes::Canvas::default();
    canvas.resize(cw, ch);
    scenes::paint_look(&mut canvas, &look, time, audio);
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        o.3 = o.3.wrapping_add(1);
        let frame = o.3;
        o.0.resize(cw * ch * 4, 255);
        scenes::upscale(&canvas, &mut o.0, cw, ch, 4, look.grain, frame);
        for a in o.0.iter_mut().skip(3).step_by(4) {
            *a = 255;
        }
        o.1 = cw as u32;
        o.2 = ch as u32;
        o.0.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn width() -> u32 {
    OUT.with(|o| o.borrow().1)
}

#[unsafe(no_mangle)]
pub extern "C" fn height() -> u32 {
    OUT.with(|o| o.borrow().2)
}
