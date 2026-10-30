//! What the listener hears, as one short tag: "FLAC 24/96", "Opus 160k", "PCM 16/44.1".
//! Lossless formats show bit depth and sample rate, lossy ones a nominal bitrate.

use serde_json::Value;
use tokio::time::{Duration, Instant};

/// A bitrate change for the same codec and rate waits this long, so VBR never flickers.
const SETTLE: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct Quality {
    codec: String,
    file_format: String,
    samplerate: u64,
    sample_format: String,
    demux_bitrate: f64,
    bitrate_sum: f64,
    bitrate_samples: u32,
    pub shown: String,
    shown_key: (String, u64),
    shown_at: Option<Instant>,
}

impl Quality {
    /// A new file starts: forget its bitrate, keep the codec until mpv reports the next one.
    pub fn reset(&mut self) {
        self.demux_bitrate = 0.0;
        self.bitrate_sum = 0.0;
        self.bitrate_samples = 0;
        self.shown_at = None;
    }

    /// Another mpv instance takes over: start from scratch but keep showing the tag.
    pub fn forget(&mut self) {
        *self = Self {
            shown: std::mem::take(&mut self.shown),
            ..Self::default()
        };
    }

    /// Feeds one observed mpv property. Returns a tag when the shown one should change.
    pub fn update(&mut self, name: &str, data: &Value) -> Option<String> {
        match name {
            "audio-codec-name" => self.codec = data.as_str()?.to_string(),
            "file-format" => self.file_format = data.as_str()?.to_string(),
            "audio-params" => {
                self.samplerate = data["samplerate"].as_u64()?;
                self.sample_format = data["format"].as_str().unwrap_or_default().to_string();
            }
            "current-tracks/audio" => {
                data.as_object()?;
                self.demux_bitrate = data["demux-bitrate"].as_f64().unwrap_or(0.0);
                if let Some(codec) = data["codec"].as_str() {
                    self.codec = codec.to_string();
                }
            }
            "audio-bitrate" => {
                let bitrate = data.as_f64().filter(|b| *b > 0.0)?;
                self.bitrate_sum += bitrate;
                self.bitrate_samples += 1;
            }
            _ => return None,
        }
        let kbps = if self.demux_bitrate > 0.0 {
            Some(nominal(self.demux_bitrate))
        } else if self.bitrate_samples > 0 {
            Some(nominal(self.bitrate_sum / self.bitrate_samples as f64))
        } else {
            None
        };
        let tag = tag(
            &self.codec,
            &self.file_format,
            self.samplerate,
            &self.sample_format,
            kbps,
        )?;
        if tag == self.shown {
            return None;
        }
        let key = (self.codec.clone(), self.samplerate);
        if key == self.shown_key && self.shown_at.is_some_and(|at| at.elapsed() < SETTLE) {
            return None;
        }
        self.shown_key = key;
        self.shown_at = Some(Instant::now());
        self.shown = tag.clone();
        Some(tag)
    }
}

/// The tag for a codec, or None while mpv has not said enough yet.
pub(super) fn tag(
    codec: &str,
    file_format: &str,
    samplerate: u64,
    sample_format: &str,
    kbps: Option<u32>,
) -> Option<String> {
    let (name, lossless) = match codec {
        "" => return None,
        "flac" => ("FLAC".to_string(), true),
        "alac" => ("ALAC".into(), true),
        "wavpack" => ("WavPack".into(), true),
        "ape" => ("APE".into(), true),
        "tta" => ("TTA".into(), true),
        "truehd" | "mlp" => ("TrueHD".into(), true),
        c if c.starts_with("pcm_") => (
            if file_format.contains("aiff") {
                "AIFF"
            } else {
                "PCM"
            }
            .into(),
            true,
        ),
        "opus" => ("Opus".into(), false),
        "vorbis" => ("Vorbis".into(), false),
        "mp3" | "mp3float" => ("MP3".into(), false),
        "aac" | "aac_latm" => ("AAC".into(), false),
        "mp2" => ("MP2".into(), false),
        "wmav1" | "wmav2" | "wmapro" => ("WMA".into(), false),
        other => (other.to_uppercase(), false),
    };
    if lossless {
        let bits = depth(codec, sample_format)?;
        (samplerate > 0).then(|| format!("{name} {bits}/{}", khz(samplerate)))
    } else {
        Some(format!("{name} {}k", kbps?))
    }
}

/// Bits per sample. PCM names its depth; decoders hand 24 bit audio over as s32.
fn depth(codec: &str, sample_format: &str) -> Option<u32> {
    if let Some(rest) = codec.strip_prefix("pcm_") {
        let digits: String = rest
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        return Some(digits.parse().unwrap_or(8));
    }
    match sample_format.trim_end_matches('p') {
        "u8" => Some(8),
        "s16" => Some(16),
        "s32" => Some(24),
        "float" | "flt" => Some(32),
        "s64" | "double" | "dbl" => Some(64),
        _ => None,
    }
}

fn khz(rate: u64) -> String {
    if rate.is_multiple_of(1000) {
        (rate / 1000).to_string()
    } else {
        format!("{:.1}", rate as f64 / 1000.0)
    }
}

/// The usual encoder setting closest to a measured bitrate in bits per second.
pub(super) fn nominal(bps: f64) -> u32 {
    const LADDER: [u32; 9] = [32, 48, 64, 96, 128, 160, 192, 256, 320];
    let kbps = bps / 1000.0;
    if kbps > 352.0 {
        return ((kbps / 32.0).round() * 32.0) as u32;
    }
    *LADDER
        .iter()
        .min_by(|a, b| {
            (**a as f64 - kbps)
                .abs()
                .total_cmp(&(**b as f64 - kbps).abs())
        })
        .expect("ladder")
}

/// True when the file carries ReplayGain or R128 loudness tags.
pub(super) fn has_replaygain(metadata: &Value) -> bool {
    metadata.as_object().is_some_and(|tags| {
        tags.keys().any(|key| {
            let key = key.to_ascii_lowercase();
            key.starts_with("replaygain_") || key.starts_with("r128_")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tags_read_like_a_spec_sheet() {
        assert_eq!(
            tag("flac", "flac", 96_000, "s32", None).unwrap(),
            "FLAC 24/96"
        );
        assert_eq!(
            tag("flac", "flac", 44_100, "s16", None).unwrap(),
            "FLAC 16/44.1"
        );
        assert_eq!(
            tag("pcm_s16le", "wav", 44_100, "s16", None).unwrap(),
            "PCM 16/44.1"
        );
        assert_eq!(
            tag("pcm_s24be", "aiff", 48_000, "s32", None).unwrap(),
            "AIFF 24/48"
        );
        assert_eq!(
            tag("opus", "webm", 48_000, "floatp", Some(160)).unwrap(),
            "Opus 160k"
        );
        assert_eq!(
            tag("mp3", "mp3", 44_100, "floatp", Some(320)).unwrap(),
            "MP3 320k"
        );
        assert_eq!(
            tag("aac", "hls", 44_100, "floatp", Some(256)).unwrap(),
            "AAC 256k"
        );
        assert_eq!(tag("opus", "webm", 48_000, "floatp", None), None);
        assert_eq!(nominal(147_308.0), 160);
        assert_eq!(nominal(129_000.0), 128);
        assert_eq!(nominal(319_985.0), 320);
        assert_eq!(nominal(705_600.0), 704);
    }

    #[test]
    fn vbr_bitrate_does_not_flicker() {
        let mut q = Quality::default();
        q.update("audio-codec-name", &json!("opus"));
        q.update(
            "audio-params",
            &json!({"samplerate": 48000, "format": "floatp"}),
        );
        assert_eq!(
            q.update("audio-bitrate", &json!(150_000)).unwrap(),
            "Opus 160k"
        );
        // A dip right after is held back, the running mean barely moves anyway.
        assert_eq!(q.update("audio-bitrate", &json!(60_000)), None);
        assert_eq!(q.shown, "Opus 160k");
        q.reset();
        q.update("audio-codec-name", &json!("mp3"));
        q.update(
            "audio-params",
            &json!({"samplerate": 44100, "format": "floatp"}),
        );
        assert_eq!(
            q.update(
                "current-tracks/audio",
                &json!({"codec": "mp3", "demux-bitrate": 320000})
            )
            .unwrap(),
            "MP3 320k"
        );
        assert!(has_replaygain(&json!({"REPLAYGAIN_TRACK_GAIN": "-6.2 dB"})));
        assert!(has_replaygain(&json!({"r128_track_gain": "-512"})));
        assert!(!has_replaygain(&json!({"title": "x"})));
    }
}
