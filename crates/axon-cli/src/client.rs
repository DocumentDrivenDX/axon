//! HTTP client for talking to a running `axon serve` instance.
//!
//! Uses `reqwest::blocking::Client` so the CLI can remain synchronous — no
//! tokio runtime is needed for non-serve commands.

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

/// Options passed to [`HttpClient::put_schema`].
///
/// Grouping these into a struct keeps the method signature within clippy's
/// `too_many_arguments` limit (≤ 7 parameters including `&self` and `name`).
pub struct PutSchemaOptions<'a> {
    pub version: u64,
    pub entity_schema: Value,
    pub description: Option<&'a str>,
    pub force: bool,
    pub dry_run: bool,
    pub actor: Option<&'a str>,
}

/// HTTP client that maps CLI operations to Axon server REST endpoints.
pub struct HttpClient {
    base_url: String,
    client: Client,
}

impl HttpClient {
    /// Create a new client targeting `base_url` with the given connect timeout.
    pub fn new(base_url: &str, timeout_ms: u64) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(timeout_ms))
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        })
    }

    /// Try to reach the server's health endpoint. Returns `true` if reachable.
    pub fn is_reachable(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    // ── Entity operations ────────────────────────────────────────────────────

    /// `POST /entities/{collection}/{id}` with `{"data": ..., "actor": ...}`
    pub fn create_entity(
        &self,
        collection: &str,
        id: &str,
        data: &str,
        actor: Option<&str>,
    ) -> Result<Value> {
        let data_value: Value =
            serde_json::from_str(data).context("entity data must be valid JSON")?;
        let mut body = serde_json::json!({ "data": data_value });
        if let Some(a) = actor {
            body["actor"] = Value::String(a.to_string());
        }
        let resp = self
            .client
            .post(format!(
                "{}/entities/{}/{}",
                self.base_url, collection, id
            ))
            .json(&body)
            .send()
            .context("failed to send create entity request")?;
        Self::parse_response(resp)
    }

    /// `GET /entities/{collection}/{id}`
    pub fn get_entity(&self, collection: &str, id: &str) -> Result<Value> {
        let resp = self
            .client
            .get(format!(
                "{}/entities/{}/{}",
                self.base_url, collection, id
            ))
            .send()
            .context("failed to send get entity request")?;
        Self::parse_response(resp)
    }

    /// `POST /collections/{collection}/query` with optional limit/filter/count_only.
    pub fn list_entities(&self, collection: &str, limit: Option<usize>) -> Result<Value> {
        self.query_entities(collection, limit, None, false)
    }

    /// `POST /collections/{collection}/query` — full query with optional filter JSON and count_only.
    pub fn query_entities(
        &self,
        collection: &str,
        limit: Option<usize>,
        filter: Option<Value>,
        count_only: bool,
    ) -> Result<Value> {
        let mut body = serde_json::json!({ "collection": collection });
        if let Some(l) = limit {
            body["limit"] = Value::Number(l.into());
        }
        if let Some(f) = filter {
            body["filter"] = f;
        }
        if count_only {
            body["count_only"] = Value::Bool(true);
        }
        let resp = self
            .client
            .post(format!(
                "{}/collections/{}/query",
                self.base_url, collection
            ))
            .json(&body)
            .send()
            .context("failed to send query entities request")?;
        Self::parse_response(resp)
    }

    /// `PUT /entities/{collection}/{id}` with `{"data": ..., "expected_version": ..., "actor": ...}`
    pub fn update_entity(
        &self,
        collection: &str,
        id: &str,
        data: &str,
        expected_version: u64,
        actor: Option<&str>,
    ) -> Result<Value> {
        let data_value: Value =
            serde_json::from_str(data).context("entity data must be valid JSON")?;
        let mut body = serde_json::json!({
            "data": data_value,
            "expected_version": expected_version,
        });
        if let Some(a) = actor {
            body["actor"] = Value::String(a.to_string());
        }
        let resp = self
            .client
            .put(format!(
                "{}/entities/{}/{}",
                self.base_url, collection, id
            ))
            .json(&body)
            .send()
            .context("failed to send update entity request")?;
        Self::parse_response(resp)
    }

    /// `DELETE /entities/{collection}/{id}`
    pub fn delete_entity(
        &self,
        collection: &str,
        id: &str,
        actor: Option<&str>,
    ) -> Result<Value> {
        let mut req = self.client.delete(format!(
            "{}/entities/{}/{}",
            self.base_url, collection, id
        ));
        if let Some(a) = actor {
            req = req.json(&serde_json::json!({ "actor": a }));
        }
        let resp = req.send().context("failed to send delete entity request")?;
        Self::parse_response(resp)
    }

    // ── Collection operations ────────────────────────────────────────────────

    /// `POST /collections/{name}` with `{"schema": {"version": 1, ...}, "actor": ...}`
    pub fn create_collection(
        &self,
        name: &str,
        schema: Option<&str>,
        actor: Option<&str>,
    ) -> Result<Value> {
        let schema_body = match schema {
            Some(s) => {
                let v: Value =
                    serde_json::from_str(s).context("schema must be valid JSON")?;
                serde_json::json!({
                    "version": 1,
                    "entity_schema": v,
                })
            }
            None => serde_json::json!({ "version": 1 }),
        };
        let mut body = serde_json::json!({ "schema": schema_body });
        if let Some(a) = actor {
            body["actor"] = Value::String(a.to_string());
        }
        let resp = self
            .client
            .post(format!("{}/collections/{}", self.base_url, name))
            .json(&body)
            .send()
            .context("failed to send create collection request")?;
        Self::parse_response(resp)
    }

    /// `GET /collections`
    pub fn list_collections(&self) -> Result<Value> {
        let resp = self
            .client
            .get(format!("{}/collections", self.base_url))
            .send()
            .context("failed to send list collections request")?;
        Self::parse_response(resp)
    }

    /// `GET /collections/{name}`
    pub fn describe_collection(&self, name: &str) -> Result<Value> {
        let resp = self
            .client
            .get(format!("{}/collections/{}", self.base_url, name))
            .send()
            .context("failed to send describe collection request")?;
        Self::parse_response(resp)
    }

    /// `DELETE /collections/{name}`
    pub fn drop_collection(&self, name: &str, actor: Option<&str>) -> Result<Value> {
        let mut req = self
            .client
            .delete(format!("{}/collections/{}", self.base_url, name));
        if let Some(a) = actor {
            req = req.json(&serde_json::json!({ "actor": a }));
        }
        let resp = req
            .send()
            .context("failed to send drop collection request")?;
        Self::parse_response(resp)
    }

    // ── Database operations ──────────────────────────────────────────────────

    /// `POST /databases/{name}`
    pub fn create_database(&self, name: &str) -> Result<Value> {
        let resp = self
            .client
            .post(format!("{}/databases/{}", self.base_url, name))
            .send()
            .context("failed to send create database request")?;
        Self::parse_response(resp)
    }

    /// `GET /databases`
    pub fn list_databases(&self) -> Result<Value> {
        let resp = self
            .client
            .get(format!("{}/databases", self.base_url))
            .send()
            .context("failed to send list databases request")?;
        Self::parse_response(resp)
    }

    // ── Schema operations ────────────────────────────────────────────────────

    /// `GET /collections/{name}/schema`
    pub fn get_schema(&self, name: &str) -> Result<Value> {
        let resp = self
            .client
            .get(format!("{}/collections/{}/schema", self.base_url, name))
            .send()
            .context("failed to send get schema request")?;
        Self::parse_response(resp)
    }

    /// `PUT /collections/{name}/schema`
    ///
    /// The HTTP body is a flat `PutSchemaBody`: `{version, entity_schema, description?,
    /// link_types?, force, dry_run}` — NOT a wrapped `{schema: {...}}`.
    pub fn put_schema(&self, name: &str, opts: PutSchemaOptions<'_>) -> Result<Value> {
        let PutSchemaOptions {
            version,
            entity_schema,
            description,
            force,
            dry_run,
            actor,
        } = opts;
        let mut body = serde_json::json!({
            "version": version,
            "entity_schema": entity_schema,
            "force": force,
            "dry_run": dry_run,
        });
        if let Some(d) = description {
            body["description"] = Value::String(d.to_string());
        }
        if let Some(a) = actor {
            body["actor"] = Value::String(a.to_string());
        }
        let resp = self
            .client
            .put(format!("{}/collections/{}/schema", self.base_url, name))
            .json(&body)
            .send()
            .context("failed to send put schema request")?;
        Self::parse_response(resp)
    }

    // ── Link operations ──────────────────────────────────────────────────────

    /// `POST /links`
    pub fn create_link(
        &self,
        source_collection: &str,
        source_id: &str,
        target_collection: &str,
        target_id: &str,
        link_type: &str,
        actor: Option<&str>,
    ) -> Result<Value> {
        let mut body = serde_json::json!({
            "source_collection": source_collection,
            "source_id": source_id,
            "target_collection": target_collection,
            "target_id": target_id,
            "link_type": link_type,
            "metadata": null,
        });
        if let Some(a) = actor {
            body["actor"] = Value::String(a.to_string());
        }
        let resp = self
            .client
            .post(format!("{}/links", self.base_url))
            .json(&body)
            .send()
            .context("failed to send create link request")?;
        Self::parse_response(resp)
    }

    /// `GET /traverse/{collection}/{id}?link_type=…&max_depth=…`
    pub fn traverse(
        &self,
        collection: &str,
        id: &str,
        link_type: Option<&str>,
        max_depth: Option<usize>,
    ) -> Result<Value> {
        let mut url = format!("{}/traverse/{}/{}", self.base_url, collection, id);
        let mut params: Vec<String> = Vec::new();
        if let Some(lt) = link_type {
            params.push(format!("link_type={lt}"));
        }
        if let Some(d) = max_depth {
            params.push(format!("max_depth={d}"));
        }
        if !params.is_empty() {
            url = format!("{}?{}", url, params.join("&"));
        }
        let resp = self
            .client
            .get(&url)
            .send()
            .context("failed to send traverse request")?;
        Self::parse_response(resp)
    }

    // ── Audit operations ─────────────────────────────────────────────────────

    /// `GET /audit/query?collection=…&entity_id=…&actor=…&limit=…`
    pub fn query_audit(
        &self,
        collection: Option<&str>,
        entity_id: Option<&str>,
        actor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Value> {
        let mut params: Vec<String> = Vec::new();
        if let Some(c) = collection {
            params.push(format!("collection={c}"));
        }
        if let Some(e) = entity_id {
            params.push(format!("entity_id={e}"));
        }
        if let Some(a) = actor {
            params.push(format!("actor={a}"));
        }
        if let Some(l) = limit {
            params.push(format!("limit={l}"));
        }
        let url = if params.is_empty() {
            format!("{}/audit/query", self.base_url)
        } else {
            format!("{}/audit/query?{}", self.base_url, params.join("&"))
        };
        let resp = self
            .client
            .get(&url)
            .send()
            .context("failed to send audit query request")?;
        Self::parse_response(resp)
    }

    // ── Health ───────────────────────────────────────────────────────────────

    /// `GET /health`
    pub fn health(&self) -> Result<Value> {
        let resp = self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .context("failed to send health request")?;
        Self::parse_response(resp)
    }

    // ── User-role management ─────────────────────────────────────────────────

    /// `GET /control/users` — list all explicit user-role assignments.
    pub fn list_users(&self) -> Result<Value> {
        let resp = self
            .client
            .get(format!("{}/control/users", self.base_url))
            .send()
            .context("failed to send list-users request")?;
        Self::parse_response(resp)
    }

    /// `PUT /control/users/{login}` — assign a role to a principal.
    pub fn set_user_role(&self, login: &str, role: &str) -> Result<Value> {
        let resp = self
            .client
            .put(format!("{}/control/users/{login}", self.base_url))
            .json(&serde_json::json!({ "role": role }))
            .send()
            .context("failed to send set-user-role request")?;
        Self::parse_response(resp)
    }

    /// `DELETE /control/users/{login}` — remove an explicit role assignment.
    pub fn remove_user_role(&self, login: &str) -> Result<Value> {
        let resp = self
            .client
            .delete(format!("{}/control/users/{login}", self.base_url))
            .send()
            .context("failed to send remove-user-role request")?;
        Self::parse_response(resp)
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    /// Parse a response: if 2xx, return the JSON body; otherwise return an error.
    fn parse_response(resp: reqwest::blocking::Response) -> Result<Value> {
        let status = resp.status();
        let body = resp.text().context("failed to read response body")?;
        if status.is_success() {
            serde_json::from_str(&body).context("server returned invalid JSON")
        } else {
            // Try to extract a structured error message from the body.
            if let Ok(err_json) = serde_json::from_str::<Value>(&body) {
                if let Some(detail) = err_json.get("detail") {
                    anyhow::bail!(
                        "server error ({}): {}",
                        status,
                        detail
                    );
                }
            }
            anyhow::bail!("server error ({}): {}", status, body)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn client_new_builds_successfully() {
        let client = HttpClient::new("http://127.0.0.1:4170", 200);
        assert!(client.is_ok());
    }

    #[test]
    fn unreachable_server_returns_false() {
        // Connect to a port that almost certainly has nothing listening.
        let client = HttpClient::new("http://127.0.0.1:19999", 100).unwrap();
        assert!(!client.is_reachable());
    }

    #[test]
    fn base_url_trailing_slash_stripped() {
        let client = HttpClient::new("http://localhost:4170/", 200).unwrap();
        assert_eq!(client.base_url, "http://localhost:4170");
    }
}
