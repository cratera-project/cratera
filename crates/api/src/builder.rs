use std::process::Command;

const GREEN: &str = "\x1b[32m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub fn run_build() -> anyhow::Result<()> {
    println!("\n{BOLD}═══════════════════════════════════════════════════════════════{RESET}");
    println!(" {BOLD}Cratera MicroVM Rootfs Builder{RESET}");
    println!("{BOLD}═══════════════════════════════════════════════════════════════{RESET}\n");

    let script = "./scripts/build-rootfs.sh";
    println!("{DIM}Executing {script}...{RESET}\n");

    let mut child = Command::new("bash")
        .arg(script)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch {script}: {e}"))?;

    let status = child.wait()?;
    if status.success() {
        println!("\n{GREEN}{BOLD}✓ Rootfs build completed successfully!{RESET}\n");
    } else {
        println!("\n\x1b[31m{BOLD}✗ Rootfs build failed with status: {status}{RESET}\n");
    }

    Ok(())
}
