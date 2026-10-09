"""Small Wayland layer host for Sofia's HTML documents (JSON lines on stdin)."""

import json
import os
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
gi.require_version("WebKit2", "4.1")
gi.require_version("GtkLayerShell", "0.1")
from gi.repository import Gdk, GLib, Gtk, GtkLayerShell, WebKit2


window = Gtk.Window()
window.set_decorated(False)
window.set_app_paintable(True)
window.set_visual(window.get_screen().get_rgba_visual())
GtkLayerShell.init_for_window(window)
GtkLayerShell.set_namespace(window, "sofia-webview")
GtkLayerShell.set_layer(window, GtkLayerShell.Layer.OVERLAY)
GtkLayerShell.set_exclusive_zone(window, -1)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.TOP, True)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.LEFT, True)
GtkLayerShell.set_keyboard_mode(window, GtkLayerShell.KeyboardMode.ON_DEMAND)

webview = WebKit2.WebView()
transparent = Gdk.RGBA()
transparent.parse("rgba(0,0,0,0)")
webview.set_background_color(transparent)
if os.environ.get("SOFIA_WEBVIEW_SMOKE"):
    webview.connect(
        "notify::title",
        lambda view, _: print(f"TITLE:{view.get_title()}", file=sys.stderr, flush=True),
    )
window.add(webview)
window.connect("destroy", Gtk.main_quit)


def receive(_source, condition):
    if condition & (GLib.IO_HUP | GLib.IO_ERR):
        Gtk.main_quit()
        return False
    line = sys.stdin.readline()
    if not line:
        Gtk.main_quit()
        return False
    try:
        command = json.loads(line)
        if command["type"] == "show":
            width = max(1, int(command["width"]))
            height = max(1, int(command["height"]))
            window.resize(width, height)
            webview.set_size_request(width, height)
            GtkLayerShell.set_margin(window, GtkLayerShell.Edge.LEFT, max(0, int(command["x"])))
            GtkLayerShell.set_margin(window, GtkLayerShell.Edge.TOP, max(0, int(command["y"])))
            webview.set_opacity(float(command.get("opacity", 1.0)))
            if "html" in command:
                webview.load_html(command["html"], None)
            window.show_all()
        elif command["type"] == "close":
            window.destroy()
            return False
    except (KeyError, TypeError, ValueError) as error:
        print(f"Sofia webview command: {error}", file=sys.stderr, flush=True)
    return True


GLib.io_add_watch(sys.stdin, GLib.IO_IN | GLib.IO_HUP | GLib.IO_ERR, receive)
Gtk.main()
