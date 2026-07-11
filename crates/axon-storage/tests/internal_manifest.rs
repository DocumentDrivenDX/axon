use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Manifest {
    version: u32,
    required_names: Vec<NameClass>,
    collection_id_rules: Vec<CollectionIdRule>,
    physical_objects: Vec<PhysicalObject>,
    raw_mutation_targets: Vec<NameClass>,
    migration_exceptions: Vec<NameClass>,
    dml_boundary: DmlBoundaryManifest,
}

#[derive(Debug, Deserialize)]
struct NameClass {
    name: String,
    class: String,
}

#[derive(Debug, Deserialize)]
struct PhysicalObject {
    name: String,
    kind: String,
    class: String,
}

#[derive(Debug, Deserialize)]
struct CollectionIdRule {
    id: String,
    path: String,
    #[serde(default)]
    contains: Option<String>,
    #[serde(default)]
    not_contains: Vec<String>,
    class: String,
}

#[derive(Debug, Deserialize)]
struct DmlBoundaryManifest {
    rust_sources: Vec<String>,
    sql_sources: Vec<DmlSqlSource>,
    governed_tables: Vec<String>,
    governed_routines: Vec<String>,
    records: Vec<DmlRecord>,
    dynamic_sql_allowances: Vec<DmlDynamicSqlAllowance>,
    excluded_functions: Vec<DmlExcludedFunction>,
}

#[derive(Debug, Deserialize)]
struct DmlSqlSource {
    path: String,
    enclosing_function: String,
    kind: String,
}

#[derive(Debug, Deserialize)]
struct DmlRecord {
    source_file: String,
    enclosing_function: String,
    touched_tables: Vec<String>,
    touched_routines: Vec<String>,
    capability: String,
    mutation_class: String,
    co_commit: String,
    fault_test: String,
}

#[derive(Debug, Deserialize)]
struct DmlDynamicSqlAllowance {
    source_file: String,
    enclosing_function: String,
    entrypoint: String,
    allowance_class: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct DmlExcludedFunction {
    source_file: String,
    enclosing_function: String,
    reason: String,
}

#[derive(Debug)]
struct SourceLine {
    path: String,
    line_no: usize,
    line: String,
}

#[derive(Debug)]
struct ObjectUse {
    name: String,
    site: String,
}

#[test]
fn internal_manifest_required_names_are_unique_and_typed() {
    let manifest = load_manifest();
    assert_eq!(manifest.version, 1);

    let required = class_map(&manifest.required_names);
    for name in [
        "__axon_links__",
        "__axon_links_rev__",
        "_cdc_cursors",
        "link_set_version",
        "__axon_beads__",
        "__mutation_intents",
        "__axon_policies__",
        "auth",
        "schemas",
        "indexes",
        "audit",
        "idempotency",
        "projections",
    ] {
        assert!(
            required.contains_key(name),
            "internal manifest must classify required name {name}"
        );
    }

    assert_eq!(
        required.get("__axon_links__").map(String::as_str),
        Some("system_collection.link_forward_store")
    );
    assert_eq!(
        required.get("__axon_links_rev__").map(String::as_str),
        Some("system_collection.link_reverse_index")
    );
    assert_eq!(
        required.get("_cdc_cursors").map(String::as_str),
        Some("system_collection.checkpoint_cursor_store")
    );
    assert_eq!(
        required.get("__mutation_intents").map(String::as_str),
        Some("system_collection.mutation_intent_audit_subject")
    );
    assert_eq!(
        required.get("__axon_policies__").map(String::as_str),
        Some("system_collection.legacy_policy_alias")
    );

    assert_unique_rule_ids(&manifest.collection_id_rules);
    assert_unique_physical_objects(&manifest.physical_objects);
    assert_unique_name_classes(&manifest.raw_mutation_targets, "raw mutation target");
    assert_unique_name_classes(&manifest.migration_exceptions, "migration exception");
}

#[test]
fn internal_manifest_rejects_unmanifested() {
    let manifest = load_manifest();
    let fake_site = SourceLine {
        path: "crates/axon-storage/src/new_boundary.rs".to_owned(),
        line_no: 1,
        line: "let _ = CollectionId::new(\"__axon_new__\");".to_owned(),
    };
    assert!(
        validate_collection_id_sites(&manifest, &[fake_site]).is_err(),
        "an unmanifested CollectionId constructor must be rejected"
    );

    let physical = physical_class_map(&manifest);
    let fake_object = ObjectUse {
        name: "unmanifested_table".to_owned(),
        site: "crates/axon-storage/src/new_boundary.rs:2".to_owned(),
    };
    assert!(
        validate_object_uses("physical object", &[fake_object], &physical).is_err(),
        "an unmanifested physical object must be rejected"
    );
}

#[test]
fn internal_manifest_repository_inventory() {
    let manifest = load_manifest();
    let repo = repo_root();

    let collection_sites = collect_collection_id_sites(&repo);
    validate_collection_id_sites(&manifest, &collection_sites)
        .expect("all CollectionId::new call sites must be classified exactly once");

    let physical = physical_class_map(&manifest);
    let physical_uses = collect_create_objects(&repo);
    validate_object_uses("physical object", &physical_uses, &physical)
        .expect("all physical table/index/view/trigger/function names must be classified");

    let raw_targets = class_map(&manifest.raw_mutation_targets);
    let raw_uses = collect_raw_mutation_targets(&repo);
    validate_object_uses("raw mutation target", &raw_uses, &raw_targets)
        .expect("all raw mutation targets must be classified");

    let exceptions = class_map(&manifest.migration_exceptions);
    let migration_uses = collect_migration_objects(&repo);
    let mut migration_classes = physical;
    migration_classes.extend(exceptions);
    validate_object_uses("migration object", &migration_uses, &migration_classes)
        .expect("all migration/derived exceptions must be classified");

    validate_dml_boundary_manifest(&manifest, &repo)
        .expect("DML boundary records must stay consistent with internal manifest inventory");
}

fn load_manifest() -> Manifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("internal-manifest.json");
    let text = fs::read_to_string(&path).expect("internal manifest should be readable");
    serde_json::from_str(&text).expect("internal manifest should be valid JSON")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate should live under <repo>/crates/axon-storage")
        .to_path_buf()
}

fn class_map(items: &[NameClass]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for item in items {
        assert!(!item.class.is_empty(), "{} must have a class", item.name);
        assert!(
            map.insert(item.name.clone(), item.class.clone()).is_none(),
            "{} must have exactly one manifest class",
            item.name
        );
    }
    map
}

fn physical_class_map(manifest: &Manifest) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for item in &manifest.physical_objects {
        assert!(
            !item.kind.is_empty(),
            "{} must have a physical kind",
            item.name
        );
        assert!(!item.class.is_empty(), "{} must have a class", item.name);
        assert!(
            map.insert(item.name.clone(), item.class.clone()).is_none(),
            "{} must have exactly one manifest class",
            item.name
        );
    }
    map
}

fn assert_unique_rule_ids(rules: &[CollectionIdRule]) {
    let mut ids = BTreeSet::new();
    for rule in rules {
        assert!(!rule.class.is_empty(), "{} must have a class", rule.id);
        assert!(
            ids.insert(&rule.id),
            "duplicate CollectionId rule {}",
            rule.id
        );
    }
}

fn assert_unique_physical_objects(items: &[PhysicalObject]) {
    let mut names = BTreeSet::new();
    for item in items {
        assert!(
            names.insert((&item.name, &item.kind)),
            "duplicate physical object {} ({})",
            item.name,
            item.kind
        );
    }
}

fn assert_unique_name_classes(items: &[NameClass], label: &str) {
    let mut names = BTreeSet::new();
    for item in items {
        assert!(names.insert(&item.name), "duplicate {label} {}", item.name);
    }
}

fn validate_collection_id_sites(manifest: &Manifest, sites: &[SourceLine]) -> Result<(), String> {
    let mut failures = Vec::new();
    for site in sites {
        let matches: Vec<&CollectionIdRule> = manifest
            .collection_id_rules
            .iter()
            .filter(|rule| collection_rule_matches(rule, site))
            .collect();
        if matches.len() != 1 {
            failures.push(format!(
                "{}:{} matched {} CollectionId rules: {}",
                site.path,
                site.line_no,
                matches.len(),
                site.line.trim()
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn collection_rule_matches(rule: &CollectionIdRule, site: &SourceLine) -> bool {
    if rule.path != site.path {
        return false;
    }
    if let Some(required) = &rule.contains {
        if !site.line.contains(required) {
            return false;
        }
    }
    !rule
        .not_contains
        .iter()
        .any(|denied| site.line.contains(denied))
}

fn validate_object_uses(
    label: &str,
    uses: &[ObjectUse],
    classes: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for object_use in uses {
        if !classes.contains_key(&object_use.name) {
            failures.push(format!(
                "unmanifested {label} {} at {}",
                object_use.name, object_use.site
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn validate_dml_boundary_manifest(manifest: &Manifest, repo: &Path) -> Result<(), String> {
    let dml = &manifest.dml_boundary;
    let mut failures = Vec::new();
    let mut source_files: BTreeSet<String> = dml.rust_sources.iter().cloned().collect();
    for source in &dml.sql_sources {
        assert!(
            !source.enclosing_function.trim().is_empty(),
            "{} DML sql source must name an enclosing function",
            source.path
        );
        assert!(
            !source.kind.trim().is_empty(),
            "{} DML sql source must name a kind",
            source.path
        );
        source_files.insert(source.path.clone());
    }

    for source in &dml.rust_sources {
        if !repo.join(source).exists() {
            failures.push(format!("DML source file does not exist: {source}"));
        }
    }
    for source in &dml.sql_sources {
        if !repo.join(&source.path).exists() {
            failures.push(format!(
                "DML SQL source file does not exist: {}",
                source.path
            ));
        }
    }

    let physical = physical_class_map(manifest);
    let raw_targets = class_map(&manifest.raw_mutation_targets);
    let exceptions = class_map(&manifest.migration_exceptions);
    let mut allowed_tables: BTreeSet<String> = physical.keys().cloned().collect();
    allowed_tables.extend(raw_targets.keys().cloned());
    allowed_tables.extend(exceptions.keys().cloned());

    let governed_tables: BTreeSet<String> = dml.governed_tables.iter().cloned().collect();
    let governed_routines: BTreeSet<String> = dml.governed_routines.iter().cloned().collect();
    for table in &governed_tables {
        if !allowed_tables.contains(table) {
            failures.push(format!(
                "DML governed table {table} is not a physical object, raw target, or migration exception"
            ));
        }
    }

    let mut record_keys = BTreeSet::new();
    let mut record_sources = BTreeSet::new();
    for record in &dml.records {
        if !source_files.contains(&record.source_file) {
            failures.push(format!(
                "DML record for {} in {} references undeclared source",
                record.capability, record.source_file
            ));
        }
        if record.enclosing_function.trim().is_empty()
            || record.mutation_class.trim().is_empty()
            || record.co_commit.trim().is_empty()
            || record.fault_test.trim().is_empty()
        {
            failures.push(format!(
                "DML record {} in {} is missing function, mutation class, co-commit, or fault-test",
                record.capability, record.source_file
            ));
        }
        for table in &record.touched_tables {
            if !governed_tables.contains(table) {
                failures.push(format!(
                    "DML record {} touches undeclared governed table {table}",
                    record.capability
                ));
            }
        }
        for routine in &record.touched_routines {
            if !governed_routines.contains(routine) {
                failures.push(format!(
                    "DML record {} touches undeclared governed routine {routine}",
                    record.capability
                ));
            }
        }
        if record.touched_tables.len() == 1 && record.touched_routines.is_empty() {
            let expected = format!(
                "table:{}:{}",
                record.touched_tables[0], record.mutation_class
            );
            if record.capability != expected {
                failures.push(format!(
                    "DML record capability {} should be {expected}",
                    record.capability
                ));
            }
        }
        let key = (
            &record.source_file,
            &record.enclosing_function,
            &record.capability,
            &record.mutation_class,
        );
        if !record_keys.insert(key) {
            failures.push(format!(
                "duplicate DML record {} in {}::{}",
                record.capability, record.source_file, record.enclosing_function
            ));
        }
        record_sources.insert(record.source_file.clone());
    }

    for source in &dml.rust_sources {
        if !record_sources.contains(source) {
            failures.push(format!("DML source {source} has no checked records"));
        }
    }

    for allowance in &dml.dynamic_sql_allowances {
        if !source_files.contains(&allowance.source_file) {
            failures.push(format!(
                "dynamic SQL allowance references undeclared source {}",
                allowance.source_file
            ));
        }
        if allowance.enclosing_function.trim().is_empty()
            || allowance.entrypoint.trim().is_empty()
            || allowance.allowance_class.trim().is_empty()
            || allowance.reason.trim().is_empty()
        {
            failures.push(format!(
                "dynamic SQL allowance in {} is missing function, entrypoint, class, or reason",
                allowance.source_file
            ));
        }
    }

    for excluded in &dml.excluded_functions {
        if !source_files.contains(&excluded.source_file) {
            failures.push(format!(
                "excluded DML function references undeclared source {}",
                excluded.source_file
            ));
        }
        if excluded.enclosing_function.trim().is_empty() || excluded.reason.trim().is_empty() {
            failures.push(format!(
                "excluded DML function in {} is missing function or reason",
                excluded.source_file
            ));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn collect_collection_id_sites(repo: &Path) -> Vec<SourceLine> {
    collect_matching_lines(
        repo,
        &[
            "crates/axon-core/src",
            "crates/axon-storage/src",
            "crates/axon-storage/tests",
        ],
        "CollectionId::new(",
    )
}

fn collect_create_objects(repo: &Path) -> Vec<ObjectUse> {
    let mut uses = Vec::new();
    for source_line in collect_storage_source_lines(repo) {
        if source_line_is_comment(&source_line.line) {
            continue;
        }
        for keyword in [
            "CREATE TABLE",
            "CREATE INDEX",
            "CREATE VIEW",
            "CREATE TRIGGER",
            "CREATE FUNCTION",
        ] {
            if let Some(name) = name_after_keyword(&source_line.line, keyword, true) {
                uses.push(ObjectUse {
                    name,
                    site: format!("{}:{}", source_line.path, source_line.line_no),
                });
            }
        }
    }
    uses
}

fn collect_raw_mutation_targets(repo: &Path) -> Vec<ObjectUse> {
    let mut uses = Vec::new();
    for source_line in collect_storage_source_lines(repo) {
        if source_line_is_comment(&source_line.line) {
            continue;
        }
        for keyword in ["INSERT OR IGNORE INTO", "INSERT INTO", "DELETE FROM"] {
            if let Some(name) = name_after_keyword(&source_line.line, keyword, false) {
                uses.push(ObjectUse {
                    name,
                    site: format!("{}:{}", source_line.path, source_line.line_no),
                });
            }
        }

        let upper = source_line.line.to_ascii_uppercase();
        if !upper.contains("DO UPDATE") {
            if let Some(name) = name_after_keyword(&source_line.line, "UPDATE ", false) {
                uses.push(ObjectUse {
                    name,
                    site: format!("{}:{}", source_line.path, source_line.line_no),
                });
            }
        }

        for name in names_after_truncate(&source_line.line) {
            uses.push(ObjectUse {
                name,
                site: format!("{}:{}", source_line.path, source_line.line_no),
            });
        }
    }
    uses
}

fn collect_migration_objects(repo: &Path) -> Vec<ObjectUse> {
    let mut uses = Vec::new();
    for source_line in collect_storage_source_lines(repo) {
        if source_line_is_comment(&source_line.line) {
            continue;
        }
        for keyword in ["ALTER TABLE", "DROP TABLE", "RENAME TO"] {
            if let Some(name) = name_after_keyword(&source_line.line, keyword, true) {
                uses.push(ObjectUse {
                    name,
                    site: format!("{}:{}", source_line.path, source_line.line_no),
                });
            }
        }
    }
    uses
}

fn collect_storage_source_lines(repo: &Path) -> Vec<SourceLine> {
    collect_all_lines(repo, &["crates/axon-storage/src"])
}

fn collect_matching_lines(repo: &Path, roots: &[&str], needle: &str) -> Vec<SourceLine> {
    collect_all_lines(repo, roots)
        .into_iter()
        .filter(|line| line.line.contains(needle))
        .collect()
}

fn collect_all_lines(repo: &Path, roots: &[&str]) -> Vec<SourceLine> {
    let mut files = Vec::new();
    for root in roots {
        collect_rs_files(&repo.join(root), &mut files);
    }
    files.sort();

    let mut lines = Vec::new();
    for file in files {
        let rel = relative_path(repo, &file);
        if rel == "crates/axon-storage/tests/internal_manifest.rs" {
            continue;
        }
        let text = fs::read_to_string(&file).expect("source file should be readable");
        lines.extend(text.lines().enumerate().map(|(idx, line)| SourceLine {
            path: rel.clone(),
            line_no: idx + 1,
            line: line.to_owned(),
        }));
    }
    lines
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("source directory should be readable") {
        let entry = entry.expect("source directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn relative_path(repo: &Path, path: &Path) -> String {
    path.strip_prefix(repo)
        .expect("path should be under repo")
        .to_string_lossy()
        .replace('\\', "/")
}

fn source_line_is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
}

fn name_after_keyword(line: &str, keyword: &str, skip_if_not_exists: bool) -> Option<String> {
    let idx = line.find(keyword)?;
    let mut rest = line[idx + keyword.len()..].split_whitespace();
    if skip_if_not_exists && rest.clone().next().map(clean_ident) == Some("IF".to_owned()) {
        rest.next();
        rest.next();
        rest.next();
    }
    rest.next().map(clean_ident).filter(|name| !name.is_empty())
}

fn names_after_truncate(line: &str) -> Vec<String> {
    let Some(idx) = line.find("TRUNCATE") else {
        return Vec::new();
    };
    line[idx + "TRUNCATE".len()..]
        .split_whitespace()
        .take_while(|token| !token.eq_ignore_ascii_case("RESTART"))
        .map(clean_ident)
        .filter(|name| !name.is_empty())
        .collect()
}

fn clean_ident(token: &str) -> String {
    let trimmed = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | '(' | ')' | ',' | ';' | '\\' | '{' | '}'
        )
    });
    trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect()
}
