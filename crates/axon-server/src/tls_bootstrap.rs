//! Self-signed TLS material bootstrap for local development.
//!
//! Used by `axon serve --tls-self-signed` to ensure a usable `cert.pem` /
//! `key.pem` pair exists at a given location, generating one with `rcgen`
//! when missing.  Production deployments should provide their own CA-issued
//! certificates via `--tls-cert` / `--tls-key` and leave `--tls-self-signed`
//! unset.
//!
//! The generated certificate carries SANs for `localhost`, `127.0.0.1`,
//! `::1`, `0.0.0.0`, and the local hostname, and is valid for ten years.
//! Key material is written with mode `0600` on Unix.

use std::path::{Path, PathBuf};

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};

/// Resolve the default XDG-based location for self-signed TLS material.
///
/// Returns `($XDG_DATA_HOME/axon/tls/cert.pem, .../key.pem)`, falling back
/// to `$HOME/.local/share/axon/tls/` when `XDG_DATA_HOME` is unset.  If
/// neither is available (unusual), falls back to `./axon-tls/`.
pub fn default_tls_paths() -> (PathBuf, PathBuf) {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("axon")
        .join("tls");
    (base.join("cert.pem"), base.join("key.pem"))
}

/// Ensure a self-signed cert + key pair exists at `cert_path` / `key_path`.
///
/// If both files already exist, this is a no-op.  If either is missing,
/// a fresh self-signed certificate is generated and both files are
/// (re-)written.  The parent directory is created with mode `0700` if
/// needed, and key material is chmod'd to `0600` on Unix.
pub fn ensure_tls_material(cert_path: &Path, key_path: &Path) -> Result<(), String> {
    if cert_path.exists() && key_path.exists() {
        return Ok(());
    }

    let (cert_pem, key_pem) = generate_self_signed_pem()?;

    for path in [cert_path, key_path] {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating TLS dir {}: {e}", parent.display()))?;
            restrict_dir_mode(parent)?;
        }
    }

    write_file_secret(cert_path, cert_pem.as_bytes())?;
    write_file_secret(key_path, key_pem.as_bytes())?;

    tracing::warn!(
        cert = %cert_path.display(),
        key = %key_path.display(),
        "generated self-signed TLS certificate (dev only — do not expose publicly)"
    );

    Ok(())
}

fn generate_self_signed_pem() -> Result<(String, String), String> {
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");

    params.subject_alt_names = vec![
        SanType::DnsName(
            "localhost"
                .try_into()
                .map_err(|e| format!("SAN localhost: {e}"))?,
        ),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
    ];

    if let Some(hostname) = hostname_san() {
        if let Ok(dns) = hostname.as_str().try_into() {
            params.subject_alt_names.push(SanType::DnsName(dns));
        }
    }

    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::hours(1);
    params.not_after = now + time::Duration::days(365 * 10);

    let key = KeyPair::generate().map_err(|e| format!("generating TLS key: {e}"))?;
    let cert = params
        .self_signed(&key)
        .map_err(|e| format!("self-signing TLS cert: {e}"))?;

    Ok((cert.pem(), key.serialize_pem()))
}

fn hostname_san() -> Option<String> {
    let raw = std::env::var("HOSTNAME").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("localhost") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn write_file_secret(path: &Path, contents: &[u8]) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|e| format!("writing {}: {e}", path.display()))?;
    restrict_file_mode(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_file_mode(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod 600 {}: {e}", path.display()))
}

#[cfg(not(unix))]
fn restrict_file_mode(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn restrict_dir_mode(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod 700 {}: {e}", path.display()))
}

#[cfg(not(unix))]
fn restrict_dir_mode(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");

        ensure_tls_material(&cert, &key).expect("ensure");

        assert!(cert.exists());
        assert!(key.exists());

        // Parseable as PEM cert + PKCS8 key by rustls-pki-types (the same loader
        // serve.rs uses), so the output is wire-compatible with the listener.
        use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

        let certs: Vec<CertificateDer<'static>> =
            CertificateDer::pem_file_iter(&cert)
                .expect("open cert")
                .collect::<Result<_, _>>()
                .expect("parse certs");
        assert_eq!(certs.len(), 1);

        let _parsed_key = PrivateKeyDer::from_pem_file(&key)
            .expect("PEM contained a private key");
    }

    #[test]
    fn idempotent_when_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");

        ensure_tls_material(&cert, &key).expect("first");
        let cert_bytes = std::fs::read(&cert).expect("read cert");
        let key_bytes = std::fs::read(&key).expect("read key");

        ensure_tls_material(&cert, &key).expect("second");
        assert_eq!(cert_bytes, std::fs::read(&cert).expect("reread cert"));
        assert_eq!(key_bytes, std::fs::read(&key).expect("reread key"));
    }

    #[test]
    fn creates_parent_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cert = dir.path().join("nested").join("deep").join("cert.pem");
        let key = dir.path().join("nested").join("deep").join("key.pem");

        ensure_tls_material(&cert, &key).expect("ensure");

        assert!(cert.exists());
        assert!(key.exists());
    }

    #[cfg(unix)]
    #[test]
    fn key_is_mode_600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");

        ensure_tls_material(&cert, &key).expect("ensure");

        let mode = std::fs::metadata(&key).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "key.pem should be mode 0600, got {mode:o}");
        let cert_mode = std::fs::metadata(&cert).expect("stat").permissions().mode() & 0o777;
        assert_eq!(
            cert_mode, 0o600,
            "cert.pem should be mode 0600, got {cert_mode:o}"
        );
    }

    #[test]
    fn default_paths_use_xdg_or_home() {
        let (cert, key) = default_tls_paths();
        assert!(cert.ends_with("axon/tls/cert.pem"), "cert path: {cert:?}");
        assert!(key.ends_with("axon/tls/key.pem"), "key path: {key:?}");
    }
}
