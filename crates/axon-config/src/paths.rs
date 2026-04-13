//! Platform-appropriate path resolution for Axon.
//!
//! Uses the `dirs` crate to locate XDG-compliant directories on Linux
//! and `~/Library/Application Support/` on macOS. Falls back to
//! `~/.config/axon/` and `~/.local/share/axon/` if the platform directories
//! cannot be determined.

use std::path::PathBuf;

/// Returns the user-level configuration directory for Axon.
///
/// - Linux: `$XDG_CONFIG_HOME/axon/` (default `~/.config/axon/`)
/// - macOS: `~/Library/Application Support/axon/`
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        })
        .join("axon")
}

/// Returns the user-level data directory for Axon.
///
/// - Linux: `$XDG_DATA_HOME/axon/` (default `~/.local/share/axon/`)
/// - macOS: `~/Library/Application Support/axon/`
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local")
                .join("share")
        })
        .join("axon")
}

/// Returns the path to the user-level Axon config file.
///
/// Equivalent to `config_dir()/config.toml`.
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// Returns the default path for the SQLite database.
///
/// Equivalent to `data_dir()/axon.db`.
pub fn default_sqlite_path() -> PathBuf {
    data_dir().join("axon.db")
}

/// Returns the default path for the control-plane SQLite database.
///
/// Equivalent to `data_dir()/axon-control-plane.db`.
pub fn control_plane_sqlite_path() -> PathBuf {
    data_dir().join("axon-control-plane.db")
}

/// Returns the directory for per-tenant data.
///
/// Equivalent to `data_dir()/tenants/`.
pub fn tenants_dir() -> PathBuf {
    data_dir().join("tenants")
}

/// Returns the global (system-wide) configuration directory.
pub fn global_config_dir() -> PathBuf {
    PathBuf::from("/etc/axon")
}

/// Returns the global (system-wide) data directory.
pub fn global_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/axon")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_ends_with_axon() {
        let dir = config_dir();
        assert!(
            dir.ends_with("axon"),
            "config_dir() should end with 'axon', got: {dir:?}"
        );
    }

    #[test]
    fn data_dir_ends_with_axon() {
        let dir = data_dir();
        assert!(
            dir.ends_with("axon"),
            "data_dir() should end with 'axon', got: {dir:?}"
        );
    }

    #[test]
    fn config_file_ends_with_config_toml() {
        let path = config_file();
        assert!(
            path.ends_with("config.toml"),
            "config_file() should end with 'config.toml', got: {path:?}"
        );
    }

    #[test]
    fn default_sqlite_path_ends_with_axon_db() {
        let path = default_sqlite_path();
        assert!(
            path.ends_with("axon.db"),
            "default_sqlite_path() should end with 'axon.db', got: {path:?}"
        );
    }

    #[test]
    fn tenants_dir_ends_with_tenants() {
        let path = tenants_dir();
        assert!(
            path.ends_with("tenants"),
            "tenants_dir() should end with 'tenants', got: {path:?}"
        );
    }

    #[test]
    fn global_config_dir_is_etc_axon() {
        assert_eq!(global_config_dir(), PathBuf::from("/etc/axon"));
    }

    #[test]
    fn global_data_dir_is_var_lib_axon() {
        assert_eq!(global_data_dir(), PathBuf::from("/var/lib/axon"));
    }
}
