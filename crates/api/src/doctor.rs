use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub fn run_doctor() -> anyhow::Result<()> {
    println!("\n{BOLD}═══════════════════════════════════════════════════════════════{RESET}");
    println!(" {BOLD}Cratera System Doctor & Sandbox Diagnostic Suite{RESET}");
    println!("{BOLD}═══════════════════════════════════════════════════════════════{RESET}\n");

    let mut all_ok = true;

    // 1. Hardware Virtualization
    println!("{BOLD}[1/5] Hardware Virtualization (KVM):{RESET}");
    let kvm_path = Path::new("/dev/kvm");
    if kvm_path.exists() {
        let r_ok = fs::File::open(kvm_path).is_ok();
        let w_ok = fs::OpenOptions::new().write(true).open(kvm_path).is_ok();
        if r_ok && w_ok {
            println!("  {GREEN}✓{RESET} /dev/kvm is accessible (read-write ok)");
        } else {
            all_ok = false;
            println!("  {RED}✗{RESET} /dev/kvm permissions restricted (read={r_ok}, write={w_ok})");
            println!("    {DIM}Fix: sudo usermod -aG kvm $USER && newgrp kvm{RESET}");
        }
    } else {
        all_ok = false;
        println!(
            "  {RED}✗{RESET} /dev/kvm not found. Host does not have hardware virtualization enabled."
        );
    }

    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        if cpuinfo.contains("vmx") {
            println!("  {GREEN}✓{RESET} CPU Virtualization: Intel VT-x detected");
        } else if cpuinfo.contains("svm") {
            println!("  {GREEN}✓{RESET} CPU Virtualization: AMD-V detected");
        } else {
            println!("  {YELLOW}!{RESET} CPU Virtualization: No vmx/svm flags in /proc/cpuinfo");
        }
    }

    if let Ok(smt) = fs::read_to_string("/sys/devices/system/cpu/smt/control") {
        let smt_trimmed = smt.trim();
        println!("  {DIM}• SMT / Hyperthreading status:{RESET} {smt_trimmed}");
    }
    println!();

    // 2. Host Isolation & Jailer
    println!("{BOLD}[2/5] Host Sandbox & Security Boundaries:{RESET}");
    let uid_check = Command::new("id").args(["-u", "20001"]).output();
    match uid_check {
        Ok(out) if out.status.success() => {
            println!("  {GREEN}✓{RESET} Jailer unprivileged user (UID 20001) exists on host");
        }
        _ => {
            println!("  {YELLOW}!{RESET} Jailer user UID 20001 not configured on host");
            println!(
                "    {DIM}Tip: Run ./scripts/host-setup.sh to provision UID/GID 20001 for Jailer{RESET}"
            );
        }
    }

    let cgroups_v2 = Path::new("/sys/fs/cgroup/cgroup.controllers");
    if cgroups_v2.exists() {
        println!("  {GREEN}✓{RESET} Linux cgroups v2 active (/sys/fs/cgroup)");
    } else {
        println!("  {YELLOW}!{RESET} Unified cgroups v2 hierarchy not detected");
    }

    let work_dir = std::env::var("CRATERA_WORK_DIR").unwrap_or_else(|_| "/var/tmp/cratera".into());
    let work_path = Path::new(&work_dir);
    if let Err(e) = fs::create_dir_all(work_path) {
        println!("  {RED}✗{RESET} Work directory '{work_dir}' creation failed: {e}");
        all_ok = false;
    } else {
        println!("  {GREEN}✓{RESET} Ephemeral VM work directory: {work_dir}");
    }
    println!();

    // 3. MicroVM Runtime Binaries & Kernel
    println!("{BOLD}[3/5] MicroVM Runtime Assets:{RESET}");
    let fc_path = resolve_asset_path("CRATERA_FIRECRACKER", "images/firecracker");
    if fc_path.exists() {
        let is_exec = fs::metadata(&fc_path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
        if is_exec {
            let ver = Command::new(&fc_path)
                .arg("--version")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_else(|| "unknown".into());
            let first_line = ver.lines().next().unwrap_or("Firecracker");
            println!(
                "  {GREEN}✓{RESET} Firecracker binary: {} ({})",
                fc_path.display(),
                first_line.trim()
            );
        } else {
            all_ok = false;
            println!(
                "  {RED}✗{RESET} Firecracker binary is not executable: {}",
                fc_path.display()
            );
        }
    } else {
        all_ok = false;
        println!(
            "  {RED}✗{RESET} Firecracker binary missing at: {}",
            fc_path.display()
        );
        println!("    {DIM}Fix: Run ./scripts/fetch-runtime.sh{RESET}");
    }

    let kernel_path = resolve_asset_path("CRATERA_KERNEL", "images/vmlinux.bin");
    if kernel_path.exists() {
        let size_mb = fs::metadata(&kernel_path)
            .map(|m| m.len() as f64 / (1024.0 * 1024.0))
            .unwrap_or(0.0);
        println!(
            "  {GREEN}✓{RESET} Guest Linux Kernel: {} ({:.1} MB)",
            kernel_path.display(),
            size_mb
        );
    } else {
        all_ok = false;
        println!(
            "  {RED}✗{RESET} Linux guest kernel missing at: {}",
            kernel_path.display()
        );
        println!("    {DIM}Fix: Run ./scripts/fetch-runtime.sh{RESET}");
    }

    // 4. Guest Rootfs Format & Size
    println!("\n{BOLD}[4/5] Root Filesystem (Rootfs):{RESET}");
    let (rootfs_path, fs_type) = detect_rootfs();
    if let Some(path) = rootfs_path {
        let size_mb = fs::metadata(&path)
            .map(|m| m.len() as f64 / (1024.0 * 1024.0))
            .unwrap_or(0.0);
        println!(
            "  {GREEN}✓{RESET} Rootfs image: {} ({:.1} MB, format: {GREEN}{}{RESET})",
            path.display(),
            size_mb,
            fs_type
        );
    } else {
        all_ok = false;
        println!("  {RED}✗{RESET} Rootfs image missing (searched rootfs.squashfs, rootfs.ext4)");
        println!("    {DIM}Fix: Run ./scripts/build-rootfs.sh or cratera build{RESET}");
    }

    // 5. Multi-Language Manifest
    println!("\n{BOLD}[5/5] Multi-Language Manifest (languages.toml):{RESET}");
    let manifest_path =
        std::env::var("CRATERA_LANGUAGES_FILE").unwrap_or_else(|_| "languages.toml".into());
    if let Ok(content) = fs::read_to_string(&manifest_path) {
        if let Ok(table) = toml::from_str::<toml::Table>(&content) {
            let languages = table
                .get("languages")
                .and_then(|l| l.as_table())
                .cloned()
                .unwrap_or(table);
            let total = languages.len();
            let enabled_count = languages
                .values()
                .filter(|v| v.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true))
                .count();
            println!(
                "  {GREEN}✓{RESET} Manifest parsed: {manifest_path} ({enabled_count}/{total} languages active)"
            );
        } else {
            all_ok = false;
            println!("  {RED}✗{RESET} Failed to parse TOML in: {manifest_path}");
        }
    } else {
        println!(
            "  {YELLOW}!{RESET} Manifest file '{manifest_path}' not found on disk; using built-in defaults"
        );
    }

    println!("\n{BOLD}───────────────────────────────────────────────────────────────{RESET}");
    if all_ok {
        println!(" {GREEN}{BOLD}✓ Diagnostic Complete: System is fully operational.{RESET}");
    } else {
        println!(" {YELLOW}{BOLD}! Diagnostic Notice: Some components require attention.{RESET}");
    }
    println!("{BOLD}───────────────────────────────────────────────────────────────{RESET}\n");

    Ok(())
}

fn resolve_asset_path(env_var: &str, default_rel: &str) -> PathBuf {
    if let Ok(val) = std::env::var(env_var) {
        return PathBuf::from(val);
    }
    let p = PathBuf::from(default_rel);
    if p.exists() {
        return p;
    }
    if let Ok(root) = std::env::var("CARGO_MANIFEST_DIR") {
        let parent = Path::new(&root)
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(Path::new("."));
        let cand = parent.join(default_rel);
        if cand.exists() {
            return cand;
        }
    }
    p
}

fn detect_rootfs() -> (Option<PathBuf>, &'static str) {
    if let Ok(env_val) = std::env::var("CRATERA_ROOTFS") {
        let p = PathBuf::from(env_val);
        if p.exists() {
            let fmt = inspect_magic(&p);
            return (Some(p), fmt);
        }
    }
    let candidates = [
        ("images/rootfs.squashfs", "SquashFS (Zstandard compressed)"),
        ("images/rootfs.ext4", "ext4 Virtual Disk"),
        ("./rootfs.squashfs", "SquashFS"),
        ("./rootfs.ext4", "ext4"),
    ];
    for (rel, fmt) in candidates {
        let p = PathBuf::from(rel);
        if p.exists() {
            return (Some(p), fmt);
        }
    }
    (None, "Unknown")
}

fn inspect_magic(path: &Path) -> &'static str {
    if let Ok(mut file) = fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 4];
        if file.read_exact(&mut buf).is_ok() && (&buf == b"hsqs" || &buf == b"sqsh") {
            return "SquashFS (Zstandard compressed)";
        }
    }
    "ext4 Virtual Disk"
}
