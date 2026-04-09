//! Startup regressions for auth mode selection.

#![allow(clippy::unwrap_used)]

use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn axon_server_bin() -> &'static str {
    env!("CARGO_BIN_EXE_axon-server")
}

fn unreachable_tailscale_socket() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("/tmp/axon-missing-tailscaled-{nanos}.sock")
}

fn base_command() -> Command {
    let mut command = Command::new(axon_server_bin());
    command
        .args([
            "--storage",
            "memory",
            "--http-port",
            "0",
            "--grpc-port",
            "0",
        ])
        .env("RUST_LOG", "info")
        .env("AXON_TAILSCALE_SOCKET", unreachable_tailscale_socket())
        .env_remove("AXON_NO_AUTH")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn wait_for_startup(child: &mut Child) {
    for _ in 0..20 {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("server exited before startup completed: {status}");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn kill_and_collect(child: Child) -> Output {
    let mut child = child;
    child.kill().unwrap();
    child.wait_with_output().unwrap()
}

fn combined_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}{stderr}")
}

fn assert_bypass_starts(mut command: Command) {
    let mut child = command.spawn().unwrap();
    wait_for_startup(&mut child);
    let output = kill_and_collect(child);
    let logs = combined_output(&output);

    assert!(
        logs.contains("running in --no-auth mode: all requests succeed as admin"),
        "expected no-auth startup log, got output:\n{logs}"
    );
    assert!(
        logs.contains("HTTP gateway listening on"),
        "expected HTTP listener startup log, got output:\n{logs}"
    );
    assert!(
        logs.contains("gRPC service listening on"),
        "expected gRPC listener startup log, got output:\n{logs}"
    );
}

#[test]
fn default_startup_requires_auth_initialization() {
    let output = base_command().output().unwrap();
    let logs = combined_output(&output);

    assert!(
        !output.status.success(),
        "default startup should fail when auth cannot initialize"
    );
    assert!(
        logs.contains("failed to initialize auth via"),
        "expected auth initialization failure, got output:\n{logs}"
    );
    assert!(
        !logs.contains("running in --no-auth mode"),
        "default startup must not fall back to anonymous-admin mode:\n{logs}"
    );
}

#[test]
fn no_auth_flag_starts_with_development_bypass() {
    let mut command = base_command();
    command.arg("--no-auth");
    assert_bypass_starts(command);
}

#[test]
fn no_auth_env_starts_with_development_bypass() {
    let mut command = base_command();
    command.env("AXON_NO_AUTH", "1");
    assert_bypass_starts(command);
}
