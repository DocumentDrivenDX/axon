//! L6 route-contract guard: verifies that the legacy database-selection header
//! is no longer referenced anywhere in the codebase (ADR-018, axon-130f129f).
//!
//! This is a single synchronous test that shells out to `git grep` from the
//! workspace root.  If the grep finds any matches the test fails and prints
//! the offending file paths so they can be cleaned up.
//!
//! The header name is built from fragments at runtime so this test file
//! does not match itself when the grep walks it.

#[test]
fn no_legacy_database_header_references() {
    // CARGO_MANIFEST_DIR is `crates/axon-server`; workspace root is two
    // levels up.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root");

    // Assemble the header name from fragments so this file's source does
    // not itself match the literal string.
    let header = format!("{}-{}-{}", "X", "Axon", "Database");

    let output = std::process::Command::new("git")
        .arg("grep")
        .arg("-l")
        .arg(&header)
        .arg("--")
        .arg("crates/")
        .arg("sdk/")
        .current_dir(workspace_root)
        .output()
        .expect("git grep should run");

    let matches = String::from_utf8_lossy(&output.stdout);
    // Filter out this file even if git somehow finds a match for it.
    let filtered: Vec<&str> = matches
        .lines()
        .filter(|line| !line.contains("l6_route_contract_test.rs"))
        .collect();
    assert!(
        filtered.is_empty(),
        "{} still referenced in:\n{}",
        header,
        filtered.join("\n")
    );
}

#[test]
fn no_unprefixed_graphql_route_registrations() {
    // Verify that no route registrations use "/graphql" without the
    // /tenants/{tenant}/databases/{database}/ prefix.  The playground at
    // /graphql/playground is a top-level dev convenience and is allowed.
    //
    // We grep source files only (not test files, which deliberately reference
    // the nested path).  The playground exception is handled by filtering.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root");

    // Search for bare "/graphql" route strings in src/ files.
    let output = std::process::Command::new("git")
        .arg("grep")
        .arg("-n")
        // Match literal "/graphql" not preceded by /tenants and not the playground
        .arg(r#""/graphql""#)
        .arg("--")
        .arg("crates/axon-server/src/")
        .current_dir(workspace_root)
        .output()
        .expect("git grep should run");

    let matches = String::from_utf8_lossy(&output.stdout);
    // Allow lines that reference the nested path or the playground endpoint.
    let violations: Vec<&str> = matches
        .lines()
        .filter(|line| {
            // Exclude the playground config line (contains "graphql/playground" context)
            // Exclude lines that contain the nested path
            !line.contains("tenants/default/databases/default/graphql")
                && !line.contains("graphql/playground")
                && !line.contains("graphql-ws")
                && !line.contains("graphql-transport-ws")
        })
        .collect();

    assert!(
        violations.is_empty(),
        "Bare \"/graphql\" route registration found (should be under /tenants/{{t}}/databases/{{d}}/graphql):\n{}",
        violations.join("\n")
    );
}
