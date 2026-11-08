#!/bin/sh
# Puts tonalplayer.com together in public/ (or the folder given): the page, the
# app's icon, fonts and screenshots.
set -eu
cd "$(dirname "$0")/.."
out=${1:-public}
rm -rf "$out"
mkdir -p "$out/assets" "$out/fonts"
cp site/index.html site/style.css site/main.js "$out/"
cp ui/icon.png docs/images/social-preview.png "$out/assets/"
for image in hero themes-reel search album queue equalizer home mini; do
    cp "docs/images/$image.webp" "$out/assets/"
done
cp site/fonts/Unbounded.woff2 site/fonts/Unbounded-OFL.txt "$out/fonts/"
cp ui/fonts/Inter-Regular.ttf ui/fonts/Inter-Medium.ttf ui/fonts/Inter-SemiBold.ttf ui/fonts/Inter-Bold.ttf ui/fonts/Inter-LICENSE.txt "$out/fonts/"
echo "Built $out"
