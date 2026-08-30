mod builder;
mod doctor;
mod interactive;
mod lang_ctl;
mod server;
mod service;
mod settings;
mod tester;

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().skip(1).collect();

    // If no arguments provided, launch the Interactive Command Center
    if args.is_empty() {
        interactive::run_interactive().await?;
        return Ok(());
    }

    let command = args.first().map(|s| s.as_str()).unwrap_or("ctl");

    match command {
        "ctl" | "console" | "dashboard" | "menu" | "ui" => {
            interactive::run_interactive().await?;
        }
        "doctor" => {
            doctor::run_doctor()?;
        }
        "lang" | "languages" => {
            lang_ctl::run_lang(&args[1..])?;
        }
        "settings" | "config" | "budget" | "budgets" => {
            settings::run_settings(&args[1..])?;
        }
        "service" | "daemon" | "systemd" => {
            service::run_service_cli(&args[1..])?;
        }
        "test" | "smoke" => {
            tester::run_test(&args[1..]).await?;
        }
        "build" | "build-rootfs" => {
            builder::run_build()?;
        }
        "help" | "--help" | "-h" => {
            print_help();
        }
        "serve" | "server" | "start" => {
            init_tracing();
            server::start_server().await?;
        }
        unknown => {
            eprintln!("\x1b[31mUnknown command:\x1b[0m {unknown}\n");
            print_help();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("cratera=info,cratera_api=info,cratera_executor=info,tower_http=info")
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();
}

fn print_help() {
    println!("\n{BOLD}Cratera Interactive Command Center & MicroVM Engine{RESET}");
    println!("{DIM}Hardware-isolated code judge powered by Firecracker microVMs.{RESET}\n");

    println!("{BOLD}{CYAN}USAGE:{RESET}");
    println!("  cratera                     {DIM}# Launch Interactive Command Center menu{RESET}");
    println!("  cratera <COMMAND> [OPTIONS] {DIM}# Run direct command line task{RESET}\n");

    println!("{BOLD}{CYAN}COMMANDS:{RESET}");
    println!(
        "  {BOLD}{:<22}{RESET} Launch the interactive Terminal Control Center UI",
        "ctl, console, menu"
    );
    println!(
        "  {BOLD}{:<22}{RESET} Start the Axum HTTP Coordinator API server",
        "serve, server"
    );
    println!(
        "  {BOLD}{:<22}{RESET} Run system diagnostics (/dev/kvm, Jailer, storage, kernel)",
        "doctor"
    );
    println!(
        "  {BOLD}{:<22}{RESET} Manage multi-language toolchains (list, enable, disable, preset)",
        "lang"
    );
    println!(
        "  {BOLD}{:<22}{RESET} Manage timeouts, resource budgets per VM, and .env settings",
        "settings, config"
    );
    println!(
        "  {BOLD}{:<22}{RESET} Manage background systemd service (start, stop, restart, status)",
        "service, daemon"
    );
    println!(
        "  {BOLD}{:<22}{RESET} Execute in-guest hardware microVM test with microsecond timings",
        "test, smoke"
    );
    println!(
        "  {BOLD}{:<22}{RESET} Rebuild the guest root filesystem (SquashFS / ext4)",
        "build"
    );
    println!(
        "  {BOLD}{:<22}{RESET} Display this help message",
        "help, --help"
    );

    println!("\n{BOLD}{CYAN}EXAMPLES:{RESET}");
    println!(
        "  cratera                         {DIM}# Interactive menu (navigation & toggles){RESET}"
    );
    println!("  cratera doctor                  {DIM}# Verify KVM and host readiness{RESET}");
    println!(
        "  cratera lang list               {DIM}# Table of 30 languages and active status{RESET}"
    );
    println!(
        "  cratera lang enable go zig      {DIM}# Enable Go and Zig compilers in manifest{RESET}"
    );
    println!(
        "  cratera lang preset systems     {DIM}# Apply systems programming language preset{RESET}"
    );
    println!("  cratera service status          {DIM}# View systemd service status{RESET}");
    println!("  cratera service start           {DIM}# Start systemd service{RESET}");
    println!("  cratera service stop            {DIM}# Stop systemd service{RESET}");
    println!(
        "  cratera settings list           {DIM}# View execution time limits and VM budgets{RESET}"
    );
    println!(
        "  cratera settings set CRATERA_RUN_MS 3000  {DIM}# Update run timeout in .env{RESET}"
    );
    println!(
        "  cratera test python             {DIM}# Test Python execution in real microVM{RESET}"
    );
    println!(
        "  cratera serve                   {DIM}# Launch HTTP coordinator on 127.0.0.1:3100{RESET}\n"
    );
}
