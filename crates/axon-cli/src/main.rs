//! Axon command-line interface.
//!
//! Runs in embedded mode (direct SQLite access, no server). All state is
//! stored in the database file specified by `--db` (default: `axon.db`).
//! The audit log is in-memory and covers only the current command.

use anyhow::{Context, Result};
use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest, GetEntityRequest, TraverseRequest,
    UpdateEntityRequest,
};
use axon_audit::AuditLog;
use axon_core::id::{CollectionId, EntityId};
use axon_storage::SqliteStorageAdapter;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::Value;

// ── Output format ──────────────────────────────────────────────────────────────

#[derive(Clone, ValueEnum, Default)]
enum OutputFormat {
    #[default]
    Table,
    Json,
}

// ── CLI structure ──────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "axon", about = "Axon data store CLI", version)]
pub struct Cli {
    /// Path to the SQLite database file.
    #[arg(long, default_value = "axon.db", global = true)]
    db: String,

    /// Output format.
    #[arg(long, default_value = "table", global = true)]
    output: OutputFormat,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Collection management.
    #[command(subcommand)]
    Collection(CollectionCmd),

    /// Entity operations.
    #[command(subcommand)]
    Entity(EntityCmd),

    /// Link operations.
    #[command(subcommand)]
    Link(LinkCmd),

    /// Audit log queries.
    #[command(subcommand)]
    Audit(AuditCmd),
}

// ── Collection commands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum CollectionCmd {
    /// Register a collection (no-op in embedded mode; collections are implicit).
    Create {
        /// Collection name.
        name: String,
    },
    /// List all collections that contain at least one entity.
    List,
}

// ── Entity commands ────────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum EntityCmd {
    /// Create a new entity.
    Create {
        collection: String,
        id: String,
        /// Entity data as a JSON string.
        data: String,
        #[arg(long)]
        actor: Option<String>,
    },
    /// Retrieve an entity.
    Get { collection: String, id: String },
    /// Update an entity (optimistic concurrency control).
    Update {
        collection: String,
        id: String,
        /// Updated data as a JSON string.
        data: String,
        #[arg(long)]
        expected_version: u64,
        #[arg(long)]
        actor: Option<String>,
    },
    /// Delete an entity.
    Delete {
        collection: String,
        id: String,
        #[arg(long)]
        actor: Option<String>,
    },
}

// ── Link commands ──────────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum LinkCmd {
    /// Create a directed link between two entities.
    Create {
        #[arg(long)]
        source_collection: String,
        #[arg(long)]
        source_id: String,
        #[arg(long)]
        target_collection: String,
        #[arg(long)]
        target_id: String,
        #[arg(long)]
        link_type: String,
        #[arg(long)]
        actor: Option<String>,
    },
    /// Traverse links from a source entity.
    Traverse {
        collection: String,
        id: String,
        #[arg(long)]
        link_type: Option<String>,
        #[arg(long)]
        max_depth: Option<usize>,
    },
}

// ── Audit commands ─────────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum AuditCmd {
    /// List audit entries for an entity (current session only).
    List {
        collection: String,
        entity_id: String,
    },
}

// ── Output helpers ─────────────────────────────────────────────────────────────

fn print_entity(entity_json: Value, format: &OutputFormat) {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&entity_json).unwrap()),
        OutputFormat::Table => {
            println!(
                "collection={} id={} version={}",
                entity_json["collection"].as_str().unwrap_or(""),
                entity_json["id"].as_str().unwrap_or(""),
                entity_json["version"],
            );
            println!("data: {}", entity_json["data"]);
        }
    }
}

fn print_entities(entities: &[Value], format: &OutputFormat) {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(entities).unwrap()),
        OutputFormat::Table => {
            for e in entities {
                println!(
                    "{}/{} v{}  {}",
                    e["collection"].as_str().unwrap_or(""),
                    e["id"].as_str().unwrap_or(""),
                    e["version"],
                    e["data"],
                );
            }
        }
    }
}

// ── Entry point ────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    run(cli)
}

pub fn run(cli: Cli) -> Result<()> {
    let storage = SqliteStorageAdapter::open(&cli.db)
        .with_context(|| format!("failed to open database: {}", cli.db))?;
    let mut handler = AxonHandler::new(storage);

    match cli.command {
        Command::Collection(cmd) => run_collection(cmd, &cli.output, &mut handler),
        Command::Entity(cmd) => run_entity(cmd, &cli.output, &mut handler),
        Command::Link(cmd) => run_link(cmd, &cli.output, &mut handler),
        Command::Audit(cmd) => run_audit(cmd, &cli.output, &handler),
    }
}

fn run_collection(
    cmd: CollectionCmd,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        CollectionCmd::Create { name } => {
            // Collections are implicit — no storage action required.
            let result = serde_json::json!({ "collection": name, "status": "ok" });
            match format {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                OutputFormat::Table => println!("collection '{}' ready", name),
            }
        }
        CollectionCmd::List => {
            // Not implemented in V1 (requires range scan across all collections).
            let result = serde_json::json!({ "collections": [] });
            match format {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                OutputFormat::Table => println!("(no collections listed in this version)"),
            }
            let _ = handler; // suppress unused warning
        }
    }
    Ok(())
}

fn run_entity(
    cmd: EntityCmd,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        EntityCmd::Create {
            collection,
            id,
            data,
            actor,
        } => {
            let data: Value =
                serde_json::from_str(&data).with_context(|| "data must be valid JSON")?;
            let resp = handler
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                    data,
                    actor,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            print_entity(entity_to_json(&resp.entity), format);
        }
        EntityCmd::Get { collection, id } => {
            let resp = handler
                .get_entity(GetEntityRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            print_entity(entity_to_json(&resp.entity), format);
        }
        EntityCmd::Update {
            collection,
            id,
            data,
            expected_version,
            actor,
        } => {
            let data: Value =
                serde_json::from_str(&data).with_context(|| "data must be valid JSON")?;
            let resp = handler
                .update_entity(UpdateEntityRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                    data,
                    expected_version,
                    actor,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            print_entity(entity_to_json(&resp.entity), format);
        }
        EntityCmd::Delete {
            collection,
            id,
            actor,
        } => {
            let resp = handler
                .delete_entity(DeleteEntityRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                    actor,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "collection": resp.collection,
                        "id": resp.id,
                        "status": "deleted"
                    }))?
                ),
                OutputFormat::Table => println!("deleted {}/{}", resp.collection, resp.id),
            }
        }
    }
    Ok(())
}

fn run_link(
    cmd: LinkCmd,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        LinkCmd::Create {
            source_collection,
            source_id,
            target_collection,
            target_id,
            link_type,
            actor,
        } => {
            let resp = handler
                .create_link(CreateLinkRequest {
                    source_collection: CollectionId::new(&source_collection),
                    source_id: EntityId::new(&source_id),
                    target_collection: CollectionId::new(&target_collection),
                    target_id: EntityId::new(&target_id),
                    link_type,
                    metadata: serde_json::json!(null),
                    actor,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let link = &resp.link;
            let result = serde_json::json!({
                "source_collection": link.source_collection.to_string(),
                "source_id": link.source_id.to_string(),
                "target_collection": link.target_collection.to_string(),
                "target_id": link.target_id.to_string(),
                "link_type": link.link_type,
            });
            match format {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                OutputFormat::Table => println!(
                    "link {}/{} --[{}]--> {}/{}",
                    link.source_collection,
                    link.source_id,
                    link.link_type,
                    link.target_collection,
                    link.target_id,
                ),
            }
        }
        LinkCmd::Traverse {
            collection,
            id,
            link_type,
            max_depth,
        } => {
            let resp = handler
                .traverse(TraverseRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                    link_type,
                    max_depth,
                    direction: Default::default(),
                    hop_filter: None,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let entities: Vec<Value> = resp.entities.iter().map(entity_to_json).collect();
            print_entities(&entities, format);
        }
    }
    Ok(())
}

fn run_audit(
    cmd: AuditCmd,
    format: &OutputFormat,
    handler: &AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        AuditCmd::List {
            collection,
            entity_id,
        } => {
            let entries = handler
                .audit_log()
                .query_by_entity(&CollectionId::new(&collection), &EntityId::new(&entity_id))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json => {
                    let json_entries: Vec<Value> = entries
                        .iter()
                        .map(|e: &axon_audit::AuditEntry| {
                            serde_json::json!({
                                "id": e.id,
                                "timestamp_ns": e.timestamp_ns,
                                "collection": e.collection.to_string(),
                                "entity_id": e.entity_id.to_string(),
                                "version": e.version,
                                "mutation": e.mutation.to_string(),
                                "actor": e.actor,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&json_entries)?);
                }
                OutputFormat::Table => {
                    if entries.is_empty() {
                        println!("(no audit entries)");
                    } else {
                        for e in &entries {
                            println!(
                                "[{}] {:?} {}/{} v{} actor={}",
                                e.id, e.mutation, e.collection, e.entity_id, e.version, e.actor,
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn entity_to_json(e: &axon_core::types::Entity) -> Value {
    serde_json::json!({
        "collection": e.collection.to_string(),
        "id": e.id.to_string(),
        "version": e.version,
        "data": e.data,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn tmp_db() -> (NamedTempFile, String) {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_string_lossy().into_owned();
        (f, path)
    }

    fn make_cli(db: &str, args: &[&str]) -> Cli {
        let mut full = vec!["axon", "--db", db];
        full.extend_from_slice(args);
        Cli::parse_from(full)
    }

    #[test]
    fn collection_create_embedded_mode() {
        let (_f, db) = tmp_db();
        let cli = make_cli(&db, &["collection", "create", "tasks"]);
        run(cli).unwrap();
    }

    #[test]
    fn entity_create_get_round_trip() {
        let (_f, db) = tmp_db();

        let cli = make_cli(
            &db,
            &["entity", "create", "tasks", "t-001", r#"{"title":"hello"}"#],
        );
        run(cli).unwrap();

        let cli = make_cli(&db, &["entity", "get", "tasks", "t-001"]);
        run(cli).unwrap();
    }

    #[test]
    fn entity_create_get_json_output() {
        // Just verify it doesn't error; stdout capture not needed here.
        let (_f, db) = tmp_db();
        let cli = make_cli(
            &db,
            &[
                "--output",
                "json",
                "entity",
                "create",
                "tasks",
                "t-001",
                r#"{"title":"hello"}"#,
            ],
        );
        run(cli).unwrap();

        let cli = make_cli(
            &db,
            &["--output", "json", "entity", "get", "tasks", "t-001"],
        );
        run(cli).unwrap();
    }

    #[test]
    fn audit_list_shows_mutations() {
        // Create entity and audit list in the same handler (same session).
        let (_f, db) = tmp_db();
        let storage = SqliteStorageAdapter::open(&db).unwrap();
        let mut handler = AxonHandler::new(storage);

        handler
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                data: serde_json::json!({"title": "hi"}),
                actor: Some("agent-1".into()),
            })
            .unwrap();

        let entries = handler
            .audit_log()
            .query_by_entity(&CollectionId::new("tasks"), &EntityId::new("t-001"))
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].actor, "agent-1");
    }

    #[test]
    fn output_json_produces_valid_json() {
        let (_f, db) = tmp_db();

        // Create via handler to avoid needing stdout capture.
        let storage = SqliteStorageAdapter::open(&db).unwrap();
        let mut handler = AxonHandler::new(storage);
        let resp = handler
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                data: serde_json::json!({"title": "hi"}),
                actor: None,
            })
            .unwrap();

        let json = entity_to_json(&resp.entity);
        // Serialize and re-parse to verify it's valid JSON.
        let s = serde_json::to_string(&json).unwrap();
        let _: Value = serde_json::from_str(&s).unwrap();
    }
}
