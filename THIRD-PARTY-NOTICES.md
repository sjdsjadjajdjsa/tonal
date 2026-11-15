# Third-party notices

Tonal itself is MIT licensed ([LICENSE](LICENSE)). The downloads ship these programs and files alongside it. They run as separate programs and keep their own licenses.

| Component | Where | License | Source |
| --- | --- | --- | --- |
| [mpv](https://mpv.io) | `Tonal.app/Contents/Helpers/mpv` in the macOS download | GPL-2.0-or-later ([LICENSE.GPL](https://github.com/mpv-player/mpv/blob/master/LICENSE.GPL), [Copyright](https://github.com/mpv-player/mpv/blob/master/Copyright)) | <https://github.com/mpv-player/mpv>, the project's own macOS build (the exact release is named in `Tonal.app/Contents/Resources/MPV-VERSION.txt`) |
| [mpv](https://mpv.io) | `mpv.exe` in the Windows downloads | GPL-2.0-or-later ([LICENSE.GPL](https://github.com/mpv-player/mpv/blob/master/LICENSE.GPL), [Copyright](https://github.com/mpv-player/mpv/blob/master/Copyright)) | <https://github.com/mpv-player/mpv>, built by <https://github.com/shinchiro/mpv-winbuild-cmake> (the exact release is named in `MPV-VERSION.txt` next to `mpv.exe`) |
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | `yt-dlp` in the AppImage and Linux archive, `yt-dlp.exe` on Windows; on macOS Tonal downloads the official `yt-dlp_macos.zip` build on first launch when yt-dlp isn't installed | Unlicense | <https://github.com/yt-dlp/yt-dlp> |
| [Inter](https://rsms.me/inter/) | Built into the program (subset) | SIL Open Font License 1.1 ([ui/fonts/Inter-LICENSE.txt](ui/fonts/Inter-LICENSE.txt)) | <https://github.com/rsms/inter> |
| [SQLite](https://sqlite.org) | Built into the release programs | Public domain | <https://sqlite.org> |

The Windows and macOS downloads include the full GPL text as `mpv-LICENSE.GPL.txt` (on macOS also inside `Tonal.app/Contents/Resources`). Rust crate licenses are listed in `Cargo.lock`; all are MIT, Apache-2.0, BSD, ISC, Zlib, Unicode or similar permissive licenses. The interface uses [Slint](https://slint.dev) under the [Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0](https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md); the "Made with Slint" badge in the README is the disclosure it asks for.
