//! The equalizer panel: presets, dragging bands, the hold-to-compare button, the
//! headphone correction underneath, and the curve the panel draws. The curve eases
//! towards new settings so changes feel calm.

use super::{controller::Controller, models};
use crate::{
    AppWindow,
    integrations::autoeq::{self, Entry},
    model::{EqFilter, FilterKind, Settings},
    player::{EQ_BANDS, Equalizer},
    visualizer::{area, ribbon},
};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::{cell::RefCell, rc::Rc, time::Duration};

pub const PRESETS: [(&str, [f64; 6]); 6] = [
    ("Flat", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Bass", [6.0, 4.0, 1.0, 0.0, 0.0, 0.0]),
    ("Vocal", [-2.0, -1.0, 2.0, 4.0, 3.0, 0.0]),
    ("Bright", [0.0, 0.0, 0.0, 1.0, 4.0, 6.0]),
    ("Warm", [3.0, 2.0, 1.0, 0.0, -1.0, -2.0]),
    ("Late night", [2.0, 1.0, 0.0, -1.0, -2.0, -4.0]),
];
const MAX: f64 = 12.0;
/// Space between the graph's edges and its 20 Hz and 20 kHz ends; the panel uses it too.
const INSET: f32 = 10.0;
/// Where each band's center may go, in Hz.
const LIMITS: [(f64, f64); 6] = [
    (20.0, 120.0),
    (80.0, 300.0),
    (200.0, 800.0),
    (500.0, 2000.0),
    (1500.0, 5000.0),
    (5000.0, 16000.0),
];
/// Neighbouring bands keep at least this ratio apart.
const GAP: f64 = 1.6;
/// Results in the headphone search.
const RESULTS: usize = 6;

#[derive(Default)]
struct Curve {
    shown: [f64; 6],
    /// Where each dot sits along the graph, 0 at 20 Hz and 1 at 20 kHz.
    spots: [f64; 6],
    correction: Vec<EqFilter>,
    comparing: bool,
    animation: Timer,
    save: Timer,
}

#[derive(Default)]
struct Headphones {
    index: Option<Rc<Vec<Entry>>>,
    loading: bool,
    results: Vec<Entry>,
}

thread_local! {
    static CURVE: RefCell<Curve> = RefCell::new(Curve::default());
    static HEADPHONES: RefCell<Headphones> = RefCell::new(Headphones::default());
}

/// Room for boosts so the sound doesn't clip: half the largest boost, taken off the top.
pub(super) fn headroom(gains: &[f64; 6]) -> f64 {
    -(gains.iter().cloned().fold(0.0, f64::max) / 2.0)
}

/// What mpv should play with. Comparing silences the bands but keeps the headphone
/// correction: it is part of the headphones, not of the taste.
pub(super) fn heard(settings: &Settings, comparing: bool) -> Equalizer {
    let gains = if comparing {
        [0.0; 6]
    } else {
        settings.eq_gains
    };
    let headphone = settings.headphone.as_ref();
    Equalizer {
        gains,
        freqs: settings.eq_freqs,
        preamp: headroom(&gains) + headphone.map_or(0.0, |h| h.preamp),
        correction: headphone.map(|h| h.filters.clone()).unwrap_or_default(),
    }
}

/// Position along the graph's log axis: 0 at 20 Hz, 1 at 20 kHz.
fn spot_of(freq: f64) -> f64 {
    (freq / 20.0).ln() / 1000f64.ln()
}

fn freq_of(spot: f64) -> f64 {
    20.0 * 1000f64.powf(spot)
}

/// Keeps band `band` inside its range and between its neighbours, rounded the way
/// its label shows it.
pub(super) fn clamp_freq(freqs: &[f64; 6], band: usize, freq: f64) -> f64 {
    let (mut lo, mut hi) = LIMITS[band];
    if band > 0 {
        lo = lo.max(freqs[band - 1] * GAP);
    }
    if band < 5 {
        hi = hi.min(freqs[band + 1] / GAP);
    }
    let f = if lo <= hi {
        freq.clamp(lo, hi)
    } else {
        freqs[band]
    };
    let rounded = match f {
        f if f < 1000.0 => f.round(),
        f if f < 10_000.0 => (f / 100.0).round() * 100.0,
        f => (f / 1000.0).round() * 1000.0,
    };
    // Rounding may step past a limit; then the limit itself wins.
    if rounded < lo || rounded > hi {
        f
    } else {
        rounded
    }
}

/// "60", "400", "1.2k", "12k".
pub(super) fn format_freq(freq: f64) -> String {
    if freq < 1000.0 {
        format!("{freq:.0}")
    } else {
        let k = format!("{:.1}", freq / 1000.0);
        format!("{}k", k.trim_end_matches(".0"))
    }
}

/// Level change in dB of one filter at `freq`, from the RBJ biquad mpv's filters use.
fn response(filter: &EqFilter, freq: f64) -> f64 {
    const RATE: f64 = 48_000.0;
    let a = 10f64.powf(filter.gain / 40.0);
    let w0 = std::f64::consts::TAU * filter.freq.min(RATE * 0.49) / RATE;
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * filter.q);
    let s = 2.0 * a.sqrt() * alpha;
    let (b, d) = match filter.kind {
        FilterKind::Peak => (
            [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a],
            [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
        ),
        FilterKind::LowShelf => (
            [
                a * ((a + 1.0) - (a - 1.0) * cos + s),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - s),
            ],
            [
                (a + 1.0) + (a - 1.0) * cos + s,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - s,
            ],
        ),
        FilterKind::HighShelf => (
            [
                a * ((a + 1.0) + (a - 1.0) * cos + s),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - s),
            ],
            [
                (a + 1.0) - (a - 1.0) * cos + s,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - s,
            ],
        ),
    };
    let w = std::f64::consts::TAU * freq.min(RATE * 0.49) / RATE;
    let power = |c: [f64; 3]| {
        let re = c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos();
        let im = c[1] * w.sin() + c[2] * (2.0 * w).sin();
        re * re + im * im
    };
    10.0 * (power(b) / power(d)).log10()
}

fn floats(values: impl IntoIterator<Item = f64>) -> ModelRc<f32> {
    ModelRc::new(VecModel::from(
        values.into_iter().map(|v| v as f32).collect::<Vec<_>>(),
    ))
}

impl Controller {
    pub(super) fn eq_init(&self, window: &AppWindow) {
        let (gains, freqs, preset, bit_perfect, headphone) = {
            let mut s = self.state.lock();
            s.settings.eq_preamp = headroom(&s.settings.eq_gains);
            (
                s.settings.eq_gains,
                s.settings.eq_freqs,
                s.settings.eq_preset.clone(),
                s.settings.bit_perfect,
                s.settings.headphone.clone(),
            )
        };
        let gains = if bit_perfect { [0.0; 6] } else { gains };
        window.set_bit_perfect(bit_perfect);
        CURVE.with(|c| {
            let mut c = c.borrow_mut();
            c.shown = gains;
            c.spots = freqs.map(spot_of);
            c.correction = headphone
                .as_ref()
                .map(|h| h.filters.clone())
                .unwrap_or_default();
        });
        window.set_eq_gains(floats(gains));
        window.set_eq_spots(floats(freqs.map(spot_of)));
        window.set_eq_labels(labels(&freqs));
        window.set_eq_headphone(headphone.map(|h| h.name).unwrap_or_default().into());
        window.set_eq_preset(preset.into());
    }

    /// A preset is a curve at the standard bands, so it puts moved bands back.
    pub(crate) fn eq_choose(&self, name: &str) {
        let Some((_, gains)) = PRESETS.iter().find(|(n, _)| *n == name) else {
            return;
        };
        {
            let mut s = self.state.lock();
            s.settings.eq_gains = *gains;
            s.settings.eq_freqs = EQ_BANDS;
            s.settings.eq_preamp = headroom(gains);
            s.settings.eq_preset = name.into();
        }
        self.eq_changed(true);
    }

    /// A dot moved: up and down is its gain, sideways its frequency (`spot` on the axis).
    pub(super) fn eq_drag(&self, band: usize, gain: f64, spot: f64) {
        if band >= 6 {
            return;
        }
        {
            let mut s = self.state.lock();
            let gain = gain.clamp(-MAX, MAX);
            let freq = clamp_freq(&s.settings.eq_freqs, band, freq_of(spot));
            if gain == s.settings.eq_gains[band] && freq == s.settings.eq_freqs[band] {
                return;
            }
            s.settings.eq_gains[band] = gain;
            s.settings.eq_freqs[band] = freq;
            s.settings.eq_preamp = headroom(&s.settings.eq_gains);
            s.settings.eq_preset = "Custom".into();
        }
        self.eq_changed(false);
    }

    /// Double-click: the band goes back to 0 dB at its own frequency.
    pub(super) fn eq_reset(&self, band: usize) {
        if band >= 6 {
            return;
        }
        {
            let mut s = self.state.lock();
            s.settings.eq_gains[band] = 0.0;
            s.settings.eq_freqs[band] = EQ_BANDS[band];
            s.settings.eq_preamp = headroom(&s.settings.eq_gains);
            let flat = s.settings.eq_gains == [0.0; 6] && s.settings.eq_freqs == EQ_BANDS;
            s.settings.eq_preset = if flat { "Flat" } else { "Custom" }.into();
        }
        self.eq_changed(true);
    }

    pub(super) fn eq_compare(&self, on: bool) {
        CURVE.with(|c| c.borrow_mut().comparing = on);
        self.eq_changed(true);
    }

    /// Bit-perfect went on or off: the curve eases to flat or back.
    pub(super) fn eq_refresh(&self) {
        self.eq_changed(true);
    }

    /// The headphone search opened: load the list of models once.
    pub(super) fn eq_headphones_open(&self) {
        let start = HEADPHONES.with(|h| {
            let mut h = h.borrow_mut();
            let start = h.index.is_none() && !h.loading;
            h.loading |= start;
            start
        });
        if !start {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            let index = autoeq::index().await;
            let ui = this.clone();
            this.ui(move |w| {
                HEADPHONES.with(|h| {
                    let mut h = h.borrow_mut();
                    h.loading = false;
                    if let Some(index) = ui.result(index) {
                        h.index = Some(Rc::new(index));
                    }
                });
                ui.eq_headphones_search(&w.get_eq_headphone_query());
            });
        });
    }

    pub(super) fn eq_headphones_search(&self, query: &str) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let names: Vec<SharedString> = HEADPHONES.with(|h| {
            let mut h = h.borrow_mut();
            let found = h
                .index
                .as_ref()
                .map(|i| {
                    autoeq::search(i, query, RESULTS)
                        .into_iter()
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            h.results = found;
            h.results.iter().map(|e| e.name.as_str().into()).collect()
        });
        window.set_eq_headphone_results(ModelRc::new(VecModel::from(names)));
    }

    /// Loads the chosen model's correction and puts it under the equalizer.
    pub(super) fn eq_headphones_pick(&self, index: usize) {
        let Some(entry) = HEADPHONES.with(|h| h.borrow().results.get(index).cloned()) else {
            return;
        };
        let this = self.clone();
        tokio::spawn(async move {
            if let Some(headphone) = this.result(autoeq::correction(&entry).await) {
                this.state.lock().settings.headphone = Some(headphone);
                let ui = this.clone();
                this.ui(move |_| ui.eq_changed(true));
            }
        });
    }

    pub(super) fn eq_headphones_clear(&self) {
        self.state.lock().settings.headphone = None;
        self.eq_changed(true);
    }

    /// Debug hook: pick the best match for `query` once the list is in.
    #[cfg(feature = "debug-hooks")]
    pub(super) fn eq_headphones_debug(&self, query: String) {
        let this = self.clone();
        tokio::spawn(async move {
            let Some(index) = this.result(autoeq::index().await) else {
                return;
            };
            let Some(entry) = autoeq::search(&index, &query, 1).first().cloned().cloned() else {
                this.status(format!("No headphone matches {query}"));
                return;
            };
            if let Some(headphone) = this.result(autoeq::correction(&entry).await) {
                this.state.lock().settings.headphone = Some(headphone);
                let ui = this.clone();
                this.ui(move |_| ui.eq_changed(true));
            }
        });
    }

    /// Sends the settings to mpv, eases the curve towards them and saves a moment later.
    /// Bit-perfect keeps the settings (mpv skips the filter) and draws the curve flat.
    fn eq_changed(&self, ease: bool) {
        let comparing = CURVE.with(|c| c.borrow().comparing);
        let (heard, preset, bit_perfect, headphone) = {
            let s = self.state.lock();
            (
                heard(&s.settings, comparing),
                s.settings.eq_preset.clone(),
                s.settings.bit_perfect,
                s.settings.headphone.as_ref().map(|h| h.name.clone()),
            )
        };
        let goal = if bit_perfect { [0.0; 6] } else { heard.gains };
        let spots = heard.freqs.map(spot_of);
        let correction = if bit_perfect {
            Vec::new()
        } else {
            heard.correction.clone()
        };
        let freqs = heard.freqs;
        self.player.set_equalizer(heard);
        let Some(window) = self.window.upgrade() else {
            return;
        };
        window.set_eq_preset(preset.into());
        window.set_eq_comparing(comparing);
        window.set_bit_perfect(bit_perfect);
        window.set_eq_labels(labels(&freqs));
        window.set_eq_headphone(headphone.unwrap_or_default().into());
        let weak = window.as_weak();
        CURVE.with(|c| {
            let mut c = c.borrow_mut();
            c.correction = correction;
            if ease {
                c.animation
                    .start(TimerMode::Repeated, Duration::from_millis(16), move || {
                        if let Some(w) = weak.upgrade() {
                            step(&w, goal, spots);
                        }
                    });
            } else {
                c.shown = goal;
                c.spots = spots;
                c.animation.stop();
            }
        });
        render(&window);
        let this = self.clone();
        CURVE.with(|c| {
            c.borrow().save.start(
                TimerMode::SingleShot,
                Duration::from_millis(600),
                move || this.eq_save(),
            );
        });
    }

    fn eq_save(&self) {
        let settings = self.state.lock().settings.clone();
        if self.result(self.library.save_settings(&settings)).is_some() {
            self.ui(move |w| models::update(w.get_settings(), models::settings(&settings)));
        }
    }
}

fn labels(freqs: &[f64; 6]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        freqs
            .iter()
            .map(|f| SharedString::from(format_freq(*f)))
            .collect::<Vec<_>>(),
    ))
}

fn step(window: &AppWindow, goal: [f64; 6], spots: [f64; 6]) {
    CURVE.with(|c| {
        let mut c = c.borrow_mut();
        let mut done = true;
        for (shown, goal) in c.shown.iter_mut().zip(goal) {
            *shown += (goal - *shown) * 0.2;
            if (goal - *shown).abs() > 0.02 {
                done = false;
            } else {
                *shown = goal;
            }
        }
        for (shown, goal) in c.spots.iter_mut().zip(spots) {
            *shown += (goal - *shown) * 0.2;
            if (goal - *shown).abs() > 0.0005 {
                done = false;
            } else {
                *shown = goal;
            }
        }
        if done {
            c.animation.stop();
        }
    });
    render(window);
}

/// Draws the curve for the gains currently shown, at the graph's size, and the
/// headphone correction as a faint line underneath.
pub(super) fn render(window: &AppWindow) {
    let (w, h) = (window.get_eq_width(), window.get_eq_height());
    let (shown, spots, correction) = CURVE.with(|c| {
        let c = c.borrow();
        (c.shown, c.spots, c.correction.clone())
    });
    window.set_eq_gains(floats(shown));
    window.set_eq_spots(floats(spots));
    if w <= 1.0 || h <= 1.0 {
        return;
    }
    let span = w - 2.0 * INSET;
    let y = |g: f64| h / 2.0 - (g.clamp(-MAX, MAX) / MAX) as f32 * (h / 2.0 - 16.0);
    let mut points = Vec::with_capacity(8);
    points.push((-30.0, y(shown[0])));
    points.extend(
        shown
            .iter()
            .zip(spots)
            .map(|(g, s)| (INSET + s as f32 * span, y(*g))),
    );
    points.push((w + 30.0, y(shown[5])));
    window.set_eq_fill(area(&points, h + 1.0).into());
    window.set_eq_haze(ribbon(&points, 4.0, 4.0).into());
    window.set_eq_glow(ribbon(&points, 2.0, 2.0).into());
    window.set_eq_line(ribbon(&points, 0.8, 0.8).into());
    let faint = if correction.is_empty() {
        String::new()
    } else {
        let samples = 72;
        let points: Vec<(f32, f32)> = (0..=samples)
            .map(|i| {
                let x = w * i as f32 / samples as f32;
                let freq = freq_of(((x - INSET) / span) as f64);
                (x, y(correction.iter().map(|f| response(f, freq)).sum()))
            })
            .collect();
        ribbon(&points, 0.6, 0.6)
    };
    window.set_eq_correction(faint.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_stay_in_range_and_apart() {
        let bands = EQ_BANDS;
        assert_eq!(clamp_freq(&bands, 0, 5.0), 20.0);
        assert_eq!(clamp_freq(&bands, 0, 500.0), 93.75); // 150 / 1.6, under the 120 limit
        assert_eq!(clamp_freq(&bands, 5, 30_000.0), 16_000.0);
        assert_eq!(clamp_freq(&bands, 5, 1_000.0), 5_000.0);
        assert_eq!(clamp_freq(&bands, 3, 1_234.0), 1_200.0);
        assert_eq!(clamp_freq(&bands, 2, 333.3), 333.0);
        for (i, f) in bands.iter().enumerate() {
            assert_eq!(clamp_freq(&bands, i, *f), *f, "band {i} at its default");
        }
    }

    #[test]
    fn frequencies_read_like_the_labels() {
        assert_eq!(format_freq(60.0), "60");
        assert_eq!(format_freq(1000.0), "1k");
        assert_eq!(format_freq(1200.0), "1.2k");
        assert_eq!(format_freq(16_000.0), "16k");
        assert!((freq_of(spot_of(440.0)) - 440.0).abs() < 1e-9);
    }

    #[test]
    fn biquad_response_peaks_at_its_gain() {
        let peak = EqFilter {
            kind: FilterKind::Peak,
            freq: 1000.0,
            gain: 6.0,
            q: 1.0,
        };
        assert!((response(&peak, 1000.0) - 6.0).abs() < 0.01);
        assert!(response(&peak, 50.0).abs() < 0.1);
        let shelf = EqFilter {
            kind: FilterKind::LowShelf,
            freq: 105.0,
            gain: 6.5,
            q: 0.7,
        };
        assert!((response(&shelf, 20.0) - 6.5).abs() < 0.3);
        assert!(response(&shelf, 5000.0).abs() < 0.1);
    }
}
