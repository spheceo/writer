# dmgbuild settings for any spheceo app's install window, shared by Edit and Writer.
#   dmgbuild -s settings.py -D app=<X.app> -D background=<background.tiff> -D icon=<icon.icns> "<Name>" <out.dmg>
# The background comes from background.swift beside it; its layout matches the numbers here.
# Started from Edit's scripts/dmg/settings.py.
import os.path

app = defines["app"]
appname = os.path.basename(app)

format = "UDZO"
filesystem = "HFS+"
files = [app]
# Named " ": Finder writes names in dark grey over a background picture, so the background
# draws the Applications folder and its name itself, where they read on any art.
symlinks = {" ": "/Applications"}
icon = defines["icon"]
# No hide_extensions: it sets Finder info on the app, which breaks its signature's strict check
# (Finder hides ".app" anyway).

background = defines["background"]
# 400 pt of content plus the title bar.
window_rect = ((200, 140), (640, 428))
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
    appname: (160, 186),
    " ": (480, 186),
}
