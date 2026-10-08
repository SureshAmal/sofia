# Sofia MCP client

## Implementation

`sofia-mcp-client` uses the [official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk). It is separate from the future `sofia-mcp` system tools server.

Settings has a native GPUI Kit MCP page with reusable server cards:

- Paste standard `mcpServers` JSON to add/update multiple servers; friendly names are preserved.
- Remove servers directly from the list.
- Enable/disable each server.
- Local stdio JSON: `command`, `args`, optional `env`.
- Remote Streamable HTTP JSON: `url`, optional bearer Authorization header.
- Test initialization and paginated tool discovery.
- Enable/disable discovered tools independently.
- Save & apply persists configuration and reconnects Gemini.

Livesofia discovers enabled servers concurrently, namespaces function names by server, adapts parameter schemas, dispatches Gemini calls to the correct server, and returns MCP results to Gemini. IPC exposes requested/completed calls and success. Pause aborts local pending tasks; Gemini cancellation aborts the corresponding task.

## Verification

Verified on Linux:

- `cargo test --workspace`: 27 tests passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo build -p sofia-setting`: passed; MCP page visually inspected.
- Real Vertex Gemini probe: OAuth and Live setup succeeded, MCP echo completed, 154,562 audio bytes returned, transcript: “The tool result is sofia-mcp-ok.”
- `cargo fmt --all` and `git diff --check`: passed.


Automated fixtures exercise:

- Stdio initialization and discovery across two pages.
- Multiple servers exposing the same tool name.
- Disabled servers and tools.
- Function dispatch and results.
- Isolation of a failed server.
- Streamable HTTP with bearer authentication.
- Stable function names, local schema references, nullable types, and rejection of recursive/external schemas.
- Configuration round trip and duplicate server rejection.

`cargo run -p livesofia --example mcp_live_probe` additionally checks a real Vertex Gemini Live session calling the local echo fixture, receiving its result, and producing audio and text. It uses existing `.env` credentials and does not change user settings.

## Limits

- Connection/discovery timeout: 15 seconds per server; tool call timeout: 60 seconds.
- At most 32 pending Gemini calls; model result JSON limited to 128 KiB.
- Schema adaptation supports common object/array/scalar schemas and local references. Some JSON Schema validation constraints are omitted; composition such as `allOf` and external/recursive references is rejected. Servers must validate arguments themselves.
- No remote OAuth login flow or legacy HTTP+SSE transport UI yet.
- Connection tests start a server and discover tools; they do not execute its tools.
- Cancellation and timeouts cannot undo operations already performed by a remote server.
- Imported servers are enabled; they connect to Sofia after Save & apply. Re-import preserves existing enabled states and tool choices.
- Credentials use the existing private settings JSON storage; OS keychain integration remains future work.
- Windows runtime behavior requires testing on Windows.
- IPC version is 9; rebuild daemon and UI together.

The Sofia content database and its notes/reminders/tools remain a separate next task.

## Simplified JSON workflow verification

The parser tests cover the supplied Parallel Search MCP `npx` configuration, arguments containing spaces, HTTP bearer headers, and rejection of unsupported fields. The UI uses a bounded JSON input and compact server cards, with no manual connection form.
