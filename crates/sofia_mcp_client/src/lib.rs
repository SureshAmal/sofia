//! MCP client connections and Gemini function routing; independent of the Sofia MCP server.
mod bridge;
mod oauth;
mod schema;
pub use bridge::{ConnectedServer, McpBridge, ServerReport, ToolImage, ToolOutput, tool_name};
pub use oauth::{REDIRECT_URI as OAUTH_REDIRECT_URI, authenticate as authenticate_oauth};
