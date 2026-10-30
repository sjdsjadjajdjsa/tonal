# Changelog

All notable changes to Tonal. Versions follow [Semantic Versioning](https://semver.org).

## 0.3.0 - 2026-10-31

### Added
- Equalizer: six bands on a curve you drag, six presets and a hold-to-compare button; volume headroom is handled automatically. It opens from the player bar or Settings and changes the sound live.
- A small tag next to the time shows what plays, like FLAC 24/96, Opus 160k or MP3 320k.
- Settings → Playback → Output picks the speakers or headphones, and switches live.
- ReplayGain (on by default): files with loudness tags play at their saved level, and then the dynamic Even out volume steps aside for them.
- Now playing view: click the cover in the player bar or press Ctrl+N for a large cover, the names, the waves across the view and synced lyrics beside it on wide windows. Esc goes back.
- Local files can show an album grid, sorted by artist, year or recently added. Albums are grouped by album artist and open their album page.
- Listening stats on Home: top artists and albums for this month and all time, with play counts. Computed from your local play history only.
- Album colors (on by default, Settings → Appearance): the waves and the equalizer curve take a soft color from the cover and fade to it when the song changes.
- Bit-perfect mode: no equalizer, no volume change and no resampling by Tonal, with exclusive output on Windows and macOS. On Linux, PipeWire may still resample to its own rate.
- Equalizer headphone correction: the headphones chip searches the AutoEQ database (downloaded on demand and cached) and puts your model's correction underneath any preset, drawn as a faint line on the curve.
- Equalizer fine-tune: drag a dot sideways to move that band's frequency; the labels follow. Double-click resets gain and frequency.

## 0.2.0 - 2026-10-30

### Added
- macOS: Tonal.app and a `.dmg` for Apple Silicon (macOS 14.4 or newer), with the title bar blended into the window.
- The visualizer works on all three systems: a Core Audio process tap on macOS and WASAPI process loopback on Windows.
- Portable Windows zip that keeps its library, settings and cache next to `tonal.exe`.
- Arch Linux PKGBUILD in `packaging/aur/tonal`.
- Benchmarks against Spotify, Riff, YouTube Music (Pear), Strawberry, Amberol and Tauon, with the script that measures them.
- When a YouTube song won't play, the message shows yt-dlp's reason.
- Artist pages: photo, popular songs, albums, singles and EPs, and similar artists, with play, shuffle and radio.
- Album pages with every track in order, play, shuffle and save as playlist.
- Artist and album names are clickable in song rows, the queue, the player bar and the right-click menu; songs with several artists ask which one.
- Back and forward through pages with the sidebar arrows, Alt+Left/Right or the mouse side buttons.
- Local files get artist and album pages from their tags.
- Playback restarts by itself if mpv crashes, up to three times a minute.
- Settings → YouTube → yt-dlp → Update, to update the bundled yt-dlp from inside the app.
- YouTube uses Deno, Node.js or Bun for yt-dlp's JavaScript challenges when one is installed.
- Third-party notices, with mpv's license and source in the Windows downloads.

### Changed
- The waves rise from the very bottom of the player bar with a soft glow on the crest.
- mpv's stream cache is capped at 16 MB ahead and 8 MB behind, which cuts memory by about 160 MB on long mixes.
- Playing memory down about 6 MB by limiting glibc malloc arenas.
- Songs without a known length show no time instead of 0:00.
- Lyrics follow the song: the current line stays centered, scrolling by hand pauses following for a few seconds, clicking a line jumps there, plain lyrics scroll as a page.
- Lyrics are searched again with a cleaned title when no synced version is found.
- The app code is split into smaller modules; test hooks are left out of release builds.

### Fixed
- mpv and yt-dlp stop when Tonal quits on macOS and Windows too, including after a crash.

## 0.1.0 - 2026-10-28

First release.

- Search and stream from YouTube Music, SoundCloud and Audius, and play local files.
- Liked songs, playlists, history, most played, queue and session restore.
- Spotify playlist and album import, and Spotify login for liked songs and big playlists.
- Gapless playback, crossfade, volume levelling, shuffle, repeat and radio.
- Synced lyrics, downloads, media keys and MPRIS, mini player, Last.fm and ListenBrainz.
- Flowing wave visualizer on Linux.
- AppImage for Linux, installer and zip for Windows.
