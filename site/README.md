# tonalplayer.com

One page that feels like opening Tonal: plain HTML, CSS and a little JavaScript, no
build tools. `build.sh` puts it together in one folder that any web server can serve. Point the
server's 404 page at `404.html`. GitLab
Pages also publishes it whenever a push to `main` changes `site/`, `docs/images/` or
the icon and fonts.

## Run it

```
sh site/build.sh /tmp/site && python3 -m http.server -d /tmp/site 8765
```

`build.sh` copies the page, the app icon, the Inter fonts and the screenshots from
`docs/images` into one folder. New screenshots in `docs/images` show up on the next
push.

## The idea

The page is the app. The whole site is one Tonal window (after Poolsuite in
web-refs), and it behaves like Tonal:

- **Pages** have real addresses (`/equalizer/` and so on, each with its own title and
  share picture in `og/`) and swap in with Tonal's page transition.
- **The player bar plays a song**, "First Light", made in code by `music.js` (chords,
  bass, drums and bells on Web Audio, so there are no rights to clear). It goes through
  Tonal's six equalizer bands and a limiter; the waves along the bar follow its levels
  and settle when it pauses. The progress bar seeks, the heart rings the like bell, the
  volume slider is the master volume. Nothing makes a sound before the first press of
  play.
- **Living themes** (`scenes.js`): seven of the app's gallery looks painted live behind
  the window, with the app's palettes. Picking one on the Themes page restyles the
  window and its accent and is remembered. The Animations switch there stops all motion.
- **Now playing**: the cover in the bar (or L) opens a big cover and synced lyrics.
- **Keys**: Ctrl or Cmd K opens quick search, Space plays, 1 to 6 pick presets, the
  arrows step through pages.
- **Interface sounds**: soft taps on page changes, bubbles on presets, bells on like.
- **The title bar** matches the visitor's system: traffic lights on a Mac, buttons on
  the right on Windows, a close button on Linux.
- **404.html** is the window with nothing loaded and Tonal's error toast.

Home's feature list is a track list: hovering a row shows that screen.

The equalizer page is the app's own, with the same presets, the staggered springs,
the pops and the bubble notes (`main.js` ports `src/app/equalizer.rs` and
`src/sfx.rs`; keep them in step).

## Rules

- The owner's web-refs banned list (github.com/sjdsjadjajdjsa/web-refs, briefs/banned.md)
  applies: no glass pills, glow cards, fade-up on every section, middot chains or a
  centred hero with three cards.
- Headlines in Unbounded (`site/fonts`, subset to Latin, OFL); everything else in
  Inter, like the app. Inside the window everything follows `ui/theme.slint`.
- Dark only, with a soft glow behind the window in Tonal's colours: green `#1ed760`,
  violet `#8b7bff`, coral `#ff6b6b` and teal `#2ec4b6` from the Home mix tiles.
- Sizes come from the tokens at the top of `style.css`: spacing `--s1` to `--s9`, radii
  `--r-s` to `--r-xl`, type `--t-s` to `--t-xxl`. No other pixel values for spacing.
- Real screenshots of the app only. Every number has a source line under it (the memory
  chart comes from the README's benchmark table).
- Download links point at GitLab's "latest release" files, so nothing changes per
  release; the version line is fetched from the GitLab API.
- Motion: the window opens once, pages swap in, buttons press in, the bar's waves, the
  EQ demo. The play button and reduced motion stop all of it. On phones the sidebar
  becomes Tonal's bottom tab row and the window fills the screen; big screens show the
  same window larger. The download group for the visitor's system moves to the top.
- Check every page at 390, 1440 and 3440 wide before pushing: no sideways scroll,
  nothing outside the panel, no console errors.
- The share pictures in `og/` are screenshots of each page at 1200 by 630; take them
  again when a page changes a lot.
- The version line asks GitLab's API at most once an hour per visitor, so a busy day
  doesn't hit its rate limit.

## The domain

`tonalplayer.com` is a custom domain on the GitLab project (Deploy > Pages). Its DNS
has an `A` record for `@` to `35.185.44.232` and the `TXT` verification record GitLab
shows there.
