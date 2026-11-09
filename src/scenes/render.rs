//! Frames for the app window, as Slint pixel buffers.
use super::draw::{self, Canvas};
use super::{Audio, Preset, Scene, Style, canvas_size, detail, media, paint_look, upscale};
use slint::{Rgb8Pixel, Rgba8Pixel, SharedPixelBuffer};

/// Keeps the scratch canvases between frames so a frame allocates only its output.
#[derive(Default)]
pub struct Renderer {
    front: Canvas,
    back: Canvas,
    haze: Canvas,
    frame: u32,
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

    /// A decoded picture or video frame, dimmed like the scenes, with its frosted copy for glass.
    pub fn media_frame(&mut self, mut px: Vec<u8>, size: (u32, u32), look: &Preset) -> Frame {
        let (w, h) = (size.0.max(1) as usize, size.1.max(1) as usize);
        media::shade(&mut px, w, h, look.dim);
        let frost = (look.style == Style::Glass).then(|| {
            let (fw, fh) = ((w / 4).max(8), (h / 4).max(6));
            self.front.resize(fw, fh);
            for y in 0..fh {
                let sy = (y * h / fh).min(h - 1);
                for x in 0..fw {
                    let sx = (x * w / fw).min(w - 1);
                    let i = (sy * w + sx) * 3;
                    self.front.px[y * fw + x] = [0, 1, 2].map(|c| f32::from(px[i + c]) / 255.0);
                }
            }
            self.frost(size.0 / 2, size.1 / 2)
        });
        let image = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(&px, size.0, size.1);
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
