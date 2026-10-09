# Sofia

Windows and Linux desktop voice assistant in development. The repository root is a virtual Cargo workspace; application code lives in separate crates under `crates/`.

## Workspace

Dependencies shared across crates are declared once in `[workspace.dependencies]` in the root `Cargo.toml`; member crates inherit them with `workspace = true`. Cargo uses the workspace `target/` directory for build artifacts.

| Crate | Responsibility | Status |
| --- | --- | --- |
| [`livesofia`](crates/livesofia/) | Live model, OAuth, microphone, speaker, transcript reducer, local IPC, Live probes | Live session, IPC, microphone, and speaker stream tested on Linux |
| [`sofia-protocol`](crates/sofia_protocol/) | Shared versioned UI messages | Initial implementation |
| [`sofia-config`](crates/sofia_config/) | Shared settings file storage | Audio output and Gemini voice preferences implemented |
| [`sofia-ui-layer`](crates/sofia_ui_layer/) | GPUI Kit pill and content windows | Linux Wayland pill connected to livesofia |
| [`sofia-mcp`](crates/sofia_mcp/) | MCP tools and user-content database | Crate boundary ready |
| [`sofia-setting`](crates/sofia_setting/) | GPUI settings application | Native GPUI Kit Settings pages, prompt, Vertex credentials, audio and themes |
| [`sofia-trace`](crates/sofia_trace/) | Separate tracing application | Recent runs and tool timelines |

```sh
cargo test --workspace
cargo run -p livesofia --bin livesofia
```

Run `cargo run -p sofia-ui-layer` for the pill and `cargo run -p sofia-setting` for settings.
Run `cargo run -p sofia-trace` for the trace viewer. The daemon writes its run timeline to `~/.local/share/sofia/traces.db` (or `SOFIA_TRACE_DB`); restart `livesofia` after building this change to start recording.
Sofia starts listening when Gemini connects unless automatic listening is disabled in settings.
Left-click the pill to expand/collapse; right-click (two-finger touchpad tap) pauses/resumes capture and playback.
Pausing stays Ready and discards the pending model response.

The settings application uses GPUI Kit's `Settings`, `SettingPage`, `SettingGroup`, and `SettingField` components, including built-in search and prompt reset.
Save & apply stores settings in `~/.config/sofia/setting.json` and reconnects the daemon through `reload_settings`.
Blank Vertex credential fields fall back to `.env`. Credentials entered here are stored in the settings JSON, with owner-only file permissions on Linux.
The editable [default system prompt](crates/sofia_config/src/system_prompt.txt) is sent in Gemini's setup message.
Connection settings include a **Use Vertex AI** switch. Enable it for OAuth/Google Cloud; disable it for a Gemini API key (a blank API-key field falls back to `GEMINI_API_KEY`). Each provider keeps its own model and credentials. Without a saved toggle choice, a configured API key selects Gemini API, otherwise Vertex is used. Voice settings offer all 30 prebuilt voices and the provider default.
System theme is the default; both desktop clients register the bundled GPUI Kit icon assets and use its system appearance observer.
Custom GPUI Kit theme JSON files are loaded and watched from `~/.config/sofia/themes/`.
See [settings and audio verification](SETTINGS_TEST_REPORT.md) for results and current limits.

To test Gemini 3.8 Live, copy [.env.example](.env.example) to an ignored `.env`, fill in your own OAuth credentials and project ID, then run `cargo run -p livesofia --example live_probe`. The [live model test report](LIVE_MODEL_TEST_REPORT.md) records the successful Vertex connection and remaining voice tests.

Function calling, image input, and streamed video frames are covered by [live_multimodal_probe.rs](crates/livesofia/examples/live_multimodal_probe.rs). Run `bash crates/livesofia/scripts/create_multimodal_fixtures.sh` once, then `cargo run -p livesofia --example live_multimodal_probe -- tools`, `image`, or `video`. See the [multimodal test report](LIVE_MULTIMODAL_TEST_REPORT.md) for observed results and limits.

On Linux, `livesofia` listens at `$XDG_RUNTIME_DIR/sofia/livesofia.sock` by default. Set `SOFIA_IPC_PATH` to use another private socket path. Windows uses `\\.\pipe\sofia-livesofia`. The version 9 JSON-lines IPC protocol supports `hello`, `get_snapshot`, `subscribe_events`, `send_text`, `start_listening`, `stop_listening`, `reload_settings`, `set_speaker_muted`, `list_audio_outputs`, `select_audio_output`, `list_gemini_voices`, `select_gemini_voice`, and `ping`. A running Live session publishes connection/turn states, microphone and speaker status, selected output and Gemini voice, microphone RMS levels, input transcription, assistant text deltas/finals, tool-call notices, and errors. Event sequences are monotonic; clients that fall behind receive `resync_required` and must fetch a fresh snapshot and subscribe again. The Windows pipe ACL needs hardening before carrying user data.

To verify the connection, start `cargo run -p livesofia --bin livesofia`, wait for `Gemini Live session ready`, then run `cargo run -p livesofia --example ipc_probe` in another terminal. The probe should show `Thinking`, `Speaking`, and final text; the daemon plays Gemini's PCM audio through the selected speaker. Run `cargo run -p livesofia --example audio_output_picker` to choose the speaker, `cargo run -p livesofia --example gemini_voice_picker` to choose Gemini's speaking voice, or `cargo run -p livesofia --example voice_probe` to speak, press Enter, and hear the answer. The choices are stored in `~/.config/sofia/setting.json` on Linux and `%APPDATA%\Sofia\setting.json` on Windows. [AssistantTranscript](crates/livesofia/src/transcript.rs) maps Gemini output audio transcription to UI events. See the [IPC Live test report](IPC_LIVE_TEST_REPORT.md) and [voice audio test report](VOICE_AUDIO_TEST_REPORT.md).

Architecture and research: [architecture plan](ARCHITECTURE_PLAN.md), [GPUI Kit reference](GPUI_KIT_REFERENCE.md), [generated visualization research](GENERATIVE_VISUALIZATION_RESEARCH.md).


## MCP connections

Open **Settings → MCP**, paste a complete `{"mcpServers": {...}}` JSON configuration, and click **Add servers**. Sofia parses it and shows each server by name. Test a connection to discover tools; each server and tool can be enabled separately. Re-importing a server updates its connection while preserving enabled state and tool choices. **Save & apply** reconnects Gemini with the enabled tool declarations.

The imported format accepts `command`, `args`, and optional `env` for stdio, or `url` and optional `headers.Authorization` with a bearer token for HTTP. Stdio runs an executable directly (no shell). Standard `npx -y mcp-remote <url>` configurations work as local stdio bridges and require Node.js/npx. Internal server IDs are generated from names; tools are namespaced by these IDs. Unsupported fields are reported rather than silently discarded.

The separate `sofia-mcp-client` crate handles discovery, schema adaptation, and execution. IPC also forwards content changes for live document refresh. Rebuild the daemon and UI together for protocol version 10.

Run the local MCP checks with `cargo test -p sofia-mcp-client` (Python 3 required for fixtures). To verify a real Gemini → MCP → Gemini round trip using the existing Vertex `.env` credentials:

```bash
cargo run -p livesofia --example mcp_live_probe
```

See [MCP_CLIENT_TEST_REPORT.md](MCP_CLIENT_TEST_REPORT.md) for supported features and limits.

## Saved documents and MCP windows

`sofia-mcp` stores user documents in `~/.local/share/sofia/sofia_mcp.db` (or `SOFIA_CONTENT_DB`). This database is separate from Live model history. Its tools create, search, read, update, open, close, and list notes, todos, reminders, charts, and basic HTML documents. Notes use Markdown; todos and reminders have editable item rows; charts use native GPUI Kit plots; HTML uses GPUI Kit's selectable document renderer and does not run JavaScript or arbitrary CSS.

Open panels are managed inside the existing full-screen Wayland layer. Their geometry springs from the pill and closes back into it. MCP edits use optimistic revisions, and the UI reports a conflict instead of overwriting an unsaved draft. `due_at` is stored data; reminder scheduling and PostgreSQL synchronization are future extensions.
