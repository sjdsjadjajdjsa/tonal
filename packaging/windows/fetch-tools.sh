#!/bin/sh
# Downloads mpv.exe and yt-dlp.exe into the folder given as the first argument.
# A GITHUB_TOKEN variable, when set, signs the request so shared runners don't hit
# GitHub's rate limit. Needs curl and 7z.
set -eu
dest=$1
mkdir -p "$dest"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL -o "$dest/yt-dlp.exe" https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe

api=https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases/latest
if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fsSL -H "User-Agent: tonal-release" -H "Authorization: Bearer $GITHUB_TOKEN" "$api" > "$tmp/release.json"
else
    curl -fsSL -H "User-Agent: tonal-release" "$api" > "$tmp/release.json"
fi
url=$(grep -o '"browser_download_url": *"[^"]*/mpv-x86_64-[0-9]\{8\}-git-[0-9a-f]*\.7z"' "$tmp/release.json" | head -1 | sed 's/.*"\(https[^"]*\)"/\1/')
page=$(grep -o '"html_url": *"[^"]*/releases/tag/[^"]*"' "$tmp/release.json" | head -1 | sed 's/.*"\(https[^"]*\)"/\1/')
[ -n "$url" ] || { echo "No mpv build found in the latest mpv-winbuild-cmake release" >&2; exit 1; }
name=${url##*/}
curl -fsSL -o "$tmp/$name" "$url"
7z x "$tmp/$name" -o"$tmp/mpv" -y > /dev/null
cp "$tmp/mpv/mpv.exe" "$dest/"
curl -fsSL -o "$dest/mpv-LICENSE.GPL.txt" https://raw.githubusercontent.com/mpv-player/mpv/master/LICENSE.GPL
printf '%s from %s\n' "$name" "$page" > "$dest/MPV-VERSION.txt"
echo "Bundled $name and yt-dlp"
