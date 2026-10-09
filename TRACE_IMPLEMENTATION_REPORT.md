# Sofia Trace: first working slice

`livesofia` records runs into a separate SQLite database while its existing IPC event hub publishes activity. The GPUI Kit trace application reads that database independently; it can be opened or closed without affecting audio, Gemini, or the pill.

The viewer shows the last 100 runs and the selected run's ordered timeline. Each run shows its input, assistant output, status, duration, and tool count. Timeline rows show event order, relative time, tool duration, and error detail. The viewer refreshes when SQLite's data version changes; selecting an older run pauses automatic selection until **Latest run** is pressed.

The recorder stores voice and text inputs, final spoken transcription, tool start/finish, cancellations, and daemon errors. A tool failure remains in the same run as the assistant response. A run left open by a daemon restart is marked `abandoned` on the next daemon start. Event bodies are limited to 4,096 characters and the database file is owner readable on Unix. Audio samples are not recorded.

## Verification

- `cargo test -p sofia-trace-store`: a two-run sequence verifies order, failure status, tool duration, and the following successful run.
- `cargo check --workspace`: all crates compile.
- `cargo build -p sofia-trace` and a short launch on the local Wayland session: the separate window mapped without a startup panic; its live database was populated with a sample run.

## Next optimizations

1. Add a stable run ID at the Live session source and attach it to events. The current recorder infers run boundaries from input and ready/listening states, so delayed provider events could be assigned to an adjacent run.
2. Persist redacted tool request/response summaries and provider error codes. The first slice has tool names, durations, success, and failure messages, but lacks argument/result inspection.
3. Record provider usage metadata when available (input/output/cache tokens, model, session ID) and separate model wait, tool execution, and response playback timing.
4. Move writes into a bounded dedicated writer with batched transactions if event volume grows. The current recorder writes a few SQLite rows per significant event and skips audio level/spectrum events.
5. Add filters for status, date, and tool name, plus pagination and export once the run count makes the recent-run table insufficient.
