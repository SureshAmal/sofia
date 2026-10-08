# Sofia activity, tool-call reports, and debugging traces

Research and architecture proposal, 8 October 2026. This describes `sofia_trace`, a **separate small GPUI Kit desktop application** for local activity reports and debugging, inspired by Langfuse and LangSmith. It does not implement tracing or send data to either service.

## What the research suggests

[Langfuse](https://langfuse.com/docs/observability/data-model) groups observations into a trace and traces into a session; its observation types include model generations, tools, retrieval, and events. Its dashboards use token counts, latency, costs, and feedback. [LangSmith](https://docs.langchain.com/langsmith/observability-concepts) uses runs for individual steps, traces for an operation, and threads for a multi-turn session. Those are useful patterns for Sofia. The smallest useful subset is an ordered execution timeline, a nested detail view, searchable failures, and usage totals. Prompt management, hosted datasets, evaluations, collaboration, alerting, and a full observability backend can wait.

Use a **session** for one voice conversation and a **trace** for one user turn or explicit assistant task. The provider's long-running Live WebSocket is a separate connection lifecycle linked to the session; a reconnect does not create a new conversation. A trace contains spans for live-model activity, memory retrieval, tool calls, MCP execution, summarization, UI actions, and errors. Parallel tools are siblings with separate durations. This is an architectural mapping to Sofia, not a claim that the external products prescribe its exact schema.

| Capability seen in Langfuse/LangSmith | Sofia decision |
| --- | --- |
| Nested model/tool/retrieval traces and multi-turn grouping | Build in the first diagnostic release. |
| Per-call tokens, duration, errors, and model/provider filters | Build from provider usage and timed local events; keep unknown fields explicit. |
| Session replay and full prompt/response inspection | Start with a safe event timeline and links to Sofia's stored transcript; keep prompt/tool payload capture off by default. |
| Feedback or “this was wrong” marker | Defer until the first read-only trace viewer is stable; add a narrow annotation command later. |
| Prompt registry, experiments, judge evaluations, shared workspaces, alerting | Defer until Sofia has a concrete need and stable trace data. |
| Vendor-hosted ingestion | Offer optional OTLP export after local reporting works. |

## Separate desktop client and layout

Package `sofia_trace` as its own executable. It can be launched from Settings, the pill's menu, or directly from the OS. It is an authorized client of the `livesofia` IPC protocol with read-only access to traces and user data: it subscribes to `ActivityChanged`, connection, tool, and error events and calls `ListTraces`/`GetTrace`/report queries. It may request navigation to a linked Sofia window but cannot edit a record. It does not host the model, MCP tools, or its own content database. The voice pill stays compact; detailed tracing lives in this application. Keep the basic token analytics requested for `sofia_setting`, while the trace app owns the detailed per-turn and per-tool reports.

Use a Langfuse-inspired three-pane layout with GPUI Kit components:

```text
┌────────────────────────────────────────────────────────────────────────┐
│ Sofia Trace   ● Live   Search   time range   model/tool/status filters │
├───────────────┬───────────────────────────────────┬────────────────────┤
│ Overview      │ Sessions / Traces / Tools / Logs │ Selected trace     │
│ Sessions      │ sortable DataTable or timeline   │ nested spans       │
│ Traces        │ time, status, tool, latency       │ events + timings   │
│ Tool calls    │ token counts and error badge      │ safe detail/export │
│ Logs & errors │                                   │                    │
│ Reports       │                                   │                    │
└───────────────┴───────────────────────────────────┴────────────────────┘
```

The Overview page shows `livesofia`, MCP, and provider connection health, active session, latest errors, recent calls, and small usage/latency charts. Sessions groups turns. Traces shows the nested execution tree. Tool calls filters calls by server/name/status/duration. Logs & errors shows timestamp, severity, component, code, safe message, and correlation IDs for structured application events. Reports has trends and breakdowns. A selected row opens details in the right pane; keyboard navigation and text selection work throughout. GPUI Kit `DataTable`, charts, tabs, split panes, badges, and `TextView` cover the first implementation. The trace app can open a linked Sofia note/window through the existing UI commands; the data still comes from `sofia_mcp`.

The app should work while the main GPUI pill client is closed, provided `livesofia` is running. If `livesofia` stops, show an explicit disconnected state and a reconnect action. A read-only recovery mode for its SQLite trace records or rotated local logs is a later crash-diagnostics feature; it must never mutate or migrate `livesofia.db`.

## What the user sees

| View | Visible information | Purpose |
| --- | --- | --- |
| Pill/card activity | Listening, thinking, speaking, calling a named tool, tool progress, error/retry | Minimal status only; detailed tracing opens in `sofia_trace`. |
| Turn timeline | Timestamped ordered steps: transcript received, model response started, tool requested, policy decision, tool running/completed, answer spoken, reconnect/interruption | Explain one interaction without overwhelming the voice UI. |
| Tool-call detail | Tool/server name, call ID, start/end and duration, status, safe argument summary, result summary, error code, retry/cancel state, resulting entity or window link | Diagnose a slow or failed action. |
| Trace tree | Parent/child spans, parallel calls, provider/model, usage metadata, first transcript/audio latency, tool and retrieval durations, related logs | Developer/debug view, opened from a turn. |
| Reports | Usage by hour/day/week/month/year; tool count, duration percentiles, failure rate, reconnect rate, dropped audio frames; filters by model, tool, period, and status | Find trends without reading every trace. |

In `sofia_trace`, use GPUI Kit `DataTable` for searchable tool/trace rows, charts for trends, and selectable `TextView` for details. The activity view should show **observable actions and results**, not claim access to hidden model reasoning. Show a provider-supplied thought summary only if the provider explicitly exposes one and the user chooses to display it. An entry such as “Sofia searched 3 notes” should be derived from a tool/retrieval event, not invented from the answer text.

Example user-facing sequence: `Listening → transcript ready (0.8 s) → checking notes → sofia.notes.search (120 ms, 3 matches) → creating reminder → sofia.reminders.create (55 ms, saved) → speaking`. Opening the trace shows the two tool spans, their IDs, safe argument summaries, and provider-reported usage if available. The note and reminder contents are retrieved from `sofia_mcp` only when the user opens the linked record.

## Data ownership and event flow

`livesofia` owns trace and usage records in `livesofia.db`. `sofia_mcp` owns user content in `sofia_mcp.db` and executes content/system tools. A tool call carries `trace_id`, `span_id`, `parent_span_id`, `session_id`, `turn_id`, `tool_call_id`, and `request_id` through the local service boundary. `sofia_mcp` returns timed status/progress/result metadata with the same IDs. `livesofia` writes one span per invocation and publishes compact `ActivityChanged` events over its existing UI IPC stream. `sofia_trace` draws the timeline as events arrive and loads persisted trace detail on demand. An MCP content change still follows the separate `EntityChanged` path.

Do not copy complete note bodies or reminder details into `livesofia.db` for tool traces. Store record IDs, revisions, operation names, field names, sizes, and status. A user who opens a linked note reads its current content from `sofia_mcp`; a trace may also record a content hash/revision to identify what was seen then without duplicating the document. Likewise, avoid recording provider keys, PostgreSQL URLs, raw microphone audio, raw system prompts, and full external tool results in ordinary traces. Conversation transcripts already belong to the LLM history domain; trace rows should reference their segment IDs instead of duplicating text. This preserves the two-database boundary while still allowing the model to read MCP data transiently for an authorized tool call.

### Proposed `livesofia.db` trace tables

| Table | Main fields |
| --- | --- |
| `traces` | trace ID, session/turn IDs, start/end UTC, status, root operation, provider/model, app version, error summary, privacy mode. |
| `trace_spans` | span/parent IDs, trace ID, kind (`model`, `tool`, `retrieval`, `summary`, `ipc`, `ui`, `audio`, `system`), name, start/end UTC, status/error type, safe structured attributes. |
| `trace_events` | sequence, optional trace/span IDs, timestamp, event kind, severity, source component, diagnostic code, compact safe metadata; bounded by retention. Uncorrelated startup/connection errors remain queryable in Logs & errors. |
| Future `trace_feedback` | Trace ID, user rating/flag and optional note; add only when annotation is enabled. |

Keep `usage_events` as the **canonical** token data already planned. A model span references its usage event; reports must not add the two together. Save provider-reported input, output, total, and cached tokens separately, with null for absent fields and an `estimated` flag for estimates. Cached tokens may be a subset of input tokens and must not be added again to the total. If Live API reports only connection/session cumulative usage, compute turn deltas only when consecutive counters and reset behavior are verified; otherwise show usage at connection/session level and mark turn allocation unknown. Cost is displayed only when a maintained rate card and the provider's token categories are known; label any result as an estimate. [Langfuse token and cost model](https://langfuse.com/docs/observability/features/token-and-cost-tracking), [Gemini Live API reference](https://ai.google.dev/api/live.md).

### Suggested lifecycle events

`TurnStarted`, `InputTranscriptReady`, `ModelStarted`, `FirstOutputReceived`, `ToolRequested`, `ToolPolicyChecked`, `ToolQueued`, `ToolStarted`, `ToolProgress`, `ToolSucceeded`, `ToolFailed`, `ToolCancelled`, `ToolResultSent`, `AnswerStarted`, `AnswerCompleted`, `TurnInterrupted`, `ConnectionLost`, `ConnectionResumed`, `UsageReported`, and `TurnEnded`. Every event includes an ID, time, source process, parent trace/span, and status. A tool request may run concurrently with speech or another tool. Retried calls have distinct attempt spans under one logical call ID. The tool's final result and `ToolResultSent` are separate events, which helps locate failures in execution versus failures returning the result to Gemini.

For long tools, ask for MCP progress when available and map it into `ToolProgress`; do not invent percentages when the server only provides text. The [MCP SDK progress guide](https://ts.sdk.modelcontextprotocol.io/v2/servers/logging-progress-cancellation) shows request-scoped progress tokens and optional notifications. That guide also states MCP logging is deprecated in the 2026-07-28 revision, so Sofia should use its own structured trace stream for durable reporting instead of depending on MCP log notifications.

## Diagnostics without a large backend

1. Instrument `livesofia` and `sofia_mcp` with Rust [`tracing`](https://docs.rs/tracing/latest/tracing/) spans. Pass trace context explicitly across the local IPC boundary and preserve it when tool calls hop to another MCP server. Generate stable IDs before dispatch so model, tool, and UI events can be joined.
2. Persist a compact, filtered subset of events and spans to `livesofia.db` through a bounded writer. Do not write one row per audio frame, FFT frame, token delta, or waveform sample. Coalesce high-frequency updates; store milestone timestamps and counters instead. A tracing failure must never block the live audio path.
3. Build the separate `sofia_trace` desktop app with filters by session, time, tool, error, and slow duration. Show parent-child timing, retries, provider error code, failed schema validation, and MCP process/transport failure. Add a redacted JSON export for one trace or time range that the user can inspect before sharing.
4. Keep metadata-only tracing on by default with a bounded retention period configurable in Settings. Diagnostic capture of transcript excerpts or tool payloads is a separate, time-limited opt-in; payloads from `sofia_mcp` should stay in its data domain or be assembled only in an explicit export. Deleting a session removes its traces and usage linkage according to a documented retention policy.
5. Treat optional OpenTelemetry export as an integration, off by default. Map Sofia spans to the current [OpenTelemetry GenAI conventions](https://github.com/open-telemetry/semantic-conventions-genai) in one adapter because those conventions evolve. Rust [`tracing-opentelemetry`](https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/) bridges tracing spans, and [`opentelemetry-otlp`](https://docs.rs/opentelemetry-otlp/latest/opentelemetry_otlp/) exports to compatible endpoints. Both [Langfuse](https://langfuse.com/integrations/native/opentelemetry) and [LangSmith](https://docs.langchain.com/langsmith/trace-with-opentelemetry) document OTLP ingestion. Langfuse's [legacy trace ingestion API](https://langfuse.com/docs/api-and-data-platform/features/public-api) is scheduled to sunset on 16 November 2026, which further favors OTLP for a future integration. Never send Sofia data to a remote endpoint merely because the local activity view is enabled.

The OpenTelemetry GenAI conventions include model/provider, operation, tool call, and usage attributes; they warn that messages, tool arguments, and results can contain sensitive data. The exact semantic convention version and exported fields need to be pinned during implementation. [OpenTelemetry GenAI attributes](https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/), [GenAI conventions repository](https://github.com/open-telemetry/semantic-conventions-genai).

## Acceptance checks

- A turn with two parallel tools shows both calls, correct parentage, individual durations, progress where available, and a single final result per call ID.
- A failed/retried tool distinguishes policy denial, validation failure, MCP transport failure, tool execution error, timeout, cancellation, and failure to return the result to the model.
- A reconnect preserves the conversation/session ID, records the connection transition, and never double-counts tool attempts or usage.
- Reports reconcile to `usage_events`; unavailable token or cost fields display as unknown. Cached tokens are not added twice.
- Notes and reminder bodies are absent from `livesofia.db` trace payloads; user-content storage and PostgreSQL sync remain in `sofia_mcp`.
- The live activity view stays responsive under audio load, and trace writes can be dropped/coalesced with a visible diagnostics-gap marker if the bounded queue overflows.
- Optional OTLP export remains disabled until the user configures an endpoint and reviews which fields leave the machine.
- `sofia_trace` launches directly, shows live updates while the pill UI is closed, links a selected tool call to its turn, and clearly reports when `livesofia` disconnects.
