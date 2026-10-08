# GPUI Kit 0.7.1 reference for Sofia

Research date: 8 October 2026. This document describes the version pinned by this repository, `gpui-kit = 0.7.1`, and separates it from changes merged after that release. Findings were checked against the installed Rust crate with the `cratemd` MCP tool, the GPUI Kit source, and the linked upstream releases. This is a design reference; no UI code has been implemented here.

## How GPUI Kit is organized

`gpui-kit` is an application facade. It re-exports GPUI at `gpui_kit::*`, behavior and infrastructure at `gpui_kit::base`, styled components at `gpui_kit::component`, and default assets at `gpui_kit::assets`. Its default features are `component` and `assets`; `speech`, `inspector`, `profiler`, `test-support`, `decimal`, and Tree-sitter language features are opt-in. Version 0.7.1 pins the GPUI snapshot to `gpui-pre 0.3.8`. The `cratemd` feature inspection and [GPUI Kit README](https://github.com/longbridge/gpui-kit/blob/main/README.md) both support this layout; the [0.7.1 release](https://github.com/longbridge/gpui-kit/releases/tag/v0.7.1) states the snapshot pin.

The usual application lifecycle is: call `gpui_kit::application().run(...)`, then `gpui_kit::init(cx)` once, then `gpui_kit::open_window(...)`. `open_window` wraps app content in Base `Root`, which supplies overlay hosting and common window behavior. App state lives in GPUI `Entity` and `Global` values; a view implements `Render` and gets a `Window` plus `Context<Self>`. Update entities on the GPUI context and call `cx.notify()` when rendered state changes. Do not perform live audio/network/database work in `render`. [Getting started](https://gpui-kit.com/docs/getting-started/), [GPUI Kit source](https://docs.rs/gpui-kit/0.7.1/gpui_kit/).

Minimal shape, for reference:

```rust
use gpui_kit::*;

struct SofiaView;

impl Render for SofiaView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("Sofia")
    }
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|_| SofiaView)
        }).expect("open Sofia window");
    });
}
```

## Themes: use Kit's loader and registry

The earlier Sofia plan overstated the need for a custom theme system. GPUI Component already provides `Theme`, `ThemeRegistry`, theme JSON types, a schema, light/dark selection, system appearance synchronization, global refresh, and directory watching. Use these APIs for loading and applying themes. Sofia settings only need to persist the selected light/dark theme names and any Sofia-only preferences. [GPUI Kit theme guide](https://gpui-kit.com/component/theme/), [theme schema](https://github.com/longbridge/gpui-kit/blob/v0.7.1/.theme-schema.json).

| Capability | Existing API / behavior | Sofia use |
| --- | --- | --- |
| Current colors | `use gpui_kit::component::ActiveTheme as _; cx.theme().background`, `.foreground`, `.primary` | Style pill, cards, windows, and settings from active theme. |
| Signal colors | `cx.theme().green`, `.blue`, `.red` are available; so are semantic `.success`, `.info`, `.danger` | Keep user voice green, assistant blue, errors red across light/dark modes. |
| Theme registry | `ThemeRegistry::global(cx).themes()` and `sorted_themes()` | Populate a settings theme picker; select by the **theme entry name**. |
| Load and hot reload | `ThemeRegistry::watch_dir(path, cx, on_load)` | Point to Sofia's `~/.config/sofia/themes` directory on Linux, and the matching Windows app config directory. |
| Apply selected theme | `Theme::update(cx, |current| current.apply_config(&theme))` | Update active theme and refresh all windows through Kit's built-in path. |
| Mode and system | `Theme::change(...)`, `Theme::sync_system_appearance(...)` | Follow system or apply user-selected light/dark mode. |
| Rich backgrounds | Theme JSON supports solid colors and some two-stop `linear-gradient(...)` background tokens | Use `cx.theme().tokens.*.background` when a gradient is needed; direct color fields remain solid colors. |
| Schema / loading errors | `ThemeSet`/`ThemeConfig` derive `JsonSchema` and deserialize with Serde; malformed JSON is logged and skipped during directory reload | Use the published JSON schema in an editor; show a settings error if the user's file was ignored. |

**File format matters.** `ThemeRegistry::watch_dir` scans immediate `.json` files and parses each as a `ThemeSet` containing a top-level `themes` array. A bare single-theme object in `~/.config/sofia/themes/themename.json` will not load. This compact example has the required shape; unspecified colors inherit Kit defaults:

```json
{
  "$schema": "https://github.com/longbridge/gpui-kit/raw/refs/tags/v0.7.1/.theme-schema.json",
  "name": "Sofia",
  "themes": [
    {
      "name": "Sofia Dark",
      "mode": "dark",
      "colors": {
        "background": "#111111",
        "foreground": "#f5f5f5",
        "base.green": "#47c78a",
        "base.blue": "#60a5fa",
        "base.red": "#f87171"
      }
    }
  ]
}
```

`gpui_kit::init` installs the registry and default light/dark themes. To offer the other example themes from GPUI Kit's repository, bundle/copy their JSON files into a watched directory; being present upstream does not mean they are all automatically loaded into a Sofia binary. The registry handles file changes and refreshes active windows. Its loader checks JSON structure, but the code does **not** claim to enforce all visual contrast/accessibility rules; QA still has to check those. The current watcher logs invalid files and skips them rather than returning a UI error automatically. These details are from the installed `gpui-component 0.7.1` `theme/registry.rs` and `theme/schema.rs`, inspected with `cratemd`.

Sofia should not add a second theme parser or its own directory watcher. A small settings adapter can call the registry, remember selected theme names in `setting.json`, and display load errors to users. `ColorSelect` plus `ColorPickerState` can be used to edit a valid theme file; write it atomically and let the registry reload it. [ColorPicker docs](https://gpui-kit.com/component/color-picker/), [theme guide](https://gpui-kit.com/component/theme/).

## Component catalog in 0.7.1

The component layer has more controls than Sofia needs immediately. The following map groups the public catalog by what Sofia would build. `gpui_kit::component::<module>` is the import pattern; specific symbols are checked in the component docs or with `cratemd` before coding. [Official component index](https://gpui-kit.com/component/), [GPUI Kit API docs](https://docs.rs/gpui-kit/0.7.1/gpui_kit/component/).

| Area | Available components / primitives | Likely Sofia use |
| --- | --- | --- |
| Layout and navigation | Root View, Dock, Tabs, Sidebar, Resizable, Scrollable, VirtualList, Carousel, Breadcrumb, TitleBar, Toolbar, StatusBar | Settings navigation, multiple content panels, long history lists. |
| Text and editing | TextView, Input, Textarea, Editor, Label, Kbd, Clipboard, Message, MessageScroller, Bubble | Selectable streamed answers, search, editing notes, conversation view. |
| Forms and choices | Form, GroupBox, Checkbox, Radio, Switch, Toggle, Select, Combobox, NumberInput, Slider, Stepper, OtpInput, ColorPicker, ColorSelect, DatePicker, TimeField, Calendar, Questionnaire | Settings, theme editor, reminder/task forms. |
| Commands and actions | Button, DropdownButton, Command, Menu, Popover, Tooltip, HoverCard, Dialog, AlertDialog, Sheet | Pill actions, tool status, confirmations, commands. |
| Feedback | Alert, Notification, Progress, Spinner, Shimmer, Skeleton, Empty, Badge, Tag, Rating, Attachment, Avatar, Image, Marker | Connection/tool states, loading, errors, files. |
| Data | Plot, LineChart, BarChart, AreaChart, PieChart, RadarChart, CandlestickChart, SankeyChart, Table, DataTable, DescriptionList, Tree, Pagination | Token/runtime analytics and tool-produced visualizer windows. |
| Audio UI | `SpeechState`, `SpeechButton`, `SpeechWaveform`, `SpeechEvent`, audio/recognition traits | Optional dictation controls; waveform or its ideas may inform the pill. |

GPUI Base adds the reusable behavior behind several components: selectable rich text, virtual lists, motion, focus, overlay/root handling, layout, and testing support. This is why GPUI Kit should remain Sofia's first UI dependency instead of mixing separate table, Markdown, chart, and animation libraries. [GPUI Kit architecture](https://github.com/longbridge/gpui-kit/blob/main/docs/ARCHITECTURE.md).

## Speech in 0.7.1: useful, with a boundary

Enable `gpui-kit = { version = "0.7.1", features = ["speech"] }` to get the default microphone and supported system recognizer. `SpeechState` owns a *dictation* session: `AudioInput` captures, `SpeechRecognizer` transcribes, `SpeechEvent` emits partial/final text, `SpeechButton` toggles capture, and `SpeechWaveform` displays input levels. Windows uses the system recognizer if available; Linux requires an application recognizer. GPUI Kit's built-in Linux microphone uses ALSA and the `speech` feature brings in `cpal`. [Speech docs](https://gpui-kit.com/component/speech/), [0.7.1 release](https://github.com/longbridge/gpui-kit/releases/tag/v0.7.1).

`SpeechWaveform` is a scrolling **amplitude level** view, with a sample about every 80 ms. It is not an FFT frequency spectrum and does not render assistant output or tool states by itself. Sofia's full-duplex Gemini Live conversation, barge-in, playback, and tool scheduling should remain in `livesofia`. The pill can either consume level events from `livesofia` and draw a custom FFT, or adapt/extend the waveform presentation if level bars are enough. Avoid running an independent `SpeechState` microphone beside `livesofia` merely to animate the pill; that would duplicate capture and can cause device/echo problems. Use the speech controls in settings or text dictation only if they add a distinct user action. [Speech docs](https://gpui-kit.com/component/speech/).

## Markdown, charts, and motion

**Markdown and selectable text.** `TextView::markdown(...)` / the `markdown(...)` helper render rich text; selection is enabled by default. `TextViewState` supports repeated `set_text` updates for streamed responses, source-to-rendered range mapping, and bounded/scrollable code blocks. Use a stable state entity for incremental assistant text in `markwindow` and the response card. Simple HTML can render through `TextView::html`, but this is not a browser. [TextView docs](https://gpui-kit.com/docs/components/text-view), [0.7.1 Markdown changes](https://github.com/longbridge/gpui-kit/releases/tag/v0.7.1).

**Charts and tables.** Built-in line, bar, area, pie, radar, candlestick, and Sankey charts cover most `visualizerwindow` content. In 0.7.1 the built-in charts draw in on first paint and respect reduced motion. Use `.appear(false)` to disable that animation, `.appear_key(...)` to replay it, and `PlotAppearScope` to preserve completion through virtual-list remounts. `DataTable` supplies sortable, virtualized rows, selection, and resizable columns. Feed charts structured data with units and labels, and use a table fallback when a requested chart type is unsupported. [Charts](https://gpui-kit.com/docs/components/chart), [DataTable](https://gpui-kit.com/docs/components/data-table), [0.7.1 release](https://github.com/longbridge/gpui-kit/releases/tag/v0.7.1).

**Motion.** GPUI Kit offers basic element animation, GPUI Base keyed `transition`/`spring`/presence, and component motion tokens. For Sofia's pill-to-window morph, use stable spring channels for shell bounds/radius and a transition for content opacity; reverse or retarget on interruption. Respect `cx.reduce_motion()` and only request frames while motion is active. Chart appear motion is separate from the window morph. [Animation guide](https://gpui-kit.com/docs/animation/).

**Model-generated visualization.** Use a validated, versioned Sofia Viz document to compose charts, tables, custom painted diagrams, animations, and typed interactions inside `visualizerwindow`. GPUI's transparent window background plus a theme surface color at 70% alpha can make the panel translucent without fading text and chart marks. `TextView::html` remains a simple rich-text renderer. `gpui-shell` is a separate JavaScript-based GPUI view runtime that could later provide native model-composed UI, but its 0.7.1 M0 preview is unpublished and is not a browser. See [generative visualization research](GENERATIVE_VISUALIZATION_RESEARCH.md), [GPUI Shell](https://gpui-kit.com/shell/), and [WindowOptions](https://docs.rs/gpui-pre/latest/gpui/struct.WindowOptions.html).

## WebView and `gpui-fast`: exact release boundaries

The [GPUI Kit 0.7.1 release](https://github.com/longbridge/gpui-kit/releases/tag/v0.7.1) predates [PR #3395](https://github.com/longbridge/gpui-kit/pull/3395), merged on 7 October 2026. In the published 0.7.1 tree, the experimental browser integration is `gpui-wry`; the [versioned WebView guide](https://gpui-kit.com/docs/webview/) describes macOS/Windows support and unfinished Linux hosting. Do not present post-release main-branch behavior as already available from Sofia's pinned crate.

PR #3395 renames the integration to `gpui-webview` and adds Linux **X11/XWayland** child WebView hosting through WebKitGTK. It explicitly says native Wayland cannot embed that GTK child into another client's `wl_surface`. On Linux, its example starts GPUI in X11 mode (`WindowingModes::X11`), including when running under a Wayland desktop via XWayland. The PR's own test report says input/focus transfer inside the WebView was not yet tested. Windows WebView2 still covers GPUI overlays in that PR. [PR #3395](https://github.com/longbridge/gpui-kit/pull/3395).

[GPUI Fast 0.1.3](https://github.com/longbridge/gpui-fast/releases/tag/v0.1.3) is a separate GPUI runtime/renderer package. It improves overlay blending over native content on X11 when a compositor is present, and the PR uses it for WebView composition. Its release says native-content window composition is supported across macOS, Windows, and Linux, but that does not mean GPUI Kit 0.7.1 automatically gets the feature or that a native Wayland WebView is solved. GPUI Kit 0.7.1 is pinned to `gpui-pre 0.3.8`; do not add `gpui-fast` as a second `gpui` dependency without an explicit compatible build, since two GPUI package families can produce incompatible `Window`/`Entity` types. This compatibility concern is an architectural inference from the two dependency families.

For Sofia, keep Markdown and simple HTML in `TextView` on the Wayland layer-shell client. Treat a true `webwindow` as a separate, optional renderer: on Linux it may need an XWayland window/process, which cannot be the same native Wayland layer-shell surface as the pill. On Windows, test native WebView2 overlay and input order. When selecting a newer GPUI Kit release or the PR branch, pin every matching GPUI dependency and rerun the platform matrix before enabling it. [PR #3395](https://github.com/longbridge/gpui-kit/pull/3395), [GPUI Fast 0.1.3](https://github.com/longbridge/gpui-fast/releases/tag/v0.1.3).

## Sofia implementation map

| Sofia area | Prefer from GPUI Kit 0.7.1 | Additional work still needed |
| --- | --- | --- |
| Pill | GPUI window/painting, active theme colors, Base motion | Custom layer-shell/Windows overlay positioning, drag, FFT, and IPC state binding. |
| Morphing windows | `open_window`, `Root`, Base spring/transition | Cross-surface geometry/input prototype on each platform. |
| Notes and responses | `Textarea`/`TextareaState` for editing Markdown, `Input` for titles, `TextView` for selectable preview | MCP-owned records; `livesofia` relays committed `EntityChanged` updates to the versioned window registry. See [data and sync contract](DATA_SYNC_REFERENCE.md). |
| Charts and analytics | Charts, `DataTable`, `PlotAppearScope` | Usage aggregation and chart-spec validation. |
| Separate Sofia Trace app | `DataTable`, charts, `TextView`, navigation and split layout components | Dedicated sessions/traces/tools/logs/reports desktop client over `livesofia` IPC; see [observability plan](OBSERVABILITY_REPORT.md). |
| Settings | Form controls, `ColorSelect`, `ThemeRegistry`, `Theme` | Preference storage and theme load error display; no second theme engine. |
| Speech UI | `SpeechWaveform`/`SpeechButton` where appropriate | Gemini Live transport/audio remains in `livesofia`; custom FFT if required. |
| Browser window | `gpui-wry` only where published support is proven; future `gpui-webview` branch/release | XWayland/Windows integration tests and separate Wayland behavior. |

Before implementation, use GPUI Kit's headless `test-support` for component/state tests and real desktop smoke tests for layer-shell, WebView, focus, audio devices, and window morphing. Headless tests cannot prove native compositor behavior. [Testing guide](https://gpui-kit.com/docs/testing/), [WebView guide](https://gpui-kit.com/docs/webview/).
