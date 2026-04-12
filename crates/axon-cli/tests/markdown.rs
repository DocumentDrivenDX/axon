use std::process::Command;

use axon_api::response::GetEntityMarkdownResponse;
use axon_core::id::{CollectionId, Namespace};
use axon_schema::schema::{CollectionSchema, CollectionView};
use axon_storage::{adapter::StorageAdapter, SqliteStorageAdapter};
use serde_json::json;
use tempfile::NamedTempFile;

fn axon_bin() -> &'static str {
    env!("CARGO_BIN_EXE_axon")
}

fn run(db: &str, args: &[&str]) -> std::process::Output {
    Command::new(axon_bin())
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("failed to run axon binary")
}

fn run_ok(db: &str, args: &[&str]) -> String {
    let output = run(db, args);
    assert!(
        output.status.success(),
        "command failed: {}\nstderr: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout should be valid UTF-8")
}

fn run_err(db: &str, args: &[&str]) -> String {
    let output = run(db, args);
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded: {}",
        args.join(" ")
    );
    String::from_utf8(output.stderr).expect("stderr should be valid UTF-8")
}

fn seed_namespaced_collection(db: &str, qualified: &str) {
    let mut storage = SqliteStorageAdapter::open(db).expect("open sqlite db");
    let qualified = CollectionId::new(qualified);
    let bare = CollectionId::new("tasks");
    let namespace = Namespace::new("prod", "billing");

    storage
        .create_database("prod")
        .expect("create database should succeed");
    storage
        .create_namespace(&namespace)
        .expect("create namespace should succeed");
    storage
        .register_collection_in_namespace(&bare, &namespace)
        .expect("register collection should succeed");
    storage
        .put_schema(&CollectionSchema {
            collection: qualified,
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"}
                },
                "required": ["title"]
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        })
        .expect("put schema should succeed");
}

fn seed_invalid_template(db: &str, collection: &str, template: &str) {
    let mut storage = SqliteStorageAdapter::open(db).expect("open sqlite db");
    storage
        .put_collection_view(&CollectionView::new(
            CollectionId::new(collection),
            template,
        ))
        .expect("put invalid template directly should succeed");
}

#[test]
fn collection_template_commands_round_trip() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();

    run_ok(&db_path, &["collections", "create", "tasks"]);

    run_ok(
        &db_path,
        &[
            "collections",
            "template",
            "put",
            "tasks",
            "--template",
            "# {{title}}",
        ],
    );

    let get_output = run_ok(
        &db_path,
        &["--output", "json", "collections", "template", "get", "tasks"],
    );
    assert!(get_output.contains(r#""collection": "tasks""#));
    assert!(get_output.contains(r##""template": "# {{title}}""##));

    run_ok(&db_path, &["collections", "template", "delete", "tasks"]);

    let err_output = run_err(
        &db_path,
        &["--output", "json", "collections", "template", "get", "tasks"],
    );
    assert!(err_output.contains("has no markdown template defined"));
}

#[test]
fn qualified_collection_template_commands_preserve_namespace_identity() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();
    let qualified = "prod.billing.tasks";

    seed_namespaced_collection(&db_path, qualified);

    let put_output = run_ok(
        &db_path,
        &[
            "--output",
            "json",
            "collections",
            "template",
            "put",
            qualified,
            "--template",
            "# {{title}}",
        ],
    );
    assert!(put_output.contains(r#""collection": "prod.billing.tasks""#));

    let get_json = run_ok(
        &db_path,
        &[
            "--output",
            "json",
            "collections",
            "template",
            "get",
            qualified,
        ],
    );
    assert!(get_json.contains(r#""collection": "prod.billing.tasks""#));

    let get_table = run_ok(&db_path, &["collections", "template", "get", qualified]);
    assert!(get_table.contains("collection: prod.billing.tasks"));
}

#[test]
fn entity_get_can_render_markdown() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();

    run_ok(&db_path, &["collections", "create", "tasks"]);
    run_ok(
        &db_path,
        &[
            "collections",
            "template",
            "put",
            "tasks",
            "--template",
            "# {{title}}\n\nStatus: {{status}}",
        ],
    );
    run_ok(
        &db_path,
        &[
            "entities",
            "create",
            "tasks",
            "--id",
            "t-001",
            "--data",
            r#"{"title":"hello","status":"open"}"#,
        ],
    );

    let markdown = run_ok(
        &db_path,
        &["entities", "get", "tasks", "t-001", "--render", "markdown"],
    );
    assert_eq!(markdown.trim(), "# hello\n\nStatus: open");
}

#[test]
fn entity_get_markdown_honors_json_output() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();

    run_ok(&db_path, &["collections", "create", "tasks"]);
    run_ok(
        &db_path,
        &[
            "collections",
            "template",
            "put",
            "tasks",
            "--template",
            "# {{title}}\n\nStatus: {{status}}",
        ],
    );
    run_ok(
        &db_path,
        &[
            "entities",
            "create",
            "tasks",
            "--id",
            "t-001",
            "--data",
            r#"{"title":"hello","status":"open"}"#,
        ],
    );

    let output = run_ok(
        &db_path,
        &[
            "--output", "json", "entities", "get", "tasks", "t-001", "--render", "markdown",
        ],
    );
    let response: GetEntityMarkdownResponse =
        serde_json::from_str(&output).expect("stdout should be valid JSON");
    match response {
        GetEntityMarkdownResponse::Rendered {
            entity,
            rendered_markdown,
        } => {
            assert_eq!(rendered_markdown, "# hello\n\nStatus: open");
            assert_eq!(entity.collection.to_string(), "tasks");
            assert_eq!(entity.id.to_string(), "t-001");
        }
        GetEntityMarkdownResponse::RenderFailed { detail, .. } => {
            panic!("expected rendered markdown, got failure: {detail}");
        }
    }
}

#[test]
fn entity_get_markdown_honors_yaml_output() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();

    run_ok(&db_path, &["collections", "create", "tasks"]);
    run_ok(
        &db_path,
        &[
            "collections",
            "template",
            "put",
            "tasks",
            "--template",
            "# {{title}}\n\nStatus: {{status}}",
        ],
    );
    run_ok(
        &db_path,
        &[
            "entities",
            "create",
            "tasks",
            "--id",
            "t-001",
            "--data",
            r#"{"title":"hello","status":"open"}"#,
        ],
    );

    let output = run_ok(
        &db_path,
        &[
            "--output", "yaml", "entities", "get", "tasks", "t-001", "--render", "markdown",
        ],
    );
    let response: GetEntityMarkdownResponse =
        serde_yaml::from_str(&output).expect("stdout should be valid YAML");
    match response {
        GetEntityMarkdownResponse::Rendered {
            entity,
            rendered_markdown,
        } => {
            assert_eq!(rendered_markdown, "# hello\n\nStatus: open");
            assert_eq!(entity.collection.to_string(), "tasks");
            assert_eq!(entity.id.to_string(), "t-001");
        }
        GetEntityMarkdownResponse::RenderFailed { detail, .. } => {
            panic!("expected rendered markdown, got failure: {detail}");
        }
    }
}

#[test]
fn entity_get_markdown_json_output_preserves_render_failure_payload() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();

    run_ok(&db_path, &["collections", "create", "tasks"]);
    run_ok(
        &db_path,
        &[
            "entities",
            "create",
            "tasks",
            "--id",
            "t-001",
            "--data",
            r#"{"title":"hello","status":"open"}"#,
        ],
    );
    seed_invalid_template(&db_path, "tasks", "{{#title}");

    let output = run(
        &db_path,
        &[
            "--output", "json", "entities", "get", "tasks", "t-001", "--render", "markdown",
        ],
    );
    assert!(!output.status.success(), "command unexpectedly succeeded");

    let response: GetEntityMarkdownResponse =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    match response {
        GetEntityMarkdownResponse::RenderFailed { entity, detail } => {
            assert_eq!(entity.collection.to_string(), "tasks");
            assert_eq!(entity.id.to_string(), "t-001");
            assert_eq!(entity.data["title"], "hello");
            assert!(detail.contains("failed to render markdown"));
        }
        GetEntityMarkdownResponse::Rendered {
            rendered_markdown, ..
        } => {
            panic!("expected render failure, got markdown: {rendered_markdown}");
        }
    }
}

#[test]
fn entity_get_markdown_yaml_output_preserves_render_failure_payload() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();

    run_ok(&db_path, &["collections", "create", "tasks"]);
    run_ok(
        &db_path,
        &[
            "entities",
            "create",
            "tasks",
            "--id",
            "t-001",
            "--data",
            r#"{"title":"hello","status":"open"}"#,
        ],
    );
    seed_invalid_template(&db_path, "tasks", "{{#title}");

    let output = run(
        &db_path,
        &[
            "--output", "yaml", "entities", "get", "tasks", "t-001", "--render", "markdown",
        ],
    );
    assert!(!output.status.success(), "command unexpectedly succeeded");

    let response: GetEntityMarkdownResponse =
        serde_yaml::from_slice(&output.stdout).expect("stdout should be valid YAML");
    match response {
        GetEntityMarkdownResponse::RenderFailed { entity, detail } => {
            assert_eq!(entity.collection.to_string(), "tasks");
            assert_eq!(entity.id.to_string(), "t-001");
            assert_eq!(entity.data["title"], "hello");
            assert!(detail.contains("failed to render markdown"));
        }
        GetEntityMarkdownResponse::Rendered {
            rendered_markdown, ..
        } => {
            panic!("expected render failure, got markdown: {rendered_markdown}");
        }
    }
}
