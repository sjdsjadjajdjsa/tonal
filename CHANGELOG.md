# Changelog

All notable changes to Tonal. Versions follow [Semantic Versioning](https://semver.org).

## Unreleased

### Added
- Equalizer: six bands on a curve you drag, six presets, a preamp and a hold-to-compare button. It opens from the player bar or Settings and changes the sound live.

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
