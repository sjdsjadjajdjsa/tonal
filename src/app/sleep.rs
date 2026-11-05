//! The sleep timer: music fades out and pauses after a while or at the end of the
//! song, then the volume goes back to where it was for next time.

use super::controller::Controller;
use slint::{Timer, TimerMode};
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

/// Seconds the music fades over before a timed stop.
const FADE: f64 = 30.0;
/// Seconds the last song fades over when stopping at its end.
const END_FADE: f64 = 8.0;

enum Until {
    Time(Instant),
    /// The end of the song with this id.
    EndOf(String),
}

struct Sleep {
    until: Until,
    /// The volume before the fade, once it has started.
    from: Option<f64>,
    /// Checks four times a second while the timer is set; dropping it stops that.
    _tick: Timer,
}

thread_local! {
    static SLEEP: RefCell<Option<Sleep>> = const { RefCell::new(None) };
}

impl Controller {
    /// "15", "30", "45", "60" or "90" minutes, "end" for the end of this song, "off".
    pub(super) fn sleep(&self, choice: &str) {
        self.sleep_off();
        let until = match choice {
            "end" => match self.state.lock().current.as_ref() {
                Some(t) => Until::EndOf(t.id.clone()),
                None => return,
            },
            minutes => match minutes.parse::<u64>() {
                Ok(m) if m > 0 => Until::Time(Instant::now() + Duration::from_secs(m * 60)),
                _ => return,
            },
        };
        let tick = Timer::default();
        let this = self.clone();
        tick.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            this.sleep_tick()
        });
        SLEEP.with(|s| {
            *s.borrow_mut() = Some(Sleep {
                until,
                from: None,
                _tick: tick,
            })
        });
        self.sleep_tick();
    }

    /// Cancels the timer and puts back the volume the fade took away.
    fn sleep_off(&self) {
        if let Some(sleep) = SLEEP.with(|s| s.borrow_mut().take())
            && let Some(volume) = sleep.from
        {
            self.player.set_volume(volume);
        }
        self.ui(|w| w.set_sleep_left("".into()));
    }

    fn sleep_tick(&self) {
        let (current, position, duration, paused, volume) = {
            let s = self.state.lock();
            (
                s.current.as_ref().map(|t| t.id.clone()),
                s.position,
                s.duration,
                s.paused,
                s.settings.volume,
            )
        };
        enum Step {
            Wait(String),
            Fade(f64, String),
            Stop,
        }
        let step = SLEEP.with(|s| {
            let mut s = s.borrow_mut();
            let sleep = s.as_mut()?;
            let step = match &mut sleep.until {
                Until::Time(at) => {
                    let left = at.saturating_duration_since(Instant::now()).as_secs_f64();
                    let label = format!("in {} min", (left / 60.0).ceil().max(1.0));
                    if left <= 0.0 {
                        Step::Stop
                    } else if left <= FADE && !paused {
                        Step::Fade(left / FADE, label)
                    } else {
                        Step::Wait(label)
                    }
                }
                Until::EndOf(id) => {
                    let label = "at the end of this song".to_owned();
                    if current.as_deref() != Some(id.as_str()) {
                        if sleep.from.is_some() || current.is_none() {
                            Step::Stop
                        } else {
                            *id = current.clone().unwrap_or_default();
                            Step::Wait(label)
                        }
                    } else {
                        let left = duration - position;
                        if duration > 0.0 && left <= END_FADE && !paused {
                            Step::Fade((left / END_FADE).max(0.0), label)
                        } else {
                            Step::Wait(label)
                        }
                    }
                }
            };
            if let Step::Fade(..) = step {
                sleep.from.get_or_insert(volume);
            }
            Some((step, sleep.from))
        });
        let Some((step, from)) = step else {
            return;
        };
        match step {
            Step::Wait(label) => self.ui(move |w| w.set_sleep_left(label.into())),
            Step::Fade(share, label) => {
                let target = (from.unwrap_or(volume) * share * share).round();
                if (target - volume).abs() >= 1.0 {
                    self.player.set_volume(target);
                }
                self.ui(move |w| w.set_sleep_left(label.into()));
            }
            Step::Stop => {
                self.player.set_paused(true);
                self.sleep_off();
                self.status("Sleep timer: music paused");
            }
        }
    }
}
