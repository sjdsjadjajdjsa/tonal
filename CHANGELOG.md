# Changelog

All notable changes to Tonal. Versions follow [Semantic Versioning](https://semver.org).

## 0.11.0 - 2026-11-09

### Added
- Your own colours: the "Your colors" palette has five colours you set with hue, saturation and brightness, or by typing a hex code.
- Your own background: the "Your own" scene shows a picture, GIF or video you choose, dimmed like the other scenes, and tints Tonal with its main colour. A video plays muted and on a loop, holds still on battery and stops when the window is hidden.
- Theme files: Export saves the theme on screen as a `.tonaltheme` file with its picture inside, and Import, or dropping the file on the window, adds it to your themes.
- Share codes now carry your own colours. Older codes still work.
- Windows builds are made on Linux now, so every release builds on the project's own machines.

### Fixed
- "Add to queue" put the song after the whole album or playlist that was playing. Added songs now play right after the current one, in the order you added them, and a message says so.

## 0.10.4 - 2026-11-08

### Fixed
- The player bar disappeared below the window when Tonal was made short. The sidebar's middle now scrolls instead, and the bar stays in view at any size.
- At the narrowest width the mini player button no longer falls off the end of the bar.

## 0.10.3 - 2026-11-08

### Fixed
- Windows: most of the window could turn black, leaving only the player bar, because Windows can drop parts of a window that weren't just repainted. Tonal now repaints the whole window when that can happen, and once a second besides.

## 0.10.2 - 2026-11-05

### Changed
- The first release built and published on GitLab, with every download on the GitLab release page.

## 0.10.1 - 2026-11-05

### Changed
- Tonal now lives on GitLab: gitlab.com/sjdsjadjajdjsa/tonal. Updates, downloads, issues and merge requests are there; the GitHub repository stays as a read-only copy.
- Settings, Updates and the AppImage's own updater look for new versions on GitLab.

## 0.9.0 - 2026-11-05

### Added
- Home opens on your Mixes as coloured tiles and a "Jump back in" row of the albums you played last, above what you played recently.
- Quick search: Ctrl K (Cmd K on a Mac) finds pages, playlists, songs in your library, equalizer presets and actions like the sleep timer. Arrows move, Enter runs it.
- Keep liked songs offline: an option in Settings, Library downloads every liked song in the background, one at a time, so it plays without internet.
- The song in the player bar fades in when the song changes.
- Lyrics follow the song more closely: the current line grows, the lines around it fade with distance, and the full-screen view glows in the cover's colour.

## 0.8.0 - 2026-11-05

### Added
- Animations: the window fades in when Tonal opens, pages and tabs slide softly into place, side panels glide in, dialogs rise in, covers fade in as they load, hearts pop when you like a song and buttons give a little when pressed.
- Shimmering placeholder rows while search results load, instead of an empty page.
- Settings, Animations turns all motion off.

### Fixed
- Switching equalizer presets quickly no longer shows "mpv: error running command", and the equalizer you picked stays on when the next song starts.

## 0.7.1 - 2026-11-05

### Added
- Show on Discord: your Discord profile says what you're listening to, with the cover and time left, and clears while paused. Turn it off in Settings, Show on Discord.

## 0.7.0 - 2026-11-05

### Added
- Sleep timer: the moon button next to the queue stops the music after 15 minutes to 90 minutes or at the end of the song. It fades out first, then puts the volume back for next time.
- Mixes in the sidebar that fill themselves: On repeat (songs played three times this month), Rediscover (liked or much played songs you haven't heard in two months) and Recently added.
- Each output brings back its headphone correction along with its equalizer, and the equalizer says which output it's for.

### Changed
- Shuffle spreads songs by the same artist apart and plays the songs you heard last at the end.

## 0.6.0 - 2026-11-05

### Added
- Updates from inside Tonal: Settings, Updates checks GitHub and installs the new version, then Tonal restarts on it. Works for the Mac app, the Windows installer and the AppImage; other installs get the download page. Tonal also checks once at startup and says when a new version is out.
- Your own equalizer presets: shape a curve, press + next to Custom and name it. Your presets sit under the built-in ones; the bin button next to the active one deletes it.
- Equalizer per output: headphones and speakers each keep their own equalizer, and it comes back when you switch to that output. Turn it off with Settings, Equalizer per output.
- Number keys switch presets: 1 to 6 are the built-in ones, 7 to 9 your first three.
- Soft sounds for likes, unlikes, adding to the queue, adding to a playlist and saving a preset. Settings, Interface sounds turns them all off, equalizer bubbles included.

### Changed
- Equalizer presets are much stronger, so Bass, Vocal, Bright, Warm and Late night each sound clearly different. A limiter after the equalizer keeps big boosts from clipping.

## 0.5.0 - 2026-11-05

### Added
- Equalizer presets now spring into place one band at a time. Each dot pops as it lands and plays a bubble note that rises with the band and its boost. Turn the sound off with Settings, Equalizer sounds.

## 0.4.3 - 2026-11-02

### Fixed
- Windows: parts of the window could go missing and show the desktop or other windows through holes until they were redrawn. The window is now opaque on Windows.

## 0.4.2 - 2026-11-02

### Fixed
- Windows: Tonal could close a second after opening. Media controls started before the window existed and crashed the app; they now wait for the window.
- A crash now leaves `crash.log` in Tonal's data folder, so builds without a terminal still say what went wrong.

## 0.4.1 - 2026-11-01

### Fixed
- Windows: a terminal window no longer flashes open each time a YouTube song starts.

### Changed
- mpv starts without its on-screen Lua scripts (controls, stats, console, menus), which Tonal never shows: about 7 MB and 7 threads less while playing.

## 0.4.0 - 2026-10-31

### Added
- Living themes: 15 animated background scenes (aurora, dunes, tide, misty pines, ink mountains, sakura, deep space, soft bloom, liquid glass, synthwave, contours, halftone, paper cut, city rain, ribbons) in 18 palettes, with mist and near layers for depth.
- Themes page: a gallery with live previews on hover, Surprise me, Peek, and your own saved themes. Pick the scene, palette, panel style (Clear, Glass, Neon, Solid), how it reacts to music, motion, dim, grain and smoothness.
- Every look has numbered variants; Rotate crossfades to the next one every 1, 5, 15 or 60 minutes.
- Waves take two colours from the theme, the back wave trails the front one, and both brighten on kick drums. Settings has a Beat flash choice (Off, Soft, Strong).
- "From the album" palette: the scene takes its colours from the playing song's cover and changes with each song. Two gallery looks use it, Cover glow and Album waves.
- Playlist themes: the palette button on a playlist pins the current look to it, and the look switches in when you play that playlist.
- Share codes: Share on the Themes page shows a short code to copy; paste someone else's code there to load their theme.

### Fixed
- Switching to Classic clears the background at once instead of after the mouse leaves the card.
- Esc leaves Peek.
- Shuffle play starts on a random song, seeking while paused moves the time, and Play, Next and the queue work again after the queue ends.
- Delete and Rename dialogs close when the open playlist changes, so they never act on the wrong one.
- Spotify import keeps going past a playlist it can't read and replaces an imported playlist instead of adding a copy.
- Failed Last.fm scrobbles are kept and sent again later.
- Quick pause and resume no longer stacks up wave capture threads; lyrics no longer show for a different song with a similar name.

### Changed (performance)
- A moving scene slows to 10 fps while another window has focus and to 8 fps on battery, and stops painting when Hyprland or another compositor stops drawing the window. Live theme while playing: 37.5% to 23.9% of one core unfocused; hidden on another workspace: 20% to under 1%.
- Painting a scene frame is about 12% cheaper.
- Scroll bars use the theme colour.

### Changed
- The default Classic look paints nothing, so Tonal costs the same as before unless a scene is picked. A moving scene uses about 13% of one core at 15 fps.
- While music is paused, a scene slows to a quarter speed and paints at most 6 times a second. It stops painting while the window is minimized or hidden, slows to 10 fps while another window has focus and to 8 fps on battery. Scroll bars follow the theme colour.
- Release builds use optimization level 2: the binary is 24.0 MB instead of 24.6 MB at the same CPU.

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
