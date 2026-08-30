use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

const SERVICE_NAME: &str = "cratera.service";
const UNIT_PATH: &str = "/etc/systemd/system/cratera.service";
const OPT_DIR: &str = "/opt/cratera";
const OPT_BIN: &str = "/opt/cratera/cratera";

pub fn is_systemd_available() -> bool {
    Command::new("systemctl")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn is_service_installed() -> bool {
    Path::new(UNIT_PATH).exists() && Path::new(OPT_BIN).exists()
}

pub fn is_service_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", SERVICE_NAME])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false)
}

pub fn is_service_enabled() -> bool {
    Command::new("systemctl")
        .args(["is-enabled", SERVICE_NAME])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
        .unwrap_or(false)
}

fn is_root() -> bool {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() == 0 }
}

fn run_privileged_cmd(cmd: &str, args: &[&str]) -> anyhow::Result<bool> {
    let mut command = if is_root() {
        let mut c = Command::new(cmd);
        c.args(args);
        c
    } else {
        let mut c = Command::new("sudo");
        c.arg(cmd);
        c.args(args);
        c
    };

    let status = command.status()?;
    Ok(status.success())
}

pub fn deploy_to_opt() -> anyhow::Result<bool> {
    println!("==> Deploying Cratera daemon and runtime assets to {OPT_DIR}...");

    // 1. Create directory structures
    run_privileged_cmd(
        "mkdir",
        &[
            "-p",
            OPT_DIR,
            "/opt/cratera/images",
            "/opt/cratera/images/snapshot",
            "/var/lib/cratera",
            "/usr/local/bin",
        ],
    )?;

    // 2. Locate binary to deploy
    let current_exe = std::env::current_exe().ok();
    let bin_src = if Path::new("target/release/cratera").exists() {
        "target/release/cratera"
    } else if let Some(ref exe) = current_exe {
        exe.to_str().unwrap_or("cratera")
    } else {
        "cratera"
    };

    println!("  -> Installing binary {bin_src} -> {OPT_BIN}");
    run_privileged_cmd("cp", &[bin_src, OPT_BIN])?;
    run_privileged_cmd("chmod", &["755", OPT_BIN])?;
    run_privileged_cmd("cp", &[bin_src, "/usr/local/bin/cratera"])?;
    run_privileged_cmd("chmod", &["755", "/usr/local/bin/cratera"])?;

    // 3. Copy languages.toml
    if Path::new("languages.toml").exists() {
        println!("  -> Copying languages.toml -> /opt/cratera/languages.toml");
        run_privileged_cmd("cp", &["languages.toml", "/opt/cratera/languages.toml"])?;
    }

    // 4. Copy runtime images
    if Path::new("images/firecracker").exists() {
        run_privileged_cmd("cp", &["images/firecracker", "/usr/local/bin/firecracker"])?;
        run_privileged_cmd("chmod", &["755", "/usr/local/bin/firecracker"])?;
    }
    if Path::new("images/jailer").exists() {
        run_privileged_cmd("cp", &["images/jailer", "/usr/local/bin/jailer"])?;
        run_privileged_cmd("chmod", &["755", "/usr/local/bin/jailer"])?;
    }
    if Path::new("images/vmlinux.bin").exists() {
        run_privileged_cmd(
            "cp",
            &["images/vmlinux.bin", "/opt/cratera/images/vmlinux.bin"],
        )?;
    }
    if Path::new("images/rootfs.squashfs").exists() {
        run_privileged_cmd(
            "cp",
            &[
                "images/rootfs.squashfs",
                "/opt/cratera/images/rootfs.squashfs",
            ],
        )?;
        run_privileged_cmd(
            "ln",
            &["-sfn", "rootfs.squashfs", "/opt/cratera/images/rootfs.ext4"],
        )?;
    } else if Path::new("images/rootfs.ext4").exists() {
        run_privileged_cmd(
            "cp",
            &["images/rootfs.ext4", "/opt/cratera/images/rootfs.ext4"],
        )?;
    }

    // 5. Copy .env if available
    if Path::new(".env").exists() && !Path::new("/opt/cratera/.env").exists() {
        run_privileged_cmd("cp", &[".env", "/opt/cratera/.env"])?;
        run_privileged_cmd("chmod", &["600", "/opt/cratera/.env"])?;
    }

    // 6. Host setup & permissions
    if Path::new("scripts/host-setup.sh").exists() {
        println!("  -> Configuring KVM & Jailer UID 20001 permissions...");
        let _ = run_privileged_cmd("bash", &["scripts/host-setup.sh"]);
    }

    // 7. Copy systemd unit file
    let unit_template = "deploy/cratera.service";
    if Path::new(unit_template).exists() {
        run_privileged_cmd("cp", &[unit_template, UNIT_PATH])?;
        run_privileged_cmd("systemctl", &["daemon-reload"])?;
    }

    Ok(true)
}

pub fn start_service() -> anyhow::Result<()> {
    if !is_service_installed() {
        println!("{YELLOW}! Service files not deployed to {OPT_DIR}. Deploying now...{RESET}");
        deploy_to_opt()?;
    }

    println!("==> Starting {SERVICE_NAME} via systemd...");
    let _ = run_privileged_cmd("systemctl", &["reset-failed", SERVICE_NAME]);
    let _ = run_privileged_cmd("systemctl", &["start", SERVICE_NAME]);

    // Give systemd a moment to launch and verify liveness
    sleep(Duration::from_millis(600));

    if is_service_active() {
        println!("{GREEN}✓ {SERVICE_NAME} is active & running on http://127.0.0.1:3100{RESET}");
    } else {
        eprintln!("{RED}✗ {SERVICE_NAME} failed to start. Recent journal logs:{RESET}\n");
        let _ = show_logs(15);
    }
    Ok(())
}

pub fn stop_service() -> anyhow::Result<()> {
    println!("==> Stopping {SERVICE_NAME} via systemd...");
    let _ = run_privileged_cmd("systemctl", &["stop", SERVICE_NAME]);
    sleep(Duration::from_millis(300));
    if !is_service_active() {
        println!("{YELLOW}✓ {SERVICE_NAME} stopped.{RESET}");
    } else {
        eprintln!("{RED}✗ Failed to stop {SERVICE_NAME}.{RESET}");
    }
    Ok(())
}

pub fn restart_service() -> anyhow::Result<()> {
    if !is_service_installed() {
        deploy_to_opt()?;
    }

    println!("==> Restarting {SERVICE_NAME} via systemd...");
    let _ = run_privileged_cmd("systemctl", &["daemon-reload"]);
    let _ = run_privileged_cmd("systemctl", &["reset-failed", SERVICE_NAME]);
    let _ = run_privileged_cmd("systemctl", &["restart", SERVICE_NAME]);

    sleep(Duration::from_millis(600));

    if is_service_active() {
        println!("{GREEN}✓ {SERVICE_NAME} restarted & active on http://127.0.0.1:3100{RESET}");
    } else {
        eprintln!("{RED}✗ {SERVICE_NAME} restart failed. Recent journal logs:{RESET}\n");
        let _ = show_logs(15);
    }
    Ok(())
}

pub fn show_status() -> anyhow::Result<()> {
    println!("==> Inspecting {SERVICE_NAME} status...\n");
    let _ = Command::new("systemctl")
        .args(["status", SERVICE_NAME, "--no-pager"])
        .status();
    Ok(())
}

pub fn show_logs(lines: usize) -> anyhow::Result<()> {
    let lines_str = lines.to_string();
    println!("==> Fetching last {lines} lines of systemd journal logs...\n");
    if is_root() {
        let _ = Command::new("journalctl")
            .args(["-u", SERVICE_NAME, "-n", &lines_str, "--no-pager"])
            .status();
    } else {
        let _ = Command::new("sudo")
            .args([
                "journalctl",
                "-u",
                SERVICE_NAME,
                "-n",
                &lines_str,
                "--no-pager",
            ])
            .status();
    }
    Ok(())
}

pub fn install_and_enable() -> anyhow::Result<()> {
    deploy_to_opt()?;

    println!("==> Enabling {SERVICE_NAME} on system boot...");
    let _ = run_privileged_cmd("systemctl", &["daemon-reload"]);
    let _ = run_privileged_cmd("systemctl", &["enable", "--now", SERVICE_NAME]);

    sleep(Duration::from_millis(600));

    if is_service_active() {
        println!(
            "{GREEN}✓ Service installed, enabled on boot, and active on http://127.0.0.1:3100.{RESET}"
        );
    } else {
        println!("{YELLOW}! Service enabled, but process is not active yet. Recent logs:{RESET}\n");
        let _ = show_logs(15);
    }
    Ok(())
}

pub fn disable_service() -> anyhow::Result<()> {
    println!("==> Disabling {SERVICE_NAME} from auto-start on boot...");
    if run_privileged_cmd("systemctl", &["disable", "--now", SERVICE_NAME])? {
        println!("{YELLOW}✓ {SERVICE_NAME} disabled and stopped.{RESET}");
    } else {
        eprintln!("{RED}✗ Failed to disable {SERVICE_NAME}.{RESET}");
    }
    Ok(())
}

pub fn run_service_cli(args: &[String]) -> anyhow::Result<()> {
    if !is_systemd_available() {
        eprintln!("{RED}Error:{RESET} systemd (systemctl) is not available on this host.");
        return Ok(());
    }

    match args.first().map(|s| s.as_str()) {
        Some("start") => start_service()?,
        Some("stop") => stop_service()?,
        Some("restart") => restart_service()?,
        Some("status") => show_status()?,
        Some("logs") | Some("log") | Some("journal") => show_logs(50)?,
        Some("enable") | Some("install") => install_and_enable()?,
        Some("disable") => disable_service()?,
        _ => {
            println!("\n{BOLD}Cratera Systemd Service Manager{RESET}");
            println!("Manage the background systemd service.\n");
            println!("{BOLD}{CYAN}USAGE:{RESET}");
            println!("  cratera service <SUBCOMMAND>\n");
            println!("{BOLD}{CYAN}SUBCOMMANDS:{RESET}");
            println!(
                "  {BOLD}{:<14}{RESET} Start the background systemd service",
                "start"
            );
            println!(
                "  {BOLD}{:<14}{RESET} Stop the background systemd service",
                "stop"
            );
            println!(
                "  {BOLD}{:<14}{RESET} Restart the background systemd service",
                "restart"
            );
            println!(
                "  {BOLD}{:<14}{RESET} View live systemd status and PID",
                "status"
            );
            println!(
                "  {BOLD}{:<14}{RESET} View live journald telemetry logs",
                "logs"
            );
            println!(
                "  {BOLD}{:<14}{RESET} Install binaries to /opt/cratera & enable on boot",
                "enable"
            );
            println!(
                "  {BOLD}{:<14}{RESET} Disable service from auto-starting on boot",
                "disable"
            );
            println!();
        }
    }

    Ok(())
}
