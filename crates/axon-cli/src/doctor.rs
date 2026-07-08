//! `axon doctor` — print diagnostic information about the Axon installation.

#[cfg(feature = "serve")]
use std::time::Duration;

use axon_api::handler::AxonHandler;
use axon_api::request::ListDatabasesRequest;
#[cfg(feature = "serve")]
use axon_api::response::ListDatabasesResponse;
use axon_storage::SqliteStorageAdapter;

#[cfg(feature = "serve")]
use crate::client::HttpClient;

#[cfg(feature = "serve")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerProbe {
    Reachable,
    Responded(reqwest::StatusCode),
    NotReachable,
}

pub fn run_doctor() -> anyhow::Result<()> {
    let config_path = axon_config::paths::config_file();
    let data_dir = axon_config::paths::data_dir();

    println!("Axon {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Config file: {}", config_path.display());
    println!("  exists: {}", config_path.exists());
    println!("Data directory: {}", data_dir.display());
    println!("  exists: {}", data_dir.exists());

    // Load config.
    let config = axon_config::AxonConfig::load(Some(&config_path)).unwrap_or_default();
    println!("Storage backend: {}", config.storage.backend);
    println!("HTTP port: {}", config.server.http_port);
    if let Some(grpc) = config.server.grpc_port {
        println!("gRPC port: {grpc}");
    } else {
        println!("gRPC: disabled");
    }
    println!("Resolved auth mode: {}", describe_auth_mode(&config));

    // Check server connectivity.
    #[cfg(feature = "serve")]
    {
        let server_url = config.client.server_url.trim_end_matches('/');
        let probe = probe_server(server_url, config.client.connect_timeout_ms);
        print_server_probe(&config.client.server_url, probe);

        if matches!(probe, ServerProbe::Reachable) {
            print_server_databases(server_url, config.client.connect_timeout_ms);
        } else if matches!(probe, ServerProbe::NotReachable) {
            print_unreachable_guidance(server_url);
        }
    }

    #[cfg(not(feature = "serve"))]
    {
        println!(
            "Server ({}): (connectivity check unavailable — build without 'serve' feature)",
            config.client.server_url
        );
    }

    print_embedded_databases(&config);

    Ok(())
}

fn describe_auth_mode(config: &axon_config::AxonConfig) -> String {
    match config.auth.mode.as_str() {
        "no-auth" => "no-auth (explicit opt-in; local/dev only)".to_string(),
        "guest" => format!("guest (guest role: {})", config.auth.guest_role),
        other => other.to_string(),
    }
}

#[cfg(feature = "serve")]
fn probe_server(server_url: &str, timeout_ms: u64) -> ServerProbe {
    let url = format!("{server_url}/health");
    let client = match reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(timeout_ms))
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(_) => return ServerProbe::NotReachable,
    };

    match client.get(url).send() {
        Ok(resp) if resp.status().is_success() => ServerProbe::Reachable,
        Ok(resp) => ServerProbe::Responded(resp.status()),
        Err(_) => ServerProbe::NotReachable,
    }
}

#[cfg(feature = "serve")]
fn print_server_probe(server_url: &str, probe: ServerProbe) {
    match probe {
        ServerProbe::Reachable => {
            println!("Server ({server_url}): reachable");
            println!("Effective CLI mode: client");
        }
        ServerProbe::Responded(status) => {
            println!("Server ({server_url}): responded with {status}");
            println!("Effective CLI mode: embedded");
        }
        ServerProbe::NotReachable => {
            println!("Server ({server_url}): not reachable");
            println!("Effective CLI mode: embedded");
        }
    }
}

#[cfg(feature = "serve")]
fn print_unreachable_guidance(server_url: &str) {
    println!();
    println!("Next steps:");
    println!(
        "  - Local dev/test: run `axon serve --no-auth` (or `cargo run -p axon-cli -- serve --no-auth`) and re-run `axon doctor`."
    );
    println!(
        "  - Installed service: run `axon server status`, then `axon server start` or `axon server restart` if it is stopped."
    );
    println!("  - Verify the configured URL in the loaded config file before retrying.");
    println!("  - Recheck health with `curl -fsS {server_url}/health`.");
}

#[cfg(feature = "serve")]
fn print_server_databases(server_url: &str, timeout_ms: u64) {
    let Ok(client) = HttpClient::new(server_url, timeout_ms) else {
        println!("Databases (server): unavailable (failed to build HTTP client)");
        return;
    };

    match client.list_databases() {
        Ok(value) => match serde_json::from_value::<ListDatabasesResponse>(value) {
            Ok(resp) => print_database_section("server", &resp.databases),
            Err(err) => println!("Databases (server): unavailable (invalid response: {err})"),
        },
        Err(err) => println!("Databases (server): unavailable ({err})"),
    }
}

fn print_embedded_databases(config: &axon_config::AxonConfig) {
    let sqlite_path = config.resolved_data_dir().join("axon.db");
    if !sqlite_path.exists() {
        return;
    }

    let sqlite_path = sqlite_path.to_string_lossy().into_owned();
    match SqliteStorageAdapter::open(&sqlite_path) {
        Ok(storage) => {
            let handler = AxonHandler::new(storage);
            match handler.list_databases(ListDatabasesRequest {}) {
                Ok(resp) => print_database_section("embedded SQLite", &resp.databases),
                Err(err) => println!("Databases (embedded SQLite): unavailable ({err})"),
            }
        }
        Err(err) => println!("Databases (embedded SQLite): unavailable ({err})"),
    }
}

fn print_database_section(source: &str, databases: &[String]) {
    if databases.is_empty() {
        println!("Databases ({source}): (none)");
        return;
    }

    println!("Databases ({source}):");
    for database in databases {
        println!("  - {database}");
    }
}
