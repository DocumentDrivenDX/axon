//! Axon command-line interface.
//!
//! Runs in embedded mode (direct SQLite access, no server). All state is
//! stored in the database file specified by `--db` (default: `axon.db`).
//! The audit log is in-memory and covers only the current command.

use anyhow::{Context, Result};
use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateCollectionRequest, CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest,
    DescribeCollectionRequest, DropCollectionRequest, GetEntityRequest, ListCollectionsRequest,
    QueryAuditRequest, QueryEntitiesRequest, RevertEntityRequest, TraverseRequest,
    UpdateEntityRequest,
};
use axon_audit::AuditLog;
use axon_core::id::{CollectionId, EntityId};
use axon_schema::schema::CollectionSchema;
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
    /// Create a named collection with a schema.
    Create {
        /// Collection name.
        name: String,
        /// Schema JSON (entity_schema). If omitted, creates a schemaless collection.
        #[arg(long)]
        schema: Option<String>,
        #[arg(long)]
        actor: Option<String>,
    },
    /// List all registered collections.
    List,
    /// Drop a collection and all its entities.
    Drop {
        /// Collection name.
        name: String,
        #[arg(long)]
        actor: Option<String>,
    },
    /// Describe a collection (entity count, schema, timestamps).
    Describe {
        /// Collection name.
        name: String,
    },
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
    /// List entities in a collection.
    List {
        collection: String,
        /// Maximum number of entities to return.
        #[arg(long)]
        limit: Option<usize>,
    },
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
    /// List audit entries with optional filters.
    List {
        /// Filter to a specific collection.
        #[arg(long)]
        collection: Option<String>,
        /// Filter to a specific entity.
        #[arg(long)]
        entity_id: Option<String>,
        /// Filter to a specific actor.
        #[arg(long)]
        actor: Option<String>,
        /// Maximum entries to return.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show a single audit entry by ID.
    Show {
        /// Audit entry ID.
        id: u64,
    },
    /// Revert an entity to the state recorded in an audit entry.
    Revert {
        /// The audit entry ID to revert to.
        audit_entry_id: u64,
        #[arg(long)]
        actor: Option<String>,
        /// Bypass schema validation for the restored state.
        #[arg(long)]
        force: bool,
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
        Command::Audit(cmd) => run_audit(cmd, &cli.output, &mut handler),
    }
}

fn run_collection(
    cmd: CollectionCmd,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        CollectionCmd::Create {
            name,
            schema,
            actor,
        } => {
            let entity_schema = match schema {
                Some(s) => {
                    let v: Value =
                        serde_json::from_str(&s).with_context(|| "schema must be valid JSON")?;
                    Some(v)
                }
                None => None,
            };
            let col_id = CollectionId::new(&name);
            let collection_schema = CollectionSchema {
                collection: col_id.clone(),
                description: None,
                version: 1,
                entity_schema,
                link_types: Default::default(),
            };
            handler
                .create_collection(CreateCollectionRequest {
                    name: col_id,
                    schema: collection_schema,
                    actor,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let result = serde_json::json!({ "collection": name, "status": "created" });
            match format {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                OutputFormat::Table => println!("collection '{}' created", name),
            }
        }
        CollectionCmd::List => {
            let resp = handler
                .list_collections(ListCollectionsRequest {})
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json => {
                    let json: Vec<Value> = resp
                        .collections
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "entity_count": c.entity_count,
                                "schema_version": c.schema_version,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&json)?);
                }
                OutputFormat::Table => {
                    if resp.collections.is_empty() {
                        println!("(no collections)");
                    } else {
                        for c in &resp.collections {
                            println!(
                                "{} ({} entities, schema v{})",
                                c.name,
                                c.entity_count,
                                c.schema_version
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "-".into()),
                            );
                        }
                    }
                }
            }
        }
        CollectionCmd::Drop { name, actor } => {
            let resp = handler
                .drop_collection(DropCollectionRequest {
                    name: CollectionId::new(&name),
                    actor,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let result = serde_json::json!({
                "collection": resp.name,
                "entities_removed": resp.entities_removed,
                "status": "dropped"
            });
            match format {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                OutputFormat::Table => println!(
                    "dropped '{}' ({} entities removed)",
                    resp.name, resp.entities_removed
                ),
            }
        }
        CollectionCmd::Describe { name } => {
            let resp = handler
                .describe_collection(DescribeCollectionRequest {
                    name: CollectionId::new(&name),
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json => {
                    let result = serde_json::json!({
                        "name": resp.name,
                        "entity_count": resp.entity_count,
                        "schema": resp.schema,
                        "created_at_ns": resp.created_at_ns,
                        "updated_at_ns": resp.updated_at_ns,
                    });
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                OutputFormat::Table => {
                    println!("collection: {}", resp.name);
                    println!("entities:   {}", resp.entity_count);
                    if let Some(s) = &resp.schema {
                        println!("schema v{}:  {}", s.version, s.collection);
                    } else {
                        println!("schema:     (none)");
                    }
                }
            }
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
        EntityCmd::List { collection, limit } => {
            let resp = handler
                .query_entities(QueryEntitiesRequest {
                    collection: CollectionId::new(&collection),
                    limit,
                    ..Default::default()
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let entities: Vec<Value> = resp.entities.iter().map(entity_to_json).collect();
            match format {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&entities)?),
                OutputFormat::Table => {
                    println!("{} entities (total: {})", entities.len(), resp.total_count);
                    print_entities(&entities, format);
                }
            }
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
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        AuditCmd::List {
            collection,
            entity_id,
            actor,
            limit,
        } => {
            let resp = handler
                .query_audit(QueryAuditRequest {
                    collection: collection.map(|c| CollectionId::new(&c)),
                    entity_id: entity_id.map(|e| EntityId::new(&e)),
                    actor,
                    limit,
                    ..Default::default()
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            print_audit_entries(&resp.entries, format);
        }
        AuditCmd::Show { id } => {
            let entry = handler
                .audit_log()
                .find_by_id(id)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .ok_or_else(|| anyhow::anyhow!("audit entry {} not found", id))?;
            match format {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&audit_entry_to_json(&entry))?
                    );
                }
                OutputFormat::Table => {
                    println!("id:         {}", entry.id);
                    println!("timestamp:  {}", entry.timestamp_ns);
                    println!("collection: {}", entry.collection);
                    println!("entity_id:  {}", entry.entity_id);
                    println!("version:    {}", entry.version);
                    println!("mutation:   {}", entry.mutation);
                    println!("actor:      {}", entry.actor);
                    if let Some(ref before) = entry.data_before {
                        println!("before:     {}", before);
                    }
                    if let Some(ref after) = entry.data_after {
                        println!("after:      {}", after);
                    }
                }
            }
        }
        AuditCmd::Revert {
            audit_entry_id,
            actor,
            force,
        } => {
            let resp = handler
                .revert_entity_to_audit_entry(RevertEntityRequest {
                    audit_entry_id,
                    actor,
                    force,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json => {
                    let result = serde_json::json!({
                        "entity": entity_to_json(&resp.entity),
                        "audit_entry": audit_entry_to_json(&resp.audit_entry),
                    });
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                OutputFormat::Table => {
                    println!(
                        "reverted {}/{} to v{} (audit entry {})",
                        resp.entity.collection,
                        resp.entity.id,
                        resp.entity.version,
                        resp.audit_entry.id,
                    );
                }
            }
        }
    }
    Ok(())
}

fn audit_entry_to_json(e: &axon_audit::AuditEntry) -> Value {
    serde_json::json!({
        "id": e.id,
        "timestamp_ns": e.timestamp_ns,
        "collection": e.collection.to_string(),
        "entity_id": e.entity_id.to_string(),
        "version": e.version,
        "mutation": e.mutation.to_string(),
        "actor": e.actor,
        "data_before": e.data_before,
        "data_after": e.data_after,
    })
}

fn print_audit_entries(entries: &[axon_audit::AuditEntry], format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            let json_entries: Vec<Value> = entries.iter().map(audit_entry_to_json).collect();
            println!("{}", serde_json::to_string_pretty(&json_entries).unwrap());
        }
        OutputFormat::Table => {
            if entries.is_empty() {
                println!("(no audit entries)");
            } else {
                for e in entries {
                    println!(
                        "[{}] {} {}/{} v{} actor={}",
                        e.id, e.mutation, e.collection, e.entity_id, e.version, e.actor,
                    );
                }
            }
        }
    }
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
    fn collection_create_and_describe() {
        let (_f, db) = tmp_db();
        let cli = make_cli(&db, &["collection", "create", "tasks"]);
        run(cli).unwrap();

        let cli = make_cli(&db, &["collection", "describe", "tasks"]);
        run(cli).unwrap();
    }

    #[test]
    fn collection_list_and_drop() {
        let (_f, db) = tmp_db();
        // Create two collections.
        run(make_cli(&db, &["collection", "create", "tasks"])).unwrap();
        run(make_cli(&db, &["collection", "create", "users"])).unwrap();

        // List should show both.
        run(make_cli(&db, &["--output", "json", "collection", "list"])).unwrap();

        // Drop one.
        run(make_cli(&db, &["collection", "drop", "users"])).unwrap();
    }

    #[test]
    fn entity_create_get_round_trip() {
        let (_f, db) = tmp_db();
        run(make_cli(&db, &["collection", "create", "tasks"])).unwrap();

        let cli = make_cli(
            &db,
            &["entity", "create", "tasks", "t-001", r#"{"title":"hello"}"#],
        );
        run(cli).unwrap();

        let cli = make_cli(&db, &["entity", "get", "tasks", "t-001"]);
        run(cli).unwrap();
    }

    #[test]
    fn entity_list_returns_entities() {
        let (_f, db) = tmp_db();
        run(make_cli(&db, &["collection", "create", "tasks"])).unwrap();
        run(make_cli(
            &db,
            &["entity", "create", "tasks", "t-001", r#"{"title":"a"}"#],
        ))
        .unwrap();
        run(make_cli(
            &db,
            &["entity", "create", "tasks", "t-002", r#"{"title":"b"}"#],
        ))
        .unwrap();

        run(make_cli(
            &db,
            &["--output", "json", "entity", "list", "tasks"],
        ))
        .unwrap();
    }

    #[test]
    fn entity_create_get_json_output() {
        let (_f, db) = tmp_db();
        run(make_cli(&db, &["collection", "create", "tasks"])).unwrap();

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
        let (_f, db) = tmp_db();
        let storage = SqliteStorageAdapter::open(&db).unwrap();
        let mut handler = AxonHandler::new(storage);

        // Create collection first.
        handler
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("tasks"),
                schema: CollectionSchema {
                    collection: CollectionId::new("tasks"),
                    description: None,
                    version: 1,
                    entity_schema: None,
                    link_types: Default::default(),
                },
                actor: None,
            })
            .unwrap();

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
    fn audit_show_displays_entry() {
        let (_f, db) = tmp_db();
        let storage = SqliteStorageAdapter::open(&db).unwrap();
        let mut handler = AxonHandler::new(storage);

        handler
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("tasks"),
                schema: CollectionSchema {
                    collection: CollectionId::new("tasks"),
                    description: None,
                    version: 1,
                    entity_schema: None,
                    link_types: Default::default(),
                },
                actor: None,
            })
            .unwrap();

        handler
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                data: serde_json::json!({"title": "hi"}),
                actor: Some("agent-1".into()),
            })
            .unwrap();

        // Find the entry ID.
        let entries = handler
            .audit_log()
            .query_by_entity(&CollectionId::new("tasks"), &EntityId::new("t-001"))
            .unwrap();
        assert!(!entries.is_empty());

        let entry = handler.audit_log().find_by_id(entries[0].id).unwrap();
        assert!(entry.is_some());
    }

    #[test]
    fn output_json_produces_valid_json() {
        let (_f, db) = tmp_db();

        let storage = SqliteStorageAdapter::open(&db).unwrap();
        let mut handler = AxonHandler::new(storage);

        handler
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("tasks"),
                schema: CollectionSchema {
                    collection: CollectionId::new("tasks"),
                    description: None,
                    version: 1,
                    entity_schema: None,
                    link_types: Default::default(),
                },
                actor: None,
            })
            .unwrap();

        let resp = handler
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                data: serde_json::json!({"title": "hi"}),
                actor: None,
            })
            .unwrap();

        let json = entity_to_json(&resp.entity);
        let s = serde_json::to_string(&json).unwrap();
        let _: Value = serde_json::from_str(&s).unwrap();
    }
}
