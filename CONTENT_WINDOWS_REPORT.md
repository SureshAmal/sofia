# Content windows and Sofia MCP

Implemented in the separate `sofia_content`, `sofia_mcp`, and `sofia_ui_layer` crates.

- SQLite content storage with FTS5 indexing, revision checks, open-window state, tags, and per-document dimensions.
- MCP tools for protocol discovery, create/list/get/update, open/close, and open-window listing.
- Notes and HTML are selectable GPUI Kit `TextView` documents. Todos and reminders have checkbox, add, edit, and remove controls. Charts use native GPUI Kit line and bar charts.
- Documents are presented within the existing full-screen Wayland layer. A GPUI spring morphs each panel from the pill position and fades content in during the geometry animation. Closing keeps the panel alive through the reverse animation.
- MCP writes notify the running UI through the versioned livesofia IPC channel; the UI also polls the content database to recover changes made while the daemon was offline.

Known limits: basic HTML is a document renderer, not a browser webview, so scripts, CSS, and arbitrary web interactions are not executed. Reminders currently store `due_at` values without an alarm scheduler. PostgreSQL sync, document resize/drag, and native Windows layered-shell presentation remain follow-up work.

Verification: `cargo test --workspace --all-targets` passes, including the real rmcp client/server round trip and stale-revision rejection.
