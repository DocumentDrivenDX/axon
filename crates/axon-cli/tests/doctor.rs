use std::fs;
use std::net::TcpListener;
use std::process::Command;

fn axon_bin() -> &'static str {
    env!("CARGO_BIN_EXE_axon")
}

#[test]
fn doctor_reports_unreachable_server_with_actionable_guidance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    let config_dir = config_home.join("axon");
    let data_dir = data_home.join("axon");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);

    fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"
[client]
server_url = "http://127.0.0.1:{port}"
connect_timeout_ms = 25

[auth]
mode = "no-auth"
guest_role = "admin"
"#
        ),
    )
    .expect("write config");

    let output = Command::new(axon_bin())
        .arg("doctor")
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("run axon doctor");

    assert!(
        output.status.success(),
        "doctor failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.contains("Config file:"));
    assert!(stdout.contains("exists: true"));
    assert!(stdout.contains("Resolved auth mode: no-auth (explicit opt-in; local/dev only)"));
    assert!(stdout.contains(&format!("Server (http://127.0.0.1:{port}): not reachable")));
    assert!(stdout.contains("Effective CLI mode: embedded"));
    assert!(stdout.contains("Next steps:"));
    assert!(stdout.contains("axon serve --no-auth"));
    assert!(stdout.contains("axon server status"));
    assert!(stdout.contains(&format!("curl -fsS http://127.0.0.1:{port}/health")));
}
