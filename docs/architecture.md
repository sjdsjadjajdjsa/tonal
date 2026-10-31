# Tonal architecture (shared contract)

Tonal is an ultra-light desktop music player. Rust + Slint UI + mpv (JSON IPC subprocess) for playback + yt-dlp / HTTP APIs for sources. No Electron, no webview. Linux first, Windows next (keep code cross-platform: no unix-only APIs without a `#[cfg(windows)]` path).

Rendering: Slint software renderer (winit backend). Measured lower RAM than femtovg (about 35 MB idle vs 120 MB) at equal or lower CPU. Window background is translucent so Hyprland/compositor blur shows through. Theme is gray and white (`ui/theme.slint`), Inter font embedded from `ui/fonts`.

Goals: near-0% CPU when idle (no timers/animations repainting while nothing changes), low RAM, instant UI.

## Runtime model
- Main thread runs the Slint event loop.
- One tokio multi-thread runtime (created in `app::run`, entered before anything is spawned) runs all async work.
- Backend → UI: `slint::invoke_from_event_loop` / `Weak<AppWindow>::upgrade_in_event_loop`.
- UI → backend: callbacks call `PlayerHandle` methods (non-blocking channel sends) or `tokio::spawn` async jobs.
- `Library` is sync (rusqlite behind a mutex); call it from `spawn_blocking` or directly for quick queries.

## Ownership (each helper edits ONLY its own files)
| Area | Files | Owner |
|---|---|---|
| Shared types | `src/model.rs`, `Cargo.toml`, `docs/architecture.md` | lead (ask before changing; adding a dep to Cargo.toml is allowed: append only, keep it minimal) |
| Playback engine + queue | `src/player/**` | player helper |
| Sources, search, radio, matching, local scan, downloads | `src/sources/**` | sources helper |
| SQLite library + Spotify + lyrics + scrobbling | `src/library/**`, `src/integrations/**` | library helper |
| UI, app glue, media keys/MPRIS, artwork cache | `ui/**`, `src/main.rs`, `src/app/**`, `src/media_controls.rs`, `src/artwork.rs` | UI helper |

The public signatures in each module's stub are the contract. Keep them; adding new public items is fine. If you truly must change a signature, keep the old one working or tell the lead in your report.

## Module contracts (summary; source stubs are authoritative)

### model
`Track { id: "<prefix>:<source_id>", source: SourceKind, source_id, title, artist, album, duration_secs, artwork_url, play_url }`. `play_url` is what mpv loads: a web page URL (YouTube/SoundCloud, mpv's ytdl hook resolves it), a direct stream URL (Audius), or a local file path. `Settings` (serde default), `Playlist`, `RepeatMode`, `paths::{data_dir, cache_dir}`.

### player (`src/player`)
`spawn(PlayerConfig, UnboundedSender<PlayerEvent>) -> PlayerHandle`; must be called inside the tokio runtime. Owns the queue (shuffle, repeat, history for prev), auto-advance, gapless (preload next), crossfade (two mpv instances fading), loudness normalisation (mpv `af` loudnorm/dynaudnorm), volume. Talks to `mpv --idle --no-video --no-terminal --input-ipc-server=<socket or \\.\pipe\name> --ytdl-format=bestaudio/best`. Emits `PlayerEvent`s (throttle `Position` to ~4/s max; none while paused). `NearQueueEnd` fires once when the last queued track starts and repeat is Off (app appends radio tracks when autoplay is on). `CheckNext` fires once per track about 45 s before its end with the next queue entry, so the app can cache its stream URL or swap in a fallback before the gap. `TrackFinished` fires when a track plays to the end. mpv processes are killed on `shutdown` and on drop / app exit.

### sources (`src/sources`)
`search(kind, query, limit)`: YouTube = YouTube Music InnerTube search (songs filter) via reqwest, fast and with artist/album/duration/artwork; fall back to `yt-dlp --flat-playlist -J ytsearchN:` . SoundCloud = `yt-dlp -J --flat-playlist scsearchN:`. Audius = public API (discovery node from `https://api.audius.co`), `play_url` = stream endpoint. Local = filter of library local tracks is done by the app, so `search(Local, ..)` may return Ok(empty).
`radio(seed, limit)`: YouTube Music "watch next / radio" (InnerTube `next` with `RDAMVM<id>` playlist) or yt-dlp mix; other sources: search by artist.
`match_track(source, title, artist, duration)`: best song match on a source (duration within ~5s preferred); YouTube for Spotify import, SoundCloud and Audius for playback fallback.
`missing_tools()`: names of required external tools not on PATH (`mpv`, `yt-dlp`).
`local::scan(dirs)`: walk dirs, read tags with lofty, `id = local:<abs path>`, `play_url = abs path`, artwork_url = `file://...` cached embedded cover in `cache_dir()/covers/` or None.
`download::download(track, dir, progress)`: `yt-dlp -x --audio-format opus --embed-metadata --embed-thumbnail -o ...`, progress 0.0..1.0, returns final file path.

### library (`src/library`)
SQLite in `data_dir()/library.db`, schema migrations via `PRAGMA user_version`. All methods in the stub. Settings stored as one JSON row. Session = saved queue + current index + position for restore on launch.

### integrations
- `spotify::fetch_public(url)`: playlist/album URL → (name, tracks) WITHOUT login (Spotify embed page JSON). Login flow (PKCE, user's own client id, redirect `http://127.0.0.1:8898/callback`) for liked songs + own playlists.
- `lyrics::fetch(track)`: LRCLIB (`https://lrclib.net/api/get` then `/api/search`), parse LRC to `(secs, line)`, cache on disk.
- `scrobble`: Last.fm (user key/secret, desktop auth → session key) and ListenBrainz (user token). `should_scrobble` rule.

### app / UI
Sidebar: Home (recent, most played, quick picks), Search (source tabs: YouTube, SoundCloud, Audius, Local; and All), Library (Liked, Playlists, Local files, Downloads), Settings. Bottom player bar (art, title/artist, like, prev/play/next, seek, time, shuffle/repeat, volume, queue, lyrics, mini mode). Queue panel, synced lyrics panel, mini player mode (small window). Context actions on tracks: play, play next, add to queue, like, add to playlist, download, start radio. Spotify import dialog. Media keys + MPRIS via souvlaki. Keyboard shortcuts (space, ctrl+f, arrows). Session restore. Dark theme, accent from settings.

### recovery
If the active mpv dies (IPC EOF or a broken write), the engine spawns a new one and reloads the current track at the last position with the same pause state, at most 3 times per 60 s, then stops with an error. No polling.

### playback fallback (invisible failure)
A failed YouTube load emits `PlayerEvent::LoadFailed` and the engine holds on that entry. The app races a YouTube retry (`unblock_youtube`) against SoundCloud and Audius searches at once; the first playable copy wins. The queue is never held silent longer than ~4 s (`FALLBACK_GRACE`): past that the next song starts, and a late winner repairs the entry in place (`PlayerHandle::replace` on a non-current entry fixes the slot without interrupting playback). `sources::resolve` caches direct stream URLs for 4 h (200 entries) so replays start without yt-dlp; `mpv_url()` serves them to mpv and `PlayerHandle::refresh_prefetch` re-syncs the gapless preload after a cache fill. Five load failures in a row with nothing playing in between stop playback (dead network); otherwise playback always continues.

### yt-dlp
`sources::ytdlp()` adds browser cookies and a JavaScript runtime (`deno`, `node` or `bun`, whichever is on PATH) for YouTube challenges; `ytdl_raw_options()` passes the same to mpv. `update_ytdlp()` runs `--update-to stable` for standalone builds.

### visualizer (`src/visualizer.rs`, `src/app/waves.rs`, `ui/visualizer.slint`)
Linux only. mpv instances are named `tonal-<pid>-<n>` via `--audio-client-name`; the capture thread finds them with `pw-dump` and reads mono f32 with `pw-record --target <node>` (no fallback to the mic). FFT (rustfft, 2048) into 36 log bands, auto gain, smoothing, then two filled layers plus a line as SVG path strings in pixel coordinates (Slint Path keeps aspect ratio, so the viewbox must match the element size). A 30 fps Slint Timer runs only while playing and stops once the waves settle after pause. Capture processes die with the app (PR_SET_PDEATHSIG).

### Test hooks
Only in builds with `--features debug-hooks` (src/app/debug.rs); release builds leave them out. `TONAL_DEBUG_QUERY` (search and play first result), `TONAL_DEBUG_VIEW` (mini, queue, lyrics, eq, settings, nowplaying, stats, stats-all, albums), `TONAL_DEBUG_MUSIC` (scan this folder as the music folder), `TONAL_DEBUG_CARD` (click a card after 5 s: "local-album:0", "top-artist:0", "top-album:0"), `TONAL_DEBUG_SNAPSHOT` (window snapshot PNG after 12 s), `TONAL_DEBUG_SOURCE` (0 YouTube, 1 SoundCloud, 2 Audius), `TONAL_DEBUG_PLAYLIST` (open and play a playlist id), `TONAL_DEBUG_TITLE` (window title, for compositor rules that size test windows), `TONAL_DEBUG_ARTIST` (open an artist page by name on `TONAL_DEBUG_SOURCE`), `TONAL_DEBUG_ALBUM` ("title|artist"), `TONAL_DEBUG_FAKE_BOT=1` (every YouTube load fails as if yt-dlp said "Sign in to confirm you're not a bot", to exercise the fallback). For silent tests route mpv to a null sink: `pactl load-module module-null-sink sink_name=tonal_test` and `MPV_HOME` with `audio-device=pipewire/tonal_test`; use `XDG_DATA_HOME` to keep test data out of the real library.
