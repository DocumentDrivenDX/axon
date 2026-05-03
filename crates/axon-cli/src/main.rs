//! Axon command-line interface.
//!
//! Single binary for Axon: embedded CLI, HTTP server, MCP stdio,
//! diagnostics, and service management.

#![allow(clippy::print_stdout)]

#[cfg(feature = "serve")]
mod client;
mod doctor;
mod init;
mod service;

use anyhow::{Context, Result};
use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateCollectionRequest, CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest,
    CreateNamespaceRequest, DeleteCollectionTemplateRequest, DeleteEntityRequest,
    DescribeCollectionRequest, DropCollectionRequest, DropDatabaseRequest, DropNamespaceRequest,
    GetCollectionTemplateRequest, GetEntityRequest, ListCollectionsRequest, ListDatabasesRequest,
    ListNamespaceCollectionsRequest, ListNamespacesRequest, PutCollectionTemplateRequest,
    PutSchemaRequest, QueryAuditRequest, QueryEntitiesRequest, RevalidateRequest,
    RevertEntityRequest, TraverseRequest, UpdateEntityRequest,
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

#[derive(Clone, Debug, ValueEnum, Default)]
enum EntityRenderFormat {
    #[default]
    Json,
    Markdown,
}

// ── CLI structure ──────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "axon", about = "Axon data store CLI", version)]
pub struct Cli {
    /// Path to the SQLite database file (default: XDG data dir).
    #[arg(long, global = true)]
    db: Option<String>,

    /// Output format.
    #[arg(long, default_value = "table", global = true)]
    output: OutputFormat,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Database management.
    #[command(subcommand)]
    Database(DatabaseCmd),

    /// Namespace management.
    #[command(subcommand)]
    Namespace(NamespaceCmd),

    /// Collection management.
    #[command(subcommand)]
    Collections(CollectionCmd),

    /// Entity operations.
    #[command(subcommand)]
    Entities(EntityCmd),

    /// Link operations.
    #[command(subcommand)]
    Links(LinkCmd),

    /// Audit log queries.
    #[command(subcommand)]
    Audit(AuditCmd),

    /// Schema operations.
    #[command(subcommand)]
    Schema(SchemaCmd),

    /// Show current configuration.
    #[command(subcommand)]
    Config(ConfigCmd),

    /// Bead (work item) management.
    #[command(subcommand)]
    Bead(BeadCmd),

    /// Start the Axon server (HTTP gateway, optional gRPC).
    #[cfg(feature = "serve")]
    Serve(axon_server::serve::ServeArgs),

    /// Start MCP server over stdin/stdout.
    #[cfg(feature = "serve")]
    Mcp {
        /// Backing storage adapter.
        #[arg(long, env = "AXON_STORAGE", value_enum, default_value = "sqlite")]
        storage: axon_server::serve::StorageBackend,
        /// SQLite database path.
        #[arg(long, env = "AXON_SQLITE_PATH", default_value = "axon.db")]
        sqlite_path: String,
        /// PostgreSQL DSN.
        #[arg(long, env = "AXON_POSTGRES_DSN")]
        postgres_dsn: Option<String>,
    },

    /// Show diagnostic information about the Axon installation.
    Doctor,

    /// Initialize a new Axon project.
    Init {
        /// Project name (also used as directory name).
        name: String,
    },

    /// Manage Axon as a system service (install, start, stop, …).
    #[command(subcommand)]
    Server(ServerCmd),

    /// Traverse entity relationships as a graph (shorthand for `links traverse`).
    Graph {
        /// Source collection.
        collection: String,
        /// Source entity ID.
        id: String,
        /// Restrict traversal to this link type.
        #[arg(long, short = 't')]
        link_type: Option<String>,
        /// Maximum hop depth (default: 1).
        #[arg(long, short = 'd', default_value = "1")]
        depth: usize,
    },

    /// Manage per-principal role assignments.
    #[cfg(feature = "serve")]
    #[command(subcommand)]
    User(UserCmd),

    /// Manage CORS allowed origins for browser-based clients.
    #[cfg(feature = "serve")]
    #[command(subcommand)]
    Cors(CorsCmd),
}

#[cfg(feature = "serve")]
#[derive(Subcommand, Clone)]
enum UserCmd {
    /// Grant a role to a principal (creates or updates the assignment).
    Grant {
        /// The user's login name (e.g. `erik@example.com`).
        login: String,
        /// The role to assign: `admin`, `write`, or `read`.
        role: String,
    },
    /// Revoke the explicit role assignment for a principal.
    ///
    /// After revocation, the principal falls back to tag-based or default role
    /// resolution on their next request.
    Revoke {
        /// The user's login name.
        login: String,
    },
    /// List all explicit user-role assignments.
    List,
}

#[cfg(feature = "serve")]
#[derive(Subcommand, Clone)]
enum CorsCmd {
    /// Add an allowed CORS origin (e.g. `https://sindri:5173`).
    ///
    /// Use `*` to enable wildcard mode, which allows all origins.
    Add {
        /// The origin to allow (scheme + host + optional port).
        origin: String,
    },
    /// Remove an allowed CORS origin.
    Remove {
        /// The origin to remove.
        origin: String,
    },
    /// List all allowed CORS origins.
    List,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Print resolved configuration.
    Show,
    /// Print config file path.
    Path,
}

#[derive(Subcommand)]
enum ServerCmd {
    /// Install Axon as a user service (systemd on Linux, launchd on macOS).
    Install {
        /// Install as a system-wide service (requires root).
        #[arg(long)]
        global: bool,
    },
    /// Remove the Axon service.
    Uninstall,
    /// Start the Axon service.
    Start,
    /// Stop the Axon service.
    Stop,
    /// Restart the Axon service.
    Restart,
    /// Show Axon service status.
    Status,
}

#[derive(Subcommand)]
enum DatabaseCmd {
    /// Create a database and its default schema.
    Create { name: String },
    /// List databases.
    List,
    /// Drop a database.
    Drop {
        name: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
enum NamespaceCmd {
    /// Create a schema namespace within a database.
    Create {
        #[arg(long)]
        database: String,
        schema: String,
    },
    /// List schemas within a database.
    List {
        #[arg(long)]
        database: String,
    },
    /// List collections within a namespace.
    Collections {
        #[arg(long)]
        database: String,
        schema: String,
    },
    /// Drop a schema namespace.
    Drop {
        #[arg(long)]
        database: String,
        schema: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        confirm: bool,
    },
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
        /// Confirm the destructive operation.
        #[arg(long)]
        confirm: bool,
    },
    /// Describe a collection (entity count, schema, timestamps).
    Describe {
        /// Collection name.
        name: String,
    },
    /// Manage a collection's markdown template.
    #[command(subcommand)]
    Template(CollectionTemplateCmd),
}

#[derive(Subcommand)]
enum CollectionTemplateCmd {
    /// Save or replace the markdown template for a collection.
    Put {
        collection: String,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        actor: Option<String>,
    },
    /// Show the markdown template for a collection.
    Get { collection: String },
    /// Delete the markdown template for a collection.
    Delete {
        collection: String,
        #[arg(long)]
        actor: Option<String>,
    },
}

// ── Entity commands ────────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum EntityCmd {
    /// Create a new entity.
    Create {
        /// Collection name.
        collection: String,
        /// Entity ID.
        #[arg(long, short = 'i')]
        id: String,
        /// Entity data as a JSON string.
        #[arg(long, short = 'd')]
        data: String,
        #[arg(long)]
        actor: Option<String>,
    },
    /// Retrieve an entity.
    Get {
        collection: String,
        id: String,
        #[arg(long, default_value = "json")]
        render: EntityRenderFormat,
    },
    /// List entities in a collection.
    List {
        collection: String,
        /// Maximum number of entities to return.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Update an entity (optimistic concurrency control).
    ///
    /// If --expected-version is omitted the current version is fetched automatically.
    Update {
        collection: String,
        id: String,
        /// Updated data as a JSON string.
        #[arg(long, short = 'd')]
        data: String,
        /// Expected version for OCC. Auto-fetched if omitted.
        #[arg(long)]
        expected_version: Option<u64>,
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
    /// Set a directed link between two entities (positional shorthand).
    ///
    /// Usage: axon links set <src-collection> <src-id> <tgt-collection> <tgt-id> --type <type>
    Set {
        source_collection: String,
        source_id: String,
        target_collection: String,
        target_id: String,
        /// Link type label.
        #[arg(long = "type", short = 't')]
        link_type: String,
        #[arg(long)]
        actor: Option<String>,
    },
    /// List direct outbound links from an entity.
    List {
        collection: String,
        id: String,
        /// Restrict to this link type.
        #[arg(long, short = 't')]
        link_type: Option<String>,
    },
    /// Traverse links from a source entity (multi-hop graph walk).
    Traverse {
        collection: String,
        id: String,
        /// Restrict traversal to this link type.
        #[arg(long, short = 't')]
        link_type: Option<String>,
        /// Maximum hop depth (default: unlimited).
        #[arg(long, short = 'd')]
        max_depth: Option<usize>,
    },
    /// Create a directed link (explicit long-form flags).
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
    /// Set (replace) the entity schema for a collection.
    ///
    /// Accepts either a JSON Schema object (for the entity_schema field) or a
    /// path to a file containing the same. The version is bumped automatically.
    Set {
        /// Collection name.
        collection: String,
        /// JSON Schema as a string (conflicts with --file).
        #[arg(long, short = 's', conflicts_with = "file")]
        schema: Option<String>,
        /// Path to a JSON file containing the schema (conflicts with --schema).
        #[arg(long, short = 'f', conflicts_with = "schema")]
        file: Option<String>,
        /// Apply even if the change is breaking.
        #[arg(long)]
        force: bool,
        /// Preview the diff without applying.
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        actor: Option<String>,
    },
    /// Validate a JSON file against a collection's schema.
    Validate {
        /// Collection name.
        collection: String,
        /// Path to a JSON file containing entity data.
        file: String,
    },
    /// Revalidate all entities in a collection against the current schema.
    Revalidate {
        /// Collection name.
        collection: String,
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
        OutputFormat::Json | OutputFormat::Table => match serde_json::to_string_pretty(value) {
            Ok(serialized) => println!("{serialized}"),
            Err(err) => panic!("failed to serialize CLI output as JSON: {err}"),
        },
        OutputFormat::Yaml => match serde_yaml::to_string(value) {
            Ok(serialized) => println!("{serialized}"),
            Err(err) => panic!("failed to serialize CLI output as YAML: {err}"),
        },
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

fn collection_template_to_json(
    view: &axon_schema::schema::CollectionView,
    warnings: &[String],
) -> Value {
    serde_json::json!({
        "collection": view.collection.to_string(),
        "template": view.markdown_template,
        "version": view.version,
        "updated_at_ns": view.updated_at_ns,
        "updated_by": view.updated_by,
        "warnings": warnings,
    })
}

fn read_template_source(template: Option<String>, file: Option<String>) -> Result<String> {
    match (template, file) {
        (Some(template), None) => Ok(template),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read template file: {path}")),
        (Some(_), Some(_)) => anyhow::bail!("provide either --template or --file, not both"),
        (None, None) => anyhow::bail!("template content is required via --template or --file"),
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
    // ── Commands that don't need a database ──────────────────────────────
    match &cli.command {
        #[cfg(feature = "serve")]
        Command::Serve(_) | Command::Mcp { .. } => {
            return run_server_command(cli);
        }
        Command::Doctor => return doctor::run_doctor(),
        Command::Init { ref name } => return init::run_init(name),
        Command::Server(cmd) => {
            return run_service_command(cmd);
        }
        Command::Config(ConfigCmd::Path) => {
            println!("{}", axon_config::paths::config_file().display());
            return Ok(());
        }
        // user/cors write directly to the control-plane SQLite; they never need
        // the data DB and must not go through the HTTP server (auth would reject
        // them if the caller is not on the tailnet with admin rights).
        #[cfg(feature = "serve")]
        Command::User(cmd) => return run_user_embedded(cmd.clone(), &cli.output),
        #[cfg(feature = "serve")]
        Command::Cors(cmd) => return run_cors_embedded(cmd.clone(), &cli.output),
        _ => {}
    }

    // ── Mode detection ──────────────────────────────────────────────────
    // Try connecting to the configured server URL. If reachable, use HTTP
    // client mode. Fall back to embedded SQLite otherwise.
    // Note: no tokio runtime is active here, so reqwest::blocking is safe.
    // When --db is explicitly set the caller wants a specific file; stay embedded.
    #[cfg(feature = "serve")]
    if cli.db.is_none() {
        let config = axon_config::AxonConfig::load(Some(&axon_config::paths::config_file()))
            .unwrap_or_default();
        if let Ok(http_client) =
            client::HttpClient::new(&config.client.server_url, config.client.connect_timeout_ms)
        {
            if http_client.is_reachable() {
                return run_client_mode(cli, http_client);
            }
        }
    }

    // ── Embedded data commands — open SQLite ─────────────────────────────
    let db_path = cli.db.clone().unwrap_or_else(|| {
        axon_config::paths::default_sqlite_path()
            .to_string_lossy()
            .into_owned()
    });
    let storage = SqliteStorageAdapter::open(&db_path)
        .with_context(|| format!("failed to open database: {db_path}"))?;
    let mut handler = AxonHandler::new(storage);

    match cli.command {
        Command::Database(cmd) => run_database(cmd, &cli.output, &mut handler),
        Command::Namespace(cmd) => run_namespace(cmd, &cli.output, &mut handler),
        Command::Collections(cmd) => run_collection(cmd, &cli.output, &mut handler),
        Command::Entities(cmd) => run_entity(cmd, &cli.output, &mut handler),
        Command::Links(cmd) => run_link(cmd, &cli.output, &mut handler),
        Command::Config(ConfigCmd::Show) => {
            let config = serde_json::json!({
                "db": db_path,
                "output": format!("{:?}", cli.output),
                "mode": "embedded",
            });
            print_serialized(&config, &cli.output);
            Ok(())
        }
        Command::Schema(cmd) => run_schema(cmd, &cli.output, &mut handler),
        Command::Audit(cmd) => run_audit(cmd, &cli.output, &mut handler),
        Command::Bead(cmd) => run_bead(cmd, &cli.output, &mut handler),
        Command::Graph {
            collection,
            id,
            link_type,
            depth,
        } => run_link(
            LinkCmd::Traverse {
                collection,
                id,
                link_type,
                max_depth: Some(depth),
            },
            &cli.output,
            &mut handler,
        ),
        // Already handled above; unreachable
        #[cfg(feature = "serve")]
        Command::User(_) | Command::Cors(_) | Command::Serve(_) | Command::Mcp { .. } => {
            unreachable!()
        }
        Command::Doctor
        | Command::Init { .. }
        | Command::Server(_)
        | Command::Config(ConfigCmd::Path) => unreachable!(),
    }
}

/// Run `axon user` commands against the control-plane SQLite database directly
/// (no server required).
#[cfg(feature = "serve")]
fn run_user_embedded(cmd: UserCmd, format: &OutputFormat) -> Result<()> {
    use axon_server::auth::Role;
    use axon_server::control_plane::ControlPlaneDb;

    let cp_path = axon_config::paths::control_plane_sqlite_path()
        .to_string_lossy()
        .into_owned();
    let db = ControlPlaneDb::open(&cp_path)
        .with_context(|| format!("failed to open control-plane database: {cp_path}"))?;

    match cmd {
        UserCmd::List => {
            let entries = db
                .list_user_roles()
                .map_err(|e| anyhow::anyhow!("failed to list user roles: {e}"))?;
            let users: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| serde_json::json!({ "login": e.login, "role": e.role }))
                .collect();
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    print_serialized(&serde_json::json!({ "users": users }), format);
                }
                OutputFormat::Table => {
                    if entries.is_empty() {
                        println!("No explicit user-role assignments.");
                    } else {
                        for e in &entries {
                            let role_str = serde_json::to_string(&e.role).unwrap_or_default();
                            println!("{:<40} {}", e.login, role_str.trim_matches('"'));
                        }
                    }
                }
            }
            Ok(())
        }
        UserCmd::Grant { login, role } => {
            let role: Role = match role.as_str() {
                "admin" => Role::Admin,
                "write" => Role::Write,
                "read" => Role::Read,
                other => anyhow::bail!("unknown role '{other}'; must be admin, write, or read"),
            };
            db.set_user_role(&login, &role)
                .map_err(|e| anyhow::anyhow!("failed to set role: {e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    print_serialized(&serde_json::json!({ "login": login, "role": role }), format);
                }
                OutputFormat::Table => {
                    let role_str = serde_json::to_string(&role).unwrap_or_default();
                    println!("Granted {} to {login}", role_str.trim_matches('"'));
                }
            }
            Ok(())
        }
        UserCmd::Revoke { login } => {
            let removed = db
                .remove_user_role(&login)
                .map_err(|e| anyhow::anyhow!("failed to revoke role: {e}"))?;
            if removed {
                match format {
                    OutputFormat::Json | OutputFormat::Yaml => {
                        print_serialized(
                            &serde_json::json!({ "login": login, "deleted": true }),
                            format,
                        );
                    }
                    OutputFormat::Table => println!("Revoked explicit role for {login}"),
                }
            } else {
                anyhow::bail!("no explicit role assigned to '{login}'");
            }
            Ok(())
        }
    }
}

/// Run `axon cors` commands against the control-plane SQLite database directly
/// (no server required).
#[cfg(feature = "serve")]
fn run_cors_embedded(cmd: CorsCmd, format: &OutputFormat) -> Result<()> {
    use axon_server::control_plane::ControlPlaneDb;

    let cp_path = axon_config::paths::control_plane_sqlite_path()
        .to_string_lossy()
        .into_owned();
    let db = ControlPlaneDb::open(&cp_path)
        .with_context(|| format!("failed to open control-plane database: {cp_path}"))?;

    match cmd {
        CorsCmd::List => {
            let origins = db
                .list_cors_origins()
                .map_err(|e| anyhow::anyhow!("failed to list CORS origins: {e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    print_serialized(&serde_json::json!({ "origins": origins }), format);
                }
                OutputFormat::Table => {
                    if origins.is_empty() {
                        println!("No CORS origins configured.");
                    } else {
                        for o in &origins {
                            println!("{o}");
                        }
                    }
                }
            }
            Ok(())
        }
        CorsCmd::Add { origin } => {
            db.add_cors_origin(&origin)
                .map_err(|e| anyhow::anyhow!("failed to add CORS origin: {e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    print_serialized(
                        &serde_json::json!({ "origin": origin, "added": true }),
                        format,
                    );
                }
                OutputFormat::Table => println!("Added CORS origin: {origin}"),
            }
            Ok(())
        }
        CorsCmd::Remove { origin } => {
            let removed = db
                .remove_cors_origin(&origin)
                .map_err(|e| anyhow::anyhow!("failed to remove CORS origin: {e}"))?;
            if removed {
                match format {
                    OutputFormat::Json | OutputFormat::Yaml => {
                        print_serialized(
                            &serde_json::json!({ "origin": origin, "deleted": true }),
                            format,
                        );
                    }
                    OutputFormat::Table => println!("Removed CORS origin: {origin}"),
                }
            } else {
                anyhow::bail!("origin '{origin}' was not in the allow-list");
            }
            Ok(())
        }
    }
}

#[cfg(feature = "serve")]
fn run_server_command(cli: Cli) -> Result<()> {
    let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
    match cli.command {
        Command::Serve(args) => rt.block_on(async {
            axon_server::serve::serve(args)
                .await
                .map_err(|e| anyhow::anyhow!(e))
        }),
        Command::Mcp {
            storage,
            sqlite_path,
            postgres_dsn,
        } => {
            let args = axon_server::serve::ServeArgs {
                http_port: 4170,
                grpc_port: None,
                no_auth: true,
                tailscale_socket: std::path::PathBuf::from("/run/tailscale/tailscaled.sock"),
                tailscale_default_role: axon_server::serve::DefaultRoleArg::Read,
                guest_role: None,
                auth_cache_ttl_secs: 60,
                mcp_stdio: true,
                storage,
                sqlite_path,
                postgres_dsn,
                control_plane_path: String::from("axon-control-plane.db"),
                ui_dir: None,
                tls_cert: None,
                tls_key: None,
                tls_self_signed: false,
                tls_self_signed_san: None,
            };
            rt.block_on(async {
                axon_server::serve::serve(args)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            })
        }
        _ => unreachable!(),
    }
}

fn run_service_command(cmd: &ServerCmd) -> Result<()> {
    let action = match cmd {
        ServerCmd::Install { global } => service::ServiceAction::Install { global: *global },
        ServerCmd::Uninstall => service::ServiceAction::Uninstall,
        ServerCmd::Start => service::ServiceAction::Start,
        ServerCmd::Stop => service::ServiceAction::Stop,
        ServerCmd::Restart => service::ServiceAction::Restart,
        ServerCmd::Status => service::ServiceAction::Status,
    };
    service::run_service(action)
}

fn run_database(
    cmd: DatabaseCmd,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        DatabaseCmd::Create { name } => {
            let resp = handler.create_database(CreateDatabaseRequest { name })?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => print_serialized(&resp, format),
                OutputFormat::Table => println!("{}", resp.name),
            }
            Ok(())
        }
        DatabaseCmd::List => {
            let resp = handler.list_databases(ListDatabasesRequest {})?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => print_serialized(&resp, format),
                OutputFormat::Table => {
                    for database in resp.databases {
                        println!("{database}");
                    }
                }
            }
            Ok(())
        }
        DatabaseCmd::Drop {
            name,
            force,
            confirm,
        } => {
            anyhow::ensure!(confirm, "database drop requires --confirm");
            let resp = handler.drop_database(DropDatabaseRequest { name, force })?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => print_serialized(&resp, format),
                OutputFormat::Table => println!(
                    "{} ({} collections removed)",
                    resp.name, resp.collections_removed
                ),
            }
            Ok(())
        }
    }
}

fn run_namespace(
    cmd: NamespaceCmd,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        NamespaceCmd::Create { database, schema } => {
            let resp = handler.create_namespace(CreateNamespaceRequest { database, schema })?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => print_serialized(&resp, format),
                OutputFormat::Table => println!("{}.{}", resp.database, resp.schema),
            }
            Ok(())
        }
        NamespaceCmd::List { database } => {
            let resp = handler.list_namespaces(ListNamespacesRequest { database })?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => print_serialized(&resp, format),
                OutputFormat::Table => {
                    for schema in resp.schemas {
                        println!("{schema}");
                    }
                }
            }
            Ok(())
        }
        NamespaceCmd::Collections { database, schema } => {
            let resp = handler
                .list_namespace_collections(ListNamespaceCollectionsRequest { database, schema })?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => print_serialized(&resp, format),
                OutputFormat::Table => {
                    for collection in resp.collections {
                        println!("{collection}");
                    }
                }
            }
            Ok(())
        }
        NamespaceCmd::Drop {
            database,
            schema,
            force,
            confirm,
        } => {
            anyhow::ensure!(confirm, "namespace drop requires --confirm");
            let resp = handler.drop_namespace(DropNamespaceRequest {
                database,
                schema,
                force,
            })?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => print_serialized(&resp, format),
                OutputFormat::Table => println!(
                    "{}.{} ({} collections removed)",
                    resp.database, resp.schema, resp.collections_removed
                ),
            }
            Ok(())
        }
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
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
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
        CollectionCmd::Drop {
            name,
            actor,
            confirm,
        } => {
            let resp = handler
                .drop_collection(DropCollectionRequest {
                    name: CollectionId::new(&name),
                    actor,
                    confirm,
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
        CollectionCmd::Template(cmd) => run_collection_template(cmd, format, handler)?,
    }
    Ok(())
}

fn run_collection_template(
    cmd: CollectionTemplateCmd,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        CollectionTemplateCmd::Put {
            collection,
            template,
            file,
            actor,
        } => {
            let template = read_template_source(template, file)?;
            let resp = handler
                .put_collection_template(PutCollectionTemplateRequest {
                    collection: CollectionId::new(&collection),
                    template,
                    actor,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    print_serialized(
                        &collection_template_to_json(&resp.view, &resp.warnings),
                        format,
                    );
                }
                OutputFormat::Table => {
                    println!("collection: {}", resp.view.collection);
                    println!("version:    {}", resp.view.version);
                    println!("{}", resp.view.markdown_template);
                    if !resp.warnings.is_empty() {
                        println!("warnings:");
                        for warning in &resp.warnings {
                            println!("- {warning}");
                        }
                    }
                }
            }
        }
        CollectionTemplateCmd::Get { collection } => {
            let resp = handler
                .get_collection_template(GetCollectionTemplateRequest {
                    collection: CollectionId::new(&collection),
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => {
                    print_serialized(&collection_template_to_json(&resp.view, &[]), format);
                }
                OutputFormat::Table => {
                    println!("collection: {}", resp.view.collection);
                    println!("version:    {}", resp.view.version);
                    println!("{}", resp.view.markdown_template);
                }
            }
        }
        CollectionTemplateCmd::Delete { collection, actor } => {
            let resp = handler
                .delete_collection_template(DeleteCollectionTemplateRequest {
                    collection: CollectionId::new(&collection),
                    actor,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format {
                OutputFormat::Json | OutputFormat::Yaml => print_serialized(
                    &serde_json::json!({
                        "collection": resp.collection,
                        "status": "deleted"
                    }),
                    format,
                ),
                OutputFormat::Table => println!("deleted template for '{}'", resp.collection),
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
                    audit_metadata: None,
                    attribution: None,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            print_entity(entity_to_json(&resp.entity), format);
        }
        EntityCmd::Get {
            collection,
            id,
            render,
        } => {
            let collection_id = CollectionId::new(&collection);
            let entity_id = EntityId::new(&id);
            match render {
                EntityRenderFormat::Json => {
                    let resp = handler
                        .get_entity(GetEntityRequest {
                            collection: collection_id,
                            id: entity_id,
                        })
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    print_entity(entity_to_json(&resp.entity), format);
                }
                EntityRenderFormat::Markdown => {
                    let response = handler
                        .get_entity_markdown(&collection_id, &entity_id)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    match &response {
                        axon_api::response::GetEntityMarkdownResponse::Rendered {
                            rendered_markdown,
                            ..
                        } => match format {
                            OutputFormat::Json | OutputFormat::Yaml => {
                                print_serialized(&response, format);
                            }
                            OutputFormat::Table => println!("{rendered_markdown}"),
                        },
                        axon_api::response::GetEntityMarkdownResponse::RenderFailed {
                            detail,
                            ..
                        } => match format {
                            OutputFormat::Json | OutputFormat::Yaml => {
                                print_serialized(&response, format);
                                anyhow::bail!("{detail}");
                            }
                            OutputFormat::Table => anyhow::bail!("{detail}"),
                        },
                    }
                }
            }
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
            // Auto-fetch current version when --expected-version is omitted.
            let version = match expected_version {
                Some(v) => v,
                None => {
                    handler
                        .get_entity(GetEntityRequest {
                            collection: CollectionId::new(&collection),
                            id: EntityId::new(&id),
                        })
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                        .entity
                        .version
                }
            };
            let resp = handler
                .update_entity(UpdateEntityRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                    data,
                    expected_version: version,
                    actor,
                    audit_metadata: None,
                    attribution: None,
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
                    audit_metadata: None,
                    force: false,
                    attribution: None,
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
                let mut field_filters: Vec<FilterNode> = filter
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
                    Some(field_filters.remove(0))
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

fn link_to_json(link: &axon_core::types::Link) -> Value {
    serde_json::json!({
        "source_collection": link.source_collection.to_string(),
        "source_id": link.source_id.to_string(),
        "target_collection": link.target_collection.to_string(),
        "target_id": link.target_id.to_string(),
        "link_type": link.link_type,
    })
}

fn print_links(links: &[axon_core::types::Link], format: &OutputFormat) {
    match format {
        OutputFormat::Json | OutputFormat::Yaml => {
            let json: Vec<Value> = links.iter().map(link_to_json).collect();
            print_serialized(&json, format);
        }
        OutputFormat::Table => {
            if links.is_empty() {
                println!("(no links)");
            } else {
                for l in links {
                    println!(
                        "{}/{} --[{}]--> {}/{}",
                        l.source_collection,
                        l.source_id,
                        l.link_type,
                        l.target_collection,
                        l.target_id,
                    );
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn create_link_and_print(
    source_collection: String,
    source_id: String,
    target_collection: String,
    target_id: String,
    link_type: String,
    actor: Option<String>,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    let resp = handler
        .create_link(CreateLinkRequest {
            source_collection: CollectionId::new(&source_collection),
            source_id: EntityId::new(&source_id),
            target_collection: CollectionId::new(&target_collection),
            target_id: EntityId::new(&target_id),
            link_type,
            metadata: serde_json::json!(null),
            actor,
            attribution: None,
        })
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let link = &resp.link;
    match format {
        OutputFormat::Json | OutputFormat::Yaml => {
            println!("{}", serde_json::to_string_pretty(&link_to_json(link))?);
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
    Ok(())
}

fn run_link(
    cmd: LinkCmd,
    format: &OutputFormat,
    handler: &mut AxonHandler<SqliteStorageAdapter>,
) -> Result<()> {
    match cmd {
        LinkCmd::Set {
            source_collection,
            source_id,
            target_collection,
            target_id,
            link_type,
            actor,
        }
        | LinkCmd::Create {
            source_collection,
            source_id,
            target_collection,
            target_id,
            link_type,
            actor,
        } => create_link_and_print(
            source_collection,
            source_id,
            target_collection,
            target_id,
            link_type,
            actor,
            format,
            handler,
        )?,
        LinkCmd::List {
            collection,
            id,
            link_type,
        } => {
            let resp = handler
                .traverse(TraverseRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                    link_type,
                    max_depth: Some(1),
                    direction: Default::default(),
                    hop_filter: None,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            print_links(&resp.links, format);
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
    handler: &mut AxonHandler<SqliteStorageAdapter>,
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
        SchemaCmd::Revalidate { collection } => {
            let col_id = CollectionId::new(&collection);
            let resp = handler
                .revalidate(RevalidateRequest { collection: col_id })
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            match format {
                OutputFormat::Table => {
                    println!(
                        "Revalidation complete: {} scanned, {} valid, {} invalid",
                        resp.total_scanned,
                        resp.valid_count,
                        resp.invalid.len()
                    );
                    for inv in &resp.invalid {
                        println!("  FAIL {} (v{})", inv.id, inv.version);
                        for err in &inv.errors {
                            println!("    - {err}");
                        }
                    }
                    if !resp.invalid.is_empty() {
                        std::process::exit(1);
                    }
                }
                _ => {
                    let has_failures = !resp.invalid.is_empty();
                    print_serialized(
                        &serde_json::json!({
                            "total_scanned": resp.total_scanned,
                            "valid_count": resp.valid_count,
                            "invalid_count": resp.invalid.len(),
                            "invalid": resp.invalid.iter().map(|inv| {
                                serde_json::json!({
                                    "id": inv.id,
                                    "version": inv.version,
                                    "errors": inv.errors,
                                })
                            }).collect::<Vec<_>>(),
                        }),
                        format,
                    );
                    if has_failures {
                        std::process::exit(1);
                    }
                }
            }
        }
        SchemaCmd::Set {
            collection,
            schema,
            file,
            force,
            dry_run,
            actor,
        } => {
            // Load the entity_schema JSON from --schema or --file.
            let entity_schema_json: Value = match (schema, file) {
                (Some(s), None) => {
                    serde_json::from_str(&s).with_context(|| "schema must be valid JSON")?
                }
                (None, Some(path)) => {
                    let content = std::fs::read_to_string(&path)
                        .with_context(|| format!("failed to read schema file: {path}"))?;
                    serde_json::from_str(&content)
                        .with_context(|| format!("file {path} must contain valid JSON"))?
                }
                (Some(_), Some(_)) => anyhow::bail!("provide either --schema or --file, not both"),
                (None, None) => anyhow::bail!("schema content required via --schema or --file"),
            };

            // Fetch current schema to preserve version and non-entity_schema fields.
            let existing = handler
                .get_schema(&CollectionId::new(&collection))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let base_version = existing.as_ref().map(|s| s.version).unwrap_or(0);
            let new_schema = axon_schema::schema::CollectionSchema {
                collection: CollectionId::new(&collection),
                description: existing.as_ref().and_then(|s| s.description.clone()),
                version: base_version + 1,
                entity_schema: Some(entity_schema_json),
                link_types: existing
                    .as_ref()
                    .map(|s| s.link_types.clone())
                    .unwrap_or_default(),
                access_control: existing.as_ref().and_then(|s| s.access_control.clone()),
                gates: existing
                    .as_ref()
                    .map(|s| s.gates.clone())
                    .unwrap_or_default(),
                validation_rules: existing
                    .as_ref()
                    .map(|s| s.validation_rules.clone())
                    .unwrap_or_default(),
                indexes: existing
                    .as_ref()
                    .map(|s| s.indexes.clone())
                    .unwrap_or_default(),
                compound_indexes: existing
                    .as_ref()
                    .map(|s| s.compound_indexes.clone())
                    .unwrap_or_default(),
                queries: existing
                    .as_ref()
                    .map(|s| s.queries.clone())
                    .unwrap_or_default(),
                lifecycles: Default::default(),
            };

            let resp = handler
                .handle_put_schema(PutSchemaRequest {
                    schema: new_schema,
                    actor,
                    force,
                    dry_run,
                    explain_inputs: Vec::new(),
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            if dry_run {
                let compat_str = resp.compatibility.as_ref().map(|c| format!("{c:?}"));
                let changes: Vec<String> = resp
                    .diff
                    .as_ref()
                    .map(|d| d.changes.iter().map(|c| c.description.clone()).collect())
                    .unwrap_or_default();
                match format {
                    OutputFormat::Table => {
                        println!("dry-run: compatibility={:?}", compat_str);
                        for change in &changes {
                            println!("  {change}");
                        }
                    }
                    _ => print_serialized(
                        &serde_json::json!({
                            "dry_run": true,
                            "compatibility": compat_str,
                            "changes": changes,
                        }),
                        format,
                    ),
                }
            } else {
                let compat_str = resp.compatibility.as_ref().map(|c| format!("{c:?}"));
                match format {
                    OutputFormat::Table => println!(
                        "schema updated: {} v{} ({})",
                        collection,
                        resp.schema.version,
                        compat_str.as_deref().unwrap_or("unknown"),
                    ),
                    _ => print_serialized(
                        &serde_json::json!({
                            "collection": collection,
                            "version": resp.schema.version,
                            "compatibility": compat_str,
                            "status": "updated",
                        }),
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
                    attribution: None,
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
            print_serialized(&json_entries, format);
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
        OutputFormat::Json | OutputFormat::Yaml => print_serialized(&bead_to_json(b), format),
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
            print_serialized(&json, format);
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

// ── Client mode dispatch ──────────────────────────────────────────────────────

/// Dispatch CLI commands via the HTTP client instead of embedded SQLite.
#[cfg(feature = "serve")]
fn run_client_mode(cli: Cli, client: client::HttpClient) -> Result<()> {
    match cli.command {
        Command::Database(cmd) => match cmd {
            DatabaseCmd::Create { name } => {
                let resp = client.create_database(&name)?;
                print_serialized(&resp, &cli.output);
            }
            DatabaseCmd::List => {
                let resp = client.list_databases()?;
                print_serialized(&resp, &cli.output);
            }
            DatabaseCmd::Drop { .. } => {
                anyhow::bail!("database drop is not yet available in client mode");
            }
        },
        Command::Collections(cmd) => match cmd {
            CollectionCmd::Create {
                name,
                schema,
                actor,
            } => {
                let resp = client.create_collection(&name, schema.as_deref(), actor.as_deref())?;
                print_serialized(&resp, &cli.output);
            }
            CollectionCmd::List => {
                let resp = client.list_collections()?;
                print_serialized(&resp, &cli.output);
            }
            CollectionCmd::Describe { name } => {
                let resp = client.describe_collection(&name)?;
                print_serialized(&resp, &cli.output);
            }
            CollectionCmd::Drop {
                name,
                actor,
                confirm,
            } => {
                anyhow::ensure!(confirm, "collection drop requires --confirm");
                let resp = client.drop_collection(&name, actor.as_deref())?;
                print_serialized(&resp, &cli.output);
            }
            CollectionCmd::Template(_) => {
                anyhow::bail!(
                    "collection template subcommands are not yet available in client mode"
                );
            }
        },
        Command::Entities(cmd) => match cmd {
            EntityCmd::Create {
                collection,
                id,
                data,
                actor,
            } => {
                let resp = client.create_entity(&collection, &id, &data, actor.as_deref())?;
                print_serialized(&resp, &cli.output);
            }
            EntityCmd::Get { collection, id, .. } => {
                let resp = client.get_entity(&collection, &id)?;
                print_serialized(&resp, &cli.output);
            }
            EntityCmd::List { collection, limit } => {
                let resp = client.list_entities(&collection, limit)?;
                print_serialized(&resp, &cli.output);
            }
            EntityCmd::Update {
                collection,
                id,
                data,
                expected_version,
                actor,
            } => {
                // Auto-fetch current version when --expected-version is omitted.
                let version = match expected_version {
                    Some(v) => v,
                    None => {
                        let resp = client.get_entity(&collection, &id)?;
                        resp["entity"]["version"]
                            .as_u64()
                            .ok_or_else(|| anyhow::anyhow!("unexpected entity version format"))?
                    }
                };
                let resp =
                    client.update_entity(&collection, &id, &data, version, actor.as_deref())?;
                print_serialized(&resp, &cli.output);
            }
            EntityCmd::Delete {
                collection,
                id,
                actor,
            } => {
                let resp = client.delete_entity(&collection, &id, actor.as_deref())?;
                print_serialized(&resp, &cli.output);
            }
            EntityCmd::Query {
                collection,
                filter,
                limit,
                count_only,
            } => {
                // Build filter JSON: each "--filter field=value" becomes a FieldFilter.
                // FilterNode uses internally-tagged serde: {"type": "field", "field": ..., "op": ..., "value": ...}
                let filter_json = if filter.is_empty() {
                    None
                } else {
                    let nodes: Vec<Value> = filter
                        .iter()
                        .map(|f| {
                            let (field, value) = f.split_once('=').unwrap_or((f, ""));
                            serde_json::json!({
                                "type": "field",
                                "field": field,
                                "op": "eq",
                                "value": value
                            })
                        })
                        .collect();
                    if nodes.len() == 1 {
                        nodes.into_iter().next()
                    } else {
                        Some(serde_json::json!({ "type": "and", "filters": nodes }))
                    }
                };
                let resp = client.query_entities(&collection, limit, filter_json, count_only)?;
                print_serialized(&resp, &cli.output);
            }
        },
        Command::Config(ConfigCmd::Show) => {
            let health = client.health().unwrap_or_default();
            let config = serde_json::json!({
                "mode": "client",
                "server": health,
            });
            print_serialized(&config, &cli.output);
        }
        Command::Namespace(_) => {
            anyhow::bail!("namespace commands are not yet available in client mode");
        }
        Command::Links(cmd) => match cmd {
            LinkCmd::Set {
                source_collection,
                source_id,
                target_collection,
                target_id,
                link_type,
                actor,
            }
            | LinkCmd::Create {
                source_collection,
                source_id,
                target_collection,
                target_id,
                link_type,
                actor,
            } => {
                let resp = client.create_link(
                    &source_collection,
                    &source_id,
                    &target_collection,
                    &target_id,
                    &link_type,
                    actor.as_deref(),
                )?;
                print_serialized(&resp, &cli.output);
            }
            LinkCmd::List {
                collection,
                id,
                link_type,
            } => {
                let resp = client.traverse(&collection, &id, link_type.as_deref(), Some(1))?;
                print_serialized(&resp, &cli.output);
            }
            LinkCmd::Traverse {
                collection,
                id,
                link_type,
                max_depth,
            } => {
                let resp = client.traverse(&collection, &id, link_type.as_deref(), max_depth)?;
                print_serialized(&resp, &cli.output);
            }
        },
        Command::Graph {
            collection,
            id,
            link_type,
            depth,
        } => {
            let resp = client.traverse(&collection, &id, link_type.as_deref(), Some(depth))?;
            print_serialized(&resp, &cli.output);
        }
        Command::Audit(cmd) => match cmd {
            AuditCmd::List {
                collection,
                entity_id,
                actor,
                limit,
            } => {
                let resp = client.query_audit(
                    collection.as_deref(),
                    entity_id.as_deref(),
                    actor.as_deref(),
                    limit,
                )?;
                print_serialized(&resp, &cli.output);
            }
            AuditCmd::Show { .. } | AuditCmd::Revert { .. } => {
                anyhow::bail!("audit show/revert not yet available in client mode");
            }
        },
        Command::Schema(cmd) => match cmd {
            SchemaCmd::Show { collection } => {
                let resp = client.get_schema(&collection)?;
                print_serialized(&resp, &cli.output);
            }
            SchemaCmd::Set {
                collection,
                schema,
                file,
                force,
                dry_run,
                actor,
            } => {
                // Resolve entity_schema JSON from --schema or --file.
                let entity_schema_json: Value = match (schema, file) {
                    (Some(s), None) => {
                        serde_json::from_str(&s).with_context(|| "schema must be valid JSON")?
                    }
                    (None, Some(path)) => {
                        let content = std::fs::read_to_string(&path)
                            .with_context(|| format!("failed to read schema file: {path}"))?;
                        serde_json::from_str(&content)
                            .with_context(|| format!("file {path} must contain valid JSON"))?
                    }
                    (Some(_), Some(_)) => {
                        anyhow::bail!("provide either --schema or --file, not both")
                    }
                    (None, None) => {
                        anyhow::bail!("schema content required via --schema or --file")
                    }
                };
                // Fetch current schema; GET returns {"schema": {...}}, extract inner object.
                let existing_outer = client.get_schema(&collection).ok();
                let schema_body = existing_outer
                    .and_then(|v| v.get("schema").cloned())
                    .unwrap_or_else(|| {
                        serde_json::json!({
                            "collection": collection,
                            "version": 0,
                            "link_types": {},
                            "gates": [],
                            "validation_rules": [],
                            "indexes": [],
                            "compound_indexes": []
                        })
                    });
                // Bump version and replace entity_schema.
                let cur_version = schema_body["version"].as_u64().unwrap_or(0);
                let new_version = cur_version + 1;
                let description_val = schema_body["description"].as_str().map(str::to_string);
                let resp = client.put_schema(
                    &collection,
                    client::PutSchemaOptions {
                        version: new_version,
                        entity_schema: entity_schema_json,
                        description: description_val.as_deref(),
                        force,
                        dry_run,
                        actor: actor.as_deref(),
                    },
                )?;
                print_serialized(&resp, &cli.output);
            }
            SchemaCmd::Validate { .. } | SchemaCmd::Revalidate { .. } => {
                anyhow::bail!("schema validate/revalidate not yet available in client mode");
            }
        },
        Command::Bead(_) => {
            anyhow::bail!("bead commands are not yet available in client mode");
        }
        // These are handled before mode detection; unreachable in client mode
        #[cfg(feature = "serve")]
        Command::User(_) | Command::Cors(_) | Command::Serve(_) | Command::Mcp { .. } => {
            unreachable!()
        }
        Command::Doctor
        | Command::Init { .. }
        | Command::Server(_)
        | Command::Config(ConfigCmd::Path) => unreachable!(),
    }
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
        let cli = make_cli(&db, &["collections", "create", "tasks"]);
        run(cli).unwrap();

        let cli = make_cli(&db, &["collections", "describe", "tasks"]);
        run(cli).unwrap();
    }

    #[test]
    fn database_and_namespace_commands_round_trip() {
        let (_f, db) = tmp_db();

        run(make_cli(&db, &["database", "create", "prod"])).unwrap();
        run(make_cli(
            &db,
            &["namespace", "create", "--database", "prod", "billing"],
        ))
        .unwrap();
        run(make_cli(&db, &["database", "list"])).unwrap();
        run(make_cli(&db, &["namespace", "list", "--database", "prod"])).unwrap();
        run(make_cli(
            &db,
            &["namespace", "collections", "--database", "prod", "billing"],
        ))
        .unwrap();
    }

    #[test]
    fn collection_list_and_drop() {
        let (_f, db) = tmp_db();
        // Create two collections.
        run(make_cli(&db, &["collections", "create", "tasks"])).unwrap();
        run(make_cli(&db, &["collections", "create", "users"])).unwrap();

        // List should show both.
        run(make_cli(&db, &["--output", "json", "collections", "list"])).unwrap();

        // Drop one.
        run(make_cli(
            &db,
            &["collections", "drop", "users", "--confirm"],
        ))
        .unwrap();
    }

    #[test]
    fn entity_create_get_round_trip() {
        let (_f, db) = tmp_db();
        run(make_cli(&db, &["collections", "create", "tasks"])).unwrap();

        let cli = make_cli(
            &db,
            &[
                "entities",
                "create",
                "tasks",
                "--id",
                "t-001",
                "--data",
                r#"{"title":"hello"}"#,
            ],
        );
        run(cli).unwrap();

        let cli = make_cli(&db, &["entities", "get", "tasks", "t-001"]);
        run(cli).unwrap();
    }

    #[test]
    fn entity_list_returns_entities() {
        let (_f, db) = tmp_db();
        run(make_cli(&db, &["collections", "create", "tasks"])).unwrap();
        run(make_cli(
            &db,
            &[
                "entities",
                "create",
                "tasks",
                "--id",
                "t-001",
                "--data",
                r#"{"title":"a"}"#,
            ],
        ))
        .unwrap();
        run(make_cli(
            &db,
            &[
                "entities",
                "create",
                "tasks",
                "--id",
                "t-002",
                "--data",
                r#"{"title":"b"}"#,
            ],
        ))
        .unwrap();

        run(make_cli(
            &db,
            &["--output", "json", "entities", "list", "tasks"],
        ))
        .unwrap();
    }

    #[test]
    fn entity_create_get_json_output() {
        let (_f, db) = tmp_db();
        run(make_cli(&db, &["collections", "create", "tasks"])).unwrap();

        let cli = make_cli(
            &db,
            &[
                "--output",
                "json",
                "entities",
                "create",
                "tasks",
                "--id",
                "t-001",
                "--data",
                r#"{"title":"hello"}"#,
            ],
        );
        run(cli).unwrap();

        let cli = make_cli(
            &db,
            &["--output", "json", "entities", "get", "tasks", "t-001"],
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
                    access_control: None,
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
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
                audit_metadata: None,
                attribution: None,
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
                    access_control: None,
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
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
                audit_metadata: None,
                attribution: None,
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
    fn collection_template_delete_preserves_actor_provenance() {
        let (_f, db) = tmp_db();
        let storage = SqliteStorageAdapter::open(&db).unwrap();
        let mut handler = AxonHandler::new(storage);
        let collection = CollectionId::new("tasks");

        handler
            .create_collection(CreateCollectionRequest {
                name: collection.clone(),
                schema: CollectionSchema {
                    collection: collection.clone(),
                    description: None,
                    version: 1,
                    entity_schema: None,
                    link_types: Default::default(),
                    access_control: None,
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
                },
                actor: None,
            })
            .unwrap();

        handler
            .put_collection_template(PutCollectionTemplateRequest {
                collection: collection.clone(),
                template: "# {{title}}".into(),
                actor: Some("creator".into()),
            })
            .unwrap();

        let cli = make_cli(
            &db,
            &[
                "collections",
                "template",
                "delete",
                "tasks",
                "--actor",
                "cleaner",
            ],
        );

        let Command::Collections(CollectionCmd::Template(cmd)) = cli.command else {
            panic!("expected collections template command");
        };
        run_collection_template(cmd, &OutputFormat::Json, &mut handler).unwrap();

        let delete_entries = handler
            .query_audit(QueryAuditRequest {
                collection: Some(collection),
                operation: Some("template.delete".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(delete_entries.entries.len(), 1);
        assert_eq!(delete_entries.entries[0].actor, "cleaner");
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
                    access_control: None,
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
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
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        let json = entity_to_json(&resp.entity);
        let s = serde_json::to_string(&json).unwrap();
        let _: Value = serde_json::from_str(&s).unwrap();
    }
}
