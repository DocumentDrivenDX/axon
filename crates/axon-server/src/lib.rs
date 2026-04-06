// placeholder — filled in by service.rs and gateway.rs modules
pub mod auth;
pub mod gateway;
pub mod service;

pub use auth::{AuthMode, Identity};
pub use service::AxonServiceImpl;
