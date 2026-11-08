# tonalplayer.com

One page that feels like opening Tonal: plain HTML, CSS and a little JavaScript, no
build tools. GitLab Pages publishes it whenever a push to `main` changes `site/`,
`docs/images/` or the icon and fonts.

## Run it

```
sh site/build.sh /tmp/site && python3 -m http.server -d /tmp/site 8765
```

`build.sh` copies the page, the app icon, the Inter fonts and the screenshots from
`docs/images` into one folder. New screenshots in `docs/images` show up on the next
push.

## The idea

A music player that moves with your music. The page moves the same way the app does:
the wave from the player bar runs under the hero, and the equalizer demo is the app's
own, with the same presets, the staggered springs, the pops and the bubble notes
(`main.js` ports `src/app/equalizer.rs` and `src/sfx.rs`; keep them in step).

## Rules

- Dark only, with a soft glow in Tonal's colours: green `#1ed760` for Tonal itself,
  violet `#8b7bff`, coral `#ff6b6b` and teal `#2ec4b6` from the Home mix tiles. Grey
  tones match `ui/theme.slint`.
- Sizes come from the tokens at the top of `style.css`: spacing `--s1` to `--s9`, radii
  `--r-s` to `--r-xl`, type `--t-s` to `--t-xxl`. No other pixel values for spacing.
- Real screenshots of the app only. Every number has a source line under it (the memory
  chart comes from the README's benchmark table).
- Download links point at GitLab's "latest release" files, so nothing changes per
  release; the version line is fetched from the GitLab API.
- Motion: sections rise in once, the hero wave, button presses, the EQ demo. Reduced
  motion turns all of it off. Phones get one column; the download card for the
  visitor's system moves to the top.
- Check at 390, 1440 and 3440 wide before pushing: no sideways scroll, nothing outside
  its card.

## The domain

`tonalplayer.com` is a custom domain on the GitLab project (Deploy > Pages). Its DNS
has an `A` record for `@` to `35.185.44.232` and the `TXT` verification record GitLab
shows there.
