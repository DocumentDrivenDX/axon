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
        name: "capability_struct_literal",
        code: r#"
use axon_core::{BeadSystemCollection, GovernedSystemCapability};

fn main() {
    let _forged = GovernedSystemCapability::<BeadSystemCollection> {};
}
"#,
        expected: &["private fields"],
    },
    CompileFailCase {
        name: "capability_constructor",
        code: r#"
use axon_core::{BeadSystemCollection, GovernedSystemCapability};

fn main() {
    let _forged = GovernedSystemCapability::<BeadSystemCollection>::new();
}
"#,
        expected: &["no function or associated item named `new`"],
    },
    CompileFailCase {
        name: "capability_widen_to_raw_system_collection",
        code: r#"
use axon_core::{GovernedSystemCapability, SystemCollection, BEAD_SYSTEM_CAPABILITY};

fn main() {
    let _widened: GovernedSystemCapability<SystemCollection> = BEAD_SYSTEM_CAPABILITY;
}
"#,
        expected: &["GovernedSystemCollection"],
    },
    CompileFailCase {
        name: "capability_retarget",
        code: r#"
use axon_core::{SystemCollection, BEAD_SYSTEM_CAPABILITY};

fn main() {
    let _retargeted = BEAD_SYSTEM_CAPABILITY.with_collection(SystemCollection::links());
}
"#,
        expected: &["no method named `with_collection`"],
    },
    CompileFailCase {
        name: "capability_forge_module_marker",
        code: r#"
use axon_core::{GovernedSystemCollection, SystemCollection};

struct ForgedModule;

impl GovernedSystemCollection for ForgedModule {
    const SYSTEM_COLLECTION: SystemCollection = SystemCollection::links();
}

fn main() {}
"#,
        expected: &["Sealed"],
    },
    CompileFailCase {
        name: "system_collection_unmanifested_constructor",
        code: r#"
use axon_core::{SystemCollection, SystemCollectionClass};

fn main() {
    let _unknown = SystemCollection::new(
        "__axon_unknown__",
        SystemCollectionClass::BeadCatalog,
    );
}
"#,
        expected: &["associated function `new` is private"],
    },
];

#[test]
fn governed_system_compile_fail_cases() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("axon-api should live under crates/");
    let output_root = workspace_root
        .join("target")
        .join("governed_system_compile_fail");
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
name = "governed-system-{name}"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
axon-api = {{ path = "{}", default-features = false }}
axon-core = {{ path = "{}" }}
axon-storage = {{ path = "{}" }}
"#,
        axon_api.display(),
        axon_core.display(),
        axon_storage.display()
    )
}
