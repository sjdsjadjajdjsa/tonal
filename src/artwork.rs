use anyhow::Context;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

pub type Pixels = SharedPixelBuffer<Rgba8Pixel>;

const MEMORY_ENTRIES: usize = 128;
const DISK_CAP_BYTES: u64 = 200 * 1024 * 1024;

fn memory() -> &'static Mutex<Vec<(String, Pixels)>> {
    static CACHE: OnceLock<Mutex<Vec<(String, Pixels)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::with_capacity(MEMORY_ENTRIES)))
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("Tonal/0.1")
            .build()
            .expect("http client")
    })
}

fn disk_dir() -> PathBuf {
    crate::model::paths::cache_dir().join("art")
}

fn disk_path(url: &str) -> PathBuf {
    let hash = Sha256::digest(url.as_bytes());
    let name: String = hash.iter().take(16).map(|b| format!("{b:02x}")).collect();
    disk_dir().join(name)
}

fn is_remote(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn prune_dir(dir: &Path, cap: u64) -> std::io::Result<u64> {
    let mut files = Vec::new();
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "tmp") {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        total += meta.len();
        files.push((modified, meta.len(), path));
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut removed = 0;
    for (_, len, path) in files {
        if total <= cap {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
            removed += len;
        }
    }
    Ok(removed)
}

fn prune_once() {
    static PRUNED: std::sync::Once = std::sync::Once::new();
    PRUNED.call_once(|| {
        std::thread::spawn(|| {
            if let Err(e) = prune_dir(&disk_dir(), DISK_CAP_BYTES)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                log::warn!("Could not prune artwork cache: {e}");
            }
        });
    });
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let tmp = path.with_extension(format!("{:016x}.tmp", rand::random::<u64>()));
    if let Err(e) = tokio::fs::write(&tmp, bytes).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    if let Err(e) = tokio::fs::rename(&tmp, path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    Ok(())
}

fn evict(url: &str) {
    if is_remote(url) {
        let _ = std::fs::remove_file(disk_path(url));
    }
}

async fn decode_evicting(
    url: &str,
    bytes: Vec<u8>,
    size: u32,
    radius: f32,
) -> anyhow::Result<Pixels> {
    let result = tokio::task::spawn_blocking(move || decode(&bytes, size, radius)).await?;
    if result.is_err() {
        evict(url);
    }
    result
}

fn remember(key: String, pixels: Pixels) {
    let mut cache = memory().lock();
    cache.retain(|(k, _)| *k != key);
    if cache.len() >= MEMORY_ENTRIES {
        cache.remove(0);
    }
    cache.push((key, pixels));
}

fn recall(key: &str) -> Option<Pixels> {
    let mut cache = memory().lock();
    let index = cache.iter().position(|(k, _)| k == key)?;
    let entry = cache.remove(index);
    let pixels = entry.1.clone();
    cache.push(entry);
    Some(pixels)
}

async fn fetch_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    if is_remote(url) {
        prune_once();
        let path = disk_path(url);
        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Ok(bytes);
        }
        let bytes = client()
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec();
        if let Err(e) = write_atomic(&path, &bytes).await {
            log::debug!("Could not cache artwork {url}: {e}");
        }
        Ok(bytes)
    } else {
        let path = url.strip_prefix("file://").unwrap_or(url);
        tokio::fs::read(path)
            .await
            .with_context(|| format!("reading {path}"))
    }
}

fn decode(bytes: &[u8], size: u32, radius: f32) -> anyhow::Result<Pixels> {
    let image = image::load_from_memory(bytes)?;
    let mut image = image
        .resize_to_fill(size, size, image::imageops::FilterType::Triangle)
        .to_rgba8();
    round_corners(&mut image, size as f32 * radius);
    Ok(SharedPixelBuffer::clone_from_slice(
        image.as_raw(),
        image.width(),
        image.height(),
    ))
}

fn round_corners(image: &mut image::RgbaImage, radius: f32) {
    let (w, h) = (image.width() as f32, image.height() as f32);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;
        let cx = px.clamp(radius, w - radius);
        let cy = py.clamp(radius, h - radius);
        let distance = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
        let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
        if coverage < 1.0 {
            pixel[3] = (pixel[3] as f32 * coverage) as u8;
        }
    }
}

pub async fn load(url: &str, size: u32) -> anyhow::Result<Pixels> {
    load_shaped(url, size, 0.15).await
}

/// Large artwork for one view: decoded fresh and not kept in the memory cache.
pub async fn load_large(url: &str, size: u32, radius: f32) -> anyhow::Result<Pixels> {
    let bytes = fetch_bytes(url).await?;
    decode_evicting(url, bytes, size, radius).await
}

/// A soft color taken from the artwork, calm enough to tint the waves on the gray theme.
/// None for gray or near-black art.
pub async fn color(url: &str) -> Option<[u8; 3]> {
    type Colors = Vec<(String, Option<[u8; 3]>)>;
    static COLORS: OnceLock<Mutex<Colors>> = OnceLock::new();
    let colors = COLORS.get_or_init(Default::default);
    if let Some((_, color)) = colors.lock().iter().find(|(k, _)| k == url) {
        return *color;
    }
    let bytes = fetch_bytes(url).await.ok()?;
    let decoded = tokio::task::spawn_blocking(move || {
        let image = image::load_from_memory(&bytes).ok()?;
        let small = image
            .resize_exact(16, 16, image::imageops::FilterType::Triangle)
            .to_rgb8();
        Some(dominant(small.pixels().map(|p| p.0)))
    })
    .await
    .ok()
    .flatten();
    if decoded.is_none() {
        evict(url);
    }
    let color = decoded.flatten();
    let mut colors = colors.lock();
    if colors.len() >= MEMORY_ENTRIES {
        colors.remove(0);
    }
    colors.push((url.to_string(), color));
    color
}

fn hsl([r, g, b]: [u8; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-4 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-4);
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    (h, s.min(1.0), l)
}

fn rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// The most saturated mid-luminance hue cluster, averaged, then softened and lightened.
fn dominant(pixels: impl Iterator<Item = [u8; 3]>) -> Option<[u8; 3]> {
    const BINS: usize = 12;
    let mut weight = [0.0f32; BINS];
    let mut sum = [[0.0f32; 3]; BINS];
    let mut total = 0.0f32;
    for p in pixels {
        total += 1.0;
        let (h, s, l) = hsl(p);
        if !(0.18..=0.85).contains(&l) || s < 0.18 {
            continue;
        }
        let w = s * s * (1.0 - (l - 0.5).abs() * 1.4);
        let bin = (h / 360.0 * BINS as f32) as usize % BINS;
        weight[bin] += w;
        for (acc, v) in sum[bin].iter_mut().zip(p) {
            *acc += v as f32 * w;
        }
    }
    let (bin, &best) = weight
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    if total == 0.0 || best / total < 0.02 {
        return None;
    }
    let mean = sum[bin].map(|v| (v / best).round().clamp(0.0, 255.0) as u8);
    let (h, s, _) = hsl(mean);
    Some(rgb(h, (s * 0.6).clamp(0.25, 0.5), 0.74))
}

/// Square artwork with corners rounded to `radius` times the size; 0.5 gives a circle.
pub async fn load_shaped(url: &str, size: u32, radius: f32) -> anyhow::Result<Pixels> {
    let key = format!("{size}:{radius}:{url}");
    if let Some(pixels) = recall(&key) {
        return Ok(pixels);
    }
    let bytes = fetch_bytes(url).await?;
    let pixels = decode_evicting(url, bytes, size, radius).await?;
    remember(key, pixels.clone());
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominant_color_is_the_saturated_hue_softened() {
        let mut pixels = vec![[20, 20, 22]; 200];
        pixels.extend(vec![[200, 40, 40]; 40]);
        pixels.extend(vec![[60, 60, 200]; 16]);
        let [r, g, b] = dominant(pixels.into_iter()).unwrap();
        assert!(r > g && r > b, "red wins: {r} {g} {b}");
        let (_, s, l) = hsl([r, g, b]);
        assert!(s <= 0.51 && (0.72..=0.76).contains(&l), "calm: s {s} l {l}");
    }

    #[test]
    fn gray_art_has_no_color() {
        let pixels = (0..256).map(|i| [i as u8, i as u8, i as u8]);
        assert_eq!(dominant(pixels), None);
        assert_eq!(dominant(std::iter::empty()), None);
    }

    #[test]
    fn prune_removes_oldest_until_under_cap_and_temp_files() {
        let dir = std::env::temp_dir().join(format!("tonal-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        for (i, name) in ["old", "mid", "new"].iter().enumerate() {
            let path = dir.join(name);
            std::fs::write(&path, vec![0u8; 100]).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(base + Duration::from_secs(i as u64 * 10))
                .unwrap();
        }
        std::fs::write(dir.join("x.abc.tmp"), b"partial").unwrap();
        assert_eq!(prune_dir(&dir, 250).unwrap(), 100);
        assert!(!dir.join("old").exists());
        assert!(dir.join("mid").exists() && dir.join("new").exists());
        assert!(!dir.join("x.abc.tmp").exists());
        assert_eq!(prune_dir(&dir, 1000).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn atomic_write_replaces_without_leftovers() {
        let dir = std::env::temp_dir().join(format!("tonal-art-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("entry");
        write_atomic(&path, b"one").await.unwrap();
        write_atomic(&path, b"two").await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hsl_roundtrip() {
        for c in [[255, 0, 0], [12, 200, 90], [90, 30, 240], [128, 128, 128]] {
            let (h, s, l) = hsl(c);
            let back = rgb(h, s, l);
            for (a, b) in c.iter().zip(back) {
                assert!((*a as i32 - b as i32).abs() <= 1, "{c:?} -> {back:?}");
            }
        }
    }
}
