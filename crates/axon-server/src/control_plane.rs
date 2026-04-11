//! Control-plane database for managing tenants, nodes, and database assignments.
//!
//! This SQLite database is **separate** from any tenant data store.  It tracks
//! tenant lifecycle, node topology, and which databases are assigned to which
//! tenants/nodes.

use rusqlite::{params, Connection};

use axon_core::error::AxonError;

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

/// A registered tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

/// A registered node in the cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub address: String,
    pub created_at: String,
}

/// An assignment of a database to a tenant (optionally pinned to a node).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantDatabase {
    pub tenant_id: String,
    pub db_name: String,
    pub node_id: Option<String>,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// ControlPlaneDb
// ---------------------------------------------------------------------------

/// Handle to the control-plane SQLite database.
pub struct ControlPlaneDb {
    conn: Connection,
}

impl ControlPlaneDb {
    /// Open (or create) a control-plane database at the given file path.
    pub fn open(path: &str) -> Result<Self, AxonError> {
        let conn = Connection::open(path).map_err(|e| AxonError::Storage(e.to_string()))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (useful for testing).
    pub fn open_in_memory() -> Result<Self, AxonError> {
        let conn =
            Connection::open_in_memory().map_err(|e| AxonError::Storage(e.to_string()))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Apply schema migrations.  All statements are idempotent
    /// (`CREATE TABLE IF NOT EXISTS`).
    pub fn migrate(&self) -> Result<(), AxonError> {
        self.conn
            .execute_batch(
                "PRAGMA foreign_keys = ON;

                 CREATE TABLE IF NOT EXISTS tenants (
                     id         TEXT PRIMARY KEY,
                     name       TEXT UNIQUE NOT NULL,
                     created_at TEXT NOT NULL
                 );

                 CREATE TABLE IF NOT EXISTS nodes (
                     id         TEXT PRIMARY KEY,
                     address    TEXT NOT NULL,
                     created_at TEXT NOT NULL
                 );

                 CREATE TABLE IF NOT EXISTS tenant_databases (
                     tenant_id  TEXT NOT NULL REFERENCES tenants(id),
                     db_name    TEXT NOT NULL,
                     node_id    TEXT REFERENCES nodes(id),
                     created_at TEXT NOT NULL,
                     PRIMARY KEY (tenant_id, db_name)
                 );",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    // -- tenants ------------------------------------------------------------

    /// Create a new tenant.
    pub fn create_tenant(&self, id: &str, name: &str, created_at: &str) -> Result<(), AxonError> {
        self.conn
            .execute(
                "INSERT INTO tenants (id, name, created_at) VALUES (?1, ?2, ?3)",
                params![id, name, created_at],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get a tenant by id.
    pub fn get_tenant(&self, id: &str) -> Result<Tenant, AxonError> {
        self.conn
            .query_row(
                "SELECT id, name, created_at FROM tenants WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Tenant {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    AxonError::NotFound(format!("tenant {id}"))
                }
                other => AxonError::Storage(other.to_string()),
            })
    }

    /// List all tenants, ordered by `created_at`.
    pub fn list_tenants(&self) -> Result<Vec<Tenant>, AxonError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, created_at FROM tenants ORDER BY created_at")
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Tenant {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut tenants = Vec::new();
        for row in rows {
            tenants.push(row.map_err(|e| AxonError::Storage(e.to_string()))?);
        }
        Ok(tenants)
    }

    // -- nodes --------------------------------------------------------------

    /// Register a new node.
    pub fn create_node(
        &self,
        id: &str,
        address: &str,
        created_at: &str,
    ) -> Result<(), AxonError> {
        self.conn
            .execute(
                "INSERT INTO nodes (id, address, created_at) VALUES (?1, ?2, ?3)",
                params![id, address, created_at],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get a node by id.
    pub fn get_node(&self, id: &str) -> Result<Node, AxonError> {
        self.conn
            .query_row(
                "SELECT id, address, created_at FROM nodes WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Node {
                        id: row.get(0)?,
                        address: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    AxonError::NotFound(format!("node {id}"))
                }
                other => AxonError::Storage(other.to_string()),
            })
    }

    /// List all nodes, ordered by `created_at`.
    pub fn list_nodes(&self) -> Result<Vec<Node>, AxonError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, address, created_at FROM nodes ORDER BY created_at")
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Node {
                    id: row.get(0)?,
                    address: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut nodes = Vec::new();
        for row in rows {
            nodes.push(row.map_err(|e| AxonError::Storage(e.to_string()))?);
        }
        Ok(nodes)
    }

    // -- tenant_databases ---------------------------------------------------

    /// Assign a database to a tenant, optionally pinning it to a node.
    pub fn assign_database(
        &self,
        tenant_id: &str,
        db_name: &str,
        node_id: Option<&str>,
        created_at: &str,
    ) -> Result<(), AxonError> {
        self.conn
            .execute(
                "INSERT INTO tenant_databases (tenant_id, db_name, node_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![tenant_id, db_name, node_id, created_at],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    /// List databases assigned to a given tenant.
    pub fn list_databases_for_tenant(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<TenantDatabase>, AxonError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT tenant_id, db_name, node_id, created_at
                 FROM tenant_databases
                 WHERE tenant_id = ?1
                 ORDER BY created_at",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(params![tenant_id], |row| {
                Ok(TenantDatabase {
                    tenant_id: row.get(0)?,
                    db_name: row.get(1)?,
                    node_id: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut dbs = Vec::new();
        for row in rows {
            dbs.push(row.map_err(|e| AxonError::Storage(e.to_string()))?);
        }
        Ok(dbs)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> ControlPlaneDb {
        ControlPlaneDb::open_in_memory().expect("open in-memory control-plane db")
    }

    // -- migration ----------------------------------------------------------

    #[test]
    fn migrate_is_idempotent() {
        let db = setup();
        // Running migrate a second time should not fail.
        db.migrate().expect("second migrate should succeed");
    }

    #[test]
    fn tables_exist_after_migration() {
        let db = setup();
        let tables: Vec<String> = db
            .conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .filter_map(Result::ok)
            .collect();

        assert!(tables.contains(&"tenants".to_string()));
        assert!(tables.contains(&"nodes".to_string()));
        assert!(tables.contains(&"tenant_databases".to_string()));
    }

    // -- tenants ------------------------------------------------------------

    #[test]
    fn create_and_get_tenant() {
        let db = setup();
        db.create_tenant("t1", "Acme Corp", "2026-01-01T00:00:00Z")
            .expect("create tenant");

        let t = db.get_tenant("t1").expect("get tenant");
        assert_eq!(t.id, "t1");
        assert_eq!(t.name, "Acme Corp");
        assert_eq!(t.created_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn get_missing_tenant_returns_not_found() {
        let db = setup();
        let err = db.get_tenant("nonexistent").unwrap_err();
        assert!(
            matches!(err, AxonError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
    }

    #[test]
    fn duplicate_tenant_name_fails() {
        let db = setup();
        db.create_tenant("t1", "Acme", "2026-01-01T00:00:00Z")
            .expect("first create");
        let err = db
            .create_tenant("t2", "Acme", "2026-01-02T00:00:00Z")
            .unwrap_err();
        assert!(
            matches!(err, AxonError::Storage(_)),
            "expected Storage error, got {err:?}"
        );
    }

    #[test]
    fn list_tenants_ordered() {
        let db = setup();
        db.create_tenant("t2", "Beta", "2026-02-01T00:00:00Z")
            .expect("create");
        db.create_tenant("t1", "Alpha", "2026-01-01T00:00:00Z")
            .expect("create");
        let tenants = db.list_tenants().expect("list");
        assert_eq!(tenants.len(), 2);
        assert_eq!(tenants[0].name, "Alpha");
        assert_eq!(tenants[1].name, "Beta");
    }

    // -- nodes --------------------------------------------------------------

    #[test]
    fn create_and_get_node() {
        let db = setup();
        db.create_node("n1", "10.0.0.1:5000", "2026-01-01T00:00:00Z")
            .expect("create node");
        let n = db.get_node("n1").expect("get node");
        assert_eq!(n.id, "n1");
        assert_eq!(n.address, "10.0.0.1:5000");
    }

    #[test]
    fn get_missing_node_returns_not_found() {
        let db = setup();
        let err = db.get_node("missing").unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn list_nodes_ordered() {
        let db = setup();
        db.create_node("n2", "10.0.0.2:5000", "2026-02-01T00:00:00Z")
            .expect("create");
        db.create_node("n1", "10.0.0.1:5000", "2026-01-01T00:00:00Z")
            .expect("create");
        let nodes = db.list_nodes().expect("list");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].id, "n1");
        assert_eq!(nodes[1].id, "n2");
    }

    // -- tenant_databases ---------------------------------------------------

    #[test]
    fn assign_database_without_node() {
        let db = setup();
        db.create_tenant("t1", "Acme", "2026-01-01T00:00:00Z")
            .expect("tenant");
        db.assign_database("t1", "mydb", None, "2026-01-02T00:00:00Z")
            .expect("assign");

        let dbs = db.list_databases_for_tenant("t1").expect("list");
        assert_eq!(dbs.len(), 1);
        assert_eq!(dbs[0].db_name, "mydb");
        assert!(dbs[0].node_id.is_none());
    }

    #[test]
    fn assign_database_with_node() {
        let db = setup();
        db.create_tenant("t1", "Acme", "2026-01-01T00:00:00Z")
            .expect("tenant");
        db.create_node("n1", "10.0.0.1:5000", "2026-01-01T00:00:00Z")
            .expect("node");
        db.assign_database("t1", "mydb", Some("n1"), "2026-01-02T00:00:00Z")
            .expect("assign");

        let dbs = db.list_databases_for_tenant("t1").expect("list");
        assert_eq!(dbs.len(), 1);
        assert_eq!(dbs[0].node_id.as_deref(), Some("n1"));
    }

    #[test]
    fn assign_database_to_nonexistent_tenant_fails() {
        let db = setup();
        let err = db
            .assign_database("no-such-tenant", "mydb", None, "2026-01-01T00:00:00Z")
            .unwrap_err();
        assert!(
            matches!(err, AxonError::Storage(_)),
            "expected Storage (FK violation), got {err:?}"
        );
    }

    #[test]
    fn assign_database_to_nonexistent_node_fails() {
        let db = setup();
        db.create_tenant("t1", "Acme", "2026-01-01T00:00:00Z")
            .expect("tenant");
        let err = db
            .assign_database("t1", "mydb", Some("bad-node"), "2026-01-01T00:00:00Z")
            .unwrap_err();
        assert!(
            matches!(err, AxonError::Storage(_)),
            "expected Storage (FK violation), got {err:?}"
        );
    }

    #[test]
    fn duplicate_database_assignment_fails() {
        let db = setup();
        db.create_tenant("t1", "Acme", "2026-01-01T00:00:00Z")
            .expect("tenant");
        db.assign_database("t1", "mydb", None, "2026-01-01T00:00:00Z")
            .expect("first assign");
        let err = db
            .assign_database("t1", "mydb", None, "2026-01-02T00:00:00Z")
            .unwrap_err();
        assert!(
            matches!(err, AxonError::Storage(_)),
            "expected Storage (PK violation), got {err:?}"
        );
    }
}
