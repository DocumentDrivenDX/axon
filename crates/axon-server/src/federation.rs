//! Identity federation: maps external identities (Tailscale, OIDC, email)
//! to first-class Axon users via the storage adapter's upsert path.
//!
//! This is the non-JWT arm of the auth pipeline. The JWT arm lives in
//! [`crate::auth_pipeline`]. The middleware that dispatches between the two
//! arms is wired in the path-router bead (D1), which comes later.

use axon_core::auth::User;
use axon_core::error::AxonError;
use axon_storage::StorageAdapter;

use crate::auth::TailscaleWhoisResponse;

/// Resolve a Tailscale tailnet identity to an Axon user, auto-provisioning
/// on first seen.
///
/// # Identity mapping
///
/// | Whois field | Axon mapping |
/// |-------------|-------------|
/// | `user_login` (non-empty) | `external_id` and `email` |
/// | `node_name` (fallback) | `external_id` when `user_login` is empty |
/// | `user_login` or `node_name` | `display_name` |
///
/// This matches the actor-resolution logic in
/// [`crate::auth::identity_from_tailscale`].
///
/// # Concurrency
///
/// Safe to call from multiple threads with the same `whois` simultaneously:
/// the storage adapter's `upsert_user_identity` guarantees that exactly one
/// `users` row and one `user_identities` row are created regardless of the
/// number of concurrent callers.
pub fn resolve_tailscale_identity(
    whois: &TailscaleWhoisResponse,
    storage: &dyn StorageAdapter,
) -> Result<User, AxonError> {
    let (external_id, email) = if whois.user_login.is_empty() {
        (whois.node_name.as_str(), None)
    } else {
        (whois.user_login.as_str(), Some(whois.user_login.as_str()))
    };
    let display_name = external_id;
    storage.upsert_user_identity("tailscale", external_id, display_name, email)
}
