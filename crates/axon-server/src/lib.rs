// placeholder — filled in by service.rs and gateway.rs modules
pub mod actor_scope;
pub mod auth;
mod collection_listing;
pub mod control_plane;
pub mod gateway;
pub(crate) mod mcp_http;
pub mod mcp_stdio;
pub mod rate_limit;
pub mod schema_registry;
pub mod service;

pub use auth::{AuthContext, AuthMode, Identity, Role};
pub use mcp_stdio::run_mcp_stdio;
pub use service::AxonServiceImpl;
