use super::Ctx;
use super::draw::*;

/// Stars that twinkle, brighter with the highs.
fn stars(x: usize, y: usize, v: f32, k: &Ctx, below: f32) -> f32 {
    if v > below {
        return 0.0;
    }
    let h = rand2(x as i32, y as i32, k.seed ^ 0x5717);
    if h < 0.992 {
        return 0.0;
    }
    let b = (h - 0.992) / 0.008;
    let twinkle = 0.55 + 0.45 * (k.time * 2.3 + h * 977.0).sin();
    b * twinkle * (1.0 + k.sparkle * 1.8) * (1.0 - v / below)
}

/// A ridge line per column, `amp` tall, sitting on `base`.
fn ridge(
    w: usize,
    aspect: f32,
    base: f32,
    amp: f32,
    freq: f32,
    seed: u32,
    sharp: bool,
) -> Vec<f32> {
    (0..w)
        .map(|x| {
            let px = x as f32 / w as f32 * aspect * freq;
            let n = fbm1(px, seed, 4);
            let n = if sharp {
                1.0 - (2.0 * n - 1.0).abs()
            } else {
                n
            };
            base - amp * n
        })
        .collect()
}

pub fn aurora(c: &mut Canvas, k: &Ctx) {
    let (w, a, t, p) = (c.w, c.aspect(), k.time, k.colors);
    let bands: Vec<[f32; 6]> = (0..w)
        .map(|x| {
            let px = x as f32 / w as f32 * a;
            let mut out = [0.0; 6];
            for i in 0..3 {
                let fi = i as f32;
                let s = k.seed.wrapping_add(i * 101);
                out[i as usize * 2] =
                    0.2 + 0.09 * fi + 0.2 * (fbm1(px * 0.8 + t * 0.03 + fi * 3.1, s, 3) - 0.5);
                let rays = noise1(px * 9.0 + t * 0.2 * (1.0 + fi * 0.3), s ^ 0xabc);
                let body = noise1(px * 1.4 - t * 0.04, s ^ 7);
                out[i as usize * 2 + 1] = (0.35 + 0.65 * rays).powi(2) * (0.3 + 0.7 * body);
            }
            out
        })
        .collect();
    let far = ridge(w, a, 0.74, 0.12, 1.1, k.seed ^ 11, true);
    let near = ridge(w, a, 0.86, 0.1, 1.9, k.seed ^ 13, false);
    let glow = 0.8 + 0.9 * k.pulse;
    c.fill(|x, y, _, v| {
        let mut col = mix(p[0], p[1], v.powf(1.5) * 0.9);
        for i in 0..3 {
            let (top, lit) = (bands[x][i * 2], bands[x][i * 2 + 1]);
            let d = v - top;
            let fall = if d < 0.0 {
                (d * 5.5).exp()
            } else {
                (-d * 36.0).exp()
            };
            let tint = mix(p[3], p[4], (-d * 3.0).clamp(0.0, 1.0));
            col = add(col, tint, fall * lit * glow * 0.5);
        }
        col = add(col, p[4], stars(x, y, v, k, 0.7));
        if v > far[x] {
            col = mix(col, mix(p[1], p[0], 0.55), 0.92);
        }
        if v > near[x] {
            col = scale(p[0], 0.7);
        }
        col
    });
}
pub fn dunes(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (w, a, t) = (c.w, c.aspect(), k.time);
    let (sx, sy) = (0.2 + 0.6 * k.rnd(1), 0.3 + 0.12 * k.rnd(2));
    let layers: Vec<Vec<f32>> = (0..5)
        .map(|i| {
            let f = i as f32;
            let drift = t * 0.008 * (f + 1.0);
            (0..w)
                .map(|x| {
                    let px =
                        x as f32 / w as f32 * a * (0.9 + 0.5 * f) + drift + k.rnd(10 + i) * 40.0;
                    let n = fbm1(px, k.seed.wrapping_add(i * 31), 3);
                    0.5 + f * 0.1 - (0.05 + 0.02 * f) * (n * n * 2.2)
                })
                .collect()
        })
        .collect();
    let horizon = mix(p[3], p[4], 0.35);
    let glow = 0.5 + 0.5 * k.pulse;
    c.fill(|x, y, u, v| {
        let mut col = mix(p[1], horizon, (v / 0.6).clamp(0.0, 1.0).powf(1.3));
        let d = ((u - sx) * a).hypot(v - sy);
        col = add(col, p[4], (-d * 5.0).exp() * 0.55 * glow);
        col = mix(col, p[4], 1.0 - smooth(0.055, 0.062 + 0.006 * k.pulse, d));
        for i in (0..5).rev() {
            let top = layers[i][x];
            if v < top {
                continue;
            }
            let depth = i as f32 / 4.0;
            let next = layers[i][(x + 1).min(w - 1)];
            let side = ((sx - u) * 6.0).clamp(-1.0, 1.0);
            let lit = ((top - next) * w as f32 * 0.9 * side).clamp(-1.0, 1.0);
            let base = mix(mix(horizon, p[2], 0.55), mix(p[2], p[0], 0.65), depth);
            col = scale(base, 1.0 + 0.22 * lit);
            col = add(
                col,
                p[4],
                (1.0 - (v - top) / 0.012).max(0.0) * 0.18 * (1.0 - depth),
            );
            if rand2(x as i32, y as i32, k.seed) > 0.996 - 0.004 * k.sparkle {
                col = add(col, p[4], 0.35 * (1.0 - depth * 0.5));
            }
            break;
        }
        col
    });
}
pub fn tide(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (a, t) = (c.aspect(), k.time);
    let hz = 0.5 + (k.rnd(1) - 0.5) * 0.08;
    let (sx, sy) = (0.25 + 0.5 * k.rnd(2), hz - 0.1 - 0.08 * k.rnd(3));
    let glow = 0.6 + 0.6 * k.pulse;
    c.fill(|x, y, u, v| {
        if v < hz {
            let mut col = mix(p[0], mix(p[2], p[3], 0.5), (v / hz).powf(1.8));
            let d = ((u - sx) * a).hypot(v - sy);
            col = add(col, p[4], (-d * 5.0).exp() * 0.5 * glow);
            col = mix(col, p[4], 1.0 - smooth(0.04, 0.047, d));
            let cloud = fbm2(u * a * 1.6 + t * 0.012, v * 7.0, k.seed ^ 3, 3);
            let veil = smooth(0.52, 0.78, cloud) * (1.0 - v / hz) * 0.55;
            return mix(col, mix(p[1], p[3], 0.3 + 0.4 * (1.0 - d).max(0.0)), veil);
        }
        let d = (v - hz) / (1.0 - hz);
        let z = 1.0 / (d + 0.04);
        let wx = (u - 0.5) * a * z * 1.6;
        let wz = z * 1.3;
        let wave = noise2(wx * 1.5, wz - t * 0.35, k.seed) * 0.6
            + noise2(wx * 4.0, wz * 3.0 - t * 0.8, k.seed ^ 9) * 0.4;
        let mut col = mix(mix(p[3], p[2], 0.6), mix(p[1], p[0], 0.5), d.powf(0.5));
        col = scale(col, 0.85 + 0.3 * wave);
        let path = (-((u - sx) / (0.02 + d * 0.22)).powi(2)).exp();
        col = add(col, p[4], path * smooth(0.45, 0.85, wave) * 0.7 * glow);
        let glint = rand2(x as i32, y as i32, k.seed ^ (t * 3.0) as u32);
        if glint > 0.997 - 0.006 * k.sparkle && wave > 0.55 {
            col = add(col, p[4], 0.6);
        }
        col
    });
}
pub fn pines(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (w, h, a, t) = (c.w, c.h, c.aspect(), k.time);
    let rows: Vec<Vec<f32>> = (0..5u32)
        .map(|i| {
            let f = i as f32;
            let spacing = 0.035 + 0.022 * f;
            let tall = 0.1 + 0.05 * f;
            let shift = t * 0.004 * (f + 1.0) + k.rnd(20 + i) * 9.0;
            (0..w)
                .map(|x| {
                    let px = x as f32 / w as f32 * a + shift;
                    let n = (px / spacing).floor();
                    let frac = px / spacing - n;
                    let size = 0.45 + 0.55 * rand((n as i32 as u32) ^ (i * 7919) ^ k.seed);
                    let cone = (1.0 - (frac - 0.5).abs() * 2.0).max(0.0).powf(0.85);
                    0.42 + 0.12 * f - tall * size * cone
                })
                .collect()
        })
        .collect();
    let (sx, beams) = (0.25 + 0.5 * k.rnd(3), 0.18 + 0.12 * k.pulse);
    c.fill(|x, _, u, v| {
        let fog = mix(p[1], mix(p[3], p[2], 0.5), v.powf(0.8));
        let angle = ((u - sx) * a).atan2(v + 0.15);
        let ray = noise1(angle * 14.0 + t * 0.04, k.seed ^ 77).powi(3);
        let mut col = add(fog, p[4], ray * beams * (1.0 - v));
        for i in (0..5).rev() {
            if v >= rows[i][x] {
                let depth = (i as f32 + 1.0) / 5.0;
                col = mix(fog, scale(p[0], 0.8), depth.powf(0.8));
                let mist = noise2(u * a * 1.2 + t * 0.015, v * 3.0 + i as f32, k.seed ^ 5);
                col = mix(
                    col,
                    fog,
                    smooth(0.45, 0.9, mist) * 0.35 * (1.0 - depth * 0.6),
                );
                break;
            }
        }
        col
    });
    fireflies(c, k, w, h, a);
}
fn fireflies(c: &mut Canvas, k: &Ctx, w: usize, h: usize, a: f32) {
    let t = k.time;
    for i in 0..26u32 {
        let f = 0.6 + 0.8 * k.rnd(100 + i);
        let x = (k.rnd(200 + i) * a + 0.04 * (t * 0.3 * f + i as f32).sin()).rem_euclid(a) / a;
        let y = 0.55 + 0.4 * k.rnd(300 + i) + 0.03 * (t * 0.25 * f + i as f32 * 2.0).cos();
        let blink = (0.5 + 0.5 * (t * 1.3 * f + i as f32 * 3.0).sin()).powi(2);
        let r = h as f32 * (0.012 + 0.006 * k.pulse);
        c.glow(
            x * w as f32,
            y * h as f32,
            r,
            k.colors[4],
            blink * (0.5 + 0.8 * k.pulse),
        );
    }
}
/// Soft coloured lights drifting over the deep colour, blended like a mesh gradient.
fn lights(k: &Ctx, count: u32, size: f32) -> Vec<(f32, f32, f32, C)> {
    (0..count)
        .map(|i| {
            let (f1, f2) = (0.05 + 0.07 * k.rnd(40 + i), 0.04 + 0.06 * k.rnd(60 + i));
            let ph = k.rnd(80 + i) * std::f32::consts::TAU;
            let cx = 0.5 + 0.42 * (k.time * f1 + ph).sin();
            let cy = 0.5 + 0.42 * (k.time * f2 + ph * 1.7).cos();
            let r = size * (0.75 + 0.5 * k.rnd(100 + i)) * (1.0 + 0.25 * k.pulse);
            (cx, cy, 1.0 / (r * r), k.colors[1 + i as usize % 4])
        })
        .collect()
}

fn mesh(u: f32, v: f32, a: f32, deep: C, lights: &[(f32, f32, f32, C)]) -> C {
    let mut sum = scale(deep, 0.55 + 0.25 * v);
    for &(cx, cy, inv, color) in lights {
        let q = (((u - cx) * a).powi(2) + (v - cy).powi(2)) * inv;
        let wgt = 1.0 / (1.0 + q * q * 3.0);
        sum = add(sum, color, wgt * 0.85);
    }
    sum.map(|c| 1.0 - (-c * 1.25).exp())
}

pub fn bloom(c: &mut Canvas, k: &Ctx) {
    let a = c.aspect();
    let l = lights(k, 6, 0.38);
    c.fill(|_, _, u, v| mesh(u, v, a, k.colors[0], &l));
}
pub fn liquid(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (a, t) = (c.aspect(), k.time);
    let back = lights(k, 4, 0.45);
    let deep = scale(p[0], 0.8);
    let balls: Vec<(f32, f32, f32)> = (0..7u32)
        .map(|i| {
            let (f1, f2) = (0.06 + 0.08 * k.rnd(500 + i), 0.05 + 0.07 * k.rnd(520 + i));
            let ph = k.rnd(540 + i) * std::f32::consts::TAU;
            let x = (0.5 + 0.36 * (t * f1 + ph).sin()) * a;
            let y = 0.5 + 0.34 * (t * f2 + ph * 1.3).cos();
            let beat = if i % 2 == 0 {
                0.3 * k.pulse
            } else {
                0.1 * k.pulse
            };
            let r = (0.08 + 0.07 * k.rnd(560 + i)) * (1.0 + beat);
            (x, y, r * r)
        })
        .collect();
    let light = [-0.45f32, -0.65, 0.61];
    c.fill(|_, _, u, v| {
        let (px, py) = (u * a, v);
        let (mut f, mut gx, mut gy) = (0.0f32, 0.0f32, 0.0f32);
        for &(bx, by, r2) in &balls {
            let (dx, dy) = (px - bx, py - by);
            let d2 = (dx * dx + dy * dy).max(1e-5);
            f += r2 / d2;
            let g = -2.0 * r2 / (d2 * d2);
            gx += g * dx;
            gy += g * dy;
        }
        let bg = mesh(u, v, a, deep, &back);
        let outside = add(bg, p[4], smooth(0.45, 1.0, f) * 0.12);
        if f < 0.92 {
            return outside;
        }
        let len = (gx * gx + gy * gy).sqrt().max(1e-5);
        let edge = (1.0 / f).clamp(0.0, 1.0);
        let (nx, ny) = (-gx / len * edge, -gy / len * edge);
        let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
        let seen = mesh(u + nx * 0.07, v + ny * 0.07, a, deep, &back);
        let mut col = add(scale(seen, 0.9), p[3], 0.08);
        let hue = 0.5 + 0.5 * (ny.atan2(nx) * 2.0 + t * 0.4).sin();
        col = add(col, mix(p[3], p[4], hue), edge.powi(4) * 0.75);
        let spec = (nx * light[0] + ny * light[1] + nz * light[2])
            .max(0.0)
            .powi(28);
        col = add(col, [1.0; 3], spec * 0.85);
        mix(outside, col, smooth(0.92, 1.04, f))
    });
}
pub fn synthwave(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (w, h, a, t) = (c.w, c.h, c.aspect(), k.time);
    let hz = 0.62;
    let (sx, sy, sr) = (
        0.5 + (k.rnd(1) - 0.5) * 0.3,
        hz - 0.15,
        0.2 * (1.0 + 0.05 * k.pulse),
    );
    let hills = ridge(w, a, hz, 0.14, 1.6, k.seed ^ 21, true);
    let lift = 1.0 + 0.8 * k.pulse;
    c.fill(|x, y, u, v| {
        if v < hz {
            let mut col = mix(p[0], p[1], (v / hz).powi(2));
            col = add(col, p[2], (-(hz - v) * 9.0).exp() * 0.55 * lift);
            col = add(col, p[4], stars(x, y, v, k, 0.45) * 0.8);
            let (dx, dy) = ((u - sx) * a, v - sy);
            let d = dx.hypot(dy);
            col = add(col, p[2], (-(d - sr).max(0.0) * 7.0).exp() * 0.35 * lift);
            let below = (dy / sr).max(0.0);
            let stripe = (dy * 34.0 - t * 0.4).rem_euclid(1.0) < below * 0.85;
            if d < sr && !stripe {
                col = mix(p[3], p[2], (dy / sr * 0.5 + 0.5).clamp(0.0, 1.0));
            }
            let side = smooth(0.08, 0.4, (u - 0.5).abs());
            let top = hz - (hz - hills[x]) * side;
            if v > top {
                col = scale(p[0], 0.9);
                if v - top < 1.5 / h as f32 {
                    col = add(col, p[4], 0.8 * lift);
                }
            }
            return col;
        }
        let d = v - hz;
        let z = 0.22 / d;
        let (gz, gx) = (z + t * 0.9, (u - 0.5) * a * z * 2.4);
        let dz = 0.5 - ((gz.fract() - 0.5).abs());
        let dx = 0.5 - ((gx.rem_euclid(1.0) - 0.5).abs());
        let pz = dz / (0.22 / (d * d) / h as f32);
        let px = dx / (z * 2.4 * a / w as f32);
        let line = (1.3 - pz).clamp(0.0, 1.0).max((1.3 - px).clamp(0.0, 1.0));
        let haze = (-pz.min(px) * 0.35).exp() * 0.35;
        let fade = smooth(0.0, 0.14, d);
        let mut col = mix(scale(p[1], 0.6), p[0], (d / (1.0 - hz)).sqrt());
        col = add(col, p[2], (-d * 14.0).exp() * 0.5 * lift);
        add(col, p[4], (line + haze) * fade * lift * 0.9)
    });
}
pub fn ink(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (w, a, t) = (c.w, c.aspect(), k.time);
    let mist = mix(mix(p[3], p[2], 0.5), p[1], 0.5);
    let ridges: Vec<Vec<f32>> = (0..5u32)
        .map(|i| {
            let f = i as f32;
            ridge(
                w,
                a,
                0.42 + 0.1 * f,
                0.09 + 0.04 * f,
                1.0 + 0.45 * f,
                k.seed.wrapping_add(i * 57),
                true,
            )
        })
        .collect();
    let (sx, sy) = (0.55 + 0.25 * k.rnd(1), 0.24 + 0.08 * k.rnd(2));
    c.fill(|x, y, u, v| {
        let mut col = mix(p[0], mist, (v / 0.65).clamp(0.0, 1.0).powf(1.2));
        let d = ((u - sx) * a).hypot(v - sy);
        col = add(col, p[4], (-d * 7.0).exp() * (0.2 + 0.3 * k.pulse));
        col = mix(col, p[4], (1.0 - smooth(0.075, 0.082, d)) * 0.92);
        for i in (0..5).rev() {
            let top = ridges[i][x];
            if v < top {
                continue;
            }
            let depth = i as f32 / 4.0;
            let ink = mix(mist, scale(p[0], 0.85), 0.3 + 0.65 * depth);
            let fade = ((v - top) / 0.2).clamp(0.0, 1.0).powf(0.7);
            col = mix(ink, mist, fade * 0.8 * (1.0 - depth * 0.5));
            if v - top < 0.006 {
                col = scale(col, 0.8);
            }
            break;
        }
        let drift = noise1(u * a * 0.6 + t * 0.012 + v * 2.5, k.seed ^ 21);
        col = mix(col, mist, smooth(0.55, 0.9, drift) * 0.25);
        add(col, [1.0; 3], (rand2(x as i32, y as i32, 99) - 0.5) * 0.025)
    });
}
pub fn nebula(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (a, t) = (c.aspect(), k.time);
    let core = 0.35 + 0.5 * k.pulse;
    c.fill(|x, y, u, v| {
        let (qx, qy) = (u * a * 1.5 + k.rnd(1) * 30.0, v * 1.5);
        let wx = noise2(qx * 0.6 + t * 0.01, qy * 0.6, k.seed ^ 21);
        let wy = noise2(qx * 0.6, qy * 0.6 - t * 0.008, k.seed ^ 22);
        let d = fbm2(qx + wx * 1.6 + t * 0.004, qy + wy * 1.6, k.seed ^ 23, 4);
        let dens = smooth(0.38, 0.85, d);
        let mut col = ramp(p, d * 0.9);
        col = mix(scale(p[0], 0.9), col, smooth(0.2, 0.7, d));
        col = add(col, p[4], dens.powi(3) * core);
        let lane = noise2(qx * 3.0, qy * 3.0, k.seed ^ 24);
        col = scale(col, 1.0 - smooth(0.6, 0.9, lane) * 0.35 * dens);
        add(col, p[4], stars(x, y, 0.0, k, 1.0) * 0.9)
    });
}
pub fn petals(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (w, h, a, t) = (c.w, c.h, c.aspect(), k.time);
    let hill = ridge(w, a, 0.78, 0.1, 0.9, k.seed ^ 21, false);
    let low = ridge(w, a, 0.9, 0.08, 1.6, k.seed ^ 23, false);
    c.fill(|x, _, _, v| {
        let mut col = mix(mix(p[1], p[0], 0.3), mix(p[2], p[3], 0.4), v.powf(1.2));
        if v > hill[x] {
            col = mix(p[1], p[2], 0.35);
        }
        if v > low[x] {
            col = mix(p[0], p[1], 0.6);
        }
        col
    });
    for i in 0..70u32 {
        let depth = 0.4 + 0.6 * k.rnd(500 + i);
        let fall = (0.02 + 0.03 * k.rnd(600 + i)) * depth;
        let y = ((k.rnd(700 + i) + t * fall).rem_euclid(1.2)) - 0.1;
        let sway = 0.05 * (t * 0.5 * depth + i as f32).sin() + t * 0.01 * depth;
        let x = (k.rnd(800 + i) * a + sway).rem_euclid(a) / a;
        let size = h as f32 * (0.006 + 0.012 * depth) * (1.0 + 0.2 * k.pulse);
        let spin = t * (0.6 + k.rnd(900 + i)) + i as f32;
        let color = mix(p[4], p[3], k.rnd(1000 + i));
        c.ellipse(
            x * w as f32,
            y * h as f32,
            size * 1.6,
            size * (0.35 + 0.65 * spin.cos().abs()),
            spin * 0.5,
            color,
            0.55 + 0.4 * depth,
            1.2 - depth,
        );
    }
}
pub fn topo(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (w, h, a, t) = (c.w, c.h, c.aspect(), k.time);
    let mut field = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            field[y * w + x] = fbm2(u * a * 1.3 + t * 0.01, v * 1.3 + t * 0.006, k.seed, 4);
        }
    }
    let levels = 14.0;
    let shift = t * 0.04;
    let shine = 0.55 + 0.6 * k.pulse;
    c.fill(|x, y, _, _| {
        let f = field[y * w + x];
        let gx = field[y * w + (x + 1).min(w - 1)] - field[y * w + x.saturating_sub(1)];
        let gy = field[(y + 1).min(h - 1) * w + x] - field[y.saturating_sub(1) * w + x];
        let slope = (gx.hypot(gy) * 0.5 * levels).max(1e-4);
        let e = f * levels + shift;
        let near = (e - e.round()).abs() / slope;
        let major = (e.round() as i32).rem_euclid(5) == 0;
        let width = if major { 1.4 } else { 0.8 };
        let line = (width - near).clamp(0.0, 1.0);
        let base = scale(ramp(p, (f - 0.25) * 1.6), 0.42);
        add(
            base,
            if major { p[4] } else { p[3] },
            line * shine * if major { 1.0 } else { 0.6 },
        )
    });
}
pub fn halftone(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (h, a, t) = (c.h as f32, c.aspect(), k.time);
    let cell = (h / 20.0).max(4.0);
    let (ox, oy) = (k.rnd(1) * a, k.rnd(2));
    let screens = [(0.26f32, p[2]), (1.31f32, p[4])];
    c.fill(|x, y, u, v| {
        let mut col = mix(p[0], p[1], v * 0.5);
        for (n, &(angle, ink)) in screens.iter().enumerate() {
            let (s, co) = angle.sin_cos();
            let (px, py) = (x as f32 * co + y as f32 * s, y as f32 * co - x as f32 * s);
            let (gx, gy) = ((px / cell).floor() + 0.5, (py / cell).floor() + 0.5);
            let (dx, dy) = (px - gx * cell, py - gy * cell);
            let wave = ((u * a - ox).hypot(v - oy) * 7.0 - t * 0.9 + n as f32 * 1.7).sin();
            let field = noise2(gx * 0.11 + t * 0.03, gy * 0.11, k.seed ^ n as u32);
            let amount = (0.25 + 0.5 * field + 0.25 * wave).clamp(0.0, 1.0);
            let r = cell * 0.5 * amount.sqrt() * (1.0 + 0.25 * k.pulse);
            let edge = (r - dx.hypot(dy) + 0.5).clamp(0.0, 1.0);
            col = add(col, ink, edge * 0.7);
        }
        col
    });
}
pub fn paper(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (w, h, a, t) = (c.w, c.h, c.aspect(), k.time);
    let hills: Vec<Vec<f32>> = (0..6u32)
        .map(|i| {
            let f = i as f32;
            let sway = 0.008 * (t * 0.25 + f * 1.3).sin();
            (0..w)
                .map(|x| {
                    let px = x as f32 / w as f32 * a * (0.7 + 0.22 * f) + k.rnd(50 + i) * 30.0;
                    0.36 + 0.1 * f + sway
                        - (0.12 + 0.03 * f) * fbm1(px, k.seed.wrapping_add(i * 57), 3)
                })
                .collect()
        })
        .collect();
    let (sx, sy) = (0.2 + 0.6 * k.rnd(4), 0.22 + 0.08 * k.rnd(5));
    let reach = 7.0 / h as f32;
    c.fill(|x, _, u, v| {
        let mut col = mix(p[1], mix(p[2], p[3], 0.4), v.powf(1.3));
        let d = ((u - sx) * a).hypot(v - sy);
        col = mix(col, p[4], 1.0 - smooth(0.07, 0.076 + 0.006 * k.pulse, d));
        col = add(col, p[4], (-d * 6.0).exp() * 0.2 * (1.0 + k.pulse));
        let mut layer = None;
        for i in (0..6).rev() {
            if v >= hills[i][x] {
                layer = Some(i);
                break;
            }
        }
        let front = match layer {
            Some(i) => {
                let depth = i as f32 / 5.0;
                let sky = mix(p[3], p[4], 0.3);
                col = mix(ramp(p, 0.8 - 0.75 * depth), sky, (1.0 - depth) * 0.45);
                let rim = (1.0 - (v - hills[i][x]) * h as f32 / 2.0).max(0.0);
                col = add(col, p[4], rim * (0.18 - 0.1 * depth));
                i + 1
            }
            None => 0,
        };
        if front < 6 {
            let gap = hills[front][x] - v;
            if gap > 0.0 {
                col = scale(col, 1.0 - 0.35 * (-gap / reach).exp());
            }
        }
        let grain = noise2(u * a * 60.0, v * 60.0, k.seed ^ 71) - 0.5;
        add(col, [1.0; 3], grain * 0.03)
    });
}
pub fn rain(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (w, h, a, t) = (c.w, c.h, c.aspect(), k.time);
    c.fill(|x, _, u, v| {
        let mut col = mix(p[0], p[1], v.powf(1.2));
        col = add(col, p[2], (-(1.0 - v) * 4.0).exp() * 0.35);
        let lane = ((x as f32 + v * h as f32 * 0.18) / 2.0).floor() as i32;
        let pick = rand(lane as u32 ^ k.seed);
        if pick > 0.55 {
            let speed = 0.5 + pick;
            let pos = (v * 0.8 - t * speed * 0.6 + rand(lane as u32 ^ 0x51) * 9.0).rem_euclid(1.0);
            if pos < 0.07 {
                col = add(col, p[3], (1.0 - pos / 0.07) * 0.12);
            }
        }
        let _ = u;
        col
    });
    for i in 0..36u32 {
        let r = h as f32 * (0.03 + 0.06 * k.rnd(500 + i)) * (1.0 + 0.15 * k.pulse);
        let x = (k.rnd(600 + i) * a + t * 0.004 * (k.rnd(700 + i) - 0.5)).rem_euclid(a) / a;
        let y = (k.rnd(800 + i) - t * 0.003 * (0.5 + k.rnd(900 + i))).rem_euclid(1.0);
        let color = p[2 + (i as usize % 3)];
        let twinkle = 0.7 + 0.3 * (t * 0.8 + i as f32).sin() + 0.4 * k.sparkle;
        c.ellipse(
            x * w as f32,
            y * h as f32,
            r,
            r,
            0.0,
            color,
            0.16 * twinkle,
            0.25,
        );
        c.ellipse(
            x * w as f32,
            y * h as f32,
            r * 0.9,
            r * 0.9,
            0.0,
            color,
            0.07 * twinkle,
            0.9,
        );
    }
}
pub fn ribbons(c: &mut Canvas, k: &Ctx) {
    let p = &k.colors;
    let (w, a, t) = (c.w, c.aspect(), k.time);
    let amp = 0.08 + 0.1 * k.pulse;
    let lines: Vec<Vec<f32>> = (0..5u32)
        .map(|i| {
            let f = i as f32;
            let (f1, f2) = (1.2 + 1.4 * k.rnd(500 + i), 2.5 + 2.0 * k.rnd(520 + i));
            let ph = k.rnd(540 + i) * std::f32::consts::TAU;
            (0..w)
                .map(|x| {
                    let u = x as f32 / w as f32 * a;
                    0.5 + (f - 2.0) * 0.05
                        + amp
                            * (0.6 * (u * f1 + t * (0.25 + 0.05 * f) + ph).sin()
                                + 0.4 * (u * f2 - t * 0.4 + ph * 2.0).sin())
                })
                .collect()
        })
        .collect();
    c.fill(|x, _, _, v| {
        let mut col = mix(p[0], scale(p[1], 0.7), v);
        for (i, line) in lines.iter().enumerate() {
            let d = (v - line[x]).abs();
            let width = 0.004 + 0.002 * i as f32;
            let core = (-(d / width).powi(2)).exp();
            let haze = (-d * 16.0).exp() * 0.22;
            let tint = mix(p[2], p[4], i as f32 / 4.0);
            col = add(col, tint, core * 0.9 + haze);
        }
        col
    });
}
