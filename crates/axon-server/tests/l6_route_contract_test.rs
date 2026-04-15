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

// The `no_unprefixed_graphql_route_registrations` lexical check was removed
// because axum's `.nest("/tenants/:t/databases/:d", inner)` composition is
// not visible to a grep pass: the inner builder still contains `.route("/graphql"...)`
// strings even though the routes resolve to `/tenants/:t/databases/:d/graphql`
// at runtime. A runtime integration test that hits `/graphql` (without the
// tenant prefix) and asserts 404 is a better verification than a lexical
// grep, and it lives in the graphql_mutations / graphql_contract test files.
