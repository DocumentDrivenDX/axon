//! `axon doctor` — print diagnostic information about the Axon installation.

pub fn run_doctor() -> anyhow::Result<()> {
    let config_path = axon_config::paths::config_file();
    let data_dir = axon_config::paths::data_dir();

    println!("Axon {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Config file: {}", config_path.display());
    println!("  exists: {}", config_path.exists());
    println!("Data directory: {}", data_dir.display());
    println!("  exists: {}", data_dir.exists());

    // Load config
    let config = axon_config::AxonConfig::load(Some(&config_path)).unwrap_or_default();
    println!("Storage backend: {}", config.storage.backend);
    println!("HTTP port: {}", config.server.http_port);
    if let Some(grpc) = config.server.grpc_port {
        println!("gRPC port: {grpc}");
    } else {
        println!("gRPC: disabled");
    }

    // Check server connectivity
    #[cfg(feature = "serve")]
    {
        let url = format!("{}/healthz", config.client.server_url);
        print!("Server ({}):", config.client.server_url);
        match reqwest::blocking::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_millis(
                config.client.connect_timeout_ms,
            ))
            .send()
        {
            Ok(resp) if resp.status().is_success() => println!(" reachable"),
            Ok(resp) => println!(" responded with {}", resp.status()),
            Err(_) => println!(" not reachable"),
        }
    }

    #[cfg(not(feature = "serve"))]
    {
        println!(
            "Server ({}): (connectivity check unavailable — build without 'serve' feature)",
            config.client.server_url
        );
    }

    Ok(())
}
