use crate::AppWindow;
use crate::visualizer::Visualizer;
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::RefCell;
use std::time::{Duration, Instant};

const FRAME: Duration = Duration::from_millis(33);

struct Waves {
    visualizer: Visualizer,
    timer: Timer,
    last: Instant,
}

thread_local! {
    static WAVES: RefCell<Option<Waves>> = const { RefCell::new(None) };
}

pub fn set_playing(window: &AppWindow, playing: bool) {
    let playing = playing && window.get_waves();
    WAVES.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            if !playing {
                return;
            }
            *slot = Some(Waves {
                visualizer: Visualizer::new(),
                timer: Timer::default(),
                last: Instant::now(),
            });
        }
        let waves = slot.as_mut().expect("waves");
        waves.visualizer.set_playing(playing);
        if playing {
            window.set_wave_resting(false);
        }
        if !waves.timer.running() {
            waves.last = Instant::now();
            let weak = window.as_weak();
            waves.timer.start(TimerMode::Repeated, FRAME, move || {
                if let Some(window) = weak.upgrade() {
                    tick(&window);
                }
            });
        }
    });
}

fn tick(window: &AppWindow) {
    WAVES.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(waves) = slot.as_mut() else { return };
        let now = Instant::now();
        let dt = now.duration_since(waves.last).as_secs_f32();
        waves.last = now;
        let eq_open = window.get_panel() == "eq";
        waves
            .visualizer
            .set_backdrop(eq_open.then(|| (window.get_eq_width(), window.get_eq_height())));
        // The now playing view shows the waves large; the bar hides its own meanwhile.
        let (width, height) = if window.get_now_open() && window.get_stage_width() > 1.0 {
            (window.get_stage_width(), window.get_stage_height())
        } else {
            (window.get_wave_width(), window.get_wave_height())
        };
        let frame = waves.visualizer.frame(dt, width, height);
        if waves.visualizer.is_settled() {
            waves.timer.stop();
            window.set_wave_resting(true);
            window.set_wave_back(Default::default());
            window.set_wave_front(Default::default());
            window.set_wave_back_line(Default::default());
            window.set_wave_haze(Default::default());
            window.set_wave_glow(Default::default());
            window.set_wave_line(Default::default());
            window.set_eq_backdrop(Default::default());
            return;
        }
        window.set_wave_back(frame.back.into());
        window.set_wave_front(frame.front.into());
        window.set_wave_back_line(frame.back_line.into());
        window.set_wave_haze(frame.haze.into());
        window.set_wave_glow(frame.glow.into());
        window.set_wave_line(frame.line.into());
        window.set_eq_backdrop(frame.backdrop.into());
    });
}
