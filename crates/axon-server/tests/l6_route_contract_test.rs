//! L6 route-contract guard: verifies that the `X-Axon-Database` header is no
//! longer referenced anywhere in the codebase (ADR-018, axon-130f129f).
//!
//! This is a single synchronous test that shells out to `git grep` from the
//! workspace root.  If the grep finds any matches the test fails and prints
//! the offending file paths so they can be cleaned up.

#[test]
fn no_x_axon_database_references() {
    // CARGO_MANIFEST_DIR is `crates/axon-server`; workspace root is two
    // levels up.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root");

    let output = std::process::Command::new("git")
        .arg("grep")
        .arg("-l")
        .arg("X-Axon-Database")
        .arg("--")
        .arg("crates/")
        .arg("sdk/")
        .current_dir(workspace_root)
        .output()
        .expect("git grep should run");

    let matches = String::from_utf8_lossy(&output.stdout);
    assert!(
        matches.trim().is_empty(),
        "X-Axon-Database still referenced in:\n{}",
        matches
    );
}
