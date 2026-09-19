use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct CompileFailCase {
    name: &'static str,
    code: &'static str,
    expected: &'static [&'static str],
}

const CASES: &[CompileFailCase] = &[
    CompileFailCase {
        name: "handler_storage_mut",
        code: r#"
use axon_api::handler::AxonHandler;
use axon_storage::MemoryStorageAdapter;

fn main() {
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());
    let _storage = handler.storage_mut();
}
"#,
        expected: &["no method named `storage_mut`"],
    },
    CompileFailCase {
        name: "handler_into_storage",
        code: r#"
use axon_api::handler::AxonHandler;
use axon_storage::MemoryStorageAdapter;

fn main() {
    let handler = AxonHandler::new(MemoryStorageAdapter::default());
    let _storage = handler.into_storage();
}
"#,
        expected: &["no method named `into_storage`"],
    },
    CompileFailCase {
        name: "handler_audit_and_storage_accessors",
        code: r#"
use axon_api::handler::AxonHandler;
use axon_storage::MemoryStorageAdapter;

fn main() {
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());
    let _audit = handler.audit_log_mut();
    let _combined = handler.storage_and_audit_mut();
}
"#,
        expected: &[
            "no method named `audit_log_mut`",
            "no method named `storage_and_audit_mut`",
        ],
    },
    CompileFailCase {
        name: "cursor_store_raw_access",
        code: r#"
use axon_storage::{cursor_store::StorageCursorStore, MemoryStorageAdapter};

fn main() {
    let mut store = StorageCursorStore::new(MemoryStorageAdapter::default());
    let _storage = store.storage_mut();
    let _inner = store.into_inner();
}
"#,
        expected: &[
            "no method named `storage_mut`",
            "no method named `into_inner`",
        ],
    },
    CompileFailCase {
        name: "concrete_adapter_raw_connections",
        code: r#"
use axon_storage::{PostgresStorageAdapter, SqliteStorageAdapter};

fn sqlite(adapter: SqliteStorageAdapter) {
    let _pool = adapter.pool;
    let _runtime = adapter.rt;
}

fn postgres(adapter: PostgresStorageAdapter) {
    let _pool = adapter.pool;
    let _runtime = adapter.rt;
}

fn main() {}
"#,
        expected: &[
            "field `pool` of struct `SqliteStorageAdapter` is private",
            "field `pool` of struct `PostgresStorageAdapter` is private",
        ],
    },
    CompileFailCase {
        name: "capability_construction",
        code: r#"
use axon_core::{CheckpointCapability, GovernedWriteTx, MigrationCapability};

fn main() {
    let _governed = GovernedWriteTx::storage_adapter();
    let _migration = MigrationCapability::storage_migration();
    let _checkpoint = CheckpointCapability::storage_checkpoint();
    let _forged = GovernedWriteTx {};
}
"#,
        expected: &[
            "no function or associated item named `storage_adapter`",
            "no function or associated item named `storage_migration`",
            "no function or associated item named `storage_checkpoint`",
            "cannot construct `GovernedWriteTx` with struct literal syntax due to private fields",
        ],
    },
    CompileFailCase {
        name: "test_fixtures_no_unchecked_helpers",
        code: r#"
use axon_api::test_fixtures;
use axon_storage::MemoryStorageAdapter;

fn main() {
    let _put = test_fixtures::put_collection_view_unchecked_fixture::<MemoryStorageAdapter>;
    let _drop = test_fixtures::drop_database_unchecked_fixture::<MemoryStorageAdapter>;
    let _intent = test_fixtures::create_mutation_intent_unchecked_fixture::<MemoryStorageAdapter>;
}
"#,
        expected: &[
            "cannot find value `put_collection_view_unchecked_fixture` in module `test_fixtures`",
            "cannot find value `drop_database_unchecked_fixture` in module `test_fixtures`",
            "cannot find value `create_mutation_intent_unchecked_fixture` in module `test_fixtures`",
        ],
    },
];

#[test]
fn raw_access_compile_fail_cases() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("axon-api should live under crates/");
    let output_root = workspace_root
        .join("target")
        .join("raw_access_compile_fail");
    let cases_root = output_root.join("cases");
    let target_dir = output_root.join("target");

    if cases_root.exists() {
        fs::remove_dir_all(&cases_root).expect("remove stale compile-fail cases");
    }
    fs::create_dir_all(&cases_root).expect("create compile-fail cases root");

    for case in CASES {
        run_compile_fail_case(workspace_root, &cases_root, &target_dir, case);
    }
}

fn run_compile_fail_case(
    workspace_root: &Path,
    cases_root: &Path,
    target_dir: &Path,
    case: &CompileFailCase,
) {
    let case_dir = cases_root.join(case.name);
    let src_dir = case_dir.join("src");
    fs::create_dir_all(&src_dir).expect("create compile-fail case src");
    fs::write(
        case_dir.join("Cargo.toml"),
        case_manifest(workspace_root, case.name),
    )
    .expect("write compile-fail case manifest");
    fs::write(src_dir.join("main.rs"), case.code).expect("write compile-fail case source");

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(case_dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target_dir)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .unwrap_or_else(|err| panic!("failed to run cargo for {}: {err}", case.name));

    assert!(
        !output.status.success(),
        "{} unexpectedly compiled successfully",
        case.name
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in case.expected {
        assert!(
            stderr.contains(expected),
            "{} stderr did not contain {:?}\n--- stderr ---\n{}",
            case.name,
            expected,
            stderr
        );
    }
}

fn case_manifest(workspace_root: &Path, name: &str) -> String {
    let axon_api = workspace_root.join("crates/axon-api");
    let axon_core = workspace_root.join("crates/axon-core");
    let axon_storage = workspace_root.join("crates/axon-storage");

    format!(
        r#"[workspace]

[package]
name = "raw-access-{name}"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
axon-api = {{ path = "{}", default-features = false, features = ["test-fixtures"] }}
axon-core = {{ path = "{}" }}
axon-storage = {{ path = "{}" }}
"#,
        axon_api.display(),
        axon_core.display(),
        axon_storage.display()
    )
}
