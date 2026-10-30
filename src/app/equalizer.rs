//! The equalizer panel: presets, dragging bands, the hold-to-compare button, and the
//! curve the panel draws. The curve eases towards new settings so changes feel calm.

use super::{controller::Controller, models};
use crate::{
    AppWindow,
    player::Equalizer,
    visualizer::{area, ribbon},
};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
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
const PAD: f32 = 22.0;

#[derive(Default)]
struct Curve {
    shown: [f64; 6],
    comparing: bool,
    animation: Timer,
    save: Timer,
}

thread_local! {
    static CURVE: RefCell<Curve> = RefCell::new(Curve::default());
}

/// Room for boosts so the sound doesn't clip: half the largest boost, taken off the top.
pub(super) fn headroom(gains: &[f64; 6]) -> f64 {
    -(gains.iter().cloned().fold(0.0, f64::max) / 2.0)
}

impl Controller {
    pub(super) fn eq_init(&self, window: &AppWindow) {
        let (gains, preset) = {
            let mut s = self.state.lock();
            s.settings.eq_preamp = headroom(&s.settings.eq_gains);
            (s.settings.eq_gains, s.settings.eq_preset.clone())
        };
        CURVE.with(|c| c.borrow_mut().shown = gains);
        window.set_eq_gains(ModelRc::new(VecModel::from(
            gains.iter().map(|g| *g as f32).collect::<Vec<_>>(),
        )));
        window.set_eq_preset(preset.into());
    }

    pub(crate) fn eq_choose(&self, name: &str) {
        let Some((_, gains)) = PRESETS.iter().find(|(n, _)| *n == name) else {
            return;
        };
        {
            let mut s = self.state.lock();
            s.settings.eq_gains = *gains;
            s.settings.eq_preamp = headroom(gains);
            s.settings.eq_preset = name.into();
        }
        self.eq_changed(true);
    }

    pub(super) fn eq_drag(&self, band: usize, gain: f64) {
        {
            let mut s = self.state.lock();
            let Some(slot) = s.settings.eq_gains.get_mut(band) else {
                return;
            };
            *slot = gain.clamp(-MAX, MAX);
            s.settings.eq_preamp = headroom(&s.settings.eq_gains);
            s.settings.eq_preset = "Custom".into();
        }
        self.eq_changed(false);
    }

    pub(super) fn eq_reset(&self, band: usize) {
        self.eq_drag(band, 0.0);
        let flat = self.state.lock().settings.eq_gains == [0.0; 6];
        if flat {
            self.state.lock().settings.eq_preset = "Flat".into();
            self.eq_changed(true);
        }
    }

    pub(super) fn eq_compare(&self, on: bool) {
        CURVE.with(|c| c.borrow_mut().comparing = on);
        self.eq_changed(true);
    }

    /// Sends the settings to mpv, eases the curve towards them and saves a moment later.
    fn eq_changed(&self, ease: bool) {
        let (gains, preamp, preset) = {
            let s = self.state.lock();
            (
                s.settings.eq_gains,
                s.settings.eq_preamp,
                s.settings.eq_preset.clone(),
            )
        };
        let comparing = CURVE.with(|c| c.borrow().comparing);
        let heard = if comparing {
            Equalizer::default()
        } else {
            Equalizer { gains, preamp }
        };
        self.player.set_equalizer(heard);
        let Some(window) = self.window.upgrade() else {
            return;
        };
        window.set_eq_preset(preset.into());
        window.set_eq_comparing(comparing);
        let goal = heard.gains;
        let weak = window.as_weak();
        CURVE.with(|c| {
            let mut c = c.borrow_mut();
            if ease {
                c.animation
                    .start(TimerMode::Repeated, Duration::from_millis(16), move || {
                        if let Some(w) = weak.upgrade() {
                            step(&w, goal);
                        }
                    });
            } else {
                c.shown = goal;
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
            let rows = models::settings(&settings);
            self.ui(move |w| models::update(w.get_settings(), rows));
        }
    }
}

fn step(window: &AppWindow, goal: [f64; 6]) {
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
        if done {
            c.animation.stop();
        }
    });
    render(window);
}

/// Draws the curve for the gains currently shown, at the graph's size.
pub(super) fn render(window: &AppWindow) {
    let (w, h) = (window.get_eq_width(), window.get_eq_height());
    let shown = CURVE.with(|c| c.borrow().shown);
    window.set_eq_gains(
        Rc::new(VecModel::from(
            shown.iter().map(|g| *g as f32).collect::<Vec<_>>(),
        ))
        .into(),
    );
    if w <= 1.0 || h <= 1.0 {
        return;
    }
    let step = (w - 2.0 * PAD) / 5.0;
    let y = |g: f64| h / 2.0 - (g / MAX) as f32 * (h / 2.0 - 16.0);
    let mut points = Vec::with_capacity(8);
    points.push((-30.0, y(shown[0])));
    points.extend(
        shown
            .iter()
            .enumerate()
            .map(|(i, g)| (PAD + i as f32 * step, y(*g))),
    );
    points.push((w + 30.0, y(shown[5])));
    window.set_eq_fill(area(&points, h + 1.0).into());
    window.set_eq_haze(ribbon(&points, 4.0, 4.0).into());
    window.set_eq_glow(ribbon(&points, 2.0, 2.0).into());
    window.set_eq_line(ribbon(&points, 0.8, 0.8).into());
}
