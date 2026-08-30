use std::collections::HashMap;
use std::fs;
use std::path::Path;

const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug)]
pub struct SettingDef {
    pub key: &'static str,
    pub description: &'static str,
    pub default_val: &'static str,
    pub category: &'static str,
}

pub const ALL_SETTINGS: &[SettingDef] = &[
    // 1. Timeouts & Execution Budgets
    SettingDef {
        key: "CRATERA_RUN_MS",
        description: "Execution time limit for test runs (ms)",
        default_val: "2000",
        category: "Timeouts & Execution Budgets",
    },
    SettingDef {
        key: "CRATERA_SUBMIT_MS",
        description: "Execution time limit for formal submissions (ms)",
        default_val: "5000",
        category: "Timeouts & Execution Budgets",
    },
    SettingDef {
        key: "CRATERA_MAX_TIME_MS",
        description: "Hard upper ceiling for execution timeouts (ms)",
        default_val: "10000",
        category: "Timeouts & Execution Budgets",
    },
    SettingDef {
        key: "CRATERA_COMPILE_TIMEOUT_SECS",
        description: "Guest compiler compilation time limit (seconds)",
        default_val: "12",
        category: "Timeouts & Execution Budgets",
    },
    // 2. MicroVM Hardware Budgets (Per VM)
    SettingDef {
        key: "CRATERA_VCPU",
        description: "Virtual CPU cores allocated per MicroVM",
        default_val: "2",
        category: "MicroVM Hardware Budgets (Per VM)",
    },
    SettingDef {
        key: "CRATERA_MEM_MIB",
        description: "Guest RAM memory allocated per MicroVM (MiB)",
        default_val: "2048",
        category: "MicroVM Hardware Budgets (Per VM)",
    },
    // 3. Host Sandbox & Jailer Security Limits
    SettingDef {
        key: "CRATERA_JAIL_MEM_MAX",
        description: "Host cgroup memory.max per Firecracker process (bytes)",
        default_val: "3221225472",
        category: "Host Sandbox & Jailer Limits",
    },
    SettingDef {
        key: "CRATERA_JAIL_PIDS_MAX",
        description: "Host cgroup pids.max process limit per VM",
        default_val: "64",
        category: "Host Sandbox & Jailer Limits",
    },
    SettingDef {
        key: "CRATERA_USE_JAILER",
        description: "Enable Firecracker Jailer UID 20001 chroot sandbox (0 or 1)",
        default_val: "0",
        category: "Host Sandbox & Jailer Limits",
    },
    SettingDef {
        key: "CRATERA_USE_SNAPSHOT",
        description: "Enable microVM snapshot & restore optimization (0 or 1)",
        default_val: "0",
        category: "Host Sandbox & Jailer Limits",
    },
    // 4. Network & Paths
    SettingDef {
        key: "CRATERA_BIND",
        description: "HTTP Coordinator listen address (host:port)",
        default_val: "127.0.0.1:3100",
        category: "Network & Work Paths",
    },
    SettingDef {
        key: "CRATERA_WORK_DIR",
        description: "Host NVMe directory for ephemeral VM state",
        default_val: "/var/tmp/cratera",
        category: "Network & Work Paths",
    },
    SettingDef {
        key: "CRATERA_INTERNAL_KEY",
        description: "Bearer authentication key for HTTP API",
        default_val: "dev-key",
        category: "Network & Work Paths",
    },
];

pub fn run_settings(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(|s| s.as_str()) {
        None | Some("list") | Some("show") => show_settings()?,
        Some("get") => {
            if let Some(key) = args.get(1) {
                get_setting(key)?;
            } else {
                eprintln!("{RED}Error:{RESET} Usage: cratera settings get <KEY>");
            }
        }
        Some("set") => {
            if let (Some(key), Some(val)) = (args.get(1), args.get(2)) {
                set_setting(key, val)?;
            } else {
                eprintln!("{RED}Error:{RESET} Usage: cratera settings set <KEY> <VALUE>");
            }
        }
        Some("reset") => reset_settings()?,
        Some(unknown) => {
            eprintln!("{YELLOW}Unknown settings subcommand:{RESET} {unknown}");
            eprintln!("Usage:");
            eprintln!("  cratera settings list            (View all settings & resource budgets)");
            eprintln!("  cratera settings get <KEY>       (Inspect a specific key)");
            eprintln!("  cratera settings set <KEY> <VAL> (Persist setting to .env)");
            eprintln!("  cratera settings reset           (Reset .env to default values)");
        }
    }
    Ok(())
}

fn show_settings() -> anyhow::Result<()> {
    println!(
        "\n{BOLD}══════════════════════════════════════════════════════════════════════════════════{RESET}"
    );
    println!(" {BOLD}Cratera System Resource Budgets & Runtime Configuration{RESET}");
    println!(
        "{BOLD}══════════════════════════════════════════════════════════════════════════════════{RESET}\n"
    );

    let mut current_cat = "";
    for s in ALL_SETTINGS {
        if s.category != current_cat {
            current_cat = s.category;
            println!("{BOLD}{CYAN}▶ {current_cat}{RESET}");
            println!(
                "{DIM}──────────────────────────────────────────────────────────────────────────────────{RESET}"
            );
        }
        let env_val = std::env::var(s.key).ok();
        let (val, src) = match env_val {
            Some(ref v) if v != s.default_val => (v.as_str(), format!("{GREEN}[Custom]{RESET}")),
            Some(_) => (s.default_val, format!("{DIM}[Default]{RESET}")),
            None => (s.default_val, format!("{DIM}[Default]{RESET}")),
        };

        println!(
            "  {BOLD}{:<28}{RESET} {:<16} {:<12} {DIM}{}{RESET}",
            s.key, val, src, s.description
        );
    }

    println!("\n{DIM}Tip: Modify any value with: cratera settings set <KEY> <VALUE>{RESET}");
    println!("{DIM}Example: cratera settings set CRATERA_RUN_MS 3000{RESET}\n");

    Ok(())
}

fn get_setting(key: &str) -> anyhow::Result<()> {
    let key_upper = key.to_uppercase();
    if let Some(def) = ALL_SETTINGS.iter().find(|s| s.key == key_upper) {
        let val = std::env::var(&key_upper).unwrap_or_else(|_| def.default_val.to_string());
        println!("{BOLD}{}:{RESET} {}", def.key, val);
        println!("{DIM}Description:{RESET} {}", def.description);
        println!("{DIM}Default:{RESET} {}", def.default_val);
    } else {
        eprintln!("{YELLOW}Unknown setting key:{RESET} {key}");
    }
    Ok(())
}

fn set_setting(key: &str, val: &str) -> anyhow::Result<()> {
    let key_upper = key.to_uppercase();
    let def = ALL_SETTINGS.iter().find(|s| s.key == key_upper);
    if def.is_none() {
        println!("{YELLOW}! Warning:{RESET} '{key_upper}' is not a standard Cratera setting key.");
    }

    let env_path = Path::new(".env");
    let mut env_map = HashMap::new();
    let mut original_lines = Vec::new();

    if env_path.exists()
        && let Ok(content) = fs::read_to_string(env_path)
    {
        for line in content.lines() {
            original_lines.push(line.to_string());
            if let Some((k, v)) = line.split_once('=') {
                let k_trim = k.trim();
                if !k_trim.starts_with('#') && !k_trim.is_empty() {
                    env_map.insert(k_trim.to_string(), v.trim().to_string());
                }
            }
        }
    }

    env_map.insert(key_upper.clone(), val.to_string());

    let mut new_content = String::new();
    let mut replaced = false;
    for line in &original_lines {
        if let Some((k, _)) = line.split_once('=')
            && k.trim() == key_upper
        {
            new_content.push_str(&format!("{key_upper}={val}\n"));
            replaced = true;
            continue;
        }
        new_content.push_str(line);
        new_content.push('\n');
    }
    if !replaced {
        new_content.push_str(&format!("{key_upper}={val}\n"));
    }

    fs::write(env_path, new_content)?;
    unsafe {
        std::env::set_var(&key_upper, val);
    }

    println!("{GREEN}✓ Persisted setting to .env:{RESET} {BOLD}{key_upper}={val}{RESET}");
    if let Some(d) = def {
        println!("  {DIM}{}{RESET}", d.description);
    }

    Ok(())
}

fn reset_settings() -> anyhow::Result<()> {
    let env_path = Path::new(".env");
    if env_path.exists() {
        fs::remove_file(env_path)?;
        println!("{GREEN}✓ Cleared .env file. All settings reset to default values.{RESET}");
    } else {
        println!("{DIM}.env file not present. All settings are already using defaults.{RESET}");
    }
    Ok(())
}
