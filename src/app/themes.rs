use super::controller::Controller;
use crate::scenes::{self, Audio, Preset, React, Renderer, Scene, Style};
use crate::{AppWindow, PaletteRow, ThemeCard, ThemeStudio};
use parking_lot::{Condvar, Mutex};
use slint::{Color, ComponentHandle, Image, Model, ModelRc, VecModel};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Hover {
    saved: bool,
    index: usize,
    look: Preset,
    size: (u32, u32),
}

struct Wanted {
    look: Preset,
    generation: u64,
    size: (u32, u32),
    hover: Option<Hover>,
}
/// The background painter: one thread that wakes per frame and posts images to the window.
struct Engine {
    wanted: Mutex<Wanted>,
    wake: Condvar,
    busy: AtomicBool,
}

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();
static SAVES: AtomicU64 = AtomicU64::new(0);

fn engine() -> Option<&'static Arc<Engine>> {
    ENGINE.get()
}

fn show(look: &Preset) {
    if let Some(e) = engine() {
        let mut w = e.wanted.lock();
        w.look = look.clone();
        w.generation += 1;
        e.wake.notify_one();
    }
}
fn varied(look: &Preset, offset: u32) -> Preset {
    Preset {
        seed: look.seed.wrapping_add(offset),
        ..look.clone()
    }
}

fn post(e: &Arc<Engine>, weak: &slint::Weak<AppWindow>, frame: scenes::Frame) -> bool {
    e.busy.store(true, Ordering::Release);
    let e = e.clone();
    let sent = weak.upgrade_in_event_loop(move |w| {
        let studio = w.global::<ThemeStudio>();
        studio.set_image(Image::from_rgb8(frame.image));
        if let Some(frost) = frame.frost {
            studio.set_frost(Image::from_rgb8(frost));
        }
        studio.set_on(true);
        let size = w.window().size();
        e.wanted.lock().size = (size.width, size.height);
        e.busy.store(false, Ordering::Release);
    });
    sent.is_ok()
}
fn post_thumb(
    weak: &slint::Weak<AppWindow>,
    saved: bool,
    index: usize,
    pixels: slint::SharedPixelBuffer<slint::Rgba8Pixel>,
) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let studio = w.global::<ThemeStudio>();
        let model = if saved {
            studio.get_saved()
        } else {
            studio.get_gallery()
        };
        if let Some(mut row) = model.row_data(index) {
            row.thumb = Image::from_rgba8(pixels);
            model.set_row_data(index, row);
        }
    });
}

fn clear(weak: &slint::Weak<AppWindow>) {
    let _ = weak.upgrade_in_event_loop(|w| {
        let studio = w.global::<ThemeStudio>();
        studio.set_on(false);
        studio.set_peek(false);
        studio.set_image(Image::default());
        studio.set_frost(Image::default());
    });
}
fn run(e: Arc<Engine>, weak: slint::Weak<AppWindow>) {
    let mut renderer = Renderer::default();
    let mut shown = Preset::default();
    let mut seen = u64::MAX;
    let mut fade: Option<(Preset, Instant, f32)> = None;
    let (mut time, mut last) = (0.0f32, Instant::now());
    let (mut rotated, mut offset) = (Instant::now(), 0u32);
    let (mut cleared, mut fresh) = (true, true);
    loop {
        let (look, generation, size, hover) = {
            let w = e.wanted.lock();
            (w.look.clone(), w.generation, w.size, w.hover.clone())
        };
        if generation != seen {
            seen = generation;
            if !shown.same_look(&look) {
                if shown.scene != Scene::Classic && look.scene != Scene::Classic {
                    fade = Some((varied(&shown, offset), Instant::now(), 0.9));
                }
                offset = 0;
                rotated = Instant::now();
            }
            shown = look.clone();
            fresh = true;
        }
        let every = Duration::from_secs(u64::from(look.rotate_minutes.max(1)) * 60);
        let alive = look.scene != Scene::Classic;
        if alive && look.rotate && rotated.elapsed() >= every {
            fade = Some((varied(&look, offset), Instant::now(), 4.0));
            offset = offset.wrapping_add(1);
            rotated = Instant::now();
            fresh = true;
        }
        if !alive && hover.is_none() {
            if !cleared {
                clear(&weak);
                cleared = true;
            }
            let mut w = e.wanted.lock();
            if w.generation == seen {
                e.wake.wait(&mut w);
            }
            continue;
        }
        let moving = look.fps > 0 || fade.is_some() || hover.is_some();
        if !moving && !fresh {
            let mut w = e.wanted.lock();
            if w.generation == seen {
                let left = if look.rotate {
                    every.saturating_sub(rotated.elapsed())
                } else {
                    Duration::from_secs(3600)
                };
                e.wake.wait_for(&mut w, left.max(Duration::from_millis(50)));
            }
            continue;
        }
        let start = Instant::now();
        let dt = start.duration_since(last).as_secs_f32().min(0.1);
        last = start;
        let audio = Audio::shaped(crate::visualizer::levels(), look.react, look.strength);
        time += dt * look.speed * (1.0 + audio.flow);
        if alive && !e.busy.load(Ordering::Acquire) {
            let from = fade
                .as_ref()
                .map(|(old, began, secs)| (old, began.elapsed().as_secs_f32() / secs));
            let done = from.is_some_and(|(_, t)| t >= 1.0);
            let frame = renderer.frame(&varied(&look, offset), from, time, audio, size);
            if !post(&e, &weak, frame) {
                return;
            }
            if done {
                fade = None;
            }
            cleared = false;
            fresh = false;
        }
        if let Some(h) = &hover {
            let pixels =
                scenes::thumbnail(&h.look, h.size.0, h.size.1, time, h.size.0 as f32 * 0.07);
            post_thumb(&weak, h.saved, h.index, pixels);
            fresh = false;
        }
        let fps = if hover.is_some() || fade.is_some() {
            look.fps.max(24)
        } else {
            look.fps.max(1)
        };
        let next = Duration::from_secs_f32(1.0 / fps as f32).saturating_sub(start.elapsed());
        let mut w = e.wanted.lock();
        if w.generation == seen {
            e.wake.wait_for(&mut w, next.max(Duration::from_millis(4)));
        }
    }
}
fn rgb(c: u32) -> Color {
    Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn palettes() -> Vec<PaletteRow> {
    scenes::PALETTES
        .iter()
        .map(|p| PaletteRow {
            name: p.name.into(),
            a: rgb(p.colors[0]),
            b: rgb(p.colors[1]),
            c: rgb(p.colors[2]),
            d: rgb(p.colors[3]),
            e: rgb(p.colors[4]),
        })
        .collect()
}
/// Refreshes which card is active, keeping the pictures already in the model.
fn mark(model: ModelRc<ThemeCard>, looks: &[Preset], current: &Preset) {
    for (i, look) in looks.iter().enumerate() {
        let active = look.same_look(current) && look.style == current.style;
        if let Some(mut row) = model.row_data(i).filter(|r| r.active != active) {
            row.active = active;
            model.set_row_data(i, row);
        }
    }
}

fn cards(looks: &[Preset], current: &Preset) -> ModelRc<ThemeCard> {
    let rows: Vec<ThemeCard> = looks
        .iter()
        .map(|look| ThemeCard {
            name: look.name.as_str().into(),
            detail: look.detail().into(),
            thumb: Image::default(),
            active: look.same_look(current) && look.style == current.style,
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}
fn argb(c: u32) -> Color {
    Color::from_argb_encoded(c)
}

/// Panel colours for each style: [sidebar, bar, panel, surface, raised, hover, border, text, muted, faint, popup].
fn style_colors(style: Style, accent: Color) -> [Color; 11] {
    let a = |alpha: f32| accent.with_alpha(alpha);
    match style {
        Style::Clear => [
            0x2e000000, 0x3d000000, 0x24000000, 0x0bffffff, 0x13ffffff, 0x17ffffff, 0x14ffffff,
            0xfff3f3f5, 0xffa2a2a8, 0xff6e6e74, 0xff1c1c1f,
        ]
        .map(argb),
        Style::Glass => [
            0x14ffffff, 0x18ffffff, 0x14ffffff, 0x12ffffff, 0x20ffffff, 0x26ffffff, 0x30ffffff,
            0xfffafafa, 0xffc8c8d0, 0xff8e8e96, 0xf0222228,
        ]
        .map(argb),
        Style::Neon => {
            let base = [0xd808060f, 0xe008060f, 0xd808060f].map(argb);
            [
                base[0],
                base[1],
                base[2],
                a(0.07),
                a(0.14),
                a(0.12),
                a(0.38),
                argb(0xfff7f5ff),
                argb(0xffb3adc9),
                argb(0xff6f6887),
                argb(0xf80d0a17),
            ]
        }
        Style::Solid => [
            0xf00f0f12, 0xf40f0f12, 0xee0f0f12, 0x0dffffff, 0x16ffffff, 0x1affffff, 0x14ffffff,
            0xfff3f3f5, 0xffa2a2a8, 0xff6e6e74, 0xff1c1c1f,
        ]
        .map(argb),
    }
}
fn apply_style(w: &AppWindow, style: Style, accent: Color) {
    let t = w.global::<crate::Theme>();
    let c = style_colors(style, accent);
    t.set_accent(accent);
    t.set_sidebar(c[0]);
    t.set_bar(c[1]);
    t.set_panel(c[2]);
    t.set_surface(c[3]);
    t.set_raised(c[4]);
    t.set_hover(c[5]);
    t.set_border(c[6]);
    t.set_text(c[7]);
    t.set_muted(c[8]);
    t.set_faint(c[9]);
    t.set_popup(c[10]);
    let (s, m, l) = match style {
        Style::Neon => (4.0, 8.0, 10.0),
        Style::Glass => (8.0, 13.0, 18.0),
        _ => (6.0, 10.0, 14.0),
    };
    t.set_radius_s(s);
    t.set_radius_m(m);
    t.set_radius_l(l);
}
/// The accent in use: the theme's own colour while a scene plays, otherwise the one from Settings.
pub fn accent(settings: &crate::model::Settings) -> Color {
    if settings.theme.scene != Scene::Classic {
        let [r, g, b] = settings.theme.accent();
        return Color::from_rgb_u8(r, g, b);
    }
    super::controller::parse_color(&settings.accent).unwrap_or(Color::from_rgb_u8(236, 236, 238))
}
impl Controller {
    pub fn themes_init(&self, window: &AppWindow) {
        let (look, saved) = {
            let s = self.state.lock();
            (s.settings.theme.clone(), s.settings.saved_themes.clone())
        };
        let studio = window.global::<ThemeStudio>();
        let names: Vec<slint::SharedString> = Scene::ALL.iter().map(|s| s.label().into()).collect();
        studio.set_scenes(ModelRc::new(VecModel::from(names)));
        studio.set_palettes(ModelRc::new(VecModel::from(palettes())));
        studio.set_gallery(cards(&scenes::gallery(), &look));
        studio.set_saved(cards(&saved, &look));
        let size = window.window().size();
        let engine = Arc::new(Engine {
            wanted: Mutex::new(Wanted {
                look: look.clone(),
                generation: 0,
                size: if size.width < 64 {
                    (1240, 780)
                } else {
                    (size.width, size.height)
                },
                hover: None,
            }),
            wake: Condvar::new(),
            busy: AtomicBool::new(false),
        });
        if ENGINE.set(engine.clone()).is_ok() {
            let weak = window.as_weak();
            let _ = std::thread::Builder::new()
                .name("themes".into())
                .spawn(move || run(engine, weak));
        }
        self.theme_sync(window, &look);
    }
    fn theme_sync(&self, w: &AppWindow, look: &Preset) {
        let st = w.global::<ThemeStudio>();
        st.set_name(look.name.as_str().into());
        st.set_detail(look.detail().into());
        st.set_scene(look.scene.index() as i32);
        st.set_palette(scenes::palette_index(&look.palette) as i32);
        st.set_style(look.style.index() as i32);
        st.set_react(look.react.index() as i32);
        let fps = scenes::FPS.iter().position(|f| *f == look.fps).unwrap_or(1);
        st.set_fps(fps as i32);
        let every = scenes::INTERVALS
            .iter()
            .position(|m| *m == look.rotate_minutes)
            .unwrap_or(2);
        st.set_interval(every as i32);
        st.set_speed(look.speed);
        st.set_strength(look.strength);
        st.set_dim(look.dim);
        st.set_grain(look.grain);
        st.set_seed(look.seed as i32);
        st.set_rotate(look.rotate);
        let (accent, saved, paused) = {
            let s = self.state.lock();
            (
                accent(&s.settings),
                s.settings.saved_themes.clone(),
                s.paused,
            )
        };
        mark(st.get_gallery(), &scenes::gallery(), look);
        mark(st.get_saved(), &saved, look);
        apply_style(w, look.style, accent);
        let listening = look.scene != Scene::Classic
            && look.react != React::Off
            && crate::visualizer::available();
        if st.get_listening() != listening {
            st.set_listening(listening);
            super::waves::set_playing(w, !paused);
        }
    }
    fn theme_set(&self, look: Preset) {
        self.state.lock().settings.theme = look.clone();
        show(&look);
        let this = self.clone();
        self.ui(move |w| this.theme_sync(&w, &look));
        let ticket = SAVES.fetch_add(1, Ordering::Relaxed) + 1;
        let this = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if SAVES.load(Ordering::Relaxed) == ticket {
                this.save_settings();
            }
        });
    }
    pub fn theme_action(&self, action: &str, value: &str) {
        let mut look = self.state.lock().settings.theme.clone();
        let index = value.parse::<i32>().unwrap_or(0);
        let amount = value.parse::<f32>().unwrap_or(0.0).clamp(0.0, 1.0);
        match action {
            "scene" => {
                let scene = Scene::at(index);
                if look.scene == Scene::Classic && look.palette == "Mono" && scene != Scene::Classic
                {
                    look.palette = scenes::PALETTES
                        [1 + index as usize % (scenes::PALETTES.len() - 1)]
                        .name
                        .into();
                }
                look.scene = scene;
            }
            "palette" => {
                look.palette = scenes::PALETTES
                    [(index.max(0) as usize).min(scenes::PALETTES.len() - 1)]
                .name
                .into()
            }
            "style" => look.style = Style::at(index),
            "react" => look.react = React::at(index),
            "fps" => look.fps = scenes::FPS[(index.max(0) as usize).min(2)],
            "interval" => look.rotate_minutes = scenes::INTERVALS[(index.max(0) as usize).min(3)],
            "speed" => look.speed = (amount * 3.0).max(0.1),
            "strength" => look.strength = amount,
            "dim" => look.dim = amount * 0.85,
            "grain" => look.grain = amount,
            "rotate" => look.rotate = value == "true",
            "seed" => {
                look.seed = match value {
                    "next" => look.seed.wrapping_add(1),
                    "prev" => look.seed.wrapping_sub(1).max(1),
                    _ => 1 + crate::scenes::fresh_seed() % 9999,
                }
            }
            "random" => look = crate::scenes::surprise(),
            "apply" => match self.theme_pick(value) {
                Some(picked) => look = picked,
                None => return,
            },
            "delete" => return self.theme_delete(index),
            "hover" => return self.theme_hover(value),
            "thumbs" => return self.theme_thumbs(),
            _ => return,
        }
        self.theme_set(look);
    }
    fn theme_pick(&self, value: &str) -> Option<Preset> {
        let (kind, index) = value.split_once(':')?;
        let index: usize = index.parse().ok()?;
        match kind {
            "g" => scenes::gallery().get(index).cloned(),
            _ => self.state.lock().settings.saved_themes.get(index).cloned(),
        }
    }
    fn theme_delete(&self, index: i32) {
        let removed = {
            let mut s = self.state.lock();
            let i = usize::try_from(index)
                .ok()
                .filter(|i| *i < s.settings.saved_themes.len());
            i.map(|i| s.settings.saved_themes.remove(i))
        };
        let Some(gone) = removed else { return };
        let (current, saved) = {
            let s = self.state.lock();
            (s.settings.theme.clone(), s.settings.saved_themes.clone())
        };
        self.ui(move |w| w.global::<ThemeStudio>().set_saved(cards(&saved, &current)));
        self.theme_thumbs();
        self.save_settings();
        self.status(format!("Removed \"{}\"", gone.name));
    }
    fn theme_hover(&self, value: &str) {
        let Some(e) = engine() else { return };
        let size = self.thumb_size();
        let mut wanted = e.wanted.lock();
        if let Some(left) = value.strip_prefix('-') {
            let same = wanted.hover.as_ref().is_some_and(|h| {
                left == format!("{}:{}", if h.saved { "s" } else { "g" }, h.index)
            });
            if same {
                wanted.hover = None;
            }
            return;
        }
        let Some(look) = self.theme_pick(value) else {
            return;
        };
        let saved = value.starts_with('s');
        let index = value[2..].parse().unwrap_or(0);
        wanted.hover = Some(Hover {
            saved,
            index,
            look,
            size,
        });
        wanted.generation += 1;
        e.wake.notify_one();
    }
    fn thumb_size(&self) -> (u32, u32) {
        let (width, scale) = self
            .window
            .upgrade()
            .map(|w| {
                (
                    w.global::<ThemeStudio>().get_tile_width(),
                    w.window().scale_factor(),
                )
            })
            .unwrap_or((200.0, 1.0));
        let w = (width.max(80.0) * scale).round() as u32;
        (w, (w as f32 * 0.6).round() as u32)
    }
    fn theme_thumbs(&self) {
        let (w, h) = self.thumb_size();
        let saved = self.state.lock().settings.saved_themes.clone();
        let weak = self.window.clone();
        let radius = 14.0 * w as f32 / 200.0;
        let _ = std::thread::Builder::new()
            .name("theme-thumbs".into())
            .spawn(move || {
                let lists = [(false, scenes::gallery()), (true, saved)];
                for (is_saved, looks) in lists {
                    for (i, look) in looks.iter().enumerate() {
                        let pixels = scenes::thumbnail(look, w, h, 24.0, radius);
                        post_thumb(&weak, is_saved, i, pixels);
                    }
                }
            });
    }
    pub fn theme_save(&self, name: &str) {
        let (current, saved) = {
            let mut s = self.state.lock();
            s.settings.theme.name = name.to_owned();
            let look = s.settings.theme.clone();
            let list = &mut s.settings.saved_themes;
            match list.iter_mut().find(|t| t.name == name) {
                Some(slot) => *slot = look.clone(),
                None => list.push(look.clone()),
            }
            (look, list.clone())
        };
        let this = self.clone();
        self.ui(move |w| {
            w.global::<ThemeStudio>().set_saved(cards(&saved, &current));
            this.theme_sync(&w, &current);
            this.theme_thumbs();
        });
        self.save_settings();
        self.status(format!("Saved \"{name}\""));
    }
}
