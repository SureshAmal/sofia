# Model-generated visualization windows for Sofia

Research snapshot: 8 October 2026. Design and research only; no renderer or protocol code has been implemented.

## Decision

Give the model a versioned, declarative **Sofia Viz** format for the first release. It describes data, layout, theme roles, animation presets, and allowed user actions. `sofia_ui_layer` renders it with GPUI Kit charts, tables, controls, and custom GPUI painting. This lets a model present animated, interactive visualizations in a new `visualizerwindow` on both Windows and Wayland Linux without generating executable code.

Explore two optional richer renderers later:

1. **GPUI Shell scripts** can create native, HTML-like views with JavaScript state and event handlers. This is a promising way to let a model compose its own interface, but the 0.7.1 runtime is a preview, is unpublished as a standalone crate, and runs scripts inside the host process. Gate it behind an isolated renderer and a compatibility/security spike. It does not parse HTML, CSS, or DOM. [GPUI Shell introduction](https://gpui-kit.com/shell/), [capabilities](https://gpui-kit.com/shell/capabilities/).
2. **Real HTML/CSS/JavaScript** needs a browser engine. It is feasible on Windows through the experimental GPUI Kit WebView integration, but the released 0.7.1 Linux integration is unfinished. A later PR adds X11/XWayland hosting, not native Wayland layer-shell embedding. Treat this as an optional `webwindow` in a separate renderer/process after platform tests. [GPUI Kit WebView guide](https://gpui-kit.com/docs/webview/), [PR #3395](https://github.com/longbridge/gpui-kit/pull/3395).

The requested “70% opacity of video” is interpreted here as **visualization window transparency**. The default is a panel surface at 70% opacity over the desktop; data marks, text, controls, and focus indicators remain fully opaque for readability. The setting app exposes a user-controlled slider. If actual video playback opacity is intended, that is a distinct control to add when a video renderer exists.

## What each route can actually render

| Route | Model output | Animation and interaction | Windows | Linux Wayland pill client | Use |
| --- | --- | --- | --- | --- | --- |
| GPUI Kit `TextView::html` | Simple HTML rich text | Selection and links; no DOM/CSS/JS animation | Yes | Yes | Formatted explanatory text only |
| **Sofia Viz v1** | Validated JSON scene/data | Built-in chart appear motion, native transitions, hover/select/filter, custom painted diagrams | Yes, subject to a window prototype | Yes, subject to layer-shell prototype | Recommended default |
| GPUI Shell | JavaScript GPUI view description | Native motion, state and event handlers; no browser | Research gate | Research gate | Advanced native composition later |
| WebView | HTML/CSS/JS, SVG, Canvas | Full browser-style animation and interaction | Experimental WebView2 path | No released native Wayland child hosting; XWayland after PR #3395 | Optional rich content |

`TextView::html` is a rich-text renderer, **not** an embedded browser. GPUI Kit has built-in chart components and a lower-level Plot, plus custom path/shape painting for diagrams or unusual plots. Its chart appear animation respects reduced motion. A GPUI SVG element is normally tinted as an alpha mask, so arbitrary multicolor SVG should not be assumed to work as an HTML `<svg>` document in the native path. [TextView](https://gpui-kit.com/docs/components/text-view), [charts](https://gpui-kit.com/docs/components/chart), [Plot](https://gpui-kit.com/base/plot/), [painting](https://gpui-kit.com/docs/paint/), [images](https://gpui-kit.com/docs/image/), [0.7.1 release](https://github.com/longbridge/gpui-kit/releases/tag/v0.7.1).

## Sofia Viz v1: a small language the model can generate

Use a JSON schema rather than asking the model to write GPUI code. The model chooses from supported components: `line`, `bar`, `area`, `pie`, `radar`, `candlestick`, `sankey`, `table`, `metric`, `timeline`, `flow`, and `card`. `flow`, `timeline`, and cards need Sofia-owned GPUI layouts or painting. Every data field has a stable ID, display label, unit, and type. The model supplies content and intent; the UI owns geometry, colors, fonts, accessibility, animation timing bounds, and click behavior.

Illustrative contract, **not a final schema**:

```json
{
  "schema_version": 1,
  "title": "Tasks completed this week",
  "kind": "dashboard",
  "tags": ["visualizerwindow"],
  "panels": [
    {
      "id": "daily_tasks",
      "component": "bar",
      "title": "Completed tasks by day",
      "x": {"field": "day", "label": "Day", "type": "category"},
      "y": {"field": "count", "label": "Tasks", "unit": "tasks", "type": "number"},
      "rows": [{"id": "mon", "day": "Mon", "count": 3}, {"id": "tue", "day": "Tue", "count": 5}],
      "motion": {"preset": "reveal"},
      "actions": [{"event": "select", "action": "filter", "target": "task_table"}]
    },
    {"id": "task_table", "component": "table", "rows": []}
  ]
}
```

The user should be able to hover to inspect values, select bars/rows, filter a companion table, change a date range, pause motion, copy or export data, and use keyboard controls. Sofia implements a fixed action vocabulary (`select`, `filter`, `sort`, `drill_down`, `open_record`, `set_range`) and validates target IDs. Actions that modify notes, todos, or reminders go through the existing `sofia_mcp` entity service with user authorization and expected revisions; a visualization cannot directly write its own database. An `open_record` action resolves a record ID, not arbitrary model-supplied code or URL.

Use semantic theme colors from GPUI Kit. The model may choose a role such as `accent`, `positive`, or `warning`, but cannot inject raw colors that hide text or mimic system alerts. Limit panel count, rows, point count, nesting, text length, and update rate; larger data uses aggregation or virtualized `DataTable`. Give every chart a plain-language description and table fallback so the content remains understandable with reduced motion or assistive technology. [GPUI Kit charts](https://gpui-kit.com/docs/components/chart), [DataTable](https://gpui-kit.com/docs/components/data-table), [motion guide](https://gpui-kit.com/docs/animation/).

### Live updates and window ownership

The model or an MCP tool calls `sofia.ui.open_visualization` with a Sofia Viz document. `livesofia` validates tool authority and routes the intent over the existing UI IPC. The chosen UI client validates the schema and renderer capability, creates a window, and returns `window_id`, `client_id`, and revision. Later `sofia.ui.patch_visualization` calls include `window_id`, `expected_revision`, and typed operations such as `replace_rows`, `append_rows`, `set_filter`, or `set_title`. The client applies each accepted revision and emits `WindowChanged`, so a chart updates immediately. Define replay/snapshot behavior when a UI client reconnects.

The IPC contract should advertise `viz_schema_versions`, supported component types, maximum data size, interaction support, and whether Shell or WebView is available. A different UI client can then render the same Sofia Viz document in its own toolkit. `sofia_mcp` may expose the tool interface, while `livesofia` remains the authoritative UI IPC gateway. Persistent notes/todos/reminders stay in `sofia_mcp.db`; LLM history/traces stay in `livesofia.db`. Ephemeral visualization state belongs to the window registry. Save a visualization/report as a user document in `sofia_mcp.db` only when the user asks to keep it. See [architecture plan](ARCHITECTURE_PLAN.md) and [data contract](DATA_SYNC_REFERENCE.md).

## Animation and the pill morph

The new visualization window follows Sofia's existing pill-to-window transition. Create its shell at the pill anchor, animate bounds, clipping, and corner radius, and fade chart/text content in **during** expansion. When closing, fade content and shrink the shell into the pill. The pill itself stays visible and unchanged. Start chart appear motion only after the main geometry is sufficiently stable, or render it at low progress so motion does not restart when the shell finishes. Preserve animation IDs across updates so fresh data does not replay the entire chart. Respect reduced motion and stop requesting frames when idle. [GPUI Kit motion](https://gpui-kit.com/docs/animation/), [chart appear motion in 0.7.1](https://github.com/longbridge/gpui-kit/releases/tag/v0.7.1).

For a real WebView, the browser pixels may be in a native child surface with different clipping, z-order, and input behavior. Its pixels may not participate in the same GPUI shell morph or opacity. A viable fallback is to morph a GPUI preview/placeholder and reveal the WebView when its separate window is ready; closing reverses with a preview frame. This is a **proposed visual workaround**, not a proven platform feature. Test it on Windows and XWayland. The Wayland layer-shell pill and an XWayland browser window cannot be assumed to share a single native surface. [GPUI Kit WebView guide](https://gpui-kit.com/docs/webview/), [PR #3395](https://github.com/longbridge/gpui-kit/pull/3395).

## Transparency and the settings app

Use a transparent platform window and paint the visualization **surface** with the active theme's background color multiplied by a user opacity value. Default `0.70`; suggested settings range `0.40` to `1.00`, with a live preview, numeric percentage, and a `100%` shortcut. Keep text, plot marks, controls, tooltips, and focus rings at full alpha. This gives the requested see-through window while preserving readable data. GPUI exposes transparent window backgrounds; applying opacity to a whole parent element instead would also fade its children. Actual layer-shell transparency and any WebView composition must be checked on target desktops. [GPUI WindowOptions](https://docs.rs/gpui-pre/latest/gpui/struct.WindowOptions.html), [GPUI Kit styling](https://gpui-kit.com/docs/style/), [Wry WebView builder](https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html).

Proposed addition to `~/.config/sofia/setting.json` (and the Windows equivalent):

```json
{
  "ui": {
    "visualization_surface_opacity": 0.70,
    "reduced_motion": false
  }
}
```

The settings app owns this preference and broadcasts it to all visualization windows. A model or MCP tool cannot change the global value. An optional per-window opacity override can be offered to the **user** through a window menu; if implemented, it is stored as user UI state. Clamp invalid values, migrate missing values to `0.70`, and sample contrast against light and dark desktop backgrounds. For a true HTML window, CSS can paint a `rgba()`/alpha panel over a transparent page, but native WebView transparency and Windows nonzero alpha behavior are platform-dependent; the exact 70% appearance is a prototype gate, not a guarantee. [Wry WebView builder](https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html), [GPUI Kit WebView guide](https://gpui-kit.com/docs/webview/).

## Advanced path A: GPUI Shell

GPUI Shell is a JavaScript view-description layer over `gpui-base`. It supports state, controls, event handlers, native transitions and springs, and host-provided theme tokens. It could accept a generated `View` script to compose unusual native layouts while GPUI still renders and handles input. There is no HTML, DOM, CSS, browser canvas, or browser SVG. Built-in `gpui-component` charts are not automatically its JavaScript component library, so a chart bridge or custom Shell controls would have to be verified. [Introduction](https://gpui-kit.com/shell/), [API reference](https://gpui-kit.com/shell/api/), [examples](https://gpui-kit.com/shell/examples/).

The published docs call Shell an M0 preview with an unstable API, and the v0.7.1 source marks its crate `publish = false`. It should **not** be listed as an already available crates.io dependency for Sofia. If tried, pin the whole matching GPUI Kit source revision and test binary size, compilation, Windows/Linux behavior, event handling, and chart support. Shell's default Rust capabilities deny file, network, process, clipboard, and storage access, while manifest storage has a special default: explicitly set `storage: false`, prohibit fetched dependencies, and grant no host modules that reach Sofia data or UI commands. Generated scripts remain untrusted code running inside the host by default; isolation in a separate process is preferable before enabling them for model output. [Shell status](https://gpui-kit.com/shell/), [capability details](https://gpui-kit.com/shell/capabilities/), [v0.7.1 crate manifest](https://github.com/longbridge/gpui-kit/blob/v0.7.1/crates/shell/Cargo.toml).

## Advanced path B: actual HTML

A browser renderer lets the model produce self-contained HTML with CSS animation, SVG, Canvas, and limited interaction. The GPUI Kit 0.7.1 `gpui-wry` integration is experimental and documented for Windows/macOS, with Linux hosting unfinished. Post-release PR #3395 introduces `gpui-webview` and X11/XWayland Linux hosting, and notes that WebView input/focus testing is incomplete. It does not solve native Wayland child embedding. Keep this renderer an optional, separately packaged UI client so the core pill and Sofia Viz remain available everywhere. [WebView guide](https://gpui-kit.com/docs/webview/), [PR #3395](https://github.com/longbridge/gpui-kit/pull/3395), [GPUI Fast 0.1.3](https://github.com/longbridge/gpui-fast/releases/tag/v0.1.3).

The model-generated document must be treated as untrusted active content. Load from a controlled in-memory/custom origin, never directly from `file://` or an arbitrary remote URL. Use a restrictive Content Security Policy and navigation/request allowlists; disable external network, remote scripts/fonts, popups, downloads, clipboard, camera, microphone, and file access unless a specific user-approved use case requires them. Validate any JavaScript-to-host message against a narrow JSON schema and per-window capability grant. Prefer a separate renderer process so a browser crash or exploit does not share the `livesofia` process. These are **Sofia design requirements**, not protections GPUI Kit or Wry automatically supply. Wry offers HTML loading, navigation and IPC hooks that can be used to enforce parts of this policy; a full browser sandbox and transparency behavior still require platform tests. [Wry WebView builder](https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html), [Content Security Policy specification](https://www.w3.org/TR/CSP3/).

Where the task fits Sofia Viz, the model should generate the declarative format even if HTML is available. Reserve HTML for layouts/interactions outside the native schema and require an explicit renderer capability. Do not silently turn `TextView::html` content into active JavaScript.

## Research and implementation gates

1. **Native visualization prototype:** render one line or bar chart, one table, and a custom timeline from a validated Sofia Viz document in a GPUI window. Test selection, filtering, keyboard access, live row patches, and reduced motion.
2. **Transparency and morph prototype:** verify 70% surface alpha, fully opaque content, pill expansion/collapse, window focus, click regions, and contrast on Windows and at least GNOME, KDE, and a wlroots compositor. Check whether the desired visual can be delivered by the existing layer-shell backend.
3. **Protocol prototype:** use one GPUI client and one alternative/CLI client to negotiate the Viz schema, route a window to one owner, patch by expected revision, and recover a snapshot after disconnection.
4. **Shell feasibility:** compile the pinned source revision separately; prove a no-capability generated view with animation and one interactive control, then review process isolation and whether a useful chart bridge exists. Keep it experimental until these pass.
5. **HTML feasibility:** test Windows WebView2 and XWayland with a separate renderer process. Verify transparency, 70% panel alpha, clipping, focus, resize/morph handoff, IPC validation, CSP enforcement, and graceful fallback when WebView is unavailable. Do not advertise native Wayland HTML windows before that route exists.

This plan gives Sofia model-generated animated visuals on both target desktop platforms first, while leaving room for full HTML when its security and window-composition costs are justified.
