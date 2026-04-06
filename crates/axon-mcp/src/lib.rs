//! MCP (Model Context Protocol) server for Axon.
//!
//! Implements the JSON-RPC 2.0-based MCP protocol so that AI agents can
//! discover Axon collections as typed tools and perform CRUD operations.
//!
//! # Protocol
//!
//! MCP uses newline-delimited JSON-RPC 2.0 over stdio. The server responds to:
//!
//! - `initialize` — handshake with capabilities
//! - `tools/list` — typed tool definitions derived from registered collections
//! - `tools/call` — dispatch to the appropriate handler
//! - `notifications/initialized` — client ack (no response)

pub mod handlers;
pub mod protocol;
pub mod tools;

pub use handlers::{
    build_aggregate_tool, build_crud_tools, build_link_candidates_tool, build_neighbors_tool,
    build_query_tool,
};
pub use protocol::{McpServer, McpError};
pub use tools::ToolRegistry;
