use anyhow::Context;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

pub type Pixels = SharedPixelBuffer<Rgba8Pixel>;

const MEMORY_ENTRIES: usize = 128;

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

fn disk_path(url: &str) -> PathBuf {
    let hash = Sha256::digest(url.as_bytes());
    let name: String = hash.iter().take(16).map(|b| format!("{b:02x}")).collect();
    crate::model::paths::cache_dir().join("art").join(name)
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
    if url.starts_with("http://") || url.starts_with("https://") {
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
        if let Some(dir) = path.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        let _ = tokio::fs::write(&path, &bytes).await;
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

/// Square artwork with corners rounded to `radius` times the size; 0.5 gives a circle.
pub async fn load_shaped(url: &str, size: u32, radius: f32) -> anyhow::Result<Pixels> {
    let key = format!("{size}:{radius}:{url}");
    if let Some(pixels) = recall(&key) {
        return Ok(pixels);
    }
    let bytes = fetch_bytes(url).await?;
    let pixels = tokio::task::spawn_blocking(move || decode(&bytes, size, radius)).await??;
    remember(key, pixels.clone());
    Ok(pixels)
}
