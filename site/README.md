# tonalplayer.com

One page that feels like opening Tonal: plain HTML, CSS and a little JavaScript, no
build tools. The themes come from `scenes.wasm`, Tonal's own scene painter built by
`site/wasm/build.sh` (run it after changing `src/scenes`; the built file is checked in). `build.sh` puts it together in one folder that any web server can serve. Point the
server's 404 page at `404.html`. tonalplayer.com runs on the owner's own server.

## Run it

```
sh site/build.sh /tmp/site && python3 -m http.server -d /tmp/site 8765
```

`build.sh` copies the page, the app icon, the Inter fonts and the screenshots from
`docs/images` into one folder. New screenshots in `docs/images` show up on the next
push.

## The idea

The page is the app. The whole site is Tonal in full screen, filling the browser (after
Poolsuite in web-refs), and it behaves like Tonal:

- **Pages** have real addresses (`/equalizer/` and so on, each with its own title and
  share picture in `og/`) and swap in with Tonal's page transition.
- **The player bar plays a song**, "First Light", made in code by `music.js` (a 20-bar
  waltz for harp and ocarina, with strings, soft drums, a room reverb and an echo, on Web
  Audio, so there are no rights to clear). It goes through
  Tonal's six equalizer bands and a limiter; the waves along the bar follow its levels
  and settle when it pauses. The progress bar seeks, the heart rings the like bell, the
  volume slider is the master volume. Nothing makes a sound until the visitor clicks
  something; from then on presets bubble whether or not the song plays.
- **Living themes** (`scenes.js` and `scenes.wasm`): the app's whole gallery, painted live
  by the app's own code, with the app's previews on the cards and a "Your colors" palette
  with five pickers. Picking one restyles the window and its accent and is remembered.
  The Animations switch there stops all motion.
- **Now playing**: the cover or the lyrics button in the bar (or L) opens a big cover and
  synced lines. The song has no words, so the lines name what joins the mix. The equalizer
  button in the bar opens the equalizer and a second press goes back.
- **Keys**: Ctrl or Cmd K opens quick search, Space plays, 1 to 6 pick presets, the
  arrows step through pages.
- **Interface sounds**: soft taps on page changes, bubbles on presets, bells on like.
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
- Dark only. Tonal's colours: green `#1ed760`,
  violet `#8b7bff`, coral `#ff6b6b` and teal `#2ec4b6` from the Home mix tiles.
- Sizes come from the tokens at the top of `style.css`: spacing `--s1` to `--s9`, radii
  `--r-s` to `--r-xl`, type `--t-s` to `--t-xxl`. No other pixel values for spacing.
- Real screenshots of the app only. Every number has a source line under it (the memory
  chart comes from the README's benchmark table).
- Download links point at GitHub's "latest release" files, so nothing changes per
  release; the version line is fetched from the GitHub API.
- Motion: the window opens once, pages swap in, buttons press in, the bar's waves, the
  EQ demo. The play button and reduced motion stop all of it. On phones the sidebar
  becomes Tonal's bottom tab row and the window fills the screen; big screens show the
  same window larger. The download group for the visitor's system moves to the top.
- Check every page at 390, 1440 and 3440 wide before pushing: no sideways scroll,
  nothing outside the panel, no console errors.
- The share pictures in `og/` are screenshots of each page at 1200 by 630; take them
  again when a page changes a lot.
- The version line asks GitHub's API at most once an hour per visitor, so a busy day
  doesn't hit its rate limit.

## The domain

`tonalplayer.com` is served from the owner's own server. Update it by building again and
copying the new folder there.
