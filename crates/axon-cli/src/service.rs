//! `axon install` — manage Axon as a system service (systemd / launchd).

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

/// Prominent warning printed when `--no-auth` is used at install time.
///
/// Per FEAT-028 BIN-10: the warning must name the exposure so operators
/// understand what they are opting into.
const NO_AUTH_INSTALL_WARNING: &str = "\
WARNING: Installing Axon service with authentication DISABLED (--no-auth).
All requests will succeed as admin without any credential check.
This is unsafe if the server is reachable over a network.
Omit --no-auth to install with authentication enabled (the safe default).
";

/// Which service action to perform.
pub enum ServiceAction {
    Install { global: bool, no_auth: bool },
    Uninstall,
    Start,
    Stop,
    Restart,
    Status,
}

pub fn run_service(action: ServiceAction) -> Result<()> {
    match action {
        ServiceAction::Install { global, no_auth } => install_service(global, no_auth),
        ServiceAction::Uninstall => uninstall_service(),
        ServiceAction::Start => ctl("start"),
        ServiceAction::Stop => ctl("stop"),
        ServiceAction::Restart => ctl("restart"),
        ServiceAction::Status => ctl("status"),
    }
}

// ── Installation ─────────────────────────────────────────────────────────────

fn binary_path() -> Result<PathBuf> {
    std::env::current_exe().context("could not determine path to axon binary")
}

fn install_service(global: bool, no_auth: bool) -> Result<()> {
    if no_auth {
        eprintln!("{NO_AUTH_INSTALL_WARNING}");
    }
    let bin = binary_path()?;

    if cfg!(target_os = "linux") {
        install_systemd(&bin, global, no_auth)
    } else if cfg!(target_os = "macos") {
        install_launchd(&bin, global, no_auth)
    } else {
        anyhow::bail!("service installation is not supported on this platform");
    }
}

fn uninstall_service() -> Result<()> {
    if cfg!(target_os = "linux") {
        uninstall_systemd()
    } else if cfg!(target_os = "macos") {
        uninstall_launchd()
    } else {
        anyhow::bail!("service uninstallation is not supported on this platform");
    }
}

// ── systemd (Linux) ──────────────────────────────────────────────────────────

/// User service (~/.config/systemd/user/axon.service).
/// Runs as the invoking user; `WantedBy=default.target` is correct here.
///
/// `@NO_AUTH_FLAG@` is replaced at install time: empty string for the secure
/// default, or `"--no-auth "` when the operator explicitly opts out of auth.
const SYSTEMD_USER_UNIT: &str = "\
[Unit]
Description=Axon Data Store
After=network.target

[Service]
Type=simple
ExecStart={binary_path} serve @NO_AUTH_FLAG@--tls-self-signed --sqlite-path @SQLITE_PATH@ --control-plane-path @CONTROL_PLANE_PATH@
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
";

/// System service (/etc/systemd/system/axon.service).
/// Runs as the `axon` system user; `WantedBy=multi-user.target` is standard for
/// non-graphical daemons.  The user and data directory must be created separately
/// (see `create_axon_system_user`).
///
/// `@NO_AUTH_FLAG@` is replaced at install time: empty string for the secure
/// default, or `"--no-auth "` when the operator explicitly opts out of auth.
const SYSTEMD_GLOBAL_UNIT: &str = "\
[Unit]
Description=Axon Data Store
After=network.target

[Service]
Type=simple
User=axon
Group=axon
ExecStart={binary_path} serve @NO_AUTH_FLAG@--tls-self-signed --sqlite-path @SQLITE_PATH@ --control-plane-path @CONTROL_PLANE_PATH@
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
";

fn systemd_unit_path(global: bool) -> Result<PathBuf> {
    if global {
        Ok(PathBuf::from("/etc/systemd/system/axon.service"))
    } else {
        let home = std::env::var("HOME").context("$HOME is not set")?;
        let dir = PathBuf::from(home)
            .join(".config")
            .join("systemd")
            .join("user");
        Ok(dir.join("axon.service"))
    }
}

fn create_axon_system_user() -> Result<()> {
    // Check whether the `axon` system user already exists.
    let exists = Command::new("id")
        .arg("axon")
        .status()
        .context("failed to run `id axon`")?
        .success();

    if !exists {
        run_cmd(
            "useradd",
            &[
                "--system",
                "--no-create-home",
                "--home-dir",
                "/var/lib/axon",
                "--shell",
                "/usr/sbin/nologin",
                "--comment",
                "Axon Data Store",
                "axon",
            ],
        )
        .context("failed to create `axon` system user")?;
        println!("created system user `axon`");
    }

    // Ensure the data directory exists and is owned by axon.
    std::fs::create_dir_all("/var/lib/axon").context("failed to create /var/lib/axon")?;
    run_cmd("chown", &["axon:axon", "/var/lib/axon"]).context("failed to chown /var/lib/axon")?;
    println!("data directory: /var/lib/axon");
    Ok(())
}

fn service_data_paths(global: bool) -> (PathBuf, PathBuf) {
    let data_dir = if global {
        axon_config::paths::global_data_dir()
    } else {
        axon_config::paths::data_dir()
    };
    (
        data_dir.join("axon.db"),
        data_dir.join("axon-control-plane.db"),
    )
}

fn install_systemd(bin: &std::path::Path, global: bool, no_auth: bool) -> Result<()> {
    let unit_path = systemd_unit_path(global)?;
    let template = if global {
        SYSTEMD_GLOBAL_UNIT
    } else {
        SYSTEMD_USER_UNIT
    };
    let (sqlite_path, control_plane_path) = service_data_paths(global);
    let no_auth_flag = if no_auth { "--no-auth " } else { "" };
    let unit_content = template
        .replace("{binary_path}", &bin.display().to_string())
        .replace("@NO_AUTH_FLAG@", no_auth_flag)
        .replace("@SQLITE_PATH@", &sqlite_path.display().to_string())
        .replace(
            "@CONTROL_PLANE_PATH@",
            &control_plane_path.display().to_string(),
        );

    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    std::fs::write(&unit_path, unit_content)
        .with_context(|| format!("failed to write {}", unit_path.display()))?;
    println!("wrote {}", unit_path.display());

    if global {
        create_axon_system_user()?;
        run_cmd("systemctl", &["daemon-reload"])?;
        run_cmd("systemctl", &["enable", "axon"])?;
        println!("enabled axon.service (system)");
    } else {
        run_cmd("systemctl", &["--user", "daemon-reload"])?;
        run_cmd("systemctl", &["--user", "enable", "axon"])?;
        println!("enabled axon.service (user)");
    }
    Ok(())
}

fn uninstall_systemd() -> Result<()> {
    // Try user first, then global
    let user_path = systemd_unit_path(false)?;
    let global_path = systemd_unit_path(true)?;

    if user_path.exists() {
        let _ = run_cmd("systemctl", &["--user", "stop", "axon"]);
        let _ = run_cmd("systemctl", &["--user", "disable", "axon"]);
        std::fs::remove_file(&user_path)
            .with_context(|| format!("failed to remove {}", user_path.display()))?;
        run_cmd("systemctl", &["--user", "daemon-reload"])?;
        println!("removed {}", user_path.display());
    } else if global_path.exists() {
        let _ = run_cmd("systemctl", &["stop", "axon"]);
        let _ = run_cmd("systemctl", &["disable", "axon"]);
        std::fs::remove_file(&global_path)
            .with_context(|| format!("failed to remove {}", global_path.display()))?;
        run_cmd("systemctl", &["daemon-reload"])?;
        println!("removed {}", global_path.display());
    } else {
        anyhow::bail!("no axon service unit found");
    }
    Ok(())
}

// ── launchd (macOS) ──────────────────────────────────────────────────────────

/// `@NO_AUTH_ARG@` is replaced at install time: empty string for the secure
/// default, or `"        <string>--no-auth</string>\n"` when the operator
/// explicitly opts out of auth (FEAT-028 BIN-10).
const LAUNCHD_PLIST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.axon.server</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary_path}</string>
        <string>serve</string>
@NO_AUTH_ARG@        <string>--tls-self-signed</string>
        <string>--sqlite-path</string>
        <string>@SQLITE_PATH@</string>
        <string>--control-plane-path</string>
        <string>@CONTROL_PLANE_PATH@</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
"#;

const LAUNCHD_LABEL: &str = "com.axon.server";

/// Returns `gui/<uid>` — the launchctl domain for the current user's GUI session.
fn launchd_user_domain() -> Result<String> {
    let out = Command::new("id")
        .arg("-u")
        .output()
        .context("failed to run `id -u`")?;
    let uid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(format!("gui/{uid}"))
}

fn launchd_plist_path(global: bool) -> Result<PathBuf> {
    if global {
        Ok(PathBuf::from(
            "/Library/LaunchDaemons/com.axon.server.plist",
        ))
    } else {
        let home = std::env::var("HOME").context("$HOME is not set")?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join("com.axon.server.plist"))
    }
}

fn install_launchd(bin: &std::path::Path, global: bool, no_auth: bool) -> Result<()> {
    let plist_path = launchd_plist_path(global)?;
    let (sqlite_path, control_plane_path) = service_data_paths(global);
    let no_auth_arg = if no_auth {
        "        <string>--no-auth</string>\n"
    } else {
        ""
    };
    let plist_content = LAUNCHD_PLIST_TEMPLATE
        .replace("{binary_path}", &bin.display().to_string())
        .replace("@NO_AUTH_ARG@", no_auth_arg)
        .replace("@SQLITE_PATH@", &sqlite_path.display().to_string())
        .replace(
            "@CONTROL_PLANE_PATH@",
            &control_plane_path.display().to_string(),
        );

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    std::fs::write(&plist_path, plist_content)
        .with_context(|| format!("failed to write {}", plist_path.display()))?;
    println!("wrote {}", plist_path.display());

    // `launchctl load` is deprecated; use `bootstrap` on macOS 10.15+.
    // For user agents: bootstrap gui/<uid>; for system daemons: bootstrap system.
    if global {
        run_cmd(
            "launchctl",
            &["bootstrap", "system", &plist_path.display().to_string()],
        )?;
    } else {
        let domain = launchd_user_domain()?;
        run_cmd(
            "launchctl",
            &["bootstrap", &domain, &plist_path.display().to_string()],
        )?;
    }
    println!("loaded {LAUNCHD_LABEL}");
    Ok(())
}

fn uninstall_launchd() -> Result<()> {
    let user_path = launchd_plist_path(false)?;
    let global_path = launchd_plist_path(true)?;

    let (plist_path, domain) = if user_path.exists() {
        (user_path, launchd_user_domain()?)
    } else if global_path.exists() {
        (global_path, "system".to_string())
    } else {
        anyhow::bail!("no axon launchd plist found");
    };

    // `launchctl unload` is deprecated; use `bootout`.
    let _ = run_cmd(
        "launchctl",
        &["bootout", &domain, &plist_path.display().to_string()],
    );
    std::fs::remove_file(&plist_path)
        .with_context(|| format!("failed to remove {}", plist_path.display()))?;
    println!("removed {}", plist_path.display());
    Ok(())
}

// ── Service control ──────────────────────────────────────────────────────────

fn ctl(verb: &str) -> Result<()> {
    if cfg!(target_os = "linux") {
        // Try user service first
        let user_path = systemd_unit_path(false)?;
        if user_path.exists() {
            run_cmd("systemctl", &["--user", verb, "axon"])
        } else {
            run_cmd("systemctl", &[verb, "axon"])
        }
    } else if cfg!(target_os = "macos") {
        match verb {
            "start" => run_cmd("launchctl", &["start", LAUNCHD_LABEL]),
            "stop" => run_cmd("launchctl", &["stop", LAUNCHD_LABEL]),
            "restart" => {
                let _ = run_cmd("launchctl", &["stop", LAUNCHD_LABEL]);
                run_cmd("launchctl", &["start", LAUNCHD_LABEL])
            }
            "status" => run_cmd("launchctl", &["list", LAUNCHD_LABEL]),
            _ => anyhow::bail!("unsupported service action: {verb}"),
        }
    } else {
        anyhow::bail!("service management is not supported on this platform");
    }
}

fn run_cmd(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to run {program}"))?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("{} {} exited with {}", program, args.join(" "), status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default (authenticated) install ─────────────────────────────────────────

    #[test]
    fn systemd_user_unit_default_is_authenticated() {
        // The raw template must NOT contain --no-auth; auth is the default.
        // Per CONTRACT-008 / FEAT-028 BIN-10: service installs are authenticated
        // by default; --no-auth is an explicit opt-in only.
        assert!(
            !SYSTEMD_USER_UNIT.contains("--no-auth"),
            "user systemd unit template must not contain --no-auth (authenticated by default)",
        );
    }

    #[test]
    fn systemd_global_unit_default_is_authenticated() {
        assert!(
            !SYSTEMD_GLOBAL_UNIT.contains("--no-auth"),
            "global systemd unit template must not contain --no-auth (authenticated by default)",
        );
    }

    #[test]
    fn launchd_plist_default_is_authenticated() {
        // The raw template must not include --no-auth as a <string> element.
        assert!(
            !LAUNCHD_PLIST_TEMPLATE.contains("<string>--no-auth</string>"),
            "launchd plist template must not contain --no-auth (authenticated by default)",
        );
    }

    #[test]
    fn systemd_user_unit_enables_self_signed_tls() {
        assert!(
            SYSTEMD_USER_UNIT.contains("--tls-self-signed"),
            "user systemd service must install with TLS enabled by default",
        );
    }

    #[test]
    fn systemd_global_unit_enables_self_signed_tls() {
        assert!(
            SYSTEMD_GLOBAL_UNIT.contains("--tls-self-signed"),
            "global systemd service must install with TLS enabled by default",
        );
    }

    #[test]
    fn launchd_plist_enables_self_signed_tls() {
        assert!(
            LAUNCHD_PLIST_TEMPLATE.contains("<string>--tls-self-signed</string>"),
            "launchd service must install with TLS enabled by default",
        );
    }

    // ── Opt-in no-auth install ──────────────────────────────────────────────────

    #[test]
    fn systemd_user_unit_no_auth_opt_in_inserts_flag() {
        let unit = SYSTEMD_USER_UNIT
            .replace("{binary_path}", "/usr/bin/axon")
            .replace("@NO_AUTH_FLAG@", "--no-auth ")
            .replace("@SQLITE_PATH@", "/var/lib/axon/axon.db")
            .replace("@CONTROL_PLANE_PATH@", "/var/lib/axon/axon-control-plane.db");
        assert!(
            unit.contains("serve --no-auth --tls-self-signed"),
            "explicit --no-auth opt-in must appear in generated unit",
        );
    }

    #[test]
    fn systemd_user_unit_default_install_has_no_no_auth_flag() {
        let unit = SYSTEMD_USER_UNIT
            .replace("{binary_path}", "/usr/bin/axon")
            .replace("@NO_AUTH_FLAG@", "")
            .replace("@SQLITE_PATH@", "/var/lib/axon/axon.db")
            .replace("@CONTROL_PLANE_PATH@", "/var/lib/axon/axon-control-plane.db");
        assert!(
            !unit.contains("--no-auth"),
            "default install (no opt-in) must not contain --no-auth",
        );
    }

    #[test]
    fn launchd_plist_no_auth_opt_in_inserts_element() {
        let plist = LAUNCHD_PLIST_TEMPLATE
            .replace("{binary_path}", "/usr/local/bin/axon")
            .replace("@NO_AUTH_ARG@", "        <string>--no-auth</string>\n")
            .replace("@SQLITE_PATH@", "/var/lib/axon/axon.db")
            .replace("@CONTROL_PLANE_PATH@", "/var/lib/axon/axon-control-plane.db");
        assert!(
            plist.contains("<string>--no-auth</string>"),
            "explicit --no-auth opt-in must appear in generated plist",
        );
    }

    #[test]
    fn launchd_plist_default_install_has_no_no_auth_element() {
        let plist = LAUNCHD_PLIST_TEMPLATE
            .replace("{binary_path}", "/usr/local/bin/axon")
            .replace("@NO_AUTH_ARG@", "")
            .replace("@SQLITE_PATH@", "/var/lib/axon/axon.db")
            .replace("@CONTROL_PLANE_PATH@", "/var/lib/axon/axon-control-plane.db");
        assert!(
            !plist.contains("--no-auth"),
            "default install (no opt-in) must not contain --no-auth in plist",
        );
    }
}
