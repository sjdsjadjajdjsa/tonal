# Contributing

Thanks for helping. Issues and pull requests are welcome.

## Set up

Install [Rust](https://rustup.rs), [mpv](https://mpv.io) and [yt-dlp](https://github.com/yt-dlp/yt-dlp). On Debian or Ubuntu also install `libsqlite3-dev libdbus-1-dev libfontconfig-dev libxkbcommon-dev`.

```
cargo run --release
```

## Before you open a pull request

```
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests play short generated tones through mpv on a null output, so they are silent and need no network. `cargo run --example sources_smoke` and `cargo run --example integrations_smoke` check the live services by hand.

`tools/ui-smoke.sh` opens the real app on every main view and checks each one renders (CI runs it under Xvfb). To drive the app yourself, build with `--features debug-hooks` and use the `TONAL_DEBUG_*` variables described in [docs/architecture.md](docs/architecture.md).

## Guidelines

- Keep it light. Nothing may redraw or poll while the app is idle; measure CPU and memory before and after a change that could affect them.
- One topic per pull request, with a short scoped title like `fix(lyrics): ...` or `feat(player): ...`.
- Follow the module layout in [docs/architecture.md](docs/architecture.md).
- No comments that restate the code; explain only what isn't obvious.
