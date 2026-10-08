# Live session over IPC

Tested on Linux, 8 October 2026, with `gemini-live` 0.1.9, `gemini-3.8-live`, Vertex AI in `us-central1`, and the repository's ignored OAuth `.env` file. The IPC protocol is now version 6; the original IPC-only check used version 3, and the text-turn path was rechecked after voice and output-device selection were added.

## What was verified

The `livesofia` daemon connected to Gemini Live and reached `ready`. A separate Unix-socket client completed `hello`, fetched a snapshot, subscribed to events, and sent `send_text` with “Say hello in one short sentence.” The daemon replied `accepted`. The client received `thinking`, `speaking`, and `ready` state events plus output transcription deltas and a final transcription: “Hello, it is a pleasure to meet you.” All six received events had contiguous sequence numbers. No credential or raw audio was sent over IPC.

The automated workspace tests also cover handshake, snapshot, pre-handshake rejection, event order, and the transcript reducer. The committed `ipc_probe` example was run against the daemon and also received a final spoken-output transcription: “Hello, it's nice to meet you.” It makes the real-model check repeatable:

```sh
cargo run -p livesofia --bin livesofia
# In another terminal, after the daemon logs that the Live session is ready:
cargo run -p livesofia --example ipc_probe
```

On Linux, both commands use `$XDG_RUNTIME_DIR/sofia/livesofia.sock` unless `SOFIA_IPC_PATH` is set in both terminals. On Windows, both use `\\.\pipe\sofia-livesofia`.

## Current boundary

The daemon receives spoken-output audio from Gemini and plays it on the default speaker, while IPC carries its transcription and status. Microphone capture is controlled through IPC; its device-path result is recorded in the [voice audio test report](VOICE_AUDIO_TEST_REPORT.md). Tool execution, usage persistence, and UI clients remain separate implementation steps. The daemon reconnects after a session closes; subscribers receive state changes and need a fresh snapshot after an event-buffer overflow.
