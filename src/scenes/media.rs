//! Pictures, GIFs and videos behind the "Your own" scene. mpv, which Tonal already ships for
//! music, decodes them straight to raw frames at the size the window needs.
use super::draw::Canvas;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;

pub const STILLS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "bmp", "avif", "heic", "tif", "tiff",
];
pub const MOVING: &[&str] = &["gif", "mp4", "m4v", "mov", "webm", "mkv", "avi"];
/// The biggest file Tonal copies in as a background.
const MAX_BYTES: u64 = 1 << 30;
const POSTER: (u32, u32) = (480, 300);

/// Where imported pictures and videos live.
pub fn dir() -> PathBuf {
    crate::model::paths::data_dir().join("themes")
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn is_still(name: &str) -> bool {
    STILLS.contains(&extension(name).as_str())
}

pub fn is_media(path: &Path) -> bool {
    let ext = extension(&path.to_string_lossy());
    STILLS.contains(&ext.as_str()) || MOVING.contains(&ext.as_str())
}

/// A name made only of what `import` writes, so a theme file can't point outside `dir()`.
pub fn is_safe_name(name: &str) -> bool {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    !stem.is_empty()
        && stem.chars().all(|c| c.is_ascii_hexdigit())
        && (STILLS.contains(&ext) || MOVING.contains(&ext))
}

pub fn path(name: &str) -> Option<PathBuf> {
    is_safe_name(name).then(|| dir().join(name))
}

/// A small still of a picture or video, for cards and for painting while a video starts.
pub struct Poster {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

fn decoder() -> Command {
    let mut command = Command::new("mpv");
    command
        .args([
            "--no-config",
            "--really-quiet",
            "--no-terminal",
            "--no-audio",
            "--hwdec=auto-copy-safe",
            "--o=-",
            "--of=rawvideo",
            "--ovc=rawvideo",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

/// Frames of `w` by `h`, cropped to fill like a wallpaper, `fps` a second for moving files.
fn frames(file: &Path, w: u32, h: u32, fps: Option<u32>, start: f64) -> Command {
    let mut command = decoder();
    let rate = fps.map(|f| format!("fps={f},")).unwrap_or_default();
    command.arg(format!(
        "--vf=lavfi=[{rate}scale=w={w}:h={h}:force_original_aspect_ratio=increase:flags=bilinear,crop={w}:{h},format=rgb24]"
    ));
    if fps.is_some() {
        command.args(["--loop-file=inf", &format!("--start={start:.2}")]);
    } else {
        command.args(["--image-display-duration=0", "--frames=1"]);
    }
    command.arg("--").arg(file);
    command
}

fn first_frame(file: &Path, w: u32, h: u32) -> Option<Vec<u8>> {
    let fps = (!is_still(&file.to_string_lossy())).then_some(30);
    let mut child = frames(file, w, h, fps, 0.0).spawn().ok()?;
    let mut px = vec![0u8; (w * h * 3) as usize];
    let read = child.stdout.take()?.read_exact(&mut px);
    let _ = child.kill();
    let _ = child.wait();
    read.ok().map(|_| px)
}

fn poster_path(name: &str) -> PathBuf {
    dir().join(format!("{name}.poster.png"))
}

fn make_poster(name: &str) -> Option<Poster> {
    let (w, h) = POSTER;
    let px = first_frame(&path(name)?, w, h)?;
    if let Err(e) = image::save_buffer(poster_path(name), &px, w, h, image::ExtendedColorType::Rgb8)
    {
        log::warn!("Saving the still of {name}: {e}");
    }
    Some(Poster {
        w: w as usize,
        h: h as usize,
        px,
    })
}

static POSTERS: Mutex<Option<HashMap<String, Arc<Poster>>>> = Mutex::new(None);

/// The still of an imported file, made once and kept.
pub fn poster(name: &str) -> Option<Arc<Poster>> {
    if name.is_empty() {
        return None;
    }
    if let Some(p) = POSTERS.lock().get_or_insert_default().get(name) {
        return Some(p.clone());
    }
    let poster = image::open(poster_path(name))
        .ok()
        .map(|i| {
            let i = i.to_rgb8();
            Poster {
                w: i.width() as usize,
                h: i.height() as usize,
                px: i.into_raw(),
            }
        })
        .or_else(|| make_poster(name))
        .map(Arc::new)?;
    POSTERS
        .lock()
        .get_or_insert_default()
        .insert(name.to_owned(), poster.clone());
    Some(poster)
}

/// The colour a picture is mostly made of, leaning to its most colourful parts.
pub fn main_color(poster: &Poster) -> [u8; 3] {
    let mut sum = [0.0f64; 3];
    let mut total = 0.0f64;
    for p in poster.px.as_chunks::<3>().0.iter().step_by(3) {
        let (hi, lo) = (
            p.iter().max().copied().unwrap_or(0),
            p.iter().min().copied().unwrap_or(0),
        );
        let weight = f64::from(hi - lo) / 255.0 * f64::from(hi) / 255.0 + 0.02;
        for (s, v) in sum.iter_mut().zip(p) {
            *s += f64::from(*v) * weight;
        }
        total += weight;
    }
    sum.map(|s| (s / total.max(1e-6)).round().clamp(0.0, 255.0) as u8)
}

/// Copies a picture or video in and returns its name and main colour.
pub fn import(file: &Path) -> anyhow::Result<(String, [u8; 3])> {
    use sha2::{Digest, Sha256};
    if !is_media(file) {
        anyhow::bail!("Tonal can use pictures, GIFs and videos");
    }
    let size = std::fs::metadata(file)?.len();
    if size > MAX_BYTES {
        anyhow::bail!("That file is over 1 GB");
    }
    let mut hasher = Sha256::new();
    let mut input = std::fs::File::open(file)?;
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        let n = input.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
    }
    let digest = hasher.finalize();
    let stem: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let name = format!("{stem}.{}", extension(&file.to_string_lossy()));
    std::fs::create_dir_all(dir())?;
    let target = dir().join(&name);
    if !target.exists() {
        std::fs::copy(file, &target)?;
    }
    let Some(poster) = poster(&name) else {
        let _ = std::fs::remove_file(&target);
        anyhow::bail!("Tonal couldn't open that file");
    };
    Ok((name, main_color(&poster)))
}

/// Paints a poster into a canvas, cropped to fill it.
pub fn paint(canvas: &mut Canvas, poster: &Poster) {
    let (cw, ch) = (canvas.w.max(1) as f32, canvas.h.max(1) as f32);
    let (pw, ph) = (poster.w as f32, poster.h as f32);
    let k = (pw / cw).min(ph / ch);
    let (ox, oy) = ((pw - cw * k) / 2.0, (ph - ch * k) / 2.0);
    canvas.fill(|x, y, _, _| {
        let sx = ((ox + (x as f32 + 0.5) * k) as usize).min(poster.w - 1);
        let sy = ((oy + (y as f32 + 0.5) * k) as usize).min(poster.h - 1);
        let i = (sy * poster.w + sx) * 3;
        let p = &poster.px[i..i + 3];
        let lin = |v: u8| f32::from(v) / 255.0;
        [lin(p[0]), lin(p[1]), lin(p[2])]
    });
}

/// The frame size for a window: pictures at full size, moving files at most 1280 wide.
pub fn frame_size(name: &str, window: (u32, u32)) -> (u32, u32) {
    let (w, h) = (window.0.max(64), window.1.max(64));
    let cap = if is_still(name) { 3840 } else { 1280 };
    let k = (cap as f32 / w as f32).min(1.0);
    let even = |v: f32| ((v as u32) & !1).max(16);
    (even(w as f32 * k), even(h as f32 * k))
}

/// One running decoder handing out raw RGB frames.
pub struct Stream {
    child: Child,
    out: ChildStdout,
    pub media: String,
    pub size: (u32, u32),
    pub fps: u32,
    still: bool,
    frames: u64,
    start: f64,
}

impl Stream {
    pub fn open(name: &str, size: (u32, u32), fps: u32, start: f64) -> Option<Self> {
        let still = is_still(name);
        let mut child = frames(&path(name)?, size.0, size.1, (!still).then_some(fps), start)
            .spawn()
            .map_err(|e| log::warn!("Starting the background decoder: {e}"))
            .ok()?;
        let out = child.stdout.take()?;
        Some(Self {
            child,
            out,
            media: name.to_owned(),
            size,
            fps,
            still,
            frames: 0,
            start,
        })
    }

    /// The next frame. A picture has one; a moving file loops forever.
    pub fn next(&mut self) -> Option<Vec<u8>> {
        if self.still && self.frames > 0 {
            return None;
        }
        let mut px = vec![0u8; (self.size.0 * self.size.1 * 3) as usize];
        self.out.read_exact(&mut px).ok()?;
        self.frames += 1;
        Some(px)
    }

    pub fn is_still(&self) -> bool {
        self.still
    }

    /// Seconds into the file, so a restarted decoder carries on from here.
    pub fn played(&self) -> f64 {
        self.start + self.frames as f64 / f64::from(self.fps.max(1))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Darkens a frame by `dim` with the same soft vignette the painted scenes get.
pub fn shade(px: &mut [u8], w: usize, h: usize, dim: f32) {
    let keep = 1.0 - dim.clamp(0.0, 0.85);
    let across: Vec<i32> = (0..w)
        .map(|x| {
            let du = (x as f32 + 0.5) / w as f32 - 0.5;
            (keep * (1.0 - 0.55 * du * du) * 256.0) as i32
        })
        .collect();
    for (y, row) in px.chunks_exact_mut(w * 3).enumerate() {
        let dv = (y as f32 + 0.5) / h as f32 - 0.5;
        let down = (keep * 0.77 * dv * dv * 256.0) as i32;
        for (p, a) in row.as_chunks_mut::<3>().0.iter_mut().zip(&across) {
            let f = (a - down).max(0);
            for c in p {
                *c = ((i32::from(*c) * f) >> 8).min(255) as u8;
            }
        }
    }
}
