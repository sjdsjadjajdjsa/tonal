<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/banner-dark.svg">
  <img alt="tonal, a native music player that moves with your music" src="docs/images/banner-light.svg" width="100%">
</picture>

<br><br>

[![release](https://img.shields.io/github/v/release/sjdsjadjajdjsa/tonal?style=flat-square&label=release&labelColor=0e0f12&color=1ed760)](https://github.com/sjdsjadjajdjsa/tonal/releases/latest)
[![ci](https://img.shields.io/github/actions/workflow/status/sjdsjadjajdjsa/tonal/ci.yml?branch=main&style=flat-square&label=ci&labelColor=0e0f12&color=1ed760)](https://github.com/sjdsjadjajdjsa/tonal/actions/workflows/ci.yml)
[![platforms](https://img.shields.io/badge/Linux%20·%20Windows%20·%20macOS-1ed760?style=flat-square&label=runs%20on&labelColor=0e0f12)](#install)
[![license](https://img.shields.io/badge/MIT-1ed760?style=flat-square&label=license&labelColor=0e0f12)](LICENSE)

**[Download](https://github.com/sjdsjadjajdjsa/tonal/releases/latest)** &nbsp;·&nbsp; [Features](#features) &nbsp;·&nbsp; [Screenshots](#screenshots) &nbsp;·&nbsp; [Install](#install) &nbsp;·&nbsp; [Benchmarks](#benchmarks) &nbsp;·&nbsp; [How it works](#how-it-works)

<br>

![A playlist playing over the Northern lights theme, with the flowing wave visualizer along the bottom of the window](docs/images/hero.webp)

</div>

<br>

Stream YouTube Music, SoundCloud and Audius, play your own files and bring your Spotify playlists along. No browser inside, no Premium needed: Tonal draws its own window with [Slint](https://slint.dev) and hands the audio to [mpv](https://mpv.io).

## Features

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/features-dark.svg">
  <img alt="One search over four sources, living themes, a six-point equalizer, synced lyrics, endless radio, Spotify import, artist and album pages, your library, downloads, a mini player, gapless playback and crossfade, media keys and scrobbling" src="docs/images/features-light.svg" width="100%">
</picture>

<details>
<summary>More about each feature</summary>

### Listen

- **Artists and albums:** click any artist or album name to open its page: photo, popular songs, albums, singles and similar artists, or an album with every track. Back and forward work with the arrows, Alt+←/→ and the mouse side buttons.
- **One search, four sources:** YouTube Music, SoundCloud, Audius and your local files. YouTube results come straight from YouTube Music, in under a second.
- **Now playing:** click the cover in the player bar (or `Ctrl` `N`) for a big cover, the waves and synced lyrics.
- **Good playback:** gapless, crossfade, volume levelling, shuffle and repeat, all done by mpv.
- **Radio:** when the queue runs out, Tonal keeps going with similar songs.
- **Synced lyrics** from [LRCLIB](https://lrclib.net), highlighted line by line.
- **Equalizer:** drag six points on a smooth curve, or pick a preset. Hold the compare button to hear the song without it.

### Your library

- **Likes and playlists:** liked songs, playlists, play history, most played, and a queue you can reorder. The queue and position come back when you reopen the app.
- **Spotify import:** paste a public playlist or album link and Tonal finds every song on YouTube Music (up to 100 songs). For bigger playlists and your liked songs, log in with your own Spotify developer app (Spotify only allows these for accounts with Premium). Spotify audio itself is never used.
- **Album grid:** browse your local files as albums, sorted by artist, year or recently added.
- **Listening stats:** your top artists and albums this month and of all time, on Home. They never leave your computer.
- **Downloads** for offline listening, with tags and cover art.

### Make it yours

- **Living themes:** 15 animated background scenes (aurora, dunes, ocean tide, misty pines, liquid glass, synthwave, sakura petals, city rain and more), 18 color palettes, endless seed variants that can crossfade on a timer, four panel styles (Clear, Glass, Neon, Solid) and optional music reactivity. A "From the album" palette follows the playing song's cover, playlists can keep their own theme, and themes can be shared as a short code. The default Classic look paints nothing and costs 0% CPU.
- **Album colors:** the waves and the equalizer take a soft color from the cover.
- **Flowing visualizer:** a soft wave across the bottom of the window that moves with the music. It only runs while something plays, and settles into a still line when you pause.

### On your desktop

- **Desktop integration:** media keys and MPRIS, so your keyboard, bar widgets and `playerctl` all work.
- **Mini player** and **scrobbling** to Last.fm and ListenBrainz.

</details>

## Screenshots

![Living themes moving while a song plays: Northern lights, Misty pines on Now playing, Outrun, Sakura drift, the Themes gallery and City rain](docs/images/themes-reel.webp)

<img src="docs/images/home.webp" alt="Home with recently played songs over the Liquid glass theme" width="49%"> <img src="docs/images/themes.webp" alt="The theme browser and customizer over the Outrun synthwave scene" width="49%">
<img src="docs/images/artist.webp" alt="The Weeknd artist page over the Golden hour dunes scene" width="49%"> <img src="docs/images/album.webp" alt="Daft Punk Random Access Memories album page over Sakura drift" width="49%">
<img src="docs/images/queue.webp" alt="A playlist playing with the queue panel open over City rain" width="49%"> <img src="docs/images/equalizer.webp" alt="The six band equalizer on the Bass preset over Lavender haze" width="49%">

<details>
<summary>Search, settings and the mini player</summary>

<img src="docs/images/search.webp" alt="Search results from YouTube Music for Daft Punk" width="49%"> <img src="docs/images/settings.webp" alt="Settings grouped into Playback and Library cards" width="49%">
<img src="docs/images/mini.webp" alt="The mini player with artwork, controls, progress and the wave visualizer" width="49%">

</details>

## Install

<a href="https://github.com/sjdsjadjajdjsa/tonal/releases/latest">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/install-dark.svg">
  <img alt="Linux: tonal-x86_64.AppImage. Windows: tonal-setup-x86_64.exe. macOS: tonal-macos-arm64.dmg. Arch Linux: packaging/aur/tonal" src="docs/images/install-light.svg" width="100%">
</picture>
</a>

<details>
<summary>Every download, portable mode and YouTube setup</summary>

| | Download | Run |
| --- | --- | --- |
| **Linux** | `tonal-x86_64.AppImage` | Mark it executable (`chmod +x tonal-x86_64.AppImage`) and run it. yt-dlp is inside; install [mpv](https://mpv.io) from your package manager. Needs glibc 2.35 or newer (Ubuntu 22.04, Fedora 36, Arch and so on). `tonal-linux-x86_64.tar.gz` is the same as a plain folder. |
| **Windows** | `tonal-setup-x86_64.exe` | Run it. Installs for your user only, with no admin rights, and adds a Start menu entry. mpv and yt-dlp are inside. `tonal-windows-x86_64.zip` is the portable version: unzip it anywhere (a USB stick works) and run `tonal.exe`. It keeps the library, settings and cache in `data` and `cache` folders next to it. |
| **Arch Linux** | `packaging/aur/tonal/PKGBUILD` | From a clone of this repo: `cd packaging/aur/tonal && makepkg -si`. Builds from source and uses your system's mpv and yt-dlp. |
| **macOS** | `tonal-macos-arm64.dmg` | Open it and drag Tonal to Applications. The first time, right-click Tonal and pick Open, as the app isn't signed. Install the tools with `brew install mpv yt-dlp deno`. For Apple Silicon Macs on macOS 14.4 or newer. |

Windows will say the publisher is unknown, as the installer isn't signed: click "More info", then "Run anyway". The visualizer works on Linux, Windows and macOS; on Windows it needs Windows 10 21H2 or Windows 11 or newer.

Any build turns portable when a file named `portable` or a folder named `data` sits next to the program: the library and settings then go to `data` and the cache to `cache` in that folder.

To build Tonal.app on macOS yourself, run `sh packaging/macos/build-app.sh` from a clone of this repo. It builds the app and puts it in Applications. Run `sh packaging/macos/setup-signing.sh` once before that, so macOS keeps Tonal's permissions (Full Disk Access, audio, Keychain) after each rebuild. The first time a song plays, macOS asks to let Tonal listen to its audio for the visualizer.

For full YouTube support yt-dlp needs a JavaScript runtime: install [Deno](https://deno.com) or Node.js. Tonal picks up whichever it finds. With Deno, Tonal also sets up the [bgutil PO Token provider](https://github.com/Brainicism/bgutil-ytdlp-pot-provider) for yt-dlp on first launch (about 70 MB in its cache folder, no server). When YouTube still refuses a song ("Sign in to confirm you're not a bot"), Tonal retries it with other player clients, then over IPv4, then with and without browser cookies, and remembers what worked. If YouTube blocks your network outright, only Use browser login (a signed-in browser) or another network gets through. The visualizer on Linux needs PipeWire (`pw-record`, `pw-dump`).

### If YouTube asks you to sign in

YouTube sometimes answers "Sign in to confirm you're not a bot", mostly after a lot of plays in a short time. Open **Settings → YouTube → Use browser login** and type the browser where you're signed in to YouTube (`firefox`, `chromium`, `brave`, `chrome`). Tonal then passes that browser's cookies to yt-dlp. On macOS, also turn Tonal on in System Settings → Privacy & Security → Full Disk Access, and pick Always Allow when the Keychain asks, so yt-dlp can read the browser's login. SoundCloud, Audius and local files keep working either way.

</details>

## Benchmarks

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/benchmark-dark.svg">
  <img alt="Memory and CPU of music players while playing a song" src="docs/images/benchmark-light.svg">
</picture>

<details>
<summary>How it was measured, and every number</summary>

Every player was measured the same way by [`bench/bench.py`](bench/bench.py): start it fresh, wait 30 seconds, then play a song into a silent output and measure 30 seconds of it, twice, and take the median. Memory is PSS summed over every process the app starts (mpv for Tonal, the browser processes for Electron apps), so shared libraries count once. CPU is the share of one core. Arch Linux, Hyprland, Ryzen 5 7600, October 2026.

| | Memory playing | CPU playing | CPU before playing | Plays |
| --- | --- | --- | --- | --- |
| **Tonal (waves off)** | 118 MB | 0.5% | 0.1% | same as Tonal |
| **Tonal** | 128 MB | 5.9% | 0.1% | YouTube, SoundCloud, Audius, local |
| **Tonal (live theme)** | 131 MB | 21.8% | 13.4% | same as Tonal |
| Strawberry | 133 MB | 1.8% | 0.6% | local files, Subsonic servers, internet radio; Tidal and Qobuz with a paid plan, Spotify with Premium |
| Amberol | 182 MB | 0.8% | 0.9% | local files |
| Tauon | 228 MB | 3.0% | 0.6% | local files, Plex, Jellyfin and Airsonic servers |
| Riff | 495 MB | 60.8% | 32.2% | YouTube Music, YouTube, SoundCloud, local; imports Spotify playlists (its Spotify login needs Premium) |
| YouTube Music (Pear) | 805 MB | 10.4% | 12.7% | YouTube Music |
| Spotify | 1282 MB | 2.7% | 49.0% | Spotify (free with ads, Premium without) |
| Spotifast | 100–250 MB (their claim) | not measured | not measured | Spotify, Premium only |
| *"I vibe-coded a Spotify killer in a weekend" (from a TikTok)* | *500–1000 MB* | *10%, and 100% while the fan sounds like a jet* | *yes* | *the demo video, on a loop* |

The last row is a joke. Nobody measured it, and nobody could find the download link.

- Versions: Tonal 0.3.0 (this repo, 24.0 MB binary), Spotify 1.3.4, Riff 43.2.2 and YouTube Music (Pear) 3.12.0 from their AppImages, Strawberry 1.2.31, Amberol 2026.1 and Tauon 13.1.0 from Flathub.
- Strawberry, Amberol and Tauon play a local MP3. The others stream, which costs more.
- "Plays" is checked against each project's own README, release notes and source, October 2026. Tauon removed Spotify in v10. Riff only imports Spotify playlists; it never plays Spotify audio.
- Tonal's waves cost about 5.5% of one core, because the window is redrawn by the CPU 30 times a second. Turn them off in Settings and Tonal is the lightest player measured here.
- "Live theme" is Northern lights (Aurora scene, Glass style, Pulse) at 15 fps. A moving scene costs about 13% of one core, more while it reacts to music. The default Classic look paints nothing.
- Since 0.3.0, a moving scene slows to 10 fps while another window has focus, to 8 fps on battery, and stops when the window is hidden. Measured the same way on an unfocused window, the live theme went from 37.5% to 23.9% of one core while playing, and from 20% to under 1% while hidden on another workspace.
- [Spotifast](https://github.com/crmne/spotifast) states "100–250 MB of RAM" in its README, and playback needs Spotify Premium, so it isn't measured here.
- "Before playing" is the 15 seconds right after the 30-second wait, with the app open and nothing playing.

</details>

## Keys

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/keys-dark.svg">
  <img alt="Space play or pause, Ctrl F search, Ctrl L like, Ctrl arrows previous and next, arrows seek 5 seconds, Ctrl up and down volume, Ctrl N now playing, Ctrl M mini player, Alt arrows back and forward" src="docs/images/keys-light.svg" width="100%">
</picture>

Right-click a song for play next, add to queue, start radio, download and add to playlist.

## How it works

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/how-dark.svg">
  <img alt="Sources feed the player, the player feeds the visualizer, the visualizer feeds the Slint interface. Integrations sit beside the sources and the SQLite library beside the interface." src="docs/images/how-light.svg" width="100%">
</picture>

<details>
<summary>The code, and building it yourself</summary>

- `src/player`: the playback engine. One mpv process over JSON IPC (a second one only during crossfades), with its own queue, shuffle history and gapless prefetch.
- `src/sources`: YouTube Music (InnerTube), SoundCloud (yt-dlp), Audius (public API), local files (lofty), radio, matching and downloads.
- `src/library`: SQLite for tracks, likes, playlists, history, settings and the saved session.
- `src/integrations`: Spotify import, LRCLIB lyrics, Last.fm and ListenBrainz.
- `src/visualizer`: reads Tonal's own audio (PipeWire on Linux, WASAPI process loopback on Windows, a Core Audio process tap on macOS), runs an FFT and turns it into smooth wave paths.
- `ui/`: the Slint interface.
- `docs/banner.py`, `docs/art.py` and `bench/chart.py`: draw the banner and the cards on this page from the app's icons and the bundled Inter font (`pip install fonttools`).

[docs/architecture.md](docs/architecture.md) has the details.

### Build it yourself

Needs [Rust](https://rustup.rs) 1.90 or newer, plus mpv and yt-dlp on your `PATH`. On Debian or Ubuntu you also need `libsqlite3-dev libdbus-1-dev libfontconfig-dev libxkbcommon-dev`.

```
git clone https://github.com/sjdsjadjajdjsa/tonal
cd tonal
cargo run --release
```

`cargo test` runs the tests, including real playback through mpv on a silent output.

</details>

## License

[MIT](LICENSE). Bundled programs keep their own licenses, see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md). The bundled [Inter](https://rsms.me/inter/) font is under the SIL Open Font License ([ui/fonts/Inter-LICENSE.txt](ui/fonts/Inter-LICENSE.txt)).

Tonal plays streams from public services through mpv and yt-dlp. Use it in line with the terms of those services.
