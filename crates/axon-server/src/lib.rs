// placeholder — filled in by service.rs and gateway.rs modules
pub mod auth;
pub mod gateway;
pub mod mcp_stdio;
pub mod schema_registry;
pub mod service;

pub use auth::{AuthContext, AuthMode, Identity, Role};
pub use mcp_stdio::run_mcp_stdio;
pub use service::AxonServiceImpl;
