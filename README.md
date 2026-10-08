# Sofia

Windows and Linux desktop voice assistant in development. The repository root is a virtual Cargo workspace; application code lives in separate crates under `crates/`.

## Workspace

Dependencies shared across crates are declared once in `[workspace.dependencies]` in the root `Cargo.toml`; member crates inherit them with `workspace = true`. Cargo uses the workspace `target/` directory for build artifacts.

| Crate | Responsibility | Status |
| --- | --- | --- |
| [`livesofia`](crates/livesofia/) | Live model, OAuth, microphone, speaker, transcript reducer, local IPC, Live probes | Live session, IPC, microphone, and speaker stream tested on Linux |
| [`sofia-protocol`](crates/sofia_protocol/) | Shared versioned UI messages | Initial implementation |
| [`sofia-config`](crates/sofia_config/) | Shared settings file storage | Audio output and Gemini voice preferences implemented |
| [`sofia-ui-layer`](crates/sofia_ui_layer/) | GPUI Kit pill and content windows | Crate boundary ready |
| [`sofia-mcp`](crates/sofia_mcp/) | MCP tools and user-content database | Crate boundary ready |
| [`sofia-setting`](crates/sofia_setting/) | GPUI settings application | Crate boundary ready |
| [`sofia-trace`](crates/sofia_trace/) | Separate tracing application | Crate boundary ready |

```sh
cargo test --workspace
cargo run -p livesofia --bin livesofia
```

To test Gemini 3.8 Live, copy [.env.example](.env.example) to an ignored `.env`, fill in your own OAuth credentials and project ID, then run `cargo run -p livesofia --example live_probe`. The [live model test report](LIVE_MODEL_TEST_REPORT.md) records the successful Vertex connection and remaining voice tests.

Function calling, image input, and streamed video frames are covered by [live_multimodal_probe.rs](crates/livesofia/examples/live_multimodal_probe.rs). Run `bash crates/livesofia/scripts/create_multimodal_fixtures.sh` once, then `cargo run -p livesofia --example live_multimodal_probe -- tools`, `image`, or `video`. See the [multimodal test report](LIVE_MULTIMODAL_TEST_REPORT.md) for observed results and limits.

On Linux, `livesofia` listens at `$XDG_RUNTIME_DIR/sofia/livesofia.sock` by default. Set `SOFIA_IPC_PATH` to use another private socket path. Windows uses `\\.\pipe\sofia-livesofia`. The version 6 JSON-lines IPC protocol supports `hello`, `get_snapshot`, `subscribe_events`, `send_text`, `start_listening`, `stop_listening`, `set_speaker_muted`, `list_audio_outputs`, `select_audio_output`, `list_gemini_voices`, `select_gemini_voice`, and `ping`. A running Live session publishes connection/turn states, microphone and speaker status, selected output and Gemini voice, microphone RMS levels, input transcription, assistant text deltas/finals, tool-call notices, and errors. Event sequences are monotonic; clients that fall behind receive `resync_required` and must fetch a fresh snapshot and subscribe again. The Windows pipe ACL needs hardening before carrying user data.

To verify the connection, start `cargo run -p livesofia --bin livesofia`, wait for `Gemini Live session ready`, then run `cargo run -p livesofia --example ipc_probe` in another terminal. The probe should show `Thinking`, `Speaking`, and final text; the daemon plays Gemini's PCM audio through the selected speaker. Run `cargo run -p livesofia --example audio_output_picker` to choose the speaker, `cargo run -p livesofia --example gemini_voice_picker` to choose Gemini's speaking voice, or `cargo run -p livesofia --example voice_probe` to speak, press Enter, and hear the answer. The choices are stored in `~/.config/sofia/setting.json` on Linux and `%APPDATA%\Sofia\setting.json` on Windows. [AssistantTranscript](crates/livesofia/src/transcript.rs) maps Gemini output audio transcription to UI events. See the [IPC Live test report](IPC_LIVE_TEST_REPORT.md) and [voice audio test report](VOICE_AUDIO_TEST_REPORT.md).

Architecture and research: [architecture plan](ARCHITECTURE_PLAN.md), [GPUI Kit reference](GPUI_KIT_REFERENCE.md), [generated visualization research](GENERATIVE_VISUALIZATION_RESEARCH.md).
