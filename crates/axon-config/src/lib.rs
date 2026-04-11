//! XDG-compliant path resolution and TOML config loading for Axon.
//!
//! `axon-config` provides platform-appropriate directory paths following
//! XDG conventions on Linux and standard Application Support paths on macOS,
//! along with a typed configuration model that loads from TOML files.

#![forbid(unsafe_code)]

pub mod config;
pub mod paths;

pub use config::AxonConfig;
pub use config::ConfigError;
