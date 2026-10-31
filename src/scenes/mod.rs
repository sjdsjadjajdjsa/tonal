mod depth;
mod draw;
mod painters;
#[cfg(test)]
mod tests;

use draw::{C, Canvas, hex, rand, shift_hue};
use serde::{Deserialize, Serialize};
use slint::{Rgb8Pixel, Rgba8Pixel, SharedPixelBuffer};

pub struct Palette {
    pub name: &'static str,
    pub colors: [u32; 5],
}

pub const PALETTES: &[Palette] = &[
    Palette {
        name: "Mono",
        colors: [0x0b0b0d, 0x1b1b20, 0x3c3c44, 0x9a9aa4, 0xececee],
    },
    Palette {
        name: "Boreal",
        colors: [0x030a14, 0x0a1d33, 0x134e5e, 0x3ddc97, 0x7cf5ff],
    },
    Palette {
        name: "Saffron",
        colors: [0x1a0f12, 0x5b2a2a, 0xc4683a, 0xf2b45a, 0xffd98a],
    },
    Palette {
        name: "Lagoon",
        colors: [0x04121c, 0x0b3047, 0x11698e, 0x19a7ce, 0xb8f3ff],
    },
    Palette {
        name: "Moss",
        colors: [0x0a120e, 0x1f3527, 0x4a6b52, 0x9bb7a0, 0xe3f2c1],
    },
    Palette {
        name: "Peach",
        colors: [0x2a1420, 0x7a3b5e, 0xe0707a, 0xffb38a, 0xffe3c2],
    },
    Palette {
        name: "Prism",
        colors: [0x0c0a1a, 0x2a2f6b, 0x6b5bd6, 0x45d0e6, 0xff9be8],
    },
    Palette {
        name: "Outrun",
        colors: [0x0d0221, 0x261447, 0xff2a6d, 0xffb86b, 0x05d9e8],
    },
    Palette {
        name: "Sumi",
        colors: [0x0e0e10, 0x26262b, 0x5c5c66, 0xb8b2a7, 0xe2533f],
    },
    Palette {
        name: "Nebula",
        colors: [0x05030c, 0x1d0b3b, 0x5a189a, 0xe0469b, 0xffd2f0],
    },
    Palette {
        name: "Sakura",
        colors: [0x2b1b2e, 0x6d4a6e, 0xe7a3c0, 0xffd1e1, 0xfff1f6],
    },
    Palette {
        name: "Glacier",
        colors: [0x07141f, 0x12324a, 0x3f7ea6, 0xa3d5f0, 0xf0fbff],
    },
    Palette {
        name: "Riso",
        colors: [0x141018, 0x2b2140, 0xff5e5b, 0xffd23f, 0x00cecb],
    },
    Palette {
        name: "Dusk",
        colors: [0x120c1f, 0x3b2357, 0x8e4c7f, 0xf08a5d, 0xffc996],
    },
    Palette {
        name: "Tokyo",
        colors: [0x05060d, 0x11132a, 0x2c2f6b, 0xff3cac, 0x2be4ff],
    },
    Palette {
        name: "Cyber",
        colors: [0x020407, 0x04161f, 0x00a8a8, 0x00ffd5, 0xf5ff7a],
    },
    Palette {
        name: "Ember",
        colors: [0x0f0605, 0x3b0d0a, 0xa8261b, 0xff7a2f, 0xffd27a],
    },
    Palette {
        name: "Lavender",
        colors: [0x0f0b1a, 0x2a1f45, 0x6c5ba7, 0xb9a7ff, 0xf0e8ff],
    },
    Palette {
        name: ALBUM,
        colors: [0x0d0d12, 0x262433, 0x5f5a7a, 0xa9a3c9, 0xeeeaf8],
    },
];

/// The palette that takes its colours from the playing song's cover.
pub const ALBUM: &str = "From the album";

static ALBUM_TINT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// Records the playing song's cover colour, or none.
pub fn set_album_tint(rgb: Option<[u8; 3]>) {
    let packed = rgb.map_or(u32::MAX, |[r, g, b]| u32::from_be_bytes([0, r, g, b]));
    ALBUM_TINT.store(packed, std::sync::atomic::Ordering::Relaxed);
}

pub fn album_tint() -> Option<[u8; 3]> {
    let packed = ALBUM_TINT.load(std::sync::atomic::Ordering::Relaxed);
    let [_, r, g, b] = packed.to_be_bytes();
    (packed != u32::MAX).then_some([r, g, b])
}

/// Five steps from near black to a pale tint, all in the cover's hue.
fn album_colors(rgb: [u8; 3]) -> [C; 5] {
    let mut c = rgb.map(|v| f32::from(v) / 255.0);
    let top = c.iter().fold(0.0f32, |a, &b| a.max(b));
    if top < 0.6 {
        c = draw::scale(c, 0.6 / top.max(0.05));
    }
    let white = [1.0; 3];
    [
        draw::scale(c, 0.07),
        draw::scale(c, 0.24),
        draw::scale(c, 0.58),
        draw::mix(c, white, 0.12),
        draw::mix(c, white, 0.68),
    ]
}

pub fn palette_index(name: &str) -> usize {
    PALETTES.iter().position(|p| p.name == name).unwrap_or(0)
}

macro_rules! named_enum {
    ($name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(from = "String", into = "String")]
        pub enum $name {
            #[default]
            $($variant),+
        }
        impl $name {
            pub const ALL: &[$name] = &[$($name::$variant),+];
            pub fn label(self) -> &'static str {
                match self { $($name::$variant => $label),+ }
            }
            pub fn index(self) -> usize {
                Self::ALL.iter().position(|v| *v == self).unwrap_or(0)
            }
            pub fn at(index: i32) -> Self {
                usize::try_from(index).ok().and_then(|i| Self::ALL.get(i).copied()).unwrap_or_default()
            }
        }
        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::ALL.iter().copied().find(|v| v.label() == value).unwrap_or_default()
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.label().into()
            }
        }
    };
}

named_enum!(Scene {
    Classic => "Classic",
    Aurora => "Aurora",
    Dunes => "Dunes",
    Tide => "Tide",
    Pines => "Pines",
    Bloom => "Bloom",
    Liquid => "Liquid glass",
    Synthwave => "Synthwave",
    Ink => "Ink",
    Nebula => "Nebula",
    Petals => "Petals",
    Topo => "Contours",
    Halftone => "Halftone",
    Paper => "Paper cut",
    Rain => "City rain",
    Ribbons => "Ribbons",
});

named_enum!(Style {
    Clear => "Clear",
    Glass => "Glass",
    Neon => "Neon",
    Solid => "Solid",
});

named_enum!(React {
    Off => "Still",
    Pulse => "Pulse",
    Flow => "Flow",
    Sparkle => "Sparkle",
});

pub const FPS: [u32; 3] = [0, 15, 30];
pub const INTERVALS: [u32; 4] = [1, 5, 15, 60];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    pub scene: Scene,
    pub palette: String,
    pub style: Style,
    pub seed: u32,
    pub speed: f32,
    pub react: React,
    pub strength: f32,
    pub dim: f32,
    pub grain: f32,
    pub rotate: bool,
    pub rotate_minutes: u32,
    pub fps: u32,
    /// The cover colour this look was painted with, for the album palette.
    #[serde(skip)]
    pub tint: Option<[u8; 3]>,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: "Classic".into(),
            scene: Scene::Classic,
            palette: "Mono".into(),
            style: Style::Clear,
            seed: 1,
            speed: 1.0,
            react: React::Pulse,
            strength: 0.6,
            dim: 0.3,
            grain: 0.35,
            rotate: false,
            rotate_minutes: 15,
            fps: 15,
            tint: None,
        }
    }
}

impl Preset {
    pub fn colors(&self) -> [C; 5] {
        if let Some(rgb) = self.album_rgb() {
            return album_colors(rgb);
        }
        let p = &PALETTES[palette_index(&self.palette)];
        let shift = (rand(self.seed.wrapping_mul(7919)) - 0.5) * 18.0;
        let light = 0.94 + rand(self.seed.wrapping_mul(104_729)) * 0.12;
        p.colors.map(|c| shift_hue(hex(c), shift, light))
    }

    /// The cover colour to paint with, when this look uses the album palette.
    fn album_rgb(&self) -> Option<[u8; 3]> {
        (self.palette == ALBUM)
            .then(|| self.tint.or_else(album_tint))
            .flatten()
    }

    /// Reads a code from `to_code`; anything malformed gives `None`.
    pub fn from_code(code: &str) -> Option<Preset> {
        let (head, name) = code.trim().split_once(':')?;
        let mut parts = head.strip_prefix("tonal1.")?.split('.');
        let mut int = || parts.next()?.parse::<i64>().ok();
        let scene = Scene::ALL.get(usize::try_from(int()?).ok()?).copied()?;
        let palette = PALETTES.get(usize::try_from(int()?).ok()?)?.name;
        let style = Style::ALL.get(usize::try_from(int()?).ok()?).copied()?;
        let react = React::ALL.get(usize::try_from(int()?).ok()?).copied()?;
        let seed = u32::try_from(int()?).ok()?;
        let mut pct = || Some(int()? as f32 / 100.0);
        let (speed, strength, dim, grain) = (pct()?, pct()?, pct()?, pct()?);
        let mut int = || parts.next()?.parse::<u32>().ok();
        let (fps, rotate, minutes) = (int()?, int()? == 1, int()?);
        Some(Preset {
            name: name.trim().chars().take(40).collect(),
            scene,
            palette: palette.into(),
            style,
            react,
            seed,
            speed: speed.clamp(0.0, 3.0),
            strength: strength.clamp(0.0, 1.0),
            dim: dim.clamp(0.0, 0.85),
            grain: grain.clamp(0.0, 1.0),
            rotate,
            rotate_minutes: if INTERVALS.contains(&minutes) {
                minutes
            } else {
                15
            },
            fps: if FPS.contains(&fps) { fps } else { 15 },
            tint: None,
        })
    }

    /// A short text code that holds everything but the cover colour.
    pub fn to_code(&self) -> String {
        let pct = |v: f32| (v * 100.0).round() as i32;
        format!(
            "tonal1.{}.{}.{}.{}.{}.{}.{}.{}.{}.{}.{}.{}:{}",
            self.scene.index(),
            palette_index(&self.palette),
            self.style.index(),
            self.react.index(),
            self.seed,
            pct(self.speed),
            pct(self.strength),
            pct(self.dim),
            pct(self.grain),
            self.fps,
            u8::from(self.rotate),
            self.rotate_minutes,
            self.name
        )
    }

    pub fn accent(&self) -> [u8; 3] {
        if let Some(rgb) = self.album_rgb() {
            return album_colors(rgb)[4].map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8);
        }
        PALETTES[palette_index(&self.palette)].colors[4].to_be_bytes()[1..4]
            .try_into()
            .unwrap_or([236, 236, 238])
    }

    /// Two presets look the same when only their pacing or name differ.
    pub fn same_look(&self, other: &Preset) -> bool {
        self.scene == other.scene && self.palette == other.palette && self.seed == other.seed
    }

    pub fn detail(&self) -> String {
        if self.scene == Scene::Classic {
            return format!("See-through · {}", self.style.label());
        }
        format!(
            "{} · {} · {}",
            self.scene.label(),
            self.palette,
            self.style.label()
        )
    }
}
fn look(name: &str, scene: Scene, palette: &str, style: Style, react: React, seed: u32) -> Preset {
    Preset {
        name: name.to_owned(),
        scene,
        palette: palette.to_owned(),
        style,
        react,
        seed,
        dim: match style {
            Style::Clear => 0.38,
            _ => 0.28,
        },
        ..Preset::default()
    }
}

/// The built-in themes shown in the gallery, Classic first.
pub fn gallery() -> Vec<Preset> {
    use React::*;
    use Scene::*;
    use Style::*;
    vec![
        Preset::default(),
        look("Northern lights", Aurora, "Boreal", Glass, Pulse, 11),
        look("Golden hour", Dunes, "Saffron", Clear, Pulse, 4),
        look("Low tide", Tide, "Lagoon", Glass, Sparkle, 7),
        look("Misty pines", Pines, "Moss", Solid, Flow, 3),
        look("Soft bloom", Bloom, "Peach", Glass, Pulse, 9),
        look("Liquid glass", Liquid, "Prism", Glass, Pulse, 5),
        look("Outrun", Synthwave, "Outrun", Neon, Pulse, 2),
        look("Ink mountains", Ink, "Sumi", Clear, Flow, 8),
        look("Deep space", Nebula, "Nebula", Neon, Pulse, 6),
        look("Sakura drift", Petals, "Sakura", Glass, Flow, 12),
        look("Contours", Topo, "Glacier", Solid, Pulse, 14),
        look("Riso pop", Halftone, "Riso", Solid, Pulse, 21),
        look("Paper valley", Paper, "Dusk", Clear, Flow, 17),
        look("City rain", Rain, "Tokyo", Neon, Sparkle, 19),
        look("Ribbons", Ribbons, "Cyber", Neon, Pulse, 23),
        look("Embers", Bloom, "Ember", Solid, Pulse, 31),
        look("Lavender haze", Aurora, "Lavender", Glass, Flow, 27),
        look("Cover glow", Bloom, ALBUM, Glass, Pulse, 41),
        look("Album waves", Ribbons, ALBUM, Clear, Flow, 43),
    ]
}
/// How loud the music is right now, already shaped by the theme's reaction.
#[derive(Clone, Copy, Debug, Default)]
pub struct Audio {
    pub pulse: f32,
    pub sparkle: f32,
    pub flow: f32,
}

impl Audio {
    /// `levels` is bass, mids, highs and overall energy, each 0 to 1.
    pub fn shaped(levels: [f32; 4], react: React, strength: f32) -> Self {
        let [bass, _, high, energy] = levels.map(|l| l.clamp(0.0, 1.0));
        let k = strength.clamp(0.0, 1.0);
        let (pulse, sparkle, flow) = match react {
            React::Off => (0.0, 0.0, 0.0),
            React::Pulse => (bass * 1.6, high * 0.5, 0.0),
            React::Flow => (bass * 0.35, high * 0.3, energy * 3.0),
            React::Sparkle => (bass * 0.35, high * 2.2, 0.0),
        };
        Self {
            pulse: (pulse * k).min(1.5),
            sparkle: sparkle * k,
            flow: flow * k,
        }
    }
}

/// What a scene painter gets for one frame.
pub struct Ctx {
    pub time: f32,
    pub seed: u32,
    pub colors: [C; 5],
    pub pulse: f32,
    pub sparkle: f32,
}

impl Ctx {
    /// A stable random number in 0..1 for this seed and slot.
    pub fn rnd(&self, slot: u32) -> f32 {
        rand(self.seed.wrapping_mul(0x2c1b_3c6d) ^ slot.wrapping_mul(0x297a_2d39))
    }
}

fn paint(scene: Scene, canvas: &mut Canvas, ctx: &Ctx) {
    use painters as p;
    match scene {
        Scene::Classic => canvas.fill(|_, _, u, v| {
            draw::mix(ctx.colors[2], ctx.colors[0], (v * 0.8 + u * 0.2).min(1.0))
        }),
        Scene::Aurora => p::aurora(canvas, ctx),
        Scene::Dunes => p::dunes(canvas, ctx),
        Scene::Tide => p::tide(canvas, ctx),
        Scene::Pines => p::pines(canvas, ctx),
        Scene::Bloom => p::bloom(canvas, ctx),
        Scene::Liquid => p::liquid(canvas, ctx),
        Scene::Synthwave => p::synthwave(canvas, ctx),
        Scene::Ink => p::ink(canvas, ctx),
        Scene::Nebula => p::nebula(canvas, ctx),
        Scene::Petals => p::petals(canvas, ctx),
        Scene::Topo => p::topo(canvas, ctx),
        Scene::Halftone => p::halftone(canvas, ctx),
        Scene::Paper => p::paper(canvas, ctx),
        Scene::Rain => p::rain(canvas, ctx),
        Scene::Ribbons => p::ribbons(canvas, ctx),
    }
}
fn paint_look(canvas: &mut Canvas, look: &Preset, time: f32, audio: Audio) {
    let ctx = Ctx {
        time,
        seed: look.seed,
        colors: look.colors(),
        pulse: audio.pulse,
        sparkle: audio.sparkle,
    };
    paint(look.scene, canvas, &ctx);
    depth::apply(look.scene, canvas, &ctx);
    let keep = 1.0 - look.dim.clamp(0.0, 0.85);
    canvas.fill_with(|u, v, c| {
        let (du, dv) = (u - 0.5, v - 0.5);
        let shade = keep * (1.0 - 0.55 * (du * du + dv * dv * 1.4));
        draw::scale(c, shade)
    });
}

/// Bilinear scale of the canvas into `out` (3 or 4 bytes a pixel), with film grain and dither.
fn upscale(src: &Canvas, out: &mut [u8], w: usize, h: usize, bytes: usize, grain: f32, frame: u32) {
    let (sw, sh) = (src.w, src.h);
    let taps = |n: usize, len: usize| -> Vec<(usize, usize, f32)> {
        let k = len as f32 / n as f32;
        (0..n)
            .map(|i| {
                let f = ((i as f32 + 0.5) * k - 0.5).max(0.0);
                let a = (f as usize).min(len - 1);
                (a, (a + 1).min(len - 1), f - a as f32)
            })
            .collect()
    };
    let (xs, ys) = (taps(w, sw), taps(h, sh));
    let amount = grain.clamp(0.0, 1.0) * 0.07 + 1.5 / 255.0;
    let salt = draw::hash(frame);
    let mut line = vec![[0.0f32; 3]; sw];
    for (y, &(y0, y1, ty)) in ys.iter().enumerate() {
        let (r0, r1) = (
            &src.px[y0 * sw..y0 * sw + sw],
            &src.px[y1 * sw..y1 * sw + sw],
        );
        for i in 0..sw {
            line[i] = draw::mix(r0[i], r1[i], ty);
        }
        let row = &mut out[y * w * bytes..(y + 1) * w * bytes];
        for (x, &(x0, x1, tx)) in xs.iter().enumerate() {
            let c = draw::mix(line[x0], line[x1], tx);
            let n = (draw::rand((x as u32) ^ ((y as u32) << 16) ^ salt) - 0.5) * amount;
            let p = &mut row[x * bytes..x * bytes + 3];
            for ch in 0..3 {
                p[ch] = ((c[ch] + n).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
    }
}
/// Keeps the scratch canvases between frames so a frame allocates only its output.
#[derive(Default)]
pub struct Renderer {
    front: Canvas,
    back: Canvas,
    haze: Canvas,
    frame: u32,
}

/// How much canvas a scene gets: the costly soft ones paint fewer pixels.
fn detail(scene: Scene) -> f32 {
    match scene {
        Scene::Nebula => 0.66,
        Scene::Halftone => 0.72,
        Scene::Ribbons | Scene::Topo => 0.78,
        Scene::Tide | Scene::Liquid => 0.85,
        _ => 1.0,
    }
}

/// Canvas size for an output size: about half of it times `detail`, smoothed back up on output.
fn canvas_size(w: u32, h: u32, detail: f32) -> (usize, usize) {
    let cw = ((w as f32 / 2.0 * detail) as usize).clamp(120, 640);
    let ch = (cw * h as usize / w.max(1) as usize).max(68);
    (cw, ch)
}

pub struct Frame {
    pub image: SharedPixelBuffer<Rgb8Pixel>,
    pub frost: Option<SharedPixelBuffer<Rgb8Pixel>>,
}
impl Renderer {
    /// One background frame at `size`, crossfading in from `from` while `mix` runs 0 to 1.
    pub fn frame(
        &mut self,
        look: &Preset,
        from: Option<(&Preset, f32)>,
        time: f32,
        audio: Audio,
        size: (u32, u32),
    ) -> Frame {
        let (w, h) = (size.0.max(1), size.1.max(1));
        let fine = from.map_or(1.0, |(old, _)| detail(old.scene));
        let (cw, ch) = canvas_size(w, h, detail(look.scene).min(fine));
        self.front.resize(cw, ch);
        paint_look(&mut self.front, look, time, audio);
        if let Some((old, mix)) = from {
            self.back.resize(cw, ch);
            paint_look(&mut self.back, old, time, audio);
            let t = mix.clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            for (f, b) in self.front.px.iter_mut().zip(&self.back.px) {
                *f = draw::mix(*b, *f, t);
            }
        }
        self.frame = self.frame.wrapping_add(1);
        let mut image = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
        upscale(
            &self.front,
            image.make_mut_bytes(),
            w as usize,
            h as usize,
            3,
            look.grain,
            self.frame,
        );
        let frost = (look.style == Style::Glass).then(|| self.frost(w / 2, h / 2));
        Frame { image, frost }
    }

    /// The frame blurred and lifted, for glass panels to show what is behind them.
    fn frost(&mut self, w: u32, h: u32) -> SharedPixelBuffer<Rgb8Pixel> {
        let (fw, fh) = ((self.front.w / 4).max(8), (self.front.h / 4).max(6));
        self.haze.resize(fw, fh);
        for y in 0..fh {
            for x in 0..fw {
                let mut sum = [0.0f32; 3];
                for dy in 0..4 {
                    for dx in 0..4 {
                        let sx = (x * 4 + dx).min(self.front.w - 1);
                        let sy = (y * 4 + dy).min(self.front.h - 1);
                        sum = draw::add(sum, self.front.px[sy * self.front.w + sx], 1.0 / 16.0);
                    }
                }
                self.haze.px[y * fw + x] = sum;
            }
        }
        for _ in 0..2 {
            blur(&mut self.haze);
        }
        for p in self.haze.px.iter_mut() {
            *p = draw::mix(*p, [1.0; 3], 0.07);
        }
        let mut out = SharedPixelBuffer::<Rgb8Pixel>::new(w.max(1), h.max(1));
        upscale(
            &self.haze,
            out.make_mut_bytes(),
            w.max(1) as usize,
            h.max(1) as usize,
            3,
            0.0,
            0,
        );
        out
    }
}

fn blur(c: &mut Canvas) {
    let (w, h) = (c.w, c.h);
    let src = c.px.clone();
    for y in 0..h {
        for x in 0..w {
            let l = src[y * w + x.saturating_sub(1)];
            let r = src[y * w + (x + 1).min(w - 1)];
            c.px[y * w + x] =
                draw::add(draw::scale(src[y * w + x], 0.5), draw::add(l, r, 1.0), 0.25);
        }
    }
    let src = c.px.clone();
    for y in 0..h {
        for x in 0..w {
            let u = src[y.saturating_sub(1) * w + x];
            let d = src[(y + 1).min(h - 1) * w + x];
            c.px[y * w + x] =
                draw::add(draw::scale(src[y * w + x], 0.5), draw::add(u, d, 1.0), 0.25);
        }
    }
}
/// A gallery card picture with rounded corners, `radius` in output pixels.
pub fn thumbnail(
    look: &Preset,
    w: u32,
    h: u32,
    time: f32,
    radius: f32,
) -> SharedPixelBuffer<Rgba8Pixel> {
    let (w, h) = (w.max(8), h.max(8));
    let mut canvas = Canvas::default();
    canvas.resize((w as usize).clamp(60, 480), (h as usize).clamp(36, 300));
    let mut shown = look.clone();
    if shown.scene == Scene::Classic {
        shown.dim = 0.0;
    }
    paint_look(&mut canvas, &shown, time, Audio::default());
    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    let bytes = out.make_mut_bytes();
    upscale(&canvas, bytes, w as usize, h as usize, 4, look.grain, 0);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let cx = (x as f32 + 0.5).clamp(radius, w as f32 - radius);
            let cy = (y as f32 + 0.5).clamp(radius, h as f32 - radius);
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            bytes[(y * w as usize + x) * 4 + 3] =
                ((radius - d + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    out
}

pub fn fresh_seed() -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    draw::hash(now.subsec_nanos() ^ now.as_secs() as u32)
}

/// A random look for "Surprise me": any scene but Classic, any colours and style.
pub fn surprise() -> Preset {
    let r = fresh_seed();
    let pick = |n: usize, salt: u32| draw::hash(r ^ salt) as usize % n;
    let style = Style::ALL[pick(Style::ALL.len(), 2)];
    Preset {
        name: "Surprise".into(),
        scene: Scene::ALL[1 + pick(Scene::ALL.len() - 1, 1)],
        palette: PALETTES[1 + pick(PALETTES.len() - 1, 3)].name.into(),
        style,
        react: React::ALL[1 + pick(React::ALL.len() - 1, 4)],
        seed: 1 + r % 9999,
        dim: if style == Style::Clear { 0.38 } else { 0.28 },
        ..Preset::default()
    }
}
