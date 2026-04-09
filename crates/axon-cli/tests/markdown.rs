use std::process::Command;

use axon_api::response::GetEntityMarkdownResponse;
use tempfile::NamedTempFile;

fn axon_bin() -> &'static str {
    env!("CARGO_BIN_EXE_axon")
}

fn run_ok(db: &str, args: &[&str]) -> String {
    let output = Command::new(axon_bin())
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("failed to run axon binary");
    assert!(
        output.status.success(),
        "command failed: {}\nstderr: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout should be valid UTF-8")
}

fn run_err(db: &str, args: &[&str]) -> String {
    let output = Command::new(axon_bin())
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("failed to run axon binary");
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded: {}",
        args.join(" ")
    );
    String::from_utf8(output.stderr).expect("stderr should be valid UTF-8")
}

#[test]
fn collection_template_commands_round_trip() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();

    run_ok(&db_path, &["collection", "create", "tasks"]);

    run_ok(
        &db_path,
        &[
            "collection",
            "template",
            "put",
            "tasks",
            "--template",
            "# {{title}}",
        ],
    );

    let get_output = run_ok(
        &db_path,
        &["--output", "json", "collection", "template", "get", "tasks"],
    );
    assert!(get_output.contains(r#""collection": "tasks""#));
    assert!(get_output.contains(r##""template": "# {{title}}""##));

    run_ok(&db_path, &["collection", "template", "delete", "tasks"]);

    let err_output = run_err(
        &db_path,
        &["--output", "json", "collection", "template", "get", "tasks"],
    );
    assert!(err_output.contains("has no markdown template defined"));
}

#[test]
fn entity_get_can_render_markdown() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();

    run_ok(&db_path, &["collection", "create", "tasks"]);
    run_ok(
        &db_path,
        &[
            "collection",
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
            "entity",
            "create",
            "tasks",
            "t-001",
            r#"{"title":"hello","status":"open"}"#,
        ],
    );

    let markdown = run_ok(
        &db_path,
        &["entity", "get", "tasks", "t-001", "--render", "markdown"],
    );
    assert_eq!(markdown.trim(), "# hello\n\nStatus: open");
}

#[test]
fn entity_get_markdown_honors_json_output() {
    let db = NamedTempFile::new().expect("temp db").into_temp_path();
    let db_path = db.to_string_lossy().into_owned();

    run_ok(&db_path, &["collection", "create", "tasks"]);
    run_ok(
        &db_path,
        &[
            "collection",
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
            "entity",
            "create",
            "tasks",
            "t-001",
            r#"{"title":"hello","status":"open"}"#,
        ],
    );

    let output = run_ok(
        &db_path,
        &[
            "--output", "json", "entity", "get", "tasks", "t-001", "--render", "markdown",
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

    run_ok(&db_path, &["collection", "create", "tasks"]);
    run_ok(
        &db_path,
        &[
            "collection",
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
            "entity",
            "create",
            "tasks",
            "t-001",
            r#"{"title":"hello","status":"open"}"#,
        ],
    );

    let output = run_ok(
        &db_path,
        &[
            "--output", "yaml", "entity", "get", "tasks", "t-001", "--render", "markdown",
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
