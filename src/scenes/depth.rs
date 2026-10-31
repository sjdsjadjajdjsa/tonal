use super::draw::{Canvas, mix, noise2, rand, smooth};
use super::{Ctx, Scene};

/// What sits in front of a scene: out-of-focus shapes drifting faster than it.
#[derive(Clone, Copy, PartialEq)]
enum Near {
    None,
    Bokeh,
    Motes,
    Petals,
    Drops,
}

/// How much drifting mist a scene gets, where its horizon sits, and what floats close by.
struct Depth {
    fog: f32,
    horizon: f32,
    near: Near,
}

fn depth(scene: Scene) -> Depth {
    let d = |fog, horizon, near| Depth { fog, horizon, near };
    match scene {
        Scene::Classic | Scene::Halftone => d(0.0, 0.0, Near::None),
        Scene::Aurora => d(0.35, 0.78, Near::Motes),
        Scene::Dunes => d(0.3, 0.58, Near::Motes),
        Scene::Tide => d(0.4, 0.52, Near::Motes),
        Scene::Pines => d(0.6, 0.62, Near::Motes),
        Scene::Ink => d(0.55, 0.6, Near::None),
        Scene::Paper => d(0.45, 0.6, Near::Motes),
        Scene::Synthwave => d(0.4, 0.55, Near::None),
        Scene::Rain => d(0.35, 0.6, Near::Drops),
        Scene::Petals => d(0.15, 0.7, Near::Petals),
        Scene::Bloom | Scene::Liquid | Scene::Ribbons | Scene::Topo => d(0.0, 0.0, Near::Bokeh),
        Scene::Nebula => d(0.0, 0.0, Near::Motes),
    }
}

/// Adds atmosphere and a near layer over a painted scene so it reads in three planes.
pub fn apply(scene: Scene, c: &mut Canvas, k: &Ctx) {
    let d = depth(scene);
    if d.fog > 0.0 {
        fog(c, k, &d);
    }
    match d.near {
        Near::None => {}
        Near::Bokeh => bokeh(c, k),
        Near::Motes => motes(c, k),
        Near::Petals => petals(c, k),
        Near::Drops => drops(c, k),
    }
}

/// Mist banks that hug the horizon and roll sideways, sampled on a coarse grid to stay cheap.
fn fog(c: &mut Canvas, k: &Ctx, d: &Depth) {
    const STEP: usize = 4;
    let (w, h) = (c.w.max(1), c.h.max(1));
    let (gw, gh) = (w / STEP + 2, h / STEP + 2);
    let aspect = w as f32 / h as f32;
    let t = k.time;
    let lift = 1.0 + 0.35 * k.pulse;
    let mut grid = vec![0.0f32; gw * gh];
    for gy in 0..gh {
        let v = (gy * STEP) as f32 / h as f32;
        let band = (-((v - d.horizon) / 0.14).powi(2)).exp();
        let low = smooth(d.horizon, 1.05, v) * 0.45;
        let weight = band + low;
        for gx in 0..gw {
            let u = (gx * STEP) as f32 / w as f32 * aspect;
            let roll = noise2(u * 2.2 + t * 0.03, v * 5.0, k.seed ^ 0x51ed) * 0.65
                + noise2(u * 5.5 - t * 0.055, v * 11.0 + t * 0.015, k.seed ^ 0x7a11) * 0.35;
            let thick = smooth(0.3, 0.85, roll);
            grid[gy * gw + gx] = (weight * (0.25 + 0.95 * thick) * d.fog * lift).min(0.85);
        }
    }
    let haze = mix(k.colors[2], k.colors[4], 0.45);
    let (sx, sy) = (w as f32 / STEP as f32, h as f32 / STEP as f32);
    c.fill_with(|u, v, col| {
        let (fx, fy) = (u * sx, v * sy);
        let (x0, y0) = (fx as usize, fy as usize);
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let at = |x: usize, y: usize| grid[y.min(gh - 1) * gw + x.min(gw - 1)];
        let top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * tx;
        let bottom = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * tx;
        mix(col, haze, top + (bottom - top) * ty)
    });
}

/// Moves a value along a looping path: 0..1 across the span, wrapping with a margin.
fn wrap(start: f32, speed: f32, time: f32, margin: f32) -> f32 {
    (start + speed * time).rem_euclid(1.0 + 2.0 * margin) - margin
}

/// Big soft discs of light, as if a lens were focused far behind them.
fn bokeh(c: &mut Canvas, k: &Ctx) {
    let (w, h) = (c.w as f32, c.h as f32);
    for i in 0..7u32 {
        let size = (0.07 + 0.09 * k.rnd(900 + i)) * h * (1.0 + 0.12 * k.pulse);
        let x = wrap(k.rnd(910 + i), 0.006 + 0.008 * k.rnd(920 + i), k.time, 0.15) * w;
        let y = (k.rnd(930 + i) + 0.04 * (k.time * 0.2 + i as f32).sin()) * h;
        let col = mix(k.colors[3], k.colors[4], k.rnd(940 + i));
        c.ellipse(
            x,
            y,
            size,
            size,
            0.0,
            col,
            0.07 + 0.05 * k.rnd(950 + i),
            0.55,
        );
    }
}

/// Specks of dust or snow close to the eye, catching light and drifting faster than the scene.
fn motes(c: &mut Canvas, k: &Ctx) {
    let (w, h) = (c.w as f32, c.h as f32);
    let px = h / 300.0;
    for i in 0..26u32 {
        let near = k.rnd(1000 + i);
        let speed = 0.004 + 0.012 * near;
        let x = wrap(k.rnd(1010 + i), speed, k.time, 0.05) * w;
        let sway = (k.time * (0.3 + near * 0.4) + i as f32 * 1.7).sin() * 0.02;
        let y = (k.rnd(1020 + i) + sway - 0.01 * k.time * near).rem_euclid(1.0) * h;
        let r = (0.8 + 3.5 * near * near) * px;
        let glint = 0.3 + 0.5 * k.sparkle;
        c.ellipse(
            x,
            y,
            r,
            r,
            0.0,
            k.colors[4],
            (0.25 + 0.35 * (1.0 - near)) * glint + 0.12,
            0.9,
        );
    }
}

/// Petals right in front of the lens: large, blurred and tumbling.
fn petals(c: &mut Canvas, k: &Ctx) {
    let (w, h) = (c.w as f32, c.h as f32);
    for i in 0..6u32 {
        let size = (0.04 + 0.05 * k.rnd(1100 + i)) * h;
        let fall = 0.02 + 0.02 * k.rnd(1110 + i);
        let x = wrap(k.rnd(1120 + i), fall * 0.8, k.time, 0.12) * w;
        let y = wrap(k.rnd(1130 + i), fall, k.time, 0.12) * h;
        let spin = k.time * (0.4 + k.rnd(1140 + i)) + i as f32;
        let flat = 0.45 + 0.4 * spin.sin().abs();
        c.ellipse(x, y, size, size * flat, spin * 0.6, k.colors[4], 0.32, 0.7);
    }
}

/// Raindrops on the near side of the glass, long and soft.
fn drops(c: &mut Canvas, k: &Ctx) {
    let (w, h) = (c.w as f32, c.h as f32);
    for i in 0..12u32 {
        let near = k.rnd(1200 + i);
        let x = k.rnd(1210 + i) * w;
        let y = wrap(k.rnd(1220 + i), 0.5 + 0.6 * near, k.time, 0.2) * h;
        let len = (0.05 + 0.08 * near) * h;
        let shine = mix(k.colors[3], k.colors[4], 0.6);
        c.ellipse(
            x,
            y,
            (0.6 + 1.6 * near) * h / 300.0,
            len,
            0.12,
            shine,
            0.1 + 0.12 * near,
            0.9,
        );
        if rand(k.seed ^ i.wrapping_mul(77)) > 0.7 {
            c.ellipse(
                x,
                y + len * 0.8,
                len * 0.12,
                len * 0.12,
                0.0,
                shine,
                0.08,
                0.9,
            );
        }
    }
}
