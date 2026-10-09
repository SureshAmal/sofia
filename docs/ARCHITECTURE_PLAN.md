# Sofia desktop voice assistant — research and architecture plan

Research snapshot: 8 October 2026. This is a planning document; no implementation is included.

## Goal and starting point

Build a Windows and Linux desktop voice assistant in Rust with GPUI Kit. The first live model is `gemini-3.8-live`. A Google OAuth refresh token has now been tested successfully with the Vertex Live WebSocket. Sofia should offer an operator-managed connection so ordinary users do not configure Google Cloud or sign in, plus a user-supplied OAuth credential option in settings. The interface has a draggable voice pill, a larger response card, and additional windows that tools can open for useful content. Conversation history, memory, usage, and app settings survive restarts.

The root is now a virtual Cargo workspace. `crates/livesofia` contains the IPC daemon, refreshable Google OAuth token source, transcript reducer, Live probes, microphone capture, and speaker playback. The daemon connects to `gemini-3.8-live`, accepts text and voice controls through IPC, and streams state and transcript events; see the [IPC Live test report](IPC_LIVE_TEST_REPORT.md) and [voice audio test report](VOICE_AUDIO_TEST_REPORT.md). Separate crate boundaries exist for the UI layer, MCP, settings, tracing app, and shared protocol. There is no UI, MCP host, persistence, or settings app implementation yet. The planned operator-managed service still protects shared provider credentials in a deployment where end users need no Google login.

The supplied images suggest two distinct states: a narrow vertical pill with an indicator and waveform bars, and a wider card with the waveform above selectable answer text. Treat their coral color as a design reference; the actual signal colors below are driven by state and theme.

## Recommended architecture

Keep the four core product parts as Rust workspace crates, plus a small shared contract crate and a separate `sofia_trace` desktop client crate. Make `livesofia` the local IPC server and source of truth for live state, its own LLM history/memory/usage/traces database, and window intents. The GPUI pill UI, tracing app, and other authorized UI clients connect to it through the same versioned UI protocol. `sofia_mcp` separately owns the user-content database and a typed data service. `livesofia` routes authorized data requests and relays committed change events for UI clients; it does not own or migrate MCP records.

```text
microphone -> liveSofia -> provider adapter -> Sofia credential service -> Gemini 3.8 Live
                   |  |             |
                   |  |             +-> model tool calls
                   |  +-> livesofia.db: history, memory, usage, trace reports
                   +-> state/audio events -> local IPC -> GPUI UI / other UI clients
                   +-> tool router <-> Sofia MCP <-> OS tools / other MCP servers
                                      |       +-> sofia_mcp.db: notes, todos, reminders
                                      |       +-> optional PostgreSQL sync
                                      +-> UI window commands -> Sofia UI layer

Sofia settings <-> config/theme store, service connection, livesofia analytics,
                  Sofia MCP content sync settings/status
Sofia Trace    <- live activity/trace queries <- livesofia IPC
```

| Part | Owns | Public boundary |
| --- | --- | --- |
| `livesofia` | Provider adapters, audio input/output, live session lifecycle, turn state, tool dispatch, usage and trace middleware, history and memory retrieval, optional summarizer | Typed commands/events, activity reports, and provider capability descriptions |
| `sofia_ui_layer` | GPUI Kit app, pill and cards, window registry, animations, selection, charts, platform window adapters | Versioned UI commands/events; no provider credentials |
| `sofia_mcp` | MCP server/client integration, user-content SQLite database, note/todo/reminder CRUD and search, reminders, optional PostgreSQL sync, OS tools, discovery and action policy | Typed data service and MCP tools/resources plus bridge to a small UI window API |
| `sofia_setting` | Settings window, config validation/migration, theme editor, service connection diagnostics, analytics views | Versioned settings API and read-only analytics queries |
| `sofia_trace` | Separate GPUI Kit desktop app for sessions, traces, tool calls, logs/errors, and detailed reports | Read-only trace/data queries, live activity subscription, and window navigation through `livesofia` IPC |
| `sofia_protocol` | Shared IDs, state events, window content schemas, capability/version negotiation | Stable serialized types used by all four parts |

Avoid making MCP the internal transport for every audio or animation update. High-rate FFT frames stay on the local UI event channel. MCP is for tool discovery/invocation and optional UI commands from tools. A third-party MCP server should see only explicitly exposed Sofia UI tools, not unrestricted process internals. The operator credential must remain on a Sofia-controlled service; a public desktop binary or local config cannot protect a shared long-lived key.

## 1. `livesofia`: live voice and memory

1. Define a `LiveProvider` interface with `connect`, `send_audio`, `send_text`, `cancel_or_interrupt`, `send_tool_result`, `events`, and `capabilities`. Capabilities state whether a provider supports audio, text transcription, async tools, resumable sessions, usage metadata, and cached tokens. Ship `GeminiLiveAdapter` first; leave other providers as later adapters.
2. Use an operator-managed credential path by default for users who should not log in to Google. The local development path now exchanges a provided OAuth client ID/secret and refresh token for an access token, caches it, and refreshes on reconnect; this path passed a Gemini 3.8 Live Vertex WebSocket test. The settings app can offer user-supplied OAuth credentials with secrets held in the OS credential store. Keep any shared operator credentials on a Sofia-controlled service, never in the distributed desktop app. See [test report](LIVE_MODEL_TEST_REPORT.md).
3. Use native live audio: microphone capture, device selection, echo cancellation/noise handling as available, bounded send queues, 24 kHz PCM output playback, interruption/barge-in, device disconnect recovery, and a visible mic mute state. The first implementation captures default-device audio as mono signed 16-bit PCM at its native rate, which `gemini-live` labels for server-side resampling, and plays model PCM through Rodio. The output device is selectable through IPC and stored in `setting.json`; input device selection remains planned. Input and output audio transcription are enabled for UI updates. The selected Gemini model requires `AUDIO` output; text on screen comes from output audio transcription. The daemon streams `assistant_text_delta` and `assistant_text_final` through IPC. Echo control, device-disconnect recovery, and full voice-turn validation remain to be built.
4. Make a turn reducer the source of truth for `Disconnected`, `Connecting`, `Ready`, `Listening`, `Thinking`, `Speaking`, `ToolQueued`, `ToolRunning`, `Reconnecting`, and `Error`. Emit timestamps, session/turn IDs, and diagnostic codes. Handle multiple content parts in one provider event, async function calls, tool cancellation, and interrupted speech. Distinguish transport reconnect from a new conversation.
5. Route each model function declaration to a specific approved tool. Validate arguments, use timeouts/cancellation, record start/end/result, and return exactly one result per call ID. Track long-running tools independently so speech can continue where the provider permits.
6. Persist transcripts, tool-call identity/status/timing and safe result summaries, model/provider, and usage events in `livesofia.db` before building summaries. Full MCP note/todo/reminder bodies remain in `sofia_mcp.db`; trace rows reference record IDs/revisions rather than duplicating them. Store durable LLM memory as explicit facts/preferences with provenance, timestamps, confidence, and delete controls. At session start, retrieve relevant recent turns and memory within a token budget. Full history remains searchable in the LLM database; the model cannot load all history into a finite context window at once.
7. Add optional summarization middleware after the basic history path works. Use a separately configured text model, never overwrite raw turns, store summary version/source turn range, refresh summaries incrementally, and count summary-model tokens separately. If summarization fails, continue with recent turns and retrieved memory.
8. Record provider usage metadata when sent, with input, output, total, and cached token fields nullable. Keep estimated counts in separate fields if the provider omits them. Attribute usage to provider/model/session/turn and optional tool or summarization operation. Never infer billable cost from total tokens alone.
9. Record a compact trace for each turn or assistant task: model activity, memory retrieval, tool attempts, progress, policy decisions, errors, reconnects, and usage links. Stream `ActivityChanged` to the separate `sofia_trace` desktop client and persist safe spans/events for later reports. The pill keeps only minimal status. See [observability research and plan](OBSERVABILITY_REPORT.md).

Google documents 16 kHz PCM input, 24 kHz PCM output, transcription, session resumption, and usage metadata for the Live API. The current `gemini-live` crate includes reconnection, resumption, and typed tool events, but its published release predates Gemini 3.8; validate wire support and patch or replace the adapter if a new event/configuration field is missing. Sources: [Gemini 3.8 Live model](https://ai.google.dev/gemini-api/docs/models/gemini-3.8-live), [Live API capabilities](https://ai.google.dev/gemini-api/docs/live-api/capabilities), [Gemini Live Rust crate](https://docs.rs/crate/gemini-live/0.1.9), [Vertex endpoint type](https://docs.rs/gemini-live/0.1.9/gemini_live/transport/enum.Endpoint.html), [Google ADC](https://docs.cloud.google.com/docs/authentication/application-default-credentials).

## 2. `sofia_ui_layer`: platform windows and visual states

**Voice pill and response card.** Keep the previously specified pill design and behavior. The image-inspired response card and other Sofia windows open by visually expanding from the pill and close by shrinking back into it. Show live transcript/answer with selectable and copyable text. Use green for user audio, blue for assistant audio, red for errors, and a neutral connecting/reconnecting animation; include labels or icon changes so color is not the only state cue. Use captured PCM samples for a windowed FFT, smooth the bins, and render at the UI frame rate from a small latest-frame buffer. Never place raw audio samples on the persistent IPC event stream.

**Window morph.** Each UI client receives the pill's screen-side anchor and target window bounds. On open, render a pill-shaped shell at that anchor, then animate its clip, corner radius, and size to the final window. If the pill is on the right, expansion originates beside it and grows toward the left while aligning with the available top/bottom space; mirror this on the left. Mount content at animation start and fade it progressively during expansion, reaching full opacity as the window reaches full size. On close, reverse content opacity and geometry, merging the shell into the pill. Preserve the pill itself as a separate visible object. GPUI Kit Base motion has retargetable springs and reduced-motion support. Prototype the illusion within one surface or a transition overlay before relying on cross-window coordinate/size animation, especially on Wayland. Source: [GPUI Kit animation](https://gpui-kit.com/docs/animation/).

**Drag and magnetic edge behavior.** The desired behavior is free drag followed by spring animation and snapping to the left or right display edge. Clamp to the active monitor's usable area, persist edge and offset per monitor, and handle scale changes and unplugged displays. Windows needs a platform overlay adapter for frameless, transparent, topmost, movable windows. On Wayland, GPUI 0.3.8 already has `WindowKind::LayerShell` and anchor/margin options, but its mapped layer-surface backend currently updates size rather than arbitrary position; prototype drag and runtime margin changes before promising identical physics. If the compositor lacks `zwlr_layer_shell_v1`, use a documented normal-window fallback. Test GNOME, KDE, and a wlroots compositor separately because compositor behavior varies. The pill should not reserve an exclusive screen zone by default.

**Additional windows.** Maintain a registry keyed by UUID, with `title`, `kind`, tags, owner tool/session, content revision, creation time, and focus/close policy. Titles are human labels and may repeat; UUIDs are authoritative for updates and close. A title lookup returns all matches or an ambiguity error, so a model cannot accidentally close the wrong window. Expose `list_windows`, `open_window`, `update_window`, `focus_window`, and `close_window`; allow filtering by tag and kind. Track lifecycle events, user closure, and stale owner cleanup.

| Window kind / suggested tag | Content contract | First renderer |
| --- | --- | --- |
| `markwindow` | Editable notes and Markdown, with optional preview | GPUI Kit `Textarea` for editing and `TextView` for selectable preview |
| `visualizerwindow` | Versioned Sofia Viz document: data, layout, motion presets, and typed interactions | GPUI Kit charts, tables, controls, and custom native painting |
| `webwindow` | Static rich text or, when supported, isolated active HTML with origin/policy | First use HTML `TextView` for static content; a real browser is a later platform spike |
| `userwindow` | Editable todos, reminders, forms, user actions | Typed native GPUI components |
| future kinds | Versioned custom content schema | Registry-defined capability |

Open each window through a renderer registry. Return a `window_id` and operation status to the calling tool. Enforce size/content limits and a clear policy for remote HTML/URLs. GPUI Kit `TextView` is selectable by default and renders Markdown/simple HTML. Published 0.7.1 has experimental `gpui-wry` with Windows support and unfinished Linux hosting. Post-release [PR #3395](https://github.com/longbridge/gpui-kit/pull/3395) renames it to `gpui-webview` and adds Linux X11/XWayland hosting, while native Wayland embedding remains unsupported. Gate an embedded `webwindow` by the exact crate revision and platform. See [GPUI Kit reference](GPUI_KIT_REFERENCE.md), [GPUI TextView](https://gpui-kit.com/docs/components/text-view), [GPUI WebView guide](https://gpui-kit.com/docs/webview/), and [GPUI Kit platform setup](https://gpui-kit.com/docs/installation/).

For model-generated interactive visualizations, start with the declarative Sofia Viz schema and typed live patches over UI IPC. Render its panel at 70% surface opacity by default, while keeping text and data marks opaque; let the user adjust the value in settings. GPUI Shell scripts are a later native experiment, and actual HTML/CSS/JavaScript needs an optional isolated browser renderer. See [generative visualization research](GENERATIVE_VISUALIZATION_RESEARCH.md).

**UI IPC contract (proposed).** `livesofia` hosts a versioned JSON/CBOR request-response and subscription protocol over a Unix domain socket on Linux and a named pipe on Windows. Authenticate local clients using OS peer identity/pipe ACLs. Initial commands: `GetSnapshot`, `SubscribeEvents`, `OpenWindow`, `UpdateWindow`, `ListWindows`, `FocusWindow`, `CloseWindow`, `SetPillPosition`, `GetEntity`, and typed `CreateEntity`/`PatchEntity`/`DeleteEntity` commands. The entity commands are routed to `sofia_mcp`; only it reads or writes user records. Initial events: `ConnectionChanged`, `TurnStateChanged`, `TranscriptDelta`, `ToolChanged`, `AudioSpectrum`, `WindowChanged`, `EntityChanged`, and `ErrorRaised`. Include `protocol_version`, monotonic UI `sequence`, `request_id`, `session_id`, and timestamp. Reconnect with a snapshot of live state plus an MCP-provided content snapshot and resume events after the respective sequences; define explicit behavior for missed events. Rate-limit `AudioSpectrum` and transcript deltas separately. Each UI client registers its supported window kinds and capabilities; route a window intent to one client by policy and return its owner/client ID alongside the window ID. A window displaying a note, todo, or reminder binds to its MCP entity ID and receives every committed change without refresh. The [data and sync contract](DATA_SYNC_REFERENCE.md) defines revisions, conflicts, and replay.

Add `ListTraces`, `GetTrace`, and filtered report queries as read-only UI IPC commands and `ActivityChanged` as a compact live event. `sofia_trace` is an independent UI client of this protocol and can run while the pill UI is closed. The trace IDs returned by those commands link a user-visible tool row to provider usage and any MCP record/window IDs. Trace storage stays in `livesofia.db`; the detailed contract is in [OBSERVABILITY_REPORT.md](OBSERVABILITY_REPORT.md).

## 3. `sofia_mcp`: tools and UI bridge

Host system tools in a separate module/server and connect external MCP servers through a client registry. `sofia_mcp` owns its own SQLite file, schema migrations, content search, and CRUD service for notes, todos, reminders, and future user records. Start OS access with a small reviewed set of read-only tools, then add file/actions with explicit permission scopes. Tool metadata should include side-effect class, argument schema, expected duration, and whether cancellation is supported. Log each tool invocation/result without secrets.

Expose a Sofia UI capability surface to authorized MCP callers: `sofia.ui.capabilities`, `sofia.ui.list_windows`, `sofia.ui.open_window`, `sofia.ui.update_window`, and `sofia.ui.close_window`. Capability discovery returns supported window kinds, content schemas, size limits, and protocol version. The MCP handler translates tool calls into UI IPC requests and returns the created UUID. Tool authors can show a note, chart, or todo without depending on GPUI Kit. Keep the UI wire schema independent from MCP's transport/version so a different UI implementation can serve it.

Expose typed note, todo, and reminder create/read/update/delete tools through `sofia_mcp`'s data service. The desktop editor reaches that service through routed `livesofia` IPC commands; model tool calls invoke the same service directly. A model mutation includes the entity ID and expected revision; after `sofia_mcp` commits, it emits an `EntityChanged` event that `livesofia` relays to every open view. Window control and data mutation remain separate capabilities, so closing a window does not delete a note or reminder. See the [data and sync contract](DATA_SYNC_REFERENCE.md).

Choose an MCP SDK that negotiates protocol versions with target clients. Current MCP revision `2026-07-28` changed server lifecycle, so pin a supported revision for interoperability and test `tools/list`/`tools/call` and reconnection with at least two clients. Treat model-supplied content and tool results as untrusted: validate schemas, escape/sanitize HTML, restrict URL schemes, and apply capability checks to window control. Source: [MCP 2026-07-28 changes](https://blog.modelcontextprotocol.io/posts/2026-07-28/).

## 4. `sofia_setting`: config, themes, and analytics

Use `~/.config/sofia/setting.json` and `~/.config/sofia/themes/<name>.json` on Linux as requested. On Windows use `%APPDATA%\Sofia\setting.json` and `%APPDATA%\Sofia\themes\<name>.json`, with a portable path override for development. Put `livesofia.db` and `sofia_mcp.db` in the per-user application data directory, not in a theme directory. Write Sofia settings atomically, validate at startup, keep a schema version, and migrate older settings files. For themes, use GPUI Kit's `ThemeRegistry::watch_dir`, `ThemeSet` JSON schema, `Theme::update`/`apply_config`, and built-in light/dark and system appearance handling. Each theme JSON file must have a top-level `themes` array. Map user/assistant/error states to the active theme's green/blue/red colors. Persist selected theme names and motion preferences in Sofia settings; check contrast during UI QA without building a second theme engine. See [GPUI Kit reference](GPUI_KIT_REFERENCE.md).

Suggested settings structure:

```json
{
  "schema_version": 1,
  "provider": {"kind": "sofia_managed", "model": "gemini-3.8-live", "service_url": null},
  "audio": {"input_device": "default", "output_device": "default", "push_to_talk": false},
  "memory": {"enabled": true, "summarization_enabled": false, "summary_model": null},
  "ui": {"theme": "default", "pill_edge": "right", "reduced_motion": false, "visualization_surface_opacity": 0.70},
  "sync": {"enabled": false, "backend": "postgres", "connection_ref": null, "dataset_id": null},
  "diagnostics": {"retention_days": 30, "capture_payloads": false, "otlp_export_enabled": false},
  "analytics": {"enabled": true, "retention_days": null}
}
```

Do not store shared operator credentials in desktop JSON or ship them in the app. A Sofia service owns them and exposes a narrow authenticated voice endpoint. The settings app should also let a user supply `CLIENT_ID`, `CLIENT_SECRET`, `PROJECT_ID`, and `REFRESH_TOKEN` for their own Vertex connection, then test token refresh and model setup. Hold the secret and refresh token in the OS credential store; `setting.json` keeps the project ID, mode, and credential reference. The app should offer service connection diagnostics, device tests, GPUI Kit theme selection/preview, memory export/delete, and analytics privacy/retention controls. It must also let users add and test a custom PostgreSQL URL, enable/disable content sync, and see pending changes, last sync, and conflicts. Store a PostgreSQL URL containing a password in the OS credential store too.

Create **two separate local SQLite databases** in the per-user application data directory. `livesofia.db` holds `sessions`, `turns`, `transcript_segments`, `tool_invocations`, `usage_events`, `memories`, `summaries`, `window_events`, `traces`, `trace_spans`, and `trace_events`; it is migrated only by `livesofia`. `sofia_mcp.db` holds `notes`, `todos`, `reminders`, `reminder_occurrences`, `change_log`, and `sync_state`; it is migrated only by `sofia_mcp`. Each database has its own WAL setting, foreign keys, write coordinator, indexes, and optional FTS5 indexes. No process performs cross-file joins, and LLM tool logs keep record references rather than copying complete MCP record bodies by default. The settings app queries basic usage analytics through `livesofia`; `sofia_trace` queries detailed traces and reports through the same read-only IPC API. Settings configures content sync through `sofia_mcp`; neither UI client joins the two files. Retain source turns after summarizing and define export/delete behavior per database. Optional PostgreSQL sync applies to MCP user content. Details are in the [data and sync contract](DATA_SYNC_REFERENCE.md) and [observability plan](OBSERVABILITY_REPORT.md).

Analytics has hourly, daily, weekly, monthly, and yearly views in the user's timezone, with UTC stored in the database. Show input, output, total, and cached tokens when reported; use `unknown` for unavailable fields. Clarify whether cached tokens are a subset of input tokens in each provider adapter so charts do not double count. Show call count, active session time, listening/speaking time, tool duration/count, errors, and provider/model filters. Count runtime using explicit start/stop heartbeats or intervals rather than app launch timestamps alone. Keep summarizer usage separate and offer combined totals. Query from raw `usage_events`, aggregate by selected time bucket, and present GPUI Kit line/bar charts plus a sortable `DataTable` with export. Sources: [Live usage metadata](https://ai.google.dev/gemini-api/docs/live-api/capabilities), [GPUI Kit charts](https://gpui-kit.com/docs/components/chart), [GPUI Kit DataTable](https://gpui-kit.com/docs/components/data-table).

## Delivery sequence and acceptance gates

| Phase | Deliverable | Gate before continuing |
| --- | --- | --- |
| 0. Platform and API spikes | Identify the exact AI Platform key type and prove or reject it for Gemini 3.8 Live; prove the service-held fallback, GPUI pill transparency/drag and window morph on Windows and selected Wayland compositors; inspect crate compatibility | Record working endpoint/auth, audio formats, compositor support, and any crate patches |
| 1. Workspace and contracts | Split into four core crates, `sofia_trace` client, and shared protocol; independent live/MCP DB migrations, `livesofia` IPC server, MCP data service, and UI client registration | Both stores migrate independently; clients negotiate versions, reload settings, and recover from restart |
| 2. Voice core | Microphone to Gemini to speaker, barge-in, reconnect, transcripts, tool router | Repeated conversations work with device changes and network interruption; no leaked audio queue |
| 3. Pill, content, and local records | Physics/edge snap, FFT colors, response card, note/todo/reminder CRUD in MCP SQLite, editable windows, entity relay, UI MCP bridge | Drag and restore on multiple displays; user and model can edit an open record and see committed changes without refresh; offline edits survive restart |
| 4. History, memory, and Sofia Trace | SQLite turns, search, memory retrieval, optional summarizer, trace spans/events, separate GPUI tracing desktop app with sessions/tools/logs/reports | Restart preserves history and traces; tracing app runs while pill UI is closed and lets users inspect failed/slow calls without exposing MCP record bodies |
| 5. Settings and analytics | Service/theme/audio controls, PostgreSQL sync setup/status, hourly through yearly charts and table | Usage reconciles to raw events, missing provider fields stay unknown, timezone buckets are correct; sync setup validates its connection |
| 6. Optional PostgreSQL sync | Two-way note/todo/reminder replication, retries, tombstones, and visible conflict resolution | Two installations converge after reconnect; concurrent edits never silently overwrite a draft |
| 7. Release hardening | Windows/Linux packages, startup behavior, accessibility, logging, privacy, upgrade path | OS matrix passes and failures have clear recovery paths |

## Improvements to include in the plan

1. Make provider capabilities data-driven. This prevents Gemini-specific assumptions about transcription, async tools, or cached tokens from leaking into the UI and makes future providers easier to add.
2. Treat memory as inspectable user data. Let users see why a fact was retrieved, correct it, pin it, or delete it; use source links into conversation history.
3. Add a push-to-talk option and an unmistakable mic indicator from the first voice prototype. This improves control when the assistant is always visible.
4. Introduce a tool policy with dry-run/confirmation for destructive actions, per-tool timeouts, and clear cancellation. The model should never get unrestricted OS or window control merely because an MCP server exists.
5. Add accessibility and localization early: keyboard control, selectable transcripts, screen reader labels, reduced motion, high contrast, and right-to-left layout checks.
6. Measure latency alongside token usage: mic-to-first-transcript, end-of-speech-to-first-audio, tool queue/run time, reconnect time, and dropped audio frames. These expose voice quality problems that token charts cannot show.
7. Keep an explicit compatibility matrix for Wayland compositors, Windows versions, GPU/renderer modes, and WebView availability. Gate features per platform rather than silently exposing a broken option.
8. Add replayable, redacted event traces for debugging state transitions and interrupted tool calls. Keep audio recording off by default; user-approved diagnostics can include it later.
9. Keep the normal activity view concise and offer a trace-detail mode. Reuse canonical `usage_events` for token reports, and keep optional OpenTelemetry export separate from local diagnostics. See [observability plan](OBSERVABILITY_REPORT.md).

## Decisions to settle during the spikes

- Whether the first Linux release targets all Wayland desktops or a tested set with a normal-window fallback. Layer-shell support and runtime movement need real compositor checks.
- Whether `webwindow` initially means rendered HTML/external browser or a true embedded browser. A cross-platform embedded implementation is not yet established by GPUI Kit.
- Which non-Gemini live provider comes next, and which summarization model is the default if the user enables summaries.
- Which Windows/Linux notification backend and background startup policy to use for reminders; the requirement now includes creating real reminders, so the scheduler must persist and fire due occurrences.

Do not promise that the model will "never forget" every detail. Sofia can preserve the full local record and retrieve relevant pieces, while any live model still has finite context and retrieval can miss information. Make that behavior visible and correctable in the product.
