# Sofia webview status

Sofia's `Content::Html` currently uses GPUI Kit's HTML `TextView`. It renders
document markup but is not a browser and does not execute page JavaScript.

The attempted Linux WebKitGTK helper was removed at the user's request. Sofia
must not start Python, GTK, or an additional browser process for HTML windows.

## Available Rust paths

| Path | Linux Wayland in Sofia's GPUI layer | JavaScript |
| --- | --- | --- |
| GPUI Kit `TextView` HTML | Works now | No |
| GPUI Kit `gpui-wry` | Linux host unfinished | Yes on its supported platforms |
| Wry crate | Needs a GTK widget container on Wayland | Yes |
| Servo | Rust browser engine, but no ready GPUI layer integration in this project | Yes, with substantial engine integration |

GPUI Kit describes `gpui-wry` as experimental with macOS and Windows support;
its Linux example is unfinished. Wry documents that its generic child view is
X11 only on Linux and its Wayland path uses a GTK container. Therefore adding
Wry as a Rust dependency would still bring in GTK on Linux and would not fit
Sofia's existing raw Wayland layer surface. Servo is a Rust browser engine,
but using it here requires rendering, input, focus, resizing, and lifecycle
integration with GPUI before it can replace `TextView`.

Sources: [GPUI Kit WebView guide](https://gpui-kit.com/docs/webview/),
[Wry documentation](https://docs.rs/wry/latest/wry/),
[Servo repository](https://github.com/servo/servo).

The next implementation should wait for a browser engine that can be embedded
in GPUI's Wayland surface without GTK, or an explicit decision to develop a
Servo integration. Until then, HTML documents remain selectable static content
and Sofia launches no helper for them.
