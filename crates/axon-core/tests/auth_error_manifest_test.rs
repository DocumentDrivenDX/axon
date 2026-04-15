//! Parity test: every AuthError variant must have a row in
//! schema/auth-errors.manifest.json with the correct (status, code).

use axon_core::auth::AuthError;

#[test]
fn manifest_matches_rust_variants() {
    // Resolve the manifest path relative to the workspace root.
    // CARGO_MANIFEST_DIR is crates/axon-core, so go up twice.
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/
        .unwrap()
        .parent() // workspace root
        .unwrap()
        .join("schema/auth-errors.manifest.json");

    let raw = std::fs::read_to_string(&manifest_path).expect("read manifest");
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let variants = json["variants"].as_array().unwrap();

    // Walk every variant in the manifest, construct the corresponding
    // AuthError and assert its status_code() and error_code() match.
    for v in variants {
        let rust_name = v["rust_name"].as_str().unwrap();
        let expected_code = v["code"].as_str().unwrap();
        let expected_status = v["status"].as_u64().unwrap() as u16;

        let err = auth_error_by_name(rust_name);
        assert_eq!(
            err.error_code(),
            expected_code,
            "code mismatch for {rust_name}"
        );
        assert_eq!(
            err.status_code(),
            expected_status,
            "status mismatch for {rust_name}"
        );
    }

    // Also assert the count matches AUTH_ERROR_VARIANT_COUNT.
    assert_eq!(variants.len(), axon_core::auth::AUTH_ERROR_VARIANT_COUNT);
}

fn auth_error_by_name(name: &str) -> AuthError {
    match name {
        "Unauthenticated" => AuthError::Unauthenticated,
        "CredentialMalformed" => AuthError::CredentialMalformed,
        "CredentialInvalid" => AuthError::CredentialInvalid,
        "CredentialExpired" => AuthError::CredentialExpired,
        "CredentialNotYetValid" => AuthError::CredentialNotYetValid,
        "CredentialRevoked" => AuthError::CredentialRevoked,
        "CredentialForeignIssuer" => AuthError::CredentialForeignIssuer,
        "CredentialWrongTenant" => AuthError::CredentialWrongTenant,
        "UserSuspended" => AuthError::UserSuspended,
        "NotATenantMember" => AuthError::NotATenantMember,
        "DatabaseNotGranted" => AuthError::DatabaseNotGranted,
        "OpNotGranted" => AuthError::OpNotGranted,
        "GrantsExceedIssuerRole" => AuthError::GrantsExceedIssuerRole,
        "GrantsExceedRole" => AuthError::GrantsExceedRole,
        "GrantsMalformed" => AuthError::GrantsMalformed,
        _ => panic!("unknown AuthError variant: {name}"),
    }
}
