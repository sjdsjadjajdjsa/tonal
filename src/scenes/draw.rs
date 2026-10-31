pub type C = [f32; 3];

#[inline]
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

#[inline]
pub fn rand(x: u32) -> f32 {
    (hash(x) >> 8) as f32 / 16_777_216.0
}

#[inline]
pub fn rand2(x: i32, y: i32, seed: u32) -> f32 {
    rand((x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ hash(seed))
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

pub fn noise1(x: f32, seed: u32) -> f32 {
    let i = x.floor();
    let f = fade(x - i);
    let s = hash(seed);
    let a = rand((i as i32 as u32) ^ s);
    let b = rand(((i as i32).wrapping_add(1) as u32) ^ s);
    a + (b - a) * f
}

pub fn fbm1(x: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += noise1(x * freq, seed.wrapping_add(o * 977)) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

pub fn noise2(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (fade(x - ix), fade(y - iy));
    let (xi, yi) = (ix as i32, iy as i32);
    let a = rand2(xi, yi, seed);
    let b = rand2(xi + 1, yi, seed);
    let c = rand2(xi, yi + 1, seed);
    let d = rand2(xi + 1, yi + 1, seed);
    let top = a + (b - a) * fx;
    let bottom = c + (d - c) * fx;
    top + (bottom - top) * fy
}

pub fn fbm2(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += noise2(
            x * freq + o as f32 * 17.3,
            y * freq,
            seed.wrapping_add(o * 131),
        ) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.01;
    }
    sum / norm
}

#[inline]
pub fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    fade(((x - e0) / (e1 - e0)).clamp(0.0, 1.0))
}

#[inline]
pub fn mix(a: C, b: C, t: f32) -> C {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

#[inline]
pub fn add(a: C, b: C, k: f32) -> C {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

#[inline]
pub fn scale(a: C, k: f32) -> C {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn ramp(p: &[C; 5], t: f32) -> C {
    let t = t.clamp(0.0, 1.0) * 4.0;
    let i = (t as usize).min(3);
    mix(p[i], p[i + 1], t - i as f32)
}

pub fn hex(rgb: u32) -> C {
    [
        ((rgb >> 16) & 255) as f32 / 255.0,
        ((rgb >> 8) & 255) as f32 / 255.0,
        (rgb & 255) as f32 / 255.0,
    ]
}

pub fn shift_hue(c: C, degrees: f32, light: f32) -> C {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-5 {
        return [(l * light).min(1.0); 3];
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let mut h = if max == c[0] {
        (c[1] - c[2]) / d + if c[1] < c[2] { 6.0 } else { 0.0 }
    } else if max == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    } / 6.0;
    h = (h + degrees / 360.0).rem_euclid(1.0);
    let l = (l * light).clamp(0.0, 1.0);
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |t: f32| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

/// A small float image the scenes paint into before it is scaled up.
#[derive(Default)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<C>,
}

impl Canvas {
    pub fn resize(&mut self, w: usize, h: usize) {
        if self.w != w || self.h != h {
            self.w = w;
            self.h = h;
            self.px = vec![[0.0; 3]; w * h];
        }
    }

    pub fn aspect(&self) -> f32 {
        self.w as f32 / self.h as f32
    }

    pub fn fill(&mut self, mut f: impl FnMut(usize, usize, f32, f32) -> C) {
        let (w, h) = (self.w, self.h);
        let (iw, ih) = (1.0 / w as f32, 1.0 / h as f32);
        for y in 0..h {
            let v = (y as f32 + 0.5) * ih;
            let row = &mut self.px[y * w..(y + 1) * w];
            for (x, p) in row.iter_mut().enumerate() {
                *p = f(x, y, (x as f32 + 0.5) * iw, v);
            }
        }
    }

    /// Rewrites every pixel from its position and current colour.
    pub fn fill_with(&mut self, mut f: impl FnMut(f32, f32, C) -> C) {
        let (w, h) = (self.w, self.h);
        let (iw, ih) = (1.0 / w as f32, 1.0 / h as f32);
        for (i, p) in self.px.iter_mut().enumerate() {
            let (x, y) = (i % w, i / w);
            *p = f((x as f32 + 0.5) * iw, (y as f32 + 0.5) * ih, *p);
        }
    }

    /// Adds a soft round glow centred at (x, y) in canvas pixels.
    pub fn glow(&mut self, x: f32, y: f32, r: f32, c: C, k: f32) {
        self.ellipse(x, y, r, r, 0.0, c, k, 1.0);
    }

    /// Blends a rotated ellipse; `soft` is the fraction of the radius that fades out.
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(
        &mut self,
        x: f32,
        y: f32,
        rx: f32,
        ry: f32,
        angle: f32,
        c: C,
        k: f32,
        soft: f32,
    ) {
        let r = rx.max(ry) + 1.0;
        let (x0, x1) = (
            (x - r).floor().max(0.0) as i32,
            (x + r).ceil().min(self.w as f32 - 1.0) as i32,
        );
        let (y0, y1) = (
            (y - r).floor().max(0.0) as i32,
            (y + r).ceil().min(self.h as f32 - 1.0) as i32,
        );
        if x0 > x1 || y0 > y1 {
            return;
        }
        let (sin, cos) = angle.sin_cos();
        let (irx, iry) = (1.0 / rx.max(0.3), 1.0 / ry.max(0.3));
        let soft = soft.clamp(0.05, 1.0);
        let edge = 1.0 - soft;
        for py in y0..=y1 {
            for px in x0..=x1 {
                let (dx, dy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                let (ex, ey) = ((dx * cos + dy * sin) * irx, (dy * cos - dx * sin) * iry);
                let d = (ex * ex + ey * ey).sqrt();
                if d >= 1.0 {
                    continue;
                }
                let a = if d <= edge {
                    1.0
                } else {
                    1.0 - smooth(edge, 1.0, d)
                } * k;
                let p = &mut self.px[py as usize * self.w + px as usize];
                *p = mix(*p, c, a.min(1.0));
                if k > 1.0 {
                    *p = add(*p, c, (k - 1.0) * a / k * 0.5);
                }
            }
        }
    }
}
