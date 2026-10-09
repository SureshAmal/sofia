# Sofia web windows: browser engine integration

## Decision

`Content::Html` currently goes through GPUI Kit `TextView` in
`crates/sofia_ui_layer/src/content_windows.rs`. That displays HTML source; it
does not run CSS layout, JavaScript, DOM events, canvas, or browser animations.
Replace this path with a real browser engine. Keep notes, todos, reminders,
and charts in the existing GPUI content window system.

| Platform | Host | Browser engine | Window integration |
| --- | --- | --- | --- |
| Linux Wayland | A small `sofia-webview` helper process using GTK 3 and `gtk-layer-shell` | WebKitGTK 4.1 | One Wayland layer surface per open web document, positioned beside the pill |
| Windows | A GPUI-owned content window with `gpui-wry` | WebView2 through Wry | Native child view inside the GPUI window |

The Linux helper is necessary because Wry's child view cannot be embedded in
GPUI's independently owned raw Wayland layer surface. Wry's Wayland route
requires a GTK-owned widget container. XWayland would make the web window an
ordinary X11 window, losing the layer behavior Sofia uses. GPUI Kit documents
its `gpui-wry` example as macOS/Windows only; the Linux path is unfinished.
See the [GPUI Kit WebView guide](https://gpui-kit.com/docs/webview/) and
[Wry's platform notes](https://github.com/tauri-apps/wry#platform-considerations).

`gtk-layer-shell` creates a true Wayland layer surface from a GTK window,
including edge anchors, margins, layer choice, and keyboard mode. Its
[example](https://github.com/wmww/gtk-layer-shell/blob/master/examples/simple-example.c)
shows the initialization order. WebKitGTK's `load_html` runs HTML, CSS, and
JavaScript in that GTK window; it accepts a base URI for relative resources.
See the [WebKitGTK API](https://webkitgtk.org/reference/webkit2gtk/stable/class.WebView.html)
and [`load_html`](https://webkitgtk.org/reference/webkit2gtk/2.37.91/method.WebView.load_html.html).

## Process and document flow

1. `sofia-mcp` saves an HTML document in the existing content database. Its ID,
   revision, title, tags, and open state remain the source of truth.
2. `sofia-ui-layer` observes content changes as it does today. For an open
   `Content::Html`, it sends the document and target geometry to the webview
   host. It does not render HTML through `TextView`.
3. The host creates or reuses a browser view keyed by document ID, loads the
   HTML, and sends `ready`, `load_failed`, and `closed` events back. A revision
   update reloads the existing view without creating another window.
4. Closing the document via its icon or MCP closes the browser view and updates
   the same database open state. Reopening restores the saved HTML.

Use a narrow local IPC protocol: `Open {id, revision, html, rect, opacity}`,
`Update {id, revision, html}`, `SetRect {id, rect}`, `SetOpacity {id, opacity}`,
`Close {id}`, plus host status events. Geometry is in monitor logical
coordinates with an explicit monitor ID and scale. The host owns GTK/WebKit;
GPUI never calls GTK from its render thread. The helper must detect parent
disconnect and close its surfaces.

## Presentation

Keep Sofia's pill as the existing GPUI layer. Use the current GPUI morph
rectangle as the opening and closing transition. When it reaches the target
size, reveal the matching WebKit layer surface and fade its content in. On
close, hide the browser surface before the rectangle shrinks to the pill.
Wayland does not allow GPUI to clip or transform another process's surface as
one texture, so claiming a continuous browser-pixel morph would be misleading.

The webview surface should use a transparent GTK window and transparent
WebKit background; the document itself controls its background. Apply the
user's opacity setting to the surface and forward changes immediately. The
host must update position and size when the pill moves, the display changes,
or the document dimensions change. Keyboard focus goes to the browser when
the user interacts with it and returns to the pill on close.

## Browser behavior and boundaries

- Allow ordinary DOM events, CSS animations, `requestAnimationFrame`, canvas,
  scrolling, selection, and keyboard/IME input in the engine.
- Load user HTML as a document. Resolve relative assets only against an
  explicit, controlled base URI. Decide whether external HTTPS resources are
  enabled in settings; never silently grant local file access.
- Do not expose Sofia's MCP, database, credentials, or system IPC directly to
  page JavaScript. A future page-to-Sofia bridge must use explicit, validated
  message types.
- Handle navigation, popups, downloads, permissions, renderer crashes, and
  failed loads visibly rather than replacing the window with source text.
- An LLM edit increments the document revision. Reload the existing page for
  complete HTML replacements. A later optional patch command can update DOM
  elements without resetting page state, but the first implementation should
  guarantee that complete revisions render correctly.

## Implementation order and acceptance

1. Add a Linux `sofia-webview` crate with GTK 3, WebKitGTK 4.1, and
   `gtk-layer-shell`, plus a small versioned IPC contract shared with
   `sofia-ui-layer`. This Fedora system has GTK 3, WebKitGTK 4.1, and the
   `gtk-layer-shell` runtime; `gtk-layer-shell-devel` is still required to
   compile the bindings.
2. Run an independent WebKitGTK layer-surface smoke test on the active
   Hyprland session: styled page, JavaScript click counter, CSS animation,
   text input, resize, opacity, and transparent background.
3. Wire open/update/close and geometry events from the existing content
   manager. Keep a single browser view per document ID and report failures.
4. Add the Windows `gpui-wry` host and test WebView2 child bounds, focus,
   opacity, and lifecycle on Windows. GPUI Kit warns that native child views
   can paint above GPUI overlays, so use an explicit reveal/hide handoff for
   the morph.
5. Verify with MCP-created HTML, then MCP edits and closes: no refresh button,
   no duplicate windows, and no stale page after reopening. Test multiple
   web documents, monitor scale changes, and browser-process recovery.

This is a platform integration task. A GPUI HTML renderer or a screenshot of a
browser would leave JavaScript interaction, selection, keyboard input, and
animation incomplete.
