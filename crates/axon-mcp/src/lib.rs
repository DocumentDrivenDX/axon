#![forbid(unsafe_code)]
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

pub(crate) mod error_mapping;

pub mod handlers;
pub mod prompts;
pub mod protocol;
pub mod resources;
pub mod tools;

pub use handlers::{
    build_aggregate_tool, build_aggregate_tool_tokio, build_crud_tools, build_crud_tools_tokio,
    build_link_candidates_tool, build_link_candidates_tool_tokio, build_neighbors_tool,
    build_neighbors_tool_tokio, build_query_tool, build_query_tool_tokio,
};
pub use prompts::{get_prompt_from_handler, prompt_infos, PromptRegistry};
pub use protocol::{McpError, McpServer};
pub use resources::{
    discover_collections, read_resource_from_handler, resource_infos, resource_template_infos,
    ResourceRegistry,
};
pub use tools::ToolRegistry;
