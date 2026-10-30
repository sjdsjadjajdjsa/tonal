use super::*;
use crate::model::SourceKind;
use std::{io::Write, path::PathBuf};

fn track(name: &str, path: &str) -> Track {
    Track {
        id: format!("local:{name}"),
        source: SourceKind::Local,
        source_id: name.into(),
        title: name.into(),
        artist: "Smoke test".into(),
        album: None,
        duration_secs: Some(8.0),
        artwork_url: None,
        play_url: path.into(),
        ..Default::default()
    }
}

#[test]
fn queue_shuffle_keeps_played_prefix_for_prev() {
    for _ in 0..64 {
        let mut queue = Queue::default();
        queue.replace(
            vec![
                track("A", "unused"),
                track("B", "unused"),
                track("C", "unused"),
            ],
            0,
        );
        queue.current = queue.next(RepeatMode::Off, true);
        queue.set_shuffle(true);
        let previous = queue.previous(RepeatMode::Off).unwrap();
        assert_eq!(queue.at(previous).unwrap().title, "A");
    }
}

async fn pump_until(engine: &mut Engine, predicate: impl Fn(&Engine) -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !predicate(engine) {
            let message = engine.ipc_rx.recv().await.unwrap();
            engine.message(message).await.unwrap();
        }
    })
    .await
    .unwrap();
}

async fn tone_engine(tone: &Tone) -> (Engine, UnboundedReceiver<PlayerEvent>) {
    let (tx, rx) = unbounded_channel();
    let mut engine = Engine::new(config(), tx);
    engine.command(Cmd::Play(tone.tracks(), 0)).await.unwrap();
    pump_until(&mut engine, |e| e.pos > 0.1).await;
    (engine, rx)
}

#[tokio::test]
async fn real_mpv_remove_tail_emits_near_end_once() {
    let tone = Tone::new();
    let (mut engine, mut rx) = tone_engine(&tone).await;
    while rx.try_recv().is_ok() {}
    engine.command(Cmd::Remove(1)).await.unwrap();
    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, PlayerEvent::NearQueueEnd))
            .count(),
        1
    );
    engine
        .command(Cmd::Enqueue(vec![tone.tracks()[1].clone()], false))
        .await
        .unwrap();
    engine.command(Cmd::Remove(1)).await.unwrap();
    assert!(
        !std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, PlayerEvent::NearQueueEnd))
    );
}

#[tokio::test]
async fn real_mpv_crash_restarts_same_track_position_and_pause() {
    let tone = Tone::new();
    for paused in [false, true] {
        let (mut engine, mut rx) = tone_engine(&tone).await;
        engine.command(Cmd::Seek(4.0, false)).await.unwrap();
        pump_until(&mut engine, |e| e.pos >= 4.0).await;
        engine.command(Cmd::Pause(paused)).await.unwrap();
        let track_id = engine.queue.track().unwrap().id;
        let position = engine.pos;
        let instance = engine.active.as_ref().unwrap().id;
        while rx.try_recv().is_ok() {}
        engine.active.as_mut().unwrap().kill_for_test().await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let message = engine.ipc_rx.recv().await.unwrap();
                let resumed_position = message.instance != instance
                    && message.value["name"] == "time-pos"
                    && message.value["data"].as_f64().is_some_and(|pos| pos > 2.5);
                engine.message(message).await.unwrap();
                if resumed_position {
                    break;
                }
            }
        })
        .await
        .expect("mpv must restart and resume within five seconds");
        assert_eq!(engine.queue.track().unwrap().id, track_id);
        assert!((engine.pos - position).abs() < 3.0);
        assert_eq!(engine.paused, paused);
        let mpv = engine.active.as_mut().unwrap();
        assert_eq!(
            mpv.request(json!(["get_property", "pause"])).await.unwrap(),
            json!(paused)
        );
        let actual_position = mpv
            .request(json!(["get_property", "time-pos"]))
            .await
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((actual_position - position).abs() < 3.0);
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        if paused {
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, PlayerEvent::Paused(false))),
                "recovery must not announce an unpause"
            );
        }
        let errors: Vec<_> = events
            .into_iter()
            .filter_map(|event| match event {
                PlayerEvent::Error(error) => Some(error),
                _ => None,
            })
            .collect();
        assert_eq!(errors, ["Playback engine restarted"]);
        println!("recovered same track at {actual_position:.2}s, paused={paused}");
    }
}

#[tokio::test]
async fn real_mpv_crash_restart_budget_stops_after_three() {
    let tone = Tone::new();
    let (mut engine, mut rx) = tone_engine(&tone).await;
    // Expired attempts must not consume the rolling sixty-second budget.
    engine.restarts = vec![Instant::now() - Duration::from_secs(61); 3];
    while rx.try_recv().is_ok() {}
    for attempt in 0..4 {
        let instance = engine.active.as_ref().unwrap().id;
        engine.active.as_mut().unwrap().kill_for_test().await;
        pump_until(&mut engine, |e| {
            e.active.as_ref().is_none_or(|mpv| mpv.id != instance)
        })
        .await;
        if attempt < 3 {
            assert_eq!(engine.queue.track().unwrap().title, "tone-one");
        }
        // A command failure and reader EOF can both report the same death.
        engine
            .message(Message {
                instance,
                value: json!({"event":"ipc-error"}),
            })
            .await
            .unwrap();
    }
    assert!(engine.active.is_none());
    assert!(engine.standby.is_none());
    assert!(engine.queue.current.is_none());
    assert!(engine.paused);
    let errors: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            PlayerEvent::Error(error) => Some(error),
            _ => None,
        })
        .collect();
    assert_eq!(
        errors,
        [
            "Playback engine restarted",
            "Playback engine restarted",
            "Playback engine restarted",
            "mpv keeps crashing",
        ]
    );
}

#[tokio::test]
async fn real_mpv_crash_command_error_restarts_without_reading_eof() {
    let tone = Tone::new();
    let (mut engine, mut rx) = tone_engine(&tone).await;
    let instance = engine.active.as_ref().unwrap().id;
    engine.active.as_mut().unwrap().kill_for_test().await;
    // Deliberately do not consume the EOF message: the failed write must suffice.
    let result = engine.command(Cmd::Pause(true)).await;
    assert!(result.as_ref().unwrap_err().is::<Disconnected>());
    engine.handle_result(result).await.unwrap();
    assert_ne!(engine.active.as_ref().unwrap().id, instance);
    assert_eq!(engine.queue.track().unwrap().title, "tone-one");
    assert!(engine.paused);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = engine.ipc_rx.recv().await.unwrap();
            let old_eof = message.instance == instance && message.value["event"] == "ipc-error";
            engine.message(message).await.unwrap();
            if old_eof {
                break;
            }
        }
    })
    .await
    .unwrap();
    let errors = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|event| matches!(event, PlayerEvent::Error(_)))
        .count();
    assert_eq!(errors, 1);
}

#[tokio::test]
async fn real_mpv_crash_failed_seek_preserves_intent() {
    let tone = Tone::new();
    for relative in [false, true] {
        let (mut engine, _) = tone_engine(&tone).await;
        engine.command(Cmd::Pause(true)).await.unwrap();
        let target = if relative { engine.pos + 4.0 } else { 4.0 };
        engine.active.as_mut().unwrap().kill_for_test().await;
        let result = engine.command(Cmd::Seek(4.0, relative)).await;
        assert!(result.as_ref().unwrap_err().is::<Disconnected>());
        engine.handle_result(result).await.unwrap();
        pump_until(&mut engine, |e| e.last_playhead.is_some()).await;
        let actual = engine
            .active
            .as_mut()
            .unwrap()
            .request(json!(["get_property", "time-pos"]))
            .await
            .unwrap()
            .as_f64()
            .unwrap();
        assert!(
            (actual - target).abs() < 0.3,
            "failed seek lost: relative={relative}, target={target}, actual={actual}"
        );
    }
}

#[tokio::test]
async fn real_mpv_crash_during_replacement_spawn_uses_budget() {
    let tone = Tone::new();
    for failures in [1, 3] {
        let (mut engine, mut rx) = tone_engine(&tone).await;
        while rx.try_recv().is_ok() {}
        let instance = engine.active.as_ref().unwrap().id;
        engine.active.as_mut().unwrap().kill_for_test().await;
        mpv::SPAWN_FAILURES
            .scope(std::cell::Cell::new(failures), async {
                engine
                    .handle_result(Err(Disconnected { instance }.into()))
                    .await
                    .unwrap();
            })
            .await;
        let errors: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|event| match event {
                PlayerEvent::Error(error) => Some(error),
                _ => None,
            })
            .collect();
        if failures == 1 {
            assert!(
                engine.active.is_some(),
                "replacement spawn failure swallowed"
            );
            assert_eq!(engine.restarts.len(), 2);
            assert_eq!(errors, ["Playback engine restarted"]);
            pump_until(&mut engine, |e| e.pos > 0.1).await;
            engine
                .active
                .as_mut()
                .unwrap()
                .request(json!(["get_property", "pid"]))
                .await
                .unwrap();
        } else {
            assert!(engine.active.is_none());
            assert!(
                engine.queue.current.is_none(),
                "budget exhaustion must stop playback"
            );
            assert_eq!(engine.restarts.len(), 3);
            assert_eq!(errors, ["mpv keeps crashing"]);
        }
    }
}

#[tokio::test]
async fn real_mpv_crash_mid_gapless_handshake_recovers_playing_entry() {
    let tone = Tone::new();
    for deliver_eof in [false, true] {
        let (mut engine, _) = tone_engine(&tone).await;
        engine.command(Cmd::Seek(7.8, false)).await.unwrap();
        let pending_entry = engine.pending_entry.unwrap();
        // Withhold one half of the handshake while real mpv starts track B.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let message = engine.ipc_rx.recv().await.unwrap();
                let eof = message.value["event"] == "end-file" && message.value["reason"] == "eof";
                let playlist = message.value["name"] == "playlist"
                    && message.value["data"].as_array().is_some_and(|entries| {
                        entries.iter().any(|entry| {
                            entry["current"] == true && entry["id"].as_i64() == Some(pending_entry)
                        })
                    });
                let started = message.value["name"] == "time-pos"
                    && message.value["playback_entry_id"].as_i64() == Some(pending_entry)
                    && message.value["data"].as_f64().is_some_and(|pos| pos > 0.1);
                if (eof && deliver_eof) || (playlist && !deliver_eof) {
                    engine.message(message).await.unwrap();
                }
                if started {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(engine.queue.current, Some(0));
        let instance = engine.active.as_ref().unwrap().id;
        engine.active.as_mut().unwrap().kill_for_test().await;
        engine
            .handle_result(Err(Disconnected { instance }.into()))
            .await
            .unwrap();
        assert_eq!(
            engine.queue.current,
            Some(1),
            "reloaded previous gapless track"
        );
        pump_until(&mut engine, |e| e.last_playhead.is_some()).await;
        let actual = engine
            .active
            .as_mut()
            .unwrap()
            .request(json!(["get_property", "time-pos"]))
            .await
            .unwrap()
            .as_f64()
            .unwrap();
        assert!(
            actual < 2.0,
            "must not reuse previous track's end position: {actual}"
        );
    }
}

#[tokio::test]
async fn real_mpv_crash_standby_clears_crossfade() {
    let tone = Tone::new();
    let (mut engine, mut rx) = tone_engine(&tone).await;
    engine.command(Cmd::Crossfade(2.0)).await.unwrap();
    engine.command(Cmd::Seek(6.2, false)).await.unwrap();
    pump_until(&mut engine, |e| e.fade.as_ref().is_some_and(|f| f.ready)).await;
    engine.command(Cmd::Pause(true)).await.unwrap();
    let paused_at = engine.pos;
    let instance = engine.active.as_ref().unwrap().id;
    while rx.try_recv().is_ok() {}
    engine.standby.as_mut().unwrap().kill_for_test().await;
    pump_until(&mut engine, |e| {
        e.active.as_ref().is_some_and(|mpv| mpv.id != instance)
    })
    .await;
    assert!(engine.standby.is_none());
    assert!(engine.fade.is_none());
    assert!(engine.pending.is_none());
    assert!(engine.pending_entry.is_none());
    assert!(!engine.gapless_eof && !engine.gapless_position);
    assert_eq!(engine.queue.track().unwrap().title, "tone-one");
    assert!(engine.paused);
    assert!((engine.pos - paused_at).abs() < 0.5);
    let errors: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            PlayerEvent::Error(error) => Some(error),
            _ => None,
        })
        .collect();
    assert_eq!(errors, ["Playback engine restarted"]);
}

#[tokio::test]
async fn real_mpv_restore_starts_paused_at_position() {
    let tone = Tone::new();
    let (tx, _rx) = unbounded_channel();
    let mut engine = Engine::new(config(), tx);
    engine
        .command(Cmd::Restore(tone.tracks(), 0, 3.0))
        .await
        .unwrap();
    pump_until(&mut engine, |e| e.pos > 2.5).await;
    assert!(engine.paused);
    assert!(engine.pos < 3.5, "restored at {}", engine.pos);
}

#[tokio::test]
async fn real_mpv_remove_paused_current_stays_paused() {
    let tone = Tone::new();
    let (mut engine, mut rx) = tone_engine(&tone).await;
    engine.command(Cmd::Pause(true)).await.unwrap();
    while rx.try_recv().is_ok() {}
    engine.command(Cmd::Remove(0)).await.unwrap();
    assert!(engine.paused, "removing paused A must not unpause B");
    assert!(
        !std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, PlayerEvent::Paused(false)))
    );
    // A fresh observation verifies the actual mpv pause property, not just engine state.
    engine
        .active_send(json!(["observe_property", 99, "pause"]))
        .await
        .unwrap();
    let paused = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let message = engine.ipc_rx.recv().await.unwrap();
            if message.value["id"] == 99 {
                break message.value["data"].as_bool().unwrap();
            }
        }
    })
    .await
    .unwrap();
    assert!(paused);
}

#[tokio::test]
async fn real_mpv_queued_eof_after_jump_is_ignored() {
    let tone = Tone::new();
    let (mut engine, mut rx) = tone_engine(&tone).await;
    engine
        .command(Cmd::Enqueue(
            vec![track("C", tone.0.to_str().unwrap())],
            false,
        ))
        .await
        .unwrap();
    engine
        .active_send(json!(["seek", 7.9, "absolute+exact"]))
        .await
        .unwrap();
    let eof = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = engine.ipc_rx.recv().await.unwrap();
            if message.value["event"] == "end-file" && message.value["reason"] == "eof" {
                break message;
            }
        }
    })
    .await
    .unwrap();
    engine.command(Cmd::Jump(2)).await.unwrap();
    while rx.try_recv().is_ok() {}
    engine.message(eof).await.unwrap();
    assert_eq!(engine.queue.track().unwrap().title, "C");
    assert!(
        !std::iter::from_fn(|| rx.try_recv().ok())
            .any(|e| matches!(e, PlayerEvent::TrackFinished(_)))
    );
}

#[tokio::test]
async fn real_mpv_queued_playlist_transition_after_jump_is_ignored() {
    let tone = Tone::new();
    let (mut engine, _) = tone_engine(&tone).await;
    engine
        .active_send(json!(["seek", 7.9, "absolute+exact"]))
        .await
        .unwrap();
    let transition = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = engine.ipc_rx.recv().await.unwrap();
            let legacy = message.value["name"] == "playlist-pos"
                && message.value["data"].as_i64().is_some_and(|p| p > 0);
            let identified = message.value["name"] == "playlist"
                && message.value["data"]
                    .as_array()
                    .is_some_and(|entries| entries.iter().skip(1).any(|e| e["current"] == true));
            if legacy || identified {
                break message;
            }
        }
    })
    .await
    .unwrap();
    engine.command(Cmd::Jump(0)).await.unwrap();
    // Reproduce the old EOF/position handshake without completing it first.
    engine.gapless_eof = true;
    engine.message(transition).await.unwrap();
    assert_eq!(engine.queue.track().unwrap().title, "tone-one");
}

#[tokio::test]
async fn real_mpv_loaded_then_failed_repeat_cycle_stops() {
    let tone = Tone::new();
    let (mut engine, mut rx) = tone_engine(&tone).await;
    engine.command(Cmd::Repeat(RepeatMode::All)).await.unwrap();
    // Inject a decoder failure after real mpv playback has emitted file-loaded.
    // This deterministically exercises the otherwise codec/network-dependent sequence.
    for _ in 0..2 {
        pump_until(&mut engine, |e| e.pos > 0.1).await;
        let mpv = engine.active.as_mut().unwrap();
        let playlist = mpv
            .request(json!(["get_property", "playlist"]))
            .await
            .unwrap();
        let entry_id = playlist[0]["id"].as_i64().unwrap();
        let instance = mpv.id;
        engine.message(Message { instance, value: json!({"event":"end-file", "reason":"error", "file_error":"decoder failed", "playlist_entry_id":entry_id}) }).await.unwrap();
    }
    assert!(
        engine.queue.current.is_none(),
        "Repeat All must stop after a failed queue cycle"
    );
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, PlayerEvent::Error(_))));
}

fn config() -> PlayerConfig {
    PlayerConfig {
        volume: 35.0,
        crossfade_secs: 0.0,
        normalize: false,
        gapless: true,
        shuffle: false,
        repeat: RepeatMode::Off,
    }
}

struct Tone(PathBuf);
impl Tone {
    fn new() -> Self {
        static NEXT_TONE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tonal-test-{}-{}.wav",
            std::process::id(),
            NEXT_TONE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        let samples = 44_100_u32 * 8;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + samples * 2).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x01\0").unwrap();
        file.write_all(&44_100_u32.to_le_bytes()).unwrap();
        file.write_all(&88_200_u32.to_le_bytes()).unwrap();
        file.write_all(b"\x02\0\x10\0data").unwrap();
        file.write_all(&(samples * 2).to_le_bytes()).unwrap();
        for i in 0..samples {
            let sample =
                ((i as f64 * 440.0 * std::f64::consts::TAU / 44_100.0).sin() * 3000.0) as i16;
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
        Self(path)
    }
    fn tracks(&self) -> Vec<Track> {
        let path = self.0.to_str().unwrap();
        vec![track("tone-one", path), track("tone-two", path)]
    }
}
impl Drop for Tone {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn wait_for(
    rx: &mut UnboundedReceiver<PlayerEvent>,
    predicate: impl Fn(&PlayerEvent) -> bool,
) -> PlayerEvent {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = rx.recv().await.expect("player task exited");
            match &event {
                PlayerEvent::QueueChanged { tracks, current } => {
                    println!("QueueChanged(len={}, current={current:?})", tracks.len())
                }
                PlayerEvent::NowPlaying(track) => {
                    println!("NowPlaying({:?})", track.as_ref().map(|t| &t.title))
                }
                PlayerEvent::TrackFinished(track) => println!("TrackFinished({})", track.title),
                _ => println!("{event:?}"),
            }
            if let PlayerEvent::Error(error) = &event {
                panic!("playback failed: {error}");
            }
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for playback event")
}

async fn position(rx: &mut UnboundedReceiver<PlayerEvent>, min: f64) {
    wait_for(
        rx,
        |e| matches!(e, PlayerEvent::Position { pos, .. } if *pos >= min),
    )
    .await;
}

#[test]
fn queue_restores_order_and_preserves_duplicate_identity() {
    let mut queue = Queue::default();
    let tracks = (0..12)
        .map(|i| track(&i.to_string(), "unused"))
        .collect::<Vec<_>>();
    queue.replace(tracks.clone(), 4);
    let current = queue.key();
    queue.set_shuffle(true);
    assert_eq!(queue.key(), current);
    queue.set_shuffle(false);
    assert_eq!(queue.tracks(), tracks);
    queue.insert(vec![tracks[4].clone()], true);
    assert_ne!(queue.key_at(4), queue.key_at(5));
    assert_eq!(queue.current, Some(4));
    queue.move_track(4, 1);
    assert_eq!(queue.current, Some(1));
    assert_eq!(queue.key(), current);
    assert!(queue.remove(1));
    assert_eq!(queue.current, Some(1));
    queue.current = Some(queue.len() - 1);
    assert_eq!(queue.next(RepeatMode::Off, true), None);
    assert_eq!(queue.next(RepeatMode::All, true), Some(0));
    assert_eq!(queue.next(RepeatMode::One, true), queue.current);
    assert_eq!(queue.next(RepeatMode::One, false), None);
}

#[tokio::test]
async fn real_mpv_playback_smoke() {
    let tone = Tone::new();
    let (tx, mut rx) = unbounded_channel();
    let handle = spawn(config(), tx).unwrap();
    handle.play_tracks(tone.tracks(), 0);
    position(&mut rx, 0.4).await;
    handle.set_paused(true);
    wait_for(&mut rx, |e| matches!(e, PlayerEvent::Paused(true))).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    while rx.try_recv().is_ok() {}
    tokio::time::sleep(Duration::from_millis(400)).await;
    while let Ok(event) = rx.try_recv() {
        assert!(!matches!(event, PlayerEvent::Position { .. }));
    }
    println!("Pause: no Position events for 400ms");
    handle.seek(4.0);
    handle.set_paused(false);
    position(&mut rx, 4.0).await;
    handle.prev();
    wait_for(
        &mut rx,
        |e| matches!(e, PlayerEvent::Position { pos, .. } if *pos < 1.0),
    )
    .await;
    println!("Prev after 3s restarted the current track");
    handle.next();
    wait_for(
        &mut rx,
        |e| matches!(e, PlayerEvent::NowPlaying(Some(t)) if t.title == "tone-two"),
    )
    .await;
    wait_for(&mut rx, |e| matches!(e, PlayerEvent::NearQueueEnd)).await;
    position(&mut rx, 0.3).await;
    handle.seek(7.3);
    wait_for(
        &mut rx,
        |e| matches!(e, PlayerEvent::TrackFinished(t) if t.title == "tone-two"),
    )
    .await;
    wait_for(&mut rx, |e| matches!(e, PlayerEvent::NowPlaying(None))).await;
    println!("PASS playback/pause/seek/prev/next/natural EOF");

    handle.play_tracks(tone.tracks(), 0);
    position(&mut rx, 0.1).await;
    handle.seek(7.3);
    wait_for(
        &mut rx,
        |e| matches!(e, PlayerEvent::TrackFinished(t) if t.title == "tone-one"),
    )
    .await;
    wait_for(
        &mut rx,
        |e| matches!(e, PlayerEvent::NowPlaying(Some(t)) if t.title == "tone-two"),
    )
    .await;
    position(&mut rx, 0.2).await;
    println!("PASS gapless playlist-pos transition");

    handle.set_crossfade(1.0);
    handle.play_tracks(tone.tracks(), 0);
    position(&mut rx, 0.1).await;
    handle.seek(6.8);
    wait_for(
        &mut rx,
        |e| matches!(e, PlayerEvent::TrackFinished(t) if t.title == "tone-one"),
    )
    .await;
    wait_for(
        &mut rx,
        |e| matches!(e, PlayerEvent::NowPlaying(Some(t)) if t.title == "tone-two"),
    )
    .await;
    position(&mut rx, 0.2).await;
    println!("PASS two-process crossfade");

    handle.set_normalize(true);
    handle.set_volume(52.0);
    wait_for(
        &mut rx,
        |e| matches!(e, PlayerEvent::Volume(v) if (*v - 52.0).abs() < 0.1),
    )
    .await;
    handle.set_normalize(false);
    handle.shutdown();
    tokio::time::timeout(Duration::from_secs(3), async {
        while rx.recv().await.is_some() {}
    })
    .await
    .unwrap();
    println!("PASS normalization toggle, volume, shutdown (event channel closed)");
}
