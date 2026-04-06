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

#[derive(Clone, Debug, ValueEnum, Default)]
enum OutputFormat {
    #[default]
    Table,
    Json,
    Yaml,
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

    /// Schema operations.
    #[command(subcommand)]
    Schema(SchemaCmd),

    /// Show current configuration.
    Config,

    /// Bead (work item) management.
    #[command(subcommand)]
    Bead(BeadCmd),
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
    /// Query entities with filter expressions.
    ///
    /// Filters use `field=value` syntax (equality). Multiple filters are ANDed.
    Query {
        collection: String,
        /// Filter expressions (e.g. `status=open`). Multiple allowed, ANDed together.
        #[arg(long, num_args = 1..)]
        filter: Vec<String>,
        #[arg(long)]
        limit: Option<usize>,
        /// Return only the count of matching entities.
        #[arg(long)]
        count_only: bool,
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

// ── Schema commands ───────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum SchemaCmd {
    /// Show the schema for a collection.
    Show {
        /// Collection name.
        collection: String,
    },
    /// Validate a JSON file against a collection's schema.
    Validate {
        /// Collection name.
        collection: String,
        /// Path to a JSON file containing entity data.
        file: String,
    },
}

// ── Bead commands ─────────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum BeadCmd {
    /// Initialize the bead collection (idempotent).
    Init,
    /// Create a new bead.
    Create {
        /// Bead ID.
        id: String,
        /// Bead title.
        title: String,
        /// Bead type (e.g. task, bug, feature).
        #[arg(long, default_value = "task")]
        r#type: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = "0")]
        priority: u32,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        acceptance: Option<String>,
        /// Comma-separated tags.
        #[arg(long)]
        tags: Option<String>,
    },
    /// List beads, optionally filtered by status.
    List {
        #[arg(long)]
        status: Option<String>,
    },
    /// Show the ready queue (pending beads with all deps satisfied).
    Ready,
    /// Transition a bead to a new status.
    Transition {
        /// Bead ID.
        id: String,
        /// Target status.
        status: String,
    },
    /// Add a dependency: <id> depends-on <dep_id>.
    Dep {
        /// Bead ID.
        id: String,
        /// ID of the bead this one depends on.
        dep_id: String,
    },
    /// Show the dependency tree for a bead.
    Deps {
        /// Bead ID.
        id: String,
    },
    /// Export all beads to a JSON file.
    Export {
        /// Output file path (default: stdout).
        file: Option<String>,
    },
    /// Import beads from a JSON file.
    Import {
        /// Input file path.
        file: String,
    },
}

// ── Output helpers ─────────────────────────────────────────────────────────────

/// Serialize a value as JSON or YAML to stdout.
fn print_serialized(value: &(impl serde::Serialize + ?Sized), format: &OutputFormat) {
    match format {
        OutputFormat::Json | OutputFormat::Table => {
            println!("{}", serde_json::to_string_pretty(value).unwrap());
        }
        OutputFormat::Yaml => {
            println!("{}", serde_yaml::to_string(value).unwrap());
        }
    }
}

fn print_entity(entity_json: Value, format: &OutputFormat) {
    match format {
        OutputFormat::Json | OutputFormat::Yaml => print_serialized(&entity_json, format),
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
        OutputFormat::Json | OutputFormat::Yaml => print_serialized(entities, format),
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
        Command::Config => {
            let config = serde_json::json!({
                "db": cli.db,
                "output": format!("{:?}", cli.output),
                "mode": "embedded",
            });
            print_serialized(&config, &cli.output);
            Ok(())
        }
        Command::Schema(cmd) => run_schema(cmd, &cli.output, &handler),
        Command::Audit(cmd) => run_audit(cmd, &cli.output, &mut handler),
        Command::Bead(cmd) => run_bead(cmd, &cli.output, &mut handler),
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
                gates: Default::default(),
                validation_rules: Default::default(),
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
                OutputFormat::Json | OutputFormat::Yaml => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                OutputFormat::Table => println!("collection '{}' created", name),
            }
        }
        CollectionCmd::List => {
            let resp = handler
                .list_collections(ListCollectionsRequest {})
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
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
                OutputFormat::Json | OutputFormat::Yaml => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
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
                OutputFormat::Json | OutputFormat::Yaml => {
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
                OutputFormat::Json | OutputFormat::Yaml => {
                    println!("{}", serde_json::to_string_pretty(&entities)?);
                }
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
                    force: false,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => println!(
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
        EntityCmd::Query {
            collection,
            filter,
            limit,
            count_only,
        } => {
            use axon_api::request::{FieldFilter, FilterNode, FilterOp};

            // Parse --filter args: each is "field=value" (equality).
            let filter_node = if filter.is_empty() {
                None
            } else {
                let field_filters: Vec<FilterNode> = filter
                    .iter()
                    .map(|f| {
                        let (field, value) = f.split_once('=').unwrap_or((f, ""));
                        FilterNode::Field(FieldFilter {
                            field: field.to_string(),
                            op: FilterOp::Eq,
                            value: serde_json::json!(value),
                        })
                    })
                    .collect();
                if field_filters.len() == 1 {
                    Some(field_filters.into_iter().next().unwrap())
                } else {
                    Some(FilterNode::And {
                        filters: field_filters,
                    })
                }
            };

            let resp = handler
                .query_entities(QueryEntitiesRequest {
                    collection: CollectionId::new(&collection),
                    filter: filter_node,
                    limit,
                    count_only,
                    ..Default::default()
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            if count_only {
                match format {
                    OutputFormat::Table => println!("{} matching entities", resp.total_count),
                    _ => print_serialized(
                        &serde_json::json!({"total_count": resp.total_count}),
                        format,
                    ),
                }
            } else {
                let entities: Vec<Value> = resp.entities.iter().map(entity_to_json).collect();
                match format {
                    OutputFormat::Table => {
                        println!("{} entities (total: {})", entities.len(), resp.total_count);
                        print_entities(&entities, format);
                    }
                    _ => print_serialized(
                        &serde_json::json!({
                            "entities": entities,
                            "total_count": resp.total_count,
                        }),
                        format,
                    ),
                }
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
                OutputFormat::Json | OutputFormat::Yaml => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
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

fn run_schema(
    cmd: SchemaCmd,
    format: &OutputFormat,
    handler: &AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    use axon_schema::validation::validate;

    match cmd {
        SchemaCmd::Show { collection } => {
            let resp = handler
                .get_schema(&CollectionId::new(&collection))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match resp {
                Some(schema) => {
                    let json = serde_json::to_value(&schema)?;
                    print_serialized(&json, format);
                }
                None => {
                    return Err(anyhow::anyhow!(
                        "no schema registered for collection '{collection}'"
                    ));
                }
            }
        }
        SchemaCmd::Validate { collection, file } => {
            let schema = handler
                .get_schema(&CollectionId::new(&collection))
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .ok_or_else(|| {
                    anyhow::anyhow!("no schema registered for collection '{collection}'")
                })?;

            let content =
                std::fs::read_to_string(&file).with_context(|| format!("failed to read {file}"))?;
            let data: Value =
                serde_json::from_str(&content).with_context(|| "file must contain valid JSON")?;

            if schema.entity_schema.is_some() {
                match validate(&schema, &data) {
                    Ok(()) => match format {
                        OutputFormat::Table => println!("valid"),
                        _ => print_serialized(&serde_json::json!({"valid": true}), format),
                    },
                    Err(e) => match format {
                        OutputFormat::Table => {
                            eprintln!("validation failed: {e}");
                            std::process::exit(1);
                        }
                        _ => {
                            print_serialized(
                                &serde_json::json!({"valid": false, "error": e.to_string()}),
                                format,
                            );
                            std::process::exit(1);
                        }
                    },
                }
            } else {
                match format {
                    OutputFormat::Table => println!("no entity_schema defined; all data is valid"),
                    _ => print_serialized(
                        &serde_json::json!({"valid": true, "note": "no entity_schema defined"}),
                        format,
                    ),
                }
            }
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
                OutputFormat::Json | OutputFormat::Yaml => {
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
                OutputFormat::Json | OutputFormat::Yaml => {
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
        OutputFormat::Json | OutputFormat::Yaml => {
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

fn run_bead(
    cmd: BeadCmd,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    use axon_api::bead;

    match cmd {
        BeadCmd::Init => {
            bead::init_beads(handler).map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    println!(r#"{{"status":"initialized"}}"#);
                }
                OutputFormat::Table => println!("bead collection initialized"),
            }
        }
        BeadCmd::Create {
            id,
            title,
            r#type,
            description,
            priority,
            assignee,
            acceptance,
            tags,
        } => {
            let tag_vec: Vec<String> = tags
                .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();
            let b = bead::create_bead(
                handler,
                bead::CreateBeadParams {
                    id: &id,
                    bead_type: &r#type,
                    title: &title,
                    description: description.as_deref(),
                    priority,
                    assignee: assignee.as_deref(),
                    tags: &tag_vec,
                    acceptance: acceptance.as_deref(),
                },
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            print_bead(&b, format);
        }
        BeadCmd::List { status } => {
            let beads =
                bead::list_beads(handler, status.as_deref()).map_err(|e| anyhow::anyhow!("{e}"))?;
            print_beads(&beads, format);
        }
        BeadCmd::Ready => {
            let beads = bead::ready_queue(handler).map_err(|e| anyhow::anyhow!("{e}"))?;
            print_beads(&beads, format);
        }
        BeadCmd::Transition { id, status } => {
            bead::transition_bead(handler, &id, &status).map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "id": id, "status": status
                        }))?
                    );
                }
                OutputFormat::Table => println!("{} -> {}", id, status),
            }
        }
        BeadCmd::Dep { id, dep_id } => {
            bead::add_dependency(handler, &id, &dep_id).map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "id": id, "depends_on": dep_id
                        }))?
                    );
                }
                OutputFormat::Table => println!("{} depends-on {}", id, dep_id),
            }
        }
        BeadCmd::Deps { id } => {
            let deps = bead::dependency_tree(handler, &id).map_err(|e| anyhow::anyhow!("{e}"))?;
            print_beads(&deps, format);
        }
        BeadCmd::Export { file } => {
            let exported = bead::export_beads(handler).map_err(|e| anyhow::anyhow!("{e}"))?;
            let json = serde_json::to_string_pretty(&exported)?;
            match file {
                Some(path) => std::fs::write(&path, &json)
                    .with_context(|| format!("failed to write {path}"))?,
                None => println!("{json}"),
            }
        }
        BeadCmd::Import { file } => {
            let content =
                std::fs::read_to_string(&file).with_context(|| format!("failed to read {file}"))?;
            let data: Value =
                serde_json::from_str(&content).with_context(|| "file must contain valid JSON")?;
            let count = bead::import_beads(handler, &data).map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({"imported": count}))?
                    );
                }
                OutputFormat::Table => println!("imported {} beads", count),
            }
        }
    }
    Ok(())
}

fn bead_to_json(b: &axon_api::bead::Bead) -> Value {
    serde_json::json!({
        "id": b.id,
        "type": b.bead_type,
        "status": b.status,
        "title": b.title,
        "priority": b.priority,
        "assignee": b.assignee,
        "tags": b.tags,
        "description": b.description,
        "acceptance": b.acceptance,
    })
}

fn print_bead(b: &axon_api::bead::Bead, format: &OutputFormat) {
    match format {
        OutputFormat::Json | OutputFormat::Yaml => println!(
            "{}",
            serde_json::to_string_pretty(&bead_to_json(b)).unwrap()
        ),
        OutputFormat::Table => {
            println!(
                "[{}] {} ({}) p{} {}",
                b.status, b.title, b.bead_type, b.priority, b.id,
            );
        }
    }
}

fn print_beads(beads: &[axon_api::bead::Bead], format: &OutputFormat) {
    match format {
        OutputFormat::Json | OutputFormat::Yaml => {
            let json: Vec<Value> = beads.iter().map(bead_to_json).collect();
            println!("{}", serde_json::to_string_pretty(&json).unwrap());
        }
        OutputFormat::Table => {
            if beads.is_empty() {
                println!("(no beads)");
            } else {
                for b in beads {
                    println!(
                        "[{}] {} ({}) p{} {}",
                        b.status, b.title, b.bead_type, b.priority, b.id,
                    );
                }
            }
        }
    }
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
                gates: Default::default(),
                validation_rules: Default::default(),
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
                gates: Default::default(),
                validation_rules: Default::default(),
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
                gates: Default::default(),
                validation_rules: Default::default(),
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
