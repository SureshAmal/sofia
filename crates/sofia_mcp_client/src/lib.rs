//! MCP client connections and Gemini function routing; independent of the Sofia MCP server.
mod bridge;
mod schema;
pub use bridge::{ConnectedServer, McpBridge, ServerReport, tool_name};
