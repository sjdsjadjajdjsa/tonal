# tonalplayer.com

One page that feels like opening Tonal: plain HTML, CSS and a little JavaScript, no
build tools. `build.sh` puts it together in one folder that any web server can serve. GitLab
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
web-refs): the sidebar is the navigation, each page swaps in with Tonal's own page
transition, and the player bar works. Play and pause stop every moving thing, the
progress bar is the page's scroll and can be dragged, the arrows step through the
pages, the heart rings the app's like bell and the volume slider sets the sound of the
equalizer. Home's feature list is a track list: hovering a row shows that screen.

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

## The domain

`tonalplayer.com` is a custom domain on the GitLab project (Deploy > Pages). Its DNS
has an `A` record for `@` to `35.185.44.232` and the `TXT` verification record GitLab
shows there.
