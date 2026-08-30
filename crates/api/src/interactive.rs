use crate::{builder, doctor, lang_ctl, server, service, settings, tester};
use std::io::{self, Write};

const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const MAGENTA: &str = "\x1b[35m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub async fn run_interactive() -> anyhow::Result<()> {
    loop {
        let server_active = server::is_server_running().await;
        let server_addr = server::get_server_addr().await;
        let svc_active = service::is_service_active();
        let svc_installed = service::is_service_installed();

        print_banner(server_active, &server_addr, svc_active);
        println!(
            "  {BOLD}{GREEN}[1]{RESET} {BOLD}System Diagnostics & Health Check{RESET} {DIM}(/dev/kvm, Jailer, storage, kernel){RESET}"
        );
        println!(
            "  {BOLD}{CYAN}[2]{RESET} {BOLD}Multi-Language Toolchains Manager{RESET} {DIM}(Toggle 30 languages, apply presets){RESET}"
        );
        println!(
            "  {BOLD}{YELLOW}[3]{RESET} {BOLD}Resource Budgets & Limits Editor{RESET} {DIM}(vCPU, RAM, cgroups, timeouts){RESET}"
        );
        println!(
            "  {BOLD}{MAGENTA}[4]{RESET} {BOLD}In-Guest MicroVM Smoke Tester{RESET} {DIM}(Measure microsecond execution){RESET}"
        );
        println!(
            "  {BOLD}{BOLD}[5]{RESET} {BOLD}Build / Rebuild Guest Rootfs Image{RESET} {DIM}(SquashFS / ext4){RESET}"
        );

        if server_active {
            println!(
                "  {BOLD}{RED}[6]{RESET} {BOLD}Stop Interactive Local Server{RESET} {GREEN}[Active on {}]{RESET}",
                server_addr
            );
        } else {
            println!(
                "  {BOLD}{GREEN}[6]{RESET} {BOLD}Start Interactive Local Server in Background{RESET} {DIM}[Stopped]{RESET}"
            );
        }

        if svc_active {
            println!(
                "  {BOLD}{CYAN}[7]{RESET} {BOLD}Systemd Service Manager{RESET} {GREEN}[Active & Running]{RESET}"
            );
        } else if svc_installed {
            println!(
                "  {BOLD}{CYAN}[7]{RESET} {BOLD}Systemd Service Manager{RESET} {YELLOW}[Stopped]{RESET}"
            );
        } else {
            println!(
                "  {BOLD}{CYAN}[7]{RESET} {BOLD}Systemd Service Manager{RESET} {DIM}[Not Installed]{RESET}"
            );
        }

        println!("  {BOLD}{RED}[0]{RESET} {BOLD}Exit Command Center{RESET}\n");

        let choice = prompt_input("Select an option [0-7] > ");
        match choice.trim() {
            "1" => {
                let _ = doctor::run_doctor();
                pause();
            }
            "2" => {
                handle_languages_menu()?;
            }
            "3" => {
                handle_settings_menu()?;
            }
            "4" => {
                handle_tester_menu().await?;
            }
            "5" => {
                let _ = builder::run_build();
                pause();
            }
            "6" => {
                if server_active {
                    let stopped = server::stop_server().await;
                    if stopped {
                        println!("\n{BOLD}{YELLOW}✓ Stopped HTTP Coordinator Server.{RESET}");
                    } else {
                        println!("\n{DIM}Server was already stopped.{RESET}");
                    }
                } else {
                    match server::start_server_background().await {
                        Ok(addr) => {
                            println!(
                                "\n{BOLD}{GREEN}✓ Started HTTP Coordinator Server in background on http://{addr}{RESET}"
                            );
                            println!(
                                "  {DIM}The server is running asynchronously. You can continue using all other menus.{RESET}"
                            );
                            println!(
                                "  {DIM}Select [6] again at any time to stop the server.{RESET}"
                            );
                        }
                        Err(e) => {
                            println!("\n{RED}Failed to start server:{RESET} {e}");
                        }
                    }
                }
                pause();
            }
            "7" => {
                handle_service_menu()?;
            }
            "0" | "q" | "quit" | "exit" => {
                if server_active {
                    println!(
                        "\n{DIM}Exiting Command Center. Background server remains active on http://{server_addr}.{RESET}\n"
                    );
                } else {
                    println!("\n{DIM}Exiting Cratera Command Center. Goodbye!{RESET}\n");
                }
                break;
            }
            _ => {
                println!("{YELLOW}! Please enter a valid number (0-7){RESET}");
                pause();
            }
        }
    }
    Ok(())
}

fn handle_languages_menu() -> anyhow::Result<()> {
    loop {
        println!(
            "\n{BOLD}{CYAN}╭─────────────────────────────────────────────────────────────╮{RESET}"
        );
        println!(
            "{BOLD}{CYAN}│             Multi-Language Toolchains Manager               │{RESET}"
        );
        println!(
            "{BOLD}{CYAN}╰─────────────────────────────────────────────────────────────╯{RESET}\n"
        );

        println!(
            "  {BOLD}[1]{RESET} Interactive Checklist (Use ↑/↓ to move, Enter/Space to toggle)"
        );
        println!("  {BOLD}[2]{RESET} View Full Language Manifest Table");
        println!("  {BOLD}[3]{RESET} Toggle by Keys or Numbers (e.g. '27 28' or 'go zig')");
        println!(
            "  {BOLD}[4]{RESET} Apply Curated Preset (top10, systems, web, functional, sci, minimal)"
        );
        println!("  {BOLD}[5]{RESET} Enable All 30 Languages");
        println!("  {BOLD}[6]{RESET} Minimal Preset (Rust Only)");
        println!("  {BOLD}[0]{RESET} Back to Main Menu\n");

        let choice = prompt_input("Select an option [0-6] > ");
        match choice.trim() {
            "1" | "" => {
                lang_ctl::interactive_language_picker()?;
            }
            "2" => {
                let _ = lang_ctl::run_lang(&["list".into()]);
                pause();
            }
            "3" => {
                let _ = lang_ctl::run_lang(&["list".into()]);
                let target = prompt_input(
                    "Enter language numbers or keys to toggle (e.g. '27 28' or 'd fortran') > ",
                );
                let parts: Vec<String> = target.split_whitespace().map(|s| s.to_string()).collect();
                if !parts.is_empty() {
                    let enable_choice = prompt_input("Action: [1] Enable, [2] Disable > ");
                    if enable_choice.trim() == "1" {
                        let mut args = vec!["enable".to_string()];
                        args.extend(parts);
                        let _ = lang_ctl::run_lang(&args);
                    } else if enable_choice.trim() == "2" {
                        let mut args = vec!["disable".to_string()];
                        args.extend(parts);
                        let _ = lang_ctl::run_lang(&args);
                    }
                }
                pause();
            }
            "4" => {
                println!("\nAvailable presets:");
                println!("  • {BOLD}all{RESET}        - All 30 language runtimes");
                println!(
                    "  • {BOLD}top10{RESET}      - Python, Node, Rust, C++, C, Go, Java, C#, TypeScript, Ruby"
                );
                println!(
                    "  • {BOLD}systems{RESET}    - Rust, C++, C, Go, Zig, Nim, D, Fortran, Ada, Pascal"
                );
                println!(
                    "  • {BOLD}web{RESET}        - Node, TypeScript, Python, Ruby, PHP, Elixir, Go, Java, C#"
                );
                println!(
                    "  • {BOLD}functional{RESET} - Haskell, OCaml, Clojure, Scala, Erlang, Elixir, F#"
                );
                println!("  • {BOLD}scientific{RESET} - Python, Julia, R, Fortran, C++, C, Rust");
                println!("  • {BOLD}minimal{RESET}    - Rust only\n");
                let preset = prompt_input("Enter preset name > ");
                if !preset.trim().is_empty() {
                    let _ = lang_ctl::run_lang(&["preset".into(), preset.trim().to_string()]);
                }
                pause();
            }
            "5" => {
                let _ = lang_ctl::run_lang(&["preset".into(), "all".into()]);
                pause();
            }
            "6" => {
                let _ = lang_ctl::run_lang(&["preset".into(), "minimal".into()]);
                pause();
            }
            "0" | "b" | "back" => break,
            _ => {
                println!("{YELLOW}! Please enter a valid number (0-6){RESET}");
            }
        }
    }
    Ok(())
}

fn handle_settings_menu() -> anyhow::Result<()> {
    loop {
        println!(
            "\n{BOLD}{YELLOW}╭─────────────────────────────────────────────────────────────╮{RESET}"
        );
        println!(
            "{BOLD}{YELLOW}│          Resource Budgets & Runtime Settings Editor         │{RESET}"
        );
        println!(
            "{BOLD}{YELLOW}╰─────────────────────────────────────────────────────────────╯{RESET}\n"
        );

        println!("  {BOLD}[1]{RESET} View Current Resource Budgets & .env Values");
        println!("  {BOLD}[2]{RESET} Edit Run Timeout (CRATERA_RUN_MS)");
        println!("  {BOLD}[3]{RESET} Edit Submit Timeout (CRATERA_SUBMIT_MS)");
        println!("  {BOLD}[4]{RESET} Edit MicroVM vCPU Cores (CRATERA_VCPU)");
        println!("  {BOLD}[5]{RESET} Edit MicroVM RAM Memory (CRATERA_MEM_MIB)");
        println!("  {BOLD}[6]{RESET} Toggle Firecracker Jailer Sandbox (CRATERA_USE_JAILER)");
        println!("  {BOLD}[7]{RESET} Toggle MicroVM Snapshot Restore (CRATERA_USE_SNAPSHOT)");
        println!("  {BOLD}[8]{RESET} Edit HTTP Bind Address (CRATERA_BIND)");
        println!("  {BOLD}[9]{RESET} Reset All Settings to Default (.env reset)");
        println!("  {BOLD}[0]{RESET} Back to Main Menu\n");

        let choice = prompt_input("Select an option [0-9] > ");
        match choice.trim() {
            "1" => {
                let _ = settings::run_settings(&["list".into()]);
                pause();
            }
            "2" => {
                prompt_and_set(
                    "CRATERA_RUN_MS",
                    "Enter new Run Mode timeout in ms (default: 2000) > ",
                );
            }
            "3" => {
                prompt_and_set(
                    "CRATERA_SUBMIT_MS",
                    "Enter new Submit Mode timeout in ms (default: 5000) > ",
                );
            }
            "4" => {
                prompt_and_set(
                    "CRATERA_VCPU",
                    "Enter virtual CPU cores per VM (default: 2) > ",
                );
            }
            "5" => {
                prompt_and_set(
                    "CRATERA_MEM_MIB",
                    "Enter guest RAM memory per VM in MiB (default: 2048) > ",
                );
            }
            "6" => {
                let curr = std::env::var("CRATERA_USE_JAILER").unwrap_or_else(|_| "0".into());
                let next = if curr == "1" { "0" } else { "1" };
                let _ = settings::run_settings(&[
                    "set".into(),
                    "CRATERA_USE_JAILER".into(),
                    next.into(),
                ]);
                println!("{GREEN}✓ Toggled CRATERA_USE_JAILER to {next}{RESET}");
                pause();
            }
            "7" => {
                let curr = std::env::var("CRATERA_USE_SNAPSHOT").unwrap_or_else(|_| "0".into());
                let next = if curr == "1" { "0" } else { "1" };
                let _ = settings::run_settings(&[
                    "set".into(),
                    "CRATERA_USE_SNAPSHOT".into(),
                    next.into(),
                ]);
                println!("{GREEN}✓ Toggled CRATERA_USE_SNAPSHOT to {next}{RESET}");
                pause();
            }
            "8" => {
                prompt_and_set(
                    "CRATERA_BIND",
                    "Enter HTTP Bind address (default: 127.0.0.1:3100) > ",
                );
            }
            "9" => {
                let confirm = prompt_input(
                    "Are you sure you want to reset all settings to defaults? [y/N] > ",
                );
                if confirm.trim().eq_ignore_ascii_case("y") {
                    let _ = settings::run_settings(&["reset".into()]);
                }
                pause();
            }
            "0" | "b" | "back" => break,
            _ => {
                println!("{YELLOW}! Please enter a valid number (0-9){RESET}");
            }
        }
    }
    Ok(())
}

async fn handle_tester_menu() -> anyhow::Result<()> {
    loop {
        println!(
            "\n{BOLD}{MAGENTA}╭─────────────────────────────────────────────────────────────╮{RESET}"
        );
        println!(
            "{BOLD}{MAGENTA}│          MicroVM In-Guest Hardware Smoke Tester             │{RESET}"
        );
        println!(
            "{BOLD}{MAGENTA}╰─────────────────────────────────────────────────────────────╯{RESET}\n"
        );

        println!("  {BOLD}[1]{RESET} Run Full Test Across All Core Languages");
        println!("  {BOLD}[2]{RESET} Test Specific Language (e.g. rust, python, node, cpp, go)");
        println!("  {BOLD}[0]{RESET} Back to Main Menu\n");

        let choice = prompt_input("Select an option [0-2] > ");
        match choice.trim() {
            "1" => {
                let _ = tester::run_test(&["all".into()]).await;
                pause();
            }
            "2" => {
                let lang =
                    prompt_input("Enter language to test (e.g. rust, python, node, cpp, go) > ");
                if !lang.trim().is_empty() {
                    let _ = tester::run_test(&[lang.trim().to_string()]).await;
                }
                pause();
            }
            "0" | "b" | "back" => break,
            _ => {
                println!("{YELLOW}! Please enter a valid number (0-2){RESET}");
            }
        }
    }
    Ok(())
}

fn handle_service_menu() -> anyhow::Result<()> {
    loop {
        let is_active = service::is_service_active();
        let is_enabled = service::is_service_enabled();
        let is_installed = service::is_service_installed();

        println!(
            "\n{BOLD}{CYAN}╭─────────────────────────────────────────────────────────────╮{RESET}"
        );
        println!(
            "{BOLD}{CYAN}│                   Systemd Service Manager                   │{RESET}"
        );
        println!(
            "{BOLD}{CYAN}╰─────────────────────────────────────────────────────────────╯{RESET}\n"
        );

        println!(
            "  Status:    {}",
            if is_active {
                format!("{GREEN}● Active (Running){RESET}")
            } else if is_installed {
                format!("{YELLOW}○ Inactive (Stopped){RESET}")
            } else {
                format!("{DIM}○ Unit not installed in /etc/systemd/system/{RESET}")
            }
        );
        println!(
            "  Auto-Boot: {}\n",
            if is_enabled {
                format!("{GREEN}Enabled (Starts automatically on system boot){RESET}")
            } else {
                format!("{DIM}Disabled{RESET}")
            }
        );

        if is_active {
            println!("  {BOLD}[1]{RESET} {YELLOW}Stop Systemd Service{RESET}");
            println!("  {BOLD}[2]{RESET} Restart Systemd Service");
        } else {
            println!("  {BOLD}[1]{RESET} {GREEN}Start Systemd Service{RESET}");
            println!("  {BOLD}[2]{RESET} Restart Systemd Service");
        }
        println!("  {BOLD}[3]{RESET} View Live Systemd Status (`systemctl status cratera`)");
        println!("  {BOLD}[4]{RESET} Stream Systemd Journal Logs (`journalctl -u cratera`)");
        if is_enabled {
            println!("  {BOLD}[5]{RESET} Disable Auto-Start on System Boot");
        } else {
            println!("  {BOLD}[5]{RESET} Install Unit File & Enable Auto-Start on System Boot");
        }
        println!("  {BOLD}[0]{RESET} Back to Main Menu\n");

        let choice = prompt_input("Select an option [0-5] > ");
        match choice.trim() {
            "1" => {
                if is_active {
                    let _ = service::stop_service();
                } else {
                    let _ = service::start_service();
                }
                pause();
            }
            "2" => {
                let _ = service::restart_service();
                pause();
            }
            "3" => {
                let _ = service::show_status();
                pause();
            }
            "4" => {
                let _ = service::show_logs(50);
                pause();
            }
            "5" => {
                if is_enabled {
                    let _ = service::disable_service();
                } else {
                    let _ = service::install_and_enable();
                }
                pause();
            }
            "0" | "b" | "back" => break,
            _ => {
                println!("{YELLOW}! Please enter a valid number (0-5){RESET}");
            }
        }
    }
    Ok(())
}

fn prompt_and_set(key: &str, prompt: &str) {
    let val = prompt_input(prompt);
    let val_trimmed = val.trim();
    if !val_trimmed.is_empty() {
        let _ = settings::run_settings(&["set".into(), key.into(), val_trimmed.into()]);
    }
    pause();
}

fn prompt_input(prompt: &str) -> String {
    print!("{BOLD}{prompt}{RESET}");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    buf
}

fn pause() {
    print!("\n{DIM}Press Enter to continue...{RESET}");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}

const BANNER_INNER_WIDTH: usize = 61;

fn format_box_line(visible_len: usize, inner_formatted: &str) -> String {
    let pad = BANNER_INNER_WIDTH.saturating_sub(visible_len);
    let left_pad = pad / 2;
    let right_pad = pad - left_pad;
    format!(
        "{BOLD}{CYAN}│{}{}{BOLD}{CYAN}{}│{RESET}",
        " ".repeat(left_pad),
        inner_formatted,
        " ".repeat(right_pad)
    )
}

fn print_banner(server_active: bool, server_addr: &str, svc_active: bool) {
    println!(
        "\n{BOLD}{CYAN}╭─────────────────────────────────────────────────────────────╮{RESET}"
    );
    println!(
        "{}",
        format_box_line(
            "CRATERA INTERACTIVE COMMAND CENTER".chars().count(),
            "CRATERA INTERACTIVE COMMAND CENTER"
        )
    );
    println!(
        "{}",
        format_box_line(
            "Hardware MicroVM Isolation & Multi-Language Sandbox"
                .chars()
                .count(),
            "Hardware MicroVM Isolation & Multi-Language Sandbox"
        )
    );

    if svc_active {
        let visible = "Systemd: ● Service Active & Supervised";
        let formatted = format!("Systemd: {GREEN}● Service Active & Supervised{RESET}");
        println!("{}", format_box_line(visible.chars().count(), &formatted));
    } else if server_active {
        let visible = format!("Local Dev: ● Active on http://{server_addr}");
        let formatted = format!("Local Dev: {GREEN}● Active on http://{server_addr}{RESET}");
        println!("{}", format_box_line(visible.chars().count(), &formatted));
    } else {
        let visible = "Status: ○ HTTP Coordinator Inactive [Select 6 or 7]";
        let formatted = format!("Status: {DIM}○ HTTP Coordinator Inactive [Select 6 or 7]{RESET}");
        println!("{}", format_box_line(visible.chars().count(), &formatted));
    }
    println!(
        "{BOLD}{CYAN}╰─────────────────────────────────────────────────────────────╯{RESET}\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_esc = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_esc = true;
            } else if in_esc {
                if c == 'm' {
                    in_esc = false;
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn test_all_banner_states_width() {
        let cases = [
            (
                "CRATERA INTERACTIVE COMMAND CENTER",
                "CRATERA INTERACTIVE COMMAND CENTER",
            ),
            (
                "Hardware MicroVM Isolation & Multi-Language Sandbox",
                "Hardware MicroVM Isolation & Multi-Language Sandbox",
            ),
            (
                "Systemd: ● Service Active & Supervised",
                "Systemd: \x1b[32m● Service Active & Supervised\x1b[0m",
            ),
            (
                "Local Dev: ● Active on http://127.0.0.1:3100",
                "Local Dev: \x1b[32m● Active on http://127.0.0.1:3100\x1b[0m",
            ),
            (
                "Status: ○ HTTP Coordinator Inactive [Select 6 or 7]",
                "Status: \x1b[2m○ HTTP Coordinator Inactive [Select 6 or 7]\x1b[0m",
            ),
        ];

        for (visible, formatted) in cases {
            let line = format_box_line(visible.chars().count(), formatted);
            let stripped = strip_ansi(&line);
            assert_eq!(
                stripped.chars().count(),
                BANNER_INNER_WIDTH + 2, // inner + 2 borders
                "Line '{visible}' has width {}, expected {}",
                stripped.chars().count(),
                BANNER_INNER_WIDTH + 2
            );
            assert!(stripped.starts_with('│'));
            assert!(stripped.ends_with('│'));
        }
    }
}
