# Sofia editable data, live updates, and optional PostgreSQL sync

Research and design proposal, 8 October 2026. This document extends the [architecture plan](ARCHITECTURE_PLAN.md); it does not implement the database or sync engine. “Reminder” is the record type for scheduled user alerts.

## Product behavior

Users can create, edit, complete, and delete notes, todos, and reminders in Sofia windows. The model can perform the same operations through typed MCP tools. Every open window reflects **committed** user, model, and remote-sync changes immediately, without a refresh button. A window is a view of a durable record, so closing it does not delete its contents. Two windows showing the same note stay in sync.

Keep **two independent SQLite files**. `sofia_mcp.db` stores notes, todos, reminders, and other user-created content; `livesofia.db` stores LLM conversations, tool-call metadata, memory, usage, and window events. Neither process migrates or directly queries the other's file, and there are no cross-file table joins or foreign keys. MCP data remains available when PostgreSQL is disabled or unreachable. A custom PostgreSQL URL in Settings enables optional two-way sync of MCP content so another Sofia installation, or an authorized external application using the documented schema/API, can access it. Conversation transcripts, memory, and token analytics stay in `livesofia.db` and are outside this sync scope.

## Ownership and data model

`sofia_mcp` owns the user-content SQLite file, its migrations, record commands/search, the reminder scheduler, optional PostgreSQL sync worker, and a durable content change stream. `livesofia` owns a separate LLM SQLite file and the UI IPC gateway. UI clients send content commands over the existing `livesofia` IPC connection; `livesofia` forwards them to the MCP data service and relays committed content events, without storing content records in its database. Model tools invoke the same MCP data service. `sofia_ui_layer` keeps editable GPUI state and renders committed updates. `sofia_setting` manages the PostgreSQL connection reference and reads sync status from `sofia_mcp`. A future alternate UI uses the same routed commands and events.

When the model reads a note, `sofia_mcp` returns it through a tool result for the active turn. `livesofia` records the tool name, record ID, revision, timing, and status; by default it does not copy full note bodies into its tool-result log or turn storage. The model may refer to note content in its own answer, so transcripts can still contain excerpts that were spoken or generated. Do not automatically convert MCP records into LLM memory; an explicit user action or policy would be needed for that.

For the user-facing tool timeline and debug reports, `sofia_mcp` passes correlation IDs, progress, duration, and safe result metadata back to `livesofia`. Trace storage belongs to `livesofia.db`; MCP content still belongs to `sofia_mcp.db`. See [observability plan](OBSERVABILITY_REPORT.md).

```text
UI clients -> livesofia UI IPC -> sofia_mcp data service -> sofia_mcp.db
                            <- committed content events <-
model tool calls -----------> sofia_mcp data service
livesofia voice/LLM state ---------------------------> livesofia.db
sofia_mcp sync worker <------------------------------> optional PostgreSQL
```

| Table in `sofia_mcp.db` | Minimum fields and role |
| --- | --- |
| `notes` | UUID, title, Markdown body, tags, created/updated UTC, revision, deleted UTC, author/origin metadata. |
| `todos` | UUID, title, details, status, priority, optional due UTC/time zone, ordering, timestamps, revision, deletion marker. |
| `reminders` | UUID, title, details, due UTC, IANA time zone, optional recurrence rule, enabled/status, snooze UTC, timestamps, revision, deletion marker. |
| `reminder_occurrences` | Reminder/occurrence IDs, scheduled UTC, fired/acknowledged status; enforces one local notification per occurrence. |
| `change_log` | Monotonic local sequence, unique change ID, entity kind/ID, operation, base/new revision, origin client/tool/device, timestamp, payload or patch; transactional outbox. |
| `sync_state` | Dataset/installation IDs, remote cursor, last successful sync, retry state, applied remote change IDs. |

Use schema migrations, foreign keys, indexed IDs/timestamps/status, and tombstones for deletion so an offline delete propagates. Keep stable UUIDs across devices. An opaque `dataset_id` separates unrelated Sofia datasets sharing a PostgreSQL server. Store only data needed for a record in each table; the `change_log` is an audit/replication stream, not a second source of truth. Save and export note content as Markdown. Include an attachment relation only when attachment handling and file sync are specified.

For reminders, store the user’s time zone as well as the UTC occurrence so recurring local-time reminders survive daylight-saving changes. On startup and after each change, reschedule due items; record a durable occurrence ID before delivering a notification. Decide the platform notification adapter during the Windows/Linux spike, and show due reminders in `userwindow` even if OS notification delivery is unavailable. [Jiff `Zoned`](https://docs.rs/jiff/latest/jiff/struct.Zoned.html) provides time-zone-aware arithmetic; recurrence parsing and platform notifications still need implementation choices.

## One command path and immediate window updates

1. A user edit becomes a typed `CreateEntity`, `PatchEntity`, or `DeleteEntity` request on the UI IPC connection; `livesofia` routes it to `sofia_mcp`. A model MCP tool call invokes that same MCP data service. Imported remote changes also enter there. Commands carry `entity_kind`, UUID, `expected_revision`, command ID, and origin; creation has a new UUID and no expected revision.
2. `sofia_mcp` validates fields and capabilities, writes the record and one `change_log` entry in the **same transaction in `sofia_mcp.db`**, and increments the record revision. It returns the committed record/revision to the caller. It never writes `livesofia.db`.
3. After commit, `sofia_mcp` publishes `EntityChanged { mcp_sequence, entity_kind, entity_id, revision, origin, changed_fields }`. `livesofia` relays it on the UI event stream with its own UI sequence. A window bound to that MCP entity applies the new snapshot/patch to its GPUI state and repaints. There is no polling or manual refresh.
4. `livesofia` tracks the last relayed MCP sequence and catches up from the MCP change stream after reconnect. UI clients separately track their last UI sequence; after a gap, they request live state plus an MCP content snapshot before resuming. `OpenWindow` for an existing record includes its MCP entity reference; `UpdateWindow` only changes presentation or ephemeral content. Persistent note/todo/reminder edits use entity commands.

The desktop editor should use GPUI Kit `Textarea`/`TextareaState` for multiline Markdown source, `Input` for titles, and selectable `TextView` for Markdown preview. `TextView` alone is a renderer, so it cannot satisfy the edit requirement. Typed todo/reminder forms use the Kit’s inputs, selectors, date/time controls, and buttons. GPUI Kit documents [Textarea](https://gpui-kit.com/base/primitives/textarea), [Editor](https://gpui-kit.com/docs/components/editor/), and [TextView](https://gpui-kit.com/docs/components/text-view). Use `Editor` only if code-style editing or syntax features become useful.

For live model writing, expose narrow tools such as `sofia.notes.create`, `sofia.notes.patch`, `sofia.todos.create`, `sofia.todos.patch`, `sofia.reminders.create`, and `sofia.reminders.patch`, plus read/list/delete variants. A successful tool call commits through the same command path and updates every visible window immediately. If a model streams a long note, show a clearly marked provisional draft in the window; commit meaningful batches, not every token. A rejected or canceled draft disappears or remains explicitly unsaved. The model cannot silently replace unsaved user typing.

Use optimistic concurrency for user and model commands. If `expected_revision` is stale, return a structured conflict with the latest record. Nonoverlapping field changes may be rebased automatically; competing changes to a note body or the same field require a visible diff/resolve flow that preserves both versions. Keep the local editor buffer and cursor stable while applying changes to other fields. Idempotent command IDs prevent duplicate model/tool retries from creating duplicate records.

## Optional PostgreSQL sync

Settings accepts `postgres://` or `postgresql://` connection URLs. The setup flow parses the URL, tests a TLS-verified connection, checks schema permissions/migrations, chooses or joins a Sofia dataset, and shows whether sync is active. The URL may embed a password; save the secret URL in the OS credential store and put only `connection_ref`, display host/database, dataset ID, and sync preference in `setting.json`. Never log the full URL. GPUI Kit Settings can expose Test connection, Sync now, last success, pending count, and conflicts. [Keyring](https://docs.rs/crate/keyring/latest) supports native secret stores on Windows and Linux; its exact feature/backend needs platform testing.

Treat `sofia_mcp.db` as the local working store and PostgreSQL as an optional shared replica. The MCP-owned background worker pushes unacknowledged local changes, pulls server changes after a durable cursor, and applies them to `sofia_mcp.db` through the same transaction/event path. It does not read or replicate `livesofia.db`. Use unique change IDs for retry safety and a server-assigned ordered sequence for pull. Track base/server revision or equivalent compare-and-swap metadata; two devices can diverge while offline, so local integer revisions alone are insufficient to resolve remote conflicts. Persist tombstones until all relevant devices have had a chance to see a delete. Publish a local `EntityChanged` after applying a remote change, so open windows update exactly as they do for model edits.

Do not claim safe sync from “last write wins.” For a same-record conflict, merge independent fields and preserve conflicting text versions for user resolution. The first release can surface a conflict and pause that record while other records continue syncing. Clearly show sync failures without blocking local edits. For access by other applications, document a versioned PostgreSQL schema or a narrow service API; direct writes to Sofia tables bypass change tracking and cannot promise live updates unless those applications implement the same change protocol. A PostgreSQL `LISTEN/NOTIFY` channel can reduce pull delay, but durable server sequences and polling/reconnect are still required because notifications are not a persistent queue. [PostgreSQL `LISTEN`](https://www.postgresql.org/docs/current/sql-listen.html), [PostgreSQL `NOTIFY`](https://www.postgresql.org/docs/current/sql-notify.html).

## Crate choices and risks

Use `rusqlite` with a bounded writer in each process for its **own** SQLite file; the schemas and migrations are independent. Add `tokio-postgres` with `tokio-postgres-rustls` to `sofia_mcp` for the optional remote connection; add `deadpool-postgres` only if measurements show multiple concurrent connections are useful. `sqlx` is an alternative that supports both SQLite and PostgreSQL and migrations, but switching either local layer to it should be a deliberate decision. SQLx’s own documentation notes `libsqlite3-sys` compatibility constraints when mixed with `rusqlite`. [SQLx drivers](https://docs.rs/sqlx/latest/sqlx/), [SQLx migrations](https://docs.rs/sqlx/latest/sqlx/migrate/), [tokio-postgres-rustls](https://docs.rs/crate/tokio-postgres-rustls/latest), [deadpool-postgres](https://docs.rs/deadpool-postgres/latest/deadpool_postgres/).

## Acceptance checks

- Create/edit/delete notes, todos, and reminders from UI and MCP; restart Sofia and verify the same records and revisions in `sofia_mcp.db`, with conversation/history only in `livesofia.db`.
- Keep two windows and a CLI UI client subscribed to one record; user and model changes appear in each without refresh and with ordered revisions.
- Type into a note while the model edits it; verify the local draft and cursor are preserved, conflicts are visible, and no content is silently lost.
- Disconnect PostgreSQL, create/edit/delete offline, reconnect two installations, and verify convergence, tombstones, idempotent retries, and explicit same-field conflicts.
- Test PostgreSQL settings with valid/invalid credentials and TLS failure; verify secrets never appear in settings JSON, IPC events, or logs.
- Test one-time and recurring reminders across restart, snooze, time-zone changes, and daylight-saving boundaries; verify each occurrence is delivered at most once per installation.
