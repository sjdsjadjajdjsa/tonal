# dmgbuild settings for the window tonal-macos-arm64.dmg opens to: Tonal on the left,
# Applications on the right, on packaging/macos/dmg-background.tiff (made by
# packaging/icon/make.py). Run from the repository root after build-app.sh:
#
#   dmgbuild -s packaging/macos/dmg.py Tonal tonal-macos-arm64.dmg
import os.path

app = "target/release/Tonal.app"

format = "UDZO"
filesystem = "HFS+"
files = [app]
symlinks = {"Applications": "/Applications"}
icon = os.path.join(app, "Contents", "Resources", "tonal.icns")

background = "packaging/macos/dmg-background.tiff"
window_rect = ((200, 160), (640, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
show_icon_preview = False
arrange_by = None
icon_size = 112
text_size = 13
icon_locations = {
    "Tonal.app": (160, 200),
    "Applications": (480, 200),
}
