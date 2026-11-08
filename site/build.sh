#!/bin/sh
# Puts tonalplayer.com together in public/ (or the folder given): the page, the
# app's icon, fonts and screenshots.
set -eu
cd "$(dirname "$0")/.."
out=${1:-public}
# Where the site is served from: "/" on its own domain, "/tonal/" on GitLab's address.
base=${2:-/}
rm -rf "$out"
mkdir -p "$out/assets" "$out/fonts"
cp site/index.html site/404.html site/style.css site/main.js site/music.js site/scenes.js "$out/"
cp -R site/og "$out/og"
# Each page also lives at its own address, with its own title and share picture.
page() {
    mkdir -p "$out/$1"
    sed -e "s|<title>[^<]*</title>|<title>$2</title>|" \
        -e "s|og:url\" content=\"https://tonalplayer.com/\"|og:url\" content=\"https://tonalplayer.com/$1/\"|" \
        -e "s|rel=\"canonical\" href=\"https://tonalplayer.com/\"|rel=\"canonical\" href=\"https://tonalplayer.com/$1/\"|" \
        -e "s|og:title\" content=\"[^\"]*\"|og:title\" content=\"$2\"|" \
        -e "s|og:description\" content=\"[^\"]*\"|og:description\" content=\"$3\"|" \
        -e "s|/og/home.png|/og/$1.png|" \
        site/index.html > "$out/$1/index.html"
}
page equalizer "Equalizer, Tonal" "Press a preset and hear it: Tonal's equalizer, live in your browser."
page themes "Living themes, Tonal" "Pick a scene and watch the window turn into it."
page light "Light on your computer, Tonal" "Tonal uses 128 MB while playing. Spotify uses 1,282 MB."
page download "Download Tonal" "Free for Mac, Windows and Linux."
cp ui/icon.png docs/images/social-preview.png "$out/assets/"
cp ui/icon.png "$out/favicon.ico"
for image in hero themes-reel search album queue equalizer home mini; do
    cp "docs/images/$image.webp" "$out/assets/"
done
cp site/fonts/Unbounded.woff2 site/fonts/Unbounded-OFL.txt "$out/fonts/"
cp ui/fonts/Inter-Regular.ttf ui/fonts/Inter-Medium.ttf ui/fonts/Inter-SemiBold.ttf ui/fonts/Inter-Bold.ttf ui/fonts/Inter-LICENSE.txt "$out/fonts/"
if [ "$base" != "/" ]; then
    for html in $(find "$out" -name '*.html'); do
        sed -i.bak -e "s|href=\"/|href=\"$base|g" -e "s|src=\"/|src=\"$base|g" \
            -e "s|<meta name=\"base\" content=\"/\">|<meta name=\"base\" content=\"$base\">|" "$html" && rm "$html.bak"
    done
fi
# Browsers keep old copies of scripts, so every build names its own: ?v=<hash of the code>.
v=$(cat site/main.js site/music.js site/scenes.js site/style.css | { if command -v shasum >/dev/null; then shasum; else sha1sum; fi; } | cut -c1-10)
for html in $(find "$out" -name '*.html'); do
    sed -i.bak -e "s|style.css\"|style.css?v=$v\"|" -e "s|main.js\"|main.js?v=$v\"|" "$html" && rm "$html.bak"
done
sed -i.bak -e "s|\./scenes.js\"|./scenes.js?v=$v\"|" -e "s|\./music.js\"|./music.js?v=$v\"|" "$out/main.js" && rm "$out/main.js.bak"
echo "Built $out"
