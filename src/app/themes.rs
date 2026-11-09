use super::controller::Controller;
use crate::scenes::render::{self, Renderer};
use crate::scenes::{self, Audio, Preset, React, Scene, Style, media};
use crate::{AppWindow, PaletteRow, ThemeCard, ThemeStudio};
use parking_lot::{Condvar, Mutex};
use slint::{Color, ComponentHandle, Image, Model, ModelRc, VecModel};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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
    /// The window is fully covered, so nothing needs painting.
    occluded: bool,
    /// The window is minimized (zero size on Windows).
    minimized: bool,
    /// Music is paused: the scene slows down and paints less often.
    paused: bool,
    /// The playing song's cover colour, for the album palette.
    album: Option<[u8; 3]>,
    /// Another window has focus, so the scene can move a little slower. Starts true because
    /// the compositor only reports focus once the window gets it.
    unfocused: bool,
}
/// The background painter: one thread that wakes per frame and posts images to the window.
struct Engine {
    wanted: Mutex<Wanted>,
    wake: Condvar,
    busy: AtomicBool,
}

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();
static SAVES: AtomicU64 = AtomicU64::new(0);
/// Which of the five custom colours the editor is changing.
static PICK: AtomicUsize = AtomicUsize::new(2);
/// Counts the window's redraws, so the painter can tell when the compositor stops showing it.
static DRAWS: AtomicU64 = AtomicU64::new(0);
/// The painter is waiting for the window to draw again before it paints more.
static STALLED: AtomicBool = AtomicBool::new(false);
/// Apple builds draw from a display link that never reports a redraw, so only
/// other platforms can tell a hidden window from its missing redraws.
const WATCHES_DRAWS: bool = !cfg!(target_vendor = "apple");
/// The playlist whose pinned theme is showing instead of the user's own, and that theme.
static PINNED: Mutex<Option<(i64, Preset)>> = Mutex::new(None);

fn engine() -> Option<&'static Arc<Engine>> {
    ENGINE.get()
}

fn update(change: impl FnOnce(&mut Wanted)) {
    if let Some(e) = engine() {
        let mut w = e.wanted.lock();
        change(&mut w);
        w.generation += 1;
        e.wake.notify_one();
    }
}

/// The window was covered by other windows or shown again.
pub fn set_occluded(occluded: bool) {
    if engine().is_some_and(|e| e.wanted.lock().occluded != occluded) {
        update(|w| w.occluded = occluded);
    }
}

/// The window drew a frame. Wayland compositors stop asking hidden windows to draw,
/// so frames posted without a redraw mean nobody can see the scene.
pub fn drawn() {
    DRAWS.fetch_add(1, Ordering::SeqCst);
    if STALLED.swap(false, Ordering::SeqCst)
        && let Some(e) = engine()
    {
        let _w = e.wanted.lock();
        e.wake.notify_one();
    }
}

/// The window was minimized or restored.
pub fn set_minimized(minimized: bool) {
    if engine().is_some_and(|e| e.wanted.lock().minimized != minimized) {
        update(|w| w.minimized = minimized);
    }
}

/// The window changed size, so a picture background is decoded again to fit.
pub fn set_size(width: u32, height: u32) {
    if width >= 64
        && height >= 64
        && engine().is_some_and(|e| e.wanted.lock().size != (width, height))
    {
        update(|w| w.size = (width, height));
    }
}

/// The window gained or lost keyboard focus.
pub fn set_focused(focused: bool) {
    if engine().is_some_and(|e| e.wanted.lock().unfocused == focused) {
        update(|w| w.unfocused = !focused);
    }
}

/// Frame rate caps for a scene nobody is looking at directly, and for running on battery.
const QUIET_FPS: u32 = 10;
const BATTERY_FPS: u32 = 8;

/// Whether the computer is running from its battery rather than mains power.
fn on_battery() -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") else {
            return false;
        };
        let (mut mains, mut online) = (false, false);
        for entry in dir.flatten() {
            let path = entry.path();
            let read = |name: &str| std::fs::read_to_string(path.join(name)).unwrap_or_default();
            if read("type").trim() == "Mains" {
                mains = true;
                online |= read("online").trim() == "1";
            }
        }
        mains && !online
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("pmset")
            .args(["-g", "ps"])
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("'Battery Power'"))
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut status = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.ACLineStatus == 0
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        false
    }
}

/// Music paused or resumed.
pub fn set_paused(paused: bool) {
    let changed = engine().is_some_and(|e| e.wanted.lock().paused != paused);
    if changed {
        update(|w| w.paused = paused);
    }
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

fn post(e: &Arc<Engine>, weak: &slint::Weak<AppWindow>, frame: render::Frame) -> bool {
    e.busy.store(true, Ordering::Release);
    let e = e.clone();
    let sent = weak.upgrade_in_event_loop(move |w| {
        if super::waves::ticking() {
            PENDING.with(|p| *p.borrow_mut() = Some((frame, e)));
        } else {
            show_frame(&w, frame, &e);
        }
    });
    sent.is_ok()
}

thread_local! {
    /// A painted frame held for the next wave tick, so both change in one redraw.
    static PENDING: std::cell::RefCell<Option<(render::Frame, Arc<Engine>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Shows the frame held for this wave tick, if there is one.
pub fn flush(w: &AppWindow) {
    if let Some((frame, e)) = PENDING.with(|p| p.borrow_mut().take()) {
        show_frame(w, frame, &e);
    }
}

fn show_frame(w: &AppWindow, frame: render::Frame, e: &Engine) {
    let studio = w.global::<ThemeStudio>();
    studio.set_image(Image::from_rgb8(frame.image));
    if let Some(frost) = frame.frost {
        studio.set_frost(Image::from_rgb8(frost));
    }
    studio.set_on(true);
    let size = w.window().size();
    e.wanted.lock().size = (size.width, size.height);
    e.busy.store(false, Ordering::Release);
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
        if let Some((_, e)) = PENDING.with(|p| p.borrow_mut().take()) {
            e.busy.store(false, Ordering::Release);
        }
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
    let (mut drew, mut unseen) = (DRAWS.load(Ordering::SeqCst), None::<Instant>);
    let (mut battery, mut checked) = (on_battery(), Instant::now());
    let mut stream: Option<media::Stream> = None;
    let mut failed: Option<String> = None;
    let mut resized = Instant::now();
    loop {
        if checked.elapsed() > Duration::from_secs(30) {
            battery = on_battery();
            checked = Instant::now();
        }
        let (mut look, generation, size, hover, hidden, paused, album, unfocused) = {
            let w = e.wanted.lock();
            let look = w.look.clone();
            let hidden = w.occluded || w.minimized;
            (
                look,
                w.generation,
                w.size,
                w.hover.clone(),
                hidden,
                w.paused,
                w.album,
                w.unfocused,
            )
        };
        look.tint = album;
        if generation != seen {
            seen = generation;
            let both_alive = ![shown.scene, look.scene]
                .iter()
                .any(|s| matches!(s, Scene::Classic | Scene::Picture));
            if !shown.same_look(&look) {
                if both_alive {
                    fade = Some((varied(&shown, offset), Instant::now(), 0.9));
                }
                offset = 0;
                rotated = Instant::now();
            } else if both_alive && look.palette == scenes::ALBUM && shown.tint != look.tint {
                fade = Some((varied(&shown, offset), Instant::now(), 1.5));
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
        let picture = look.scene == Scene::Picture
            && !look.media.is_empty()
            && failed.as_deref() != Some(look.media.as_str());
        if !picture && stream.is_some() {
            stream = None;
        }
        if !alive && !cleared {
            clear(&weak);
            cleared = true;
        }
        if !alive && hover.is_none() {
            let mut w = e.wanted.lock();
            if w.generation == seen {
                e.wake.wait(&mut w);
            }
            continue;
        }
        if hidden && hover.is_none() {
            let mut w = e.wanted.lock();
            if w.generation == seen {
                e.wake.wait(&mut w);
            }
            last = Instant::now();
            continue;
        }
        let now_drew = DRAWS.load(Ordering::SeqCst);
        if now_drew != drew {
            drew = now_drew;
            unseen = None;
        }
        if WATCHES_DRAWS
            && hover.is_none()
            && unseen.is_some_and(|t| t.elapsed() > Duration::from_millis(1500))
        {
            let mut w = e.wanted.lock();
            STALLED.store(true, Ordering::SeqCst);
            if w.generation == seen && DRAWS.load(Ordering::SeqCst) == drew {
                e.wake.wait_for(&mut w, Duration::from_secs(5));
            }
            STALLED.store(false, Ordering::SeqCst);
            last = Instant::now();
            continue;
        }
        if picture {
            let want = media::frame_size(&look.media, size);
            let rate = if look.fps == 0 { 15 } else { look.fps };
            let same = stream
                .as_ref()
                .is_some_and(|s| s.media == look.media && s.fps == rate);
            let stale = stream.as_ref().is_some_and(|s| s.size != want);
            if !stale {
                resized = Instant::now();
            }
            if !same || (stale && resized.elapsed() > Duration::from_millis(250)) {
                let start = stream
                    .as_ref()
                    .filter(|s| s.media == look.media)
                    .map_or(0.0, media::Stream::played);
                drop(stream.take());
                stream = media::Stream::open(&look.media, want, rate, start);
                if stream.is_none() {
                    failed = Some(look.media.clone());
                    continue;
                }
                fresh = true;
            }
            let Some(s) = stream.as_mut() else { continue };
            let playing = !s.is_still() && look.fps > 0 && !battery;
            let start = Instant::now();
            if (fresh || playing) && !e.busy.load(Ordering::Acquire) {
                match s.next() {
                    Some(px) => {
                        let frame = renderer.media_frame(px, s.size, &look);
                        if !post(&e, &weak, frame) {
                            return;
                        }
                        unseen.get_or_insert_with(Instant::now);
                        cleared = false;
                        fresh = false;
                    }
                    None if s.is_still() => fresh = false,
                    None => {
                        log::warn!("The background video {} stopped", look.media);
                        failed = Some(look.media.clone());
                        stream = None;
                        continue;
                    }
                }
            }
            fade = None;
            if let Some(h) = &hover {
                time += 1.0 / 24.0;
                let pixels =
                    render::thumbnail(&h.look, h.size.0, h.size.1, time, h.size.0 as f32 * 0.07);
                post_thumb(&weak, h.saved, h.index, pixels);
            }
            let wait = if hover.is_some() {
                Duration::from_secs_f32(1.0 / 24.0).saturating_sub(start.elapsed())
            } else if stale {
                Duration::from_millis(260)
            } else if fresh {
                Duration::from_millis(16)
            } else if playing {
                let fps = if paused { (rate / 4).max(1) } else { rate };
                Duration::from_secs_f32(1.0 / fps as f32).saturating_sub(start.elapsed())
            } else {
                Duration::from_secs(3600)
            };
            let mut w = e.wanted.lock();
            if w.generation == seen {
                e.wake.wait_for(&mut w, wait.max(Duration::from_millis(4)));
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
        let pace = if paused { 0.25 } else { 1.0 };
        time += dt * look.speed * pace * (1.0 + audio.flow);
        if alive && !e.busy.load(Ordering::Acquire) {
            let from = fade
                .as_ref()
                .map(|(old, began, secs)| (old, began.elapsed().as_secs_f32() / secs));
            let done = from.is_some_and(|(_, t)| t >= 1.0);
            let frame = renderer.frame(&varied(&look, offset), from, time, audio, size);
            if !post(&e, &weak, frame) {
                return;
            }
            unseen.get_or_insert_with(Instant::now);
            if done {
                fade = None;
            }
            cleared = false;
            fresh = false;
        }
        if let Some(h) = &hover {
            let pixels =
                render::thumbnail(&h.look, h.size.0, h.size.1, time, h.size.0 as f32 * 0.07);
            post_thumb(&weak, h.saved, h.index, pixels);
            fresh = false;
        }
        let fps = if hover.is_some() || fade.is_some() {
            look.fps.max(24)
        } else if paused {
            look.fps.clamp(1, 6)
        } else {
            let mut fps = look.fps.max(1);
            if battery {
                fps = fps.min(BATTERY_FPS);
            }
            if unfocused {
                fps = fps.min(QUIET_FPS);
            }
            fps
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

fn palettes(custom: [u32; 5]) -> Vec<PaletteRow> {
    let mut rows: Vec<PaletteRow> = scenes::PALETTES
        .iter()
        .map(|p| {
            let live = Preset {
                palette: p.name.into(),
                ..Preset::default()
            };
            let c = if p.name == scenes::ALBUM {
                live.colors().map(|c| {
                    let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
                    Color::from_rgb_u8(r, g, b)
                })
            } else {
                p.colors.map(rgb)
            };
            PaletteRow {
                name: p.name.into(),
                a: c[0],
                b: c[1],
                c: c[2],
                d: c[3],
                e: c[4],
            }
        })
        .collect();
    rows.push(custom_row(custom));
    rows
}

fn custom_row(custom: [u32; 5]) -> PaletteRow {
    let c = custom.map(rgb);
    PaletteRow {
        name: scenes::CUSTOM.into(),
        a: c[0],
        b: c[1],
        c: c[2],
        d: c[3],
        e: c[4],
    }
}

/// Hue, saturation and brightness, each 0 to 1.
fn to_hsv(c: u32) -> (f32, f32, f32) {
    let [r, g, b] = [16, 8, 0].map(|s| ((c >> s) & 255) as f32 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, if max > 0.0 { d / max } else { 0.0 }, max)
}

fn from_hsv(h: f32, s: f32, v: f32) -> u32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let byte = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// The still of a picture look, for the editor.
fn media_thumb(name: &str) -> Image {
    media::poster(name).map_or_else(Image::default, |p| {
        Image::from_rgb8(slint::SharedPixelBuffer::clone_from_slice(
            &p.px, p.w as u32, p.h as u32,
        ))
    })
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
    let pinned = PINNED.lock().as_ref().map(|(_, look)| look.clone());
    accent_for(pinned.as_ref().unwrap_or(&settings.theme), settings)
}

fn accent_for(look: &Preset, settings: &crate::model::Settings) -> Color {
    if look.scene != Scene::Classic {
        let [r, g, b] = look.accent();
        return Color::from_rgb_u8(r, g, b);
    }
    super::controller::parse_color(&settings.accent).unwrap_or(Color::from_rgb_u8(236, 236, 238))
}
impl Controller {
    pub fn themes_init(&self, window: &AppWindow) {
        let (look, saved, paused) = {
            let s = self.state.lock();
            (
                s.settings.theme.clone(),
                s.settings.saved_themes.clone(),
                s.paused,
            )
        };
        let studio = window.global::<ThemeStudio>();
        let names: Vec<slint::SharedString> = Scene::ALL.iter().map(|s| s.label().into()).collect();
        studio.set_scenes(ModelRc::new(VecModel::from(names)));
        studio.set_palettes(ModelRc::new(VecModel::from(palettes(look.custom))));
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
                occluded: false,
                minimized: false,
                paused,
                album: None,
                unfocused: true,
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
        st.set_code(look.to_code().into());
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
        let palettes = st.get_palettes();
        if let Some(last) = palettes.row_count().checked_sub(1) {
            palettes.set_row_data(last, custom_row(look.custom));
        }
        let pick = PICK.load(Ordering::Relaxed).min(4);
        let colors: Vec<Color> = look.custom.iter().map(|c| rgb(*c)).collect();
        st.set_custom(ModelRc::new(VecModel::from(colors)));
        st.set_custom_pick(pick as i32);
        let (h, s, v) = to_hsv(look.custom[pick]);
        st.set_custom_hue(h);
        st.set_custom_sat(s);
        st.set_custom_val(v);
        st.set_custom_hex(format!("#{:06x}", look.custom[pick]).into());
        st.set_picture(look.scene == Scene::Picture);
        st.set_moving(!look.media.is_empty() && !media::is_still(&look.media));
        if look.scene == Scene::Picture {
            st.set_media_thumb(media_thumb(&look.media));
        }
        let (accent, saved, paused) = {
            let s = self.state.lock();
            (
                accent_for(look, &s.settings),
                s.settings.saved_themes.clone(),
                s.paused,
            )
        };
        mark(st.get_gallery(), &scenes::gallery(), look);
        mark(st.get_saved(), &saved, look);
        apply_style(w, look.style, accent);
        let wave = w.global::<crate::WaveLook>();
        let [r, g, b] = look.colors()[2].map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8);
        wave.set_second(Color::from_rgb_u8(r, g, b));
        wave.set_two_tone(look.scene != Scene::Classic);
        let listening = look.scene != Scene::Classic
            && look.react != React::Off
            && crate::visualizer::available();
        if st.get_listening() != listening {
            st.set_listening(listening);
            super::waves::set_playing(w, !paused);
        }
    }
    fn theme_set(&self, look: Preset) {
        *PINNED.lock() = None;
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
        let mut look = self.shown_look();
        let index = value.parse::<i32>().unwrap_or(0);
        let amount = value.parse::<f32>().unwrap_or(0.0).clamp(0.0, 1.0);
        match action {
            "scene" => {
                let scene = Scene::at(index);
                if scene == Scene::Picture && look.media.is_empty() {
                    return self.theme_pick_media();
                }
                if look.scene == Scene::Classic && look.palette == "Mono" && scene != Scene::Classic
                {
                    look.palette = scenes::PALETTES
                        [1 + index as usize % (scenes::PALETTES.len() - 1)]
                        .name
                        .into();
                }
                look.scene = scene;
            }
            "palette" if index as usize >= scenes::PALETTES.len() => {
                if look.palette != scenes::CUSTOM {
                    look.custom = look.base_colors();
                }
                look.palette = scenes::CUSTOM.into();
            }
            "custom-pick" => {
                PICK.store(index.clamp(0, 4) as usize, Ordering::Relaxed);
                let this = self.clone();
                return self.ui(move |w| this.theme_sync(&w, &look));
            }
            "hue" | "sat" | "val" | "hex" => {
                let pick = PICK.load(Ordering::Relaxed).min(4);
                let (mut h, mut s, mut v) = to_hsv(look.custom[pick]);
                match action {
                    "hue" => h = amount.min(0.999),
                    "sat" => s = amount,
                    "val" => v = amount,
                    _ => {}
                }
                look.custom[pick] = if action == "hex" {
                    match scenes::parse_hex(value) {
                        Some(c) => c,
                        None => return self.status("Write a colour like #3ddc97"),
                    }
                } else {
                    from_hsv(h, s, v)
                };
                look.palette = scenes::CUSTOM.into();
            }
            "media-pick" => return self.theme_pick_media(),
            "import" => return self.theme_pick_file(),
            "export" => return self.theme_export(),
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
                        let pixels = render::thumbnail(look, w, h, 24.0, radius);
                        post_thumb(&weak, is_saved, i, pixels);
                    }
                }
            });
    }
    pub fn theme_save(&self, name: &str) {
        let mut look = self.shown_look();
        look.name = name.to_owned();
        self.theme_keep(look);
        self.status(format!("Saved \"{name}\""));
    }
    /// Puts `look` on screen and in "Your themes", replacing one with the same name.
    fn theme_keep(&self, look: Preset) {
        *PINNED.lock() = None;
        show(&look);
        let name = look.name.clone();
        let (current, saved) = {
            let mut s = self.state.lock();
            s.settings.theme = look.clone();
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
    }
    fn theme_pick_media(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            let kinds: Vec<&str> = media::STILLS.iter().chain(media::MOVING).copied().collect();
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Choose a picture or video")
                .add_filter("Pictures, GIFs and videos", &kinds)
                .pick_file()
                .await;
            if let Some(file) = picked {
                this.theme_use_media(file.path().to_path_buf());
            }
        });
    }
    fn theme_pick_file(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Import a theme")
                .add_filter("Tonal themes", &["tonaltheme", "toml"])
                .pick_file()
                .await;
            if let Some(file) = picked {
                this.theme_import(file.path().to_path_buf());
            }
        });
    }
    /// A file opened or dropped on the window: a theme to import, or a background to use.
    pub fn theme_open_file(&self, path: PathBuf) {
        if media::is_media(&path) {
            self.theme_use_media(path);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tonaltheme") || e.eq_ignore_ascii_case("toml"))
        {
            self.theme_import(path);
        }
    }
    fn theme_use_media(&self, path: PathBuf) {
        self.status("Getting your background ready…");
        let this = self.clone();
        tokio::spawn(async move {
            let done = tokio::task::spawn_blocking(move || media::import(&path)).await;
            let Some((name, rgb)) = this.result(done.map_err(Into::into).and_then(|r| r)) else {
                return;
            };
            let mut look = this.shown_look();
            look.scene = Scene::Picture;
            look.media = name;
            look.media_rgb = Some(rgb);
            look.name = "Your own".into();
            this.theme_set(look);
            this.status("Background set. Press Save to keep it with your themes");
        });
    }
    fn theme_import(&self, path: PathBuf) {
        let this = self.clone();
        tokio::spawn(async move {
            let done = tokio::task::spawn_blocking(move || super::theme_files::import(&path)).await;
            if let Some(look) = this.result(done.map_err(Into::into).and_then(|r| r)) {
                let name = look.name.clone();
                this.theme_keep(look);
                this.status(format!("Added \"{name}\" to your themes"));
            }
        });
    }
    fn theme_export(&self) {
        let look = self.shown_look();
        let this = self.clone();
        tokio::spawn(async move {
            let stem: String = look
                .name
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == ' ' || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Export this theme")
                .set_file_name(format!("{}.tonaltheme", stem.trim()))
                .add_filter("Tonal theme", &["tonaltheme"])
                .save_file()
                .await;
            let Some(file) = picked else { return };
            let path = file.path().to_path_buf();
            let shown = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let done =
                tokio::task::spawn_blocking(move || super::theme_files::export(&look, &path)).await;
            if this
                .result(done.map_err(Into::into).and_then(|r| r))
                .is_some()
            {
                this.status(format!("Exported {shown}"));
            }
        });
    }
    /// The look on screen: a playing playlist's pinned theme, or the user's own.
    fn shown_look(&self) -> Preset {
        let pinned = PINNED.lock().as_ref().map(|(_, look)| look.clone());
        pinned.unwrap_or_else(|| self.state.lock().settings.theme.clone())
    }
    fn theme_show(&self, look: Preset) {
        show(&look);
        let this = self.clone();
        self.ui(move |w| this.theme_sync(&w, &look));
    }
    /// The playing song's cover colour changed, or the song has none.
    pub fn theme_album(&self, rgb: Option<[u8; 3]>) {
        scenes::set_album_tint(rgb);
        update(|w| w.album = rgb);
        let look = self.shown_look();
        let this = self.clone();
        self.ui(move |w| {
            let studio = w.global::<ThemeStudio>();
            studio.set_palettes(ModelRc::new(VecModel::from(palettes(look.custom))));
            if look.palette == scenes::ALBUM {
                this.theme_sync(&w, &look);
            }
            if w.get_page() == "Themes" {
                this.theme_thumbs();
            }
        });
    }
    /// Music started from `playlist`: show its pinned theme, or go back to the user's own.
    pub fn theme_playing_from(&self, playlist: Option<i64>) {
        let pinned = {
            let s = self.state.lock();
            playlist.and_then(|id| Some((id, s.settings.playlist_themes.get(&id)?.clone())))
        };
        let before = std::mem::replace(&mut *PINNED.lock(), pinned.clone());
        match (pinned, before) {
            (Some((_, look)), before) if before.as_ref().map(|b| &b.1) != Some(&look) => {
                self.theme_show(look)
            }
            (None, Some(_)) => {
                let own = self.state.lock().settings.theme.clone();
                self.theme_show(own);
            }
            _ => {}
        }
    }
    /// Pins the look on screen to the open playlist, or unpins it.
    pub fn theme_pin_playlist(&self) {
        let look = self.shown_look();
        let pinned = {
            let mut s = self.state.lock();
            let Some(id) = s.playlist else { return };
            let map = &mut s.settings.playlist_themes;
            if map.remove(&id).is_none() {
                map.insert(id, look);
            }
            map.contains_key(&id)
        };
        self.ui(move |w| w.set_playlist_themed(pinned));
        let showing = PINNED.lock().as_ref().map(|(id, _)| *id);
        if !pinned && showing.is_some() && showing == self.state.lock().playlist {
            *PINNED.lock() = None;
            let own = self.state.lock().settings.theme.clone();
            self.theme_show(own);
        }
        self.save_settings();
        self.status(if pinned {
            "This playlist now plays with this theme".to_owned()
        } else {
            "This playlist uses your own theme again".to_owned()
        });
    }
    /// Loads a theme from a share code, as typed in the code dialog.
    pub fn theme_code(&self, code: &str) {
        if code.trim() == self.shown_look().to_code() {
            return;
        }
        match Preset::from_code(code) {
            Some(look) => {
                let name = look.name.clone();
                self.theme_set(look);
                self.status(format!("Loaded \"{name}\""));
            }
            None => self.status("That isn't a Tonal theme code".to_owned()),
        }
    }
}
