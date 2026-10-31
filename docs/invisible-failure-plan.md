# Invisible-failure plan (no sign-ins, music never stops)

Goal: a blocked or slow song must never stop or silence playback. No sign-ins, no new settings.

## Changes

1. **Never hold silence** — on `LoadFailed` the app waits a 4s grace for a fallback winner, then answers `skip_failed` so the next song starts; the lookup keeps running and repairs the failed queue slot in the background.
2. **Race the fallbacks** — YouTube retry, SoundCloud and Audius searches run at once; first winner swaps in.
3. **Pre-check the next track** — the engine emits `CheckNext` ~45s before the current song ends; the app resolves the next URL (caching it) or repairs it before the transition.
4. **Stream URL cache** — resolved direct URLs are cached ~4h and handed to mpv, so loads start instantly and yt-dlp runs less often (fewer 429s).
5. **No 3-strikes stop** — bot-block counting is gone; the engine stops only after 5 consecutive load failures with no playback between (dead network).

## Contracts

### `src/sources/resolve.rs` (new — helper owns)
```rust
pub fn lookup(url: &str) -> Option<String>                  // fresh cached direct URL
pub fn remember(url: &str, direct: &str)                    // TTL 4h, max 200 entries, evict oldest
pub fn forget(url: &str)
pub async fn resolve(url: &str) -> Result<String, String>   // yt-dlp -g via super::ytdlp() + ["-f","bestaudio/best","-g","--no-playlist","--no-warnings"], 60s timeout, last non-empty stdout line; remembers on success
```
No changes to any other file. Unit tests for TTL/eviction/forget (no network).

### `src/sources/pot.rs` (helper owns)
- `probe` uses `-g` (not `--simulate`) and returns `Ok(direct_url)` (last non-empty stdout line) instead of `Ok(())`.
- `unblock` returns `Result<(String /*description*/, Option<String> /*direct url*/), String>`.
- Keep the 60s probe timeout, strategy memory and config rewrite as-is.

### Engine (`src/player/mod.rs` — main tab owns)
- `PlayerEvent::CheckNext { key: u64, track: Track }` — emitted once per current track when `duration > 60 && pos > duration - 45 && !paused`, for the entry `queue.next(repeat, true)` returns (skipped when it is the current entry itself).
- `Cmd::Replace(key, track)`: if the key is held and current → swap and play (unchanged); otherwise if the key is still in the queue → swap the slot, `queue_changed()`, `sync_prefetch()`, no playback interruption.
- `Cmd::RefreshPrefetch` + `PlayerHandle::refresh_prefetch()` → `sync_prefetch()`.
- Consecutive-failure stop guard becomes `failures >= queue.len().min(5)` (both `SkipFailed` and the non-YouTube auto-skip).

### App (`src/app/*`, `src/sources/mod.rs` — main tab owns)
- `mpv_url` returns `resolve::lookup(url)` when fresh (after the debug fake-bot hook).
- `is_streamed_url(url)` — YouTube or soundcloud.com hosts.
- `LoadFailed` → forget cache entry, race (grace 4s) → winner `replace`, timeout `skip_failed` then late winner `replace`; total failure → toast "Couldn't play {title}".
- `CheckNext` → skip local files and non-streamed URLs; `resolve` ok → cache + `refresh_prefetch`; failed → background race (no skip) → winner `replace`.
- `bot_failures` state field and the 3-strikes stop are removed.

## Ownership
- Helper: `src/sources/resolve.rs` (new), `src/sources/pot.rs`. Touches nothing else.
- Main tab: `src/player/mod.rs`, `src/player/tests.rs`, `src/app/playback.rs`, `src/app/state.rs`, `src/sources/mod.rs`, docs.
