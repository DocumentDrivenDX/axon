//! Typed configuration model for Axon with TOML loading.
//!
//! Configuration is loaded from a TOML file. Missing files are treated as
//! all-defaults; malformed files produce a [`ConfigError`].

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;

// ── Error type ──────────────────────────────────────────────────────────

/// Errors that can occur while loading configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// An I/O error occurred reading the config file.
    Io(std::io::Error),
    /// The config file could not be parsed as valid TOML.
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "config I/O error: {err}"),
            Self::Parse { path, source } => {
                write!(f, "failed to parse config at {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

// ── Default helpers ─────────────────────────────────────────────────────

fn default_http_port() -> u16 {
    4170
}

fn default_backend() -> String {
    String::from("sqlite")
}

fn default_auth_mode() -> String {
    String::from("no-auth")
}

fn default_guest_role() -> String {
    String::from("admin")
}

fn default_server_url() -> String {
    String::from("http://localhost:4170")
}

fn default_connect_timeout_ms() -> u64 {
    200
}

// ── Config structs ──────────────────────────────────────────────────────

/// Top-level Axon configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AxonConfig {
    /// Server (HTTP/gRPC) settings.
    #[serde(default)]
    pub server: ServerConfig,
    /// Storage backend settings.
    #[serde(default)]
    pub storage: StorageConfig,
    /// Authentication settings.
    #[serde(default)]
    pub auth: AuthConfig,
    /// CLI client settings.
    #[serde(default)]
    pub client: ClientConfig,
}

/// HTTP and gRPC server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Port for the HTTP API (default 4170).
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    /// Optional port for the gRPC API. If `None`, gRPC is disabled.
    pub grpc_port: Option<u16>,
}

/// Storage backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Backend type: `"sqlite"`, `"postgres"`, or `"memory"`.
    #[serde(default = "default_backend")]
    pub backend: String,
    /// Data directory override. Empty string means use the XDG default.
    #[serde(default)]
    pub data_dir: String,
    /// PostgreSQL host (only used when `backend = "postgres"`).
    pub postgres_host: Option<String>,
    /// PostgreSQL port (only used when `backend = "postgres"`).
    pub postgres_port: Option<u16>,
    /// PostgreSQL superuser name.
    pub postgres_superuser: Option<String>,
    /// PostgreSQL superuser password.
    pub postgres_superpass: Option<String>,
}

/// Authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Auth mode: `"no-auth"`, `"guest"`, or `"tailscale"`.
    #[serde(default = "default_auth_mode")]
    pub mode: String,
    /// Role assigned to guest/unauthenticated users.
    #[serde(default = "default_guest_role")]
    pub guest_role: String,
}

/// CLI client configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// URL of the Axon server to connect to.
    #[serde(default = "default_server_url")]
    pub server_url: String,
    /// Connection timeout in milliseconds.
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
}

// ── Default impls ───────────────────────────────────────────────────────

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_port: default_http_port(),
            grpc_port: None,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            data_dir: String::new(),
            postgres_host: None,
            postgres_port: None,
            postgres_superuser: None,
            postgres_superpass: None,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: default_auth_mode(),
            guest_role: default_guest_role(),
        }
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: default_server_url(),
            connect_timeout_ms: default_connect_timeout_ms(),
        }
    }
}

// ── AxonConfig impl ─────────────────────────────────────────────────────

impl AxonConfig {
    /// Load configuration from the given path, or from the default location.
    ///
    /// - If `config_path` is `Some` and the file exists, it is loaded and parsed.
    /// - If `config_path` is `Some` but the file does not exist, defaults are returned.
    /// - If `config_path` is `None`, defaults are returned.
    /// - If the file exists but contains malformed TOML, a [`ConfigError`] is returned.
    pub fn load(config_path: Option<&Path>) -> Result<Self, ConfigError> {
        let Some(path) = config_path else {
            return Ok(Self::default());
        };

        match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).map_err(|source| ConfigError::Parse {
                path: path.to_path_buf(),
                source,
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(ConfigError::Io(err)),
        }
    }

    /// Returns a commented default config file suitable for writing to disk.
    pub fn default_toml() -> &'static str {
        r#"# Axon configuration file
# See https://github.com/easylabz/axon for documentation.

[server]
# Port for the HTTP API.
http_port = 4170
# Optional gRPC port. Uncomment to enable gRPC.
# grpc_port = 4171

[storage]
# Storage backend: "sqlite", "postgres", or "memory".
backend = "sqlite"
# Data directory override. Leave empty to use the XDG default
# (~/.local/share/axon/ on Linux, ~/Library/Application Support/axon/ on macOS).
data_dir = ""
# PostgreSQL connection settings (only used when backend = "postgres").
# postgres_host = "localhost"
# postgres_port = 5432
# postgres_superuser = "axon"
# postgres_superpass = ""

[auth]
# Authentication mode: "no-auth", "guest", or "tailscale".
mode = "no-auth"
# Role for guest/unauthenticated users: "admin", "editor", "viewer".
guest_role = "admin"

[client]
# URL of the Axon server for CLI commands.
server_url = "http://localhost:4170"
# Connection timeout in milliseconds.
connect_timeout_ms = 200
"#
    }

    /// Returns the resolved data directory.
    ///
    /// If `self.storage.data_dir` is non-empty, it is returned as a `PathBuf`.
    /// Otherwise, the XDG-default data directory is used.
    pub fn resolved_data_dir(&self) -> PathBuf {
        if self.storage.data_dir.is_empty() {
            paths::data_dir()
        } else {
            PathBuf::from(&self.storage.data_dir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_expected_values() {
        let cfg = AxonConfig::default();
        assert_eq!(cfg.server.http_port, 4170);
        assert_eq!(cfg.storage.backend, "sqlite");
        assert_eq!(cfg.auth.mode, "no-auth");
        assert_eq!(cfg.auth.guest_role, "admin");
        assert_eq!(cfg.client.server_url, "http://localhost:4170");
        assert_eq!(cfg.client.connect_timeout_ms, 200);
        assert!(cfg.server.grpc_port.is_none());
    }

    #[test]
    fn load_none_returns_defaults() {
        let cfg = AxonConfig::load(None).expect("load(None) should succeed");
        assert_eq!(cfg.server.http_port, 4170);
        assert_eq!(cfg.storage.backend, "sqlite");
    }

    #[test]
    fn load_valid_toml_merges_values() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[server]
http_port = 9999

[storage]
backend = "postgres"
"#,
        )
        .expect("write");

        let cfg = AxonConfig::load(Some(&path)).expect("load should succeed");
        assert_eq!(cfg.server.http_port, 9999);
        assert_eq!(cfg.storage.backend, "postgres");
        // Unspecified fields keep defaults
        assert_eq!(cfg.auth.mode, "no-auth");
        assert_eq!(cfg.client.connect_timeout_ms, 200);
    }

    #[test]
    fn load_malformed_toml_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not [ valid toml ugh").expect("write");

        let result = AxonConfig::load(Some(&path));
        assert!(result.is_err(), "malformed TOML should produce an error");
        let err = result.expect_err("malformed TOML should produce an error");
        let msg = err.to_string();
        assert!(
            msg.contains("failed to parse config"),
            "error message should mention parsing: {msg}"
        );
    }

    #[test]
    fn load_nonexistent_returns_defaults() {
        let path = Path::new("/tmp/axon-config-test-nonexistent-12345/config.toml");
        let cfg = AxonConfig::load(Some(path)).expect("nonexistent file should return defaults");
        assert_eq!(cfg.server.http_port, 4170);
    }

    #[test]
    fn default_toml_round_trips() {
        let toml_str = AxonConfig::default_toml();
        let cfg: AxonConfig =
            toml::from_str(toml_str).expect("default_toml() should parse as valid TOML");
        assert_eq!(cfg.server.http_port, 4170);
        assert_eq!(cfg.storage.backend, "sqlite");
        assert_eq!(cfg.auth.mode, "no-auth");
        assert_eq!(cfg.client.server_url, "http://localhost:4170");
    }

    #[test]
    fn resolved_data_dir_uses_override_when_set() {
        let mut cfg = AxonConfig::default();
        cfg.storage.data_dir = String::from("/custom/path");
        assert_eq!(cfg.resolved_data_dir(), PathBuf::from("/custom/path"));
    }

    #[test]
    fn resolved_data_dir_uses_xdg_when_empty() {
        let cfg = AxonConfig::default();
        let resolved = cfg.resolved_data_dir();
        assert!(
            resolved.ends_with("axon"),
            "resolved_data_dir should end with 'axon', got: {resolved:?}"
        );
    }
}
