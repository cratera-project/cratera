use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode, size,
    },
};
use std::fs;
use std::io::{Write, stdout};

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug)]
pub struct LangItem {
    pub key: String,
    pub name: String,
    pub source: String,
    pub install_mode: String,
    pub enabled: bool,
}

pub fn run_lang(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(|s| s.as_str()) {
        None => interactive_language_picker()?,
        Some("interactive") | Some("tui") | Some("picker") => interactive_language_picker()?,
        Some("list") | Some("show") => list_languages()?,
        Some("enable") => {
            let targets = &args[1..];
            if targets.is_empty() {
                eprintln!("{RED}Error:{RESET} Usage: cratera lang enable <KEY|NUM...> [KEY2...]");
            } else {
                set_languages_enabled(targets, true)?;
            }
        }
        Some("disable") => {
            let targets = &args[1..];
            if targets.is_empty() {
                eprintln!("{RED}Error:{RESET} Usage: cratera lang disable <KEY|NUM...> [KEY2...]");
            } else {
                set_languages_enabled(targets, false)?;
            }
        }
        Some("preset") => {
            if let Some(preset_name) = args.get(1) {
                apply_preset(preset_name)?;
            } else {
                eprintln!(
                    "{RED}Error:{RESET} Usage: cratera lang preset <all|top10|systems|web|functional|scientific|minimal>"
                );
            }
        }
        Some(unknown) => {
            eprintln!("{YELLOW}Unknown language subcommand:{RESET} {unknown}");
            eprintln!("Usage:");
            eprintln!(
                "  cratera lang                            (Interactive cursor-driven toggle list)"
            );
            eprintln!("  cratera lang list                       (Print full language table)");
            eprintln!(
                "  cratera lang enable <KEY|NUM...>        (Enable languages by key or #, e.g. 27 28)"
            );
            eprintln!("  cratera lang disable <KEY|NUM...>       (Disable languages by key or #)");
            eprintln!(
                "  cratera lang preset <PRESET>            (Apply a curated language preset)"
            );
        }
    }
    Ok(())
}

pub fn get_manifest_path() -> String {
    std::env::var("CRATERA_LANGUAGES_FILE").unwrap_or_else(|_| "languages.toml".into())
}

pub fn load_languages() -> anyhow::Result<(toml_edit::DocumentMut, Vec<LangItem>)> {
    let manifest_path = get_manifest_path();
    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| anyhow::anyhow!("Failed to read {manifest_path}: {e}"))?;
    let doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow::anyhow!("Failed to parse {manifest_path}: {e}"))?;

    let mut items = Vec::new();
    let root_table = doc.as_table();

    for (key, item) in root_table.iter() {
        let Some(table) = item.as_table_like() else {
            continue;
        };
        let Some(name_item) = table.get("name") else {
            continue;
        };
        let name = name_item.as_str().unwrap_or(key).to_string();
        let source = table
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string();
        let install_mode = table
            .get("install")
            .and_then(|v| v.as_str())
            .unwrap_or("docker_image")
            .to_string();
        let enabled = table
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        items.push(LangItem {
            key: key.to_string(),
            name,
            source,
            install_mode,
            enabled,
        });
    }

    Ok((doc, items))
}

pub fn interactive_language_picker() -> anyhow::Result<()> {
    let manifest_path = get_manifest_path();
    let (mut doc, mut items) = load_languages()?;

    if items.is_empty() {
        eprintln!("{RED}No languages found in {manifest_path}{RESET}");
        return Ok(());
    }

    let mut selected_idx: usize = 0;
    let mut status_msg = String::new();

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, Hide)?;

    struct RawModeGuard;
    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = execute!(stdout(), Show, LeaveAlternateScreen);
            let _ = disable_raw_mode();
        }
    }
    let _guard = RawModeGuard;

    loop {
        let (term_cols, term_rows) = size().unwrap_or((80, 24));
        let term_cols = term_cols as usize;
        let term_rows = term_rows as usize;

        // Reserve 7 rows for headers, status, and instructions
        let view_capacity = term_rows.saturating_sub(8).max(5);
        let total_items = items.len();

        // Viewport window calculation
        let scroll_offset = if selected_idx >= view_capacity {
            selected_idx - view_capacity + 1
        } else {
            0
        };
        let end_idx = (scroll_offset + view_capacity).min(total_items);

        let enabled_count = items.iter().filter(|i| i.enabled).count();

        execute!(out, MoveTo(0, 0), Clear(ClearType::All))?;

        // Header
        execute!(
            out,
            MoveTo(0, 0),
            SetForegroundColor(Color::Cyan),
            Print("═".repeat(term_cols.min(80))),
            Print("\r\n"),
            SetForegroundColor(Color::White),
            Print(" Cratera Interactive Language Manager\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print(
                " [↑/↓ or j/k] Move  •  [Enter/Space] Toggle  •  [a] All  •  [m] Minimal  •  [q] Exit\r\n"
            ),
            SetForegroundColor(Color::Cyan),
            Print("═".repeat(term_cols.min(80))),
            Print("\r\n"),
            SetForegroundColor(Color::DarkCyan),
            Print("    #   STATUS        KEY            NAME               INSTALL MODE\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(term_cols.min(80))),
            Print("\r\n"),
            ResetColor
        )?;

        // List Rows
        for (idx, item) in items[scroll_offset..end_idx].iter().enumerate() {
            let actual_idx = scroll_offset + idx;
            let is_cursor = actual_idx == selected_idx;
            let row_num = actual_idx + 1;

            let (status_icon, status_color) = if item.enabled {
                ("[x] Enabled ", Color::Green)
            } else {
                ("[ ] Disabled", Color::DarkGrey)
            };

            let line_str = format!(
                " {:>2}. {:<13} {:<14} {:<18} {:<14}",
                row_num, status_icon, item.key, item.name, item.install_mode
            );

            if is_cursor {
                execute!(
                    out,
                    SetBackgroundColor(Color::DarkBlue),
                    SetForegroundColor(Color::Yellow),
                    Print(" ❯"),
                    SetForegroundColor(Color::White),
                    Print(line_str),
                    ResetColor,
                    Print("\r\n")
                )?;
            } else {
                execute!(
                    out,
                    SetForegroundColor(Color::DarkGrey),
                    Print("  "),
                    SetForegroundColor(status_color),
                    Print(line_str),
                    ResetColor,
                    Print("\r\n")
                )?;
            }
        }

        // Footer
        execute!(
            out,
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(term_cols.min(80))),
            Print("\r\n"),
            SetForegroundColor(Color::White),
            Print(format!(
                " Active: {}/{} languages | Position: {}/{}",
                enabled_count,
                total_items,
                selected_idx + 1,
                total_items
            )),
            ResetColor
        )?;

        if !status_msg.is_empty() {
            execute!(
                out,
                SetForegroundColor(Color::Green),
                Print(format!("  •  {}", status_msg)),
                ResetColor
            )?;
        }

        execute!(out, Print("\r\n"))?;
        out.flush()?;

        // Input handler
        if let Event::Key(key_event) = event::read()? {
            if key_event.kind != KeyEventKind::Press {
                continue;
            }
            match key_event.code {
                KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {
                    if selected_idx > 0 {
                        selected_idx -= 1;
                    } else {
                        selected_idx = total_items.saturating_sub(1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {
                    if selected_idx + 1 < total_items {
                        selected_idx += 1;
                    } else {
                        selected_idx = 0;
                    }
                }
                KeyCode::PageUp => {
                    selected_idx = selected_idx.saturating_sub(view_capacity);
                }
                KeyCode::PageDown => {
                    selected_idx =
                        (selected_idx + view_capacity).min(total_items.saturating_sub(1));
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    selected_idx = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    selected_idx = total_items.saturating_sub(1);
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let item = &mut items[selected_idx];
                    item.enabled = !item.enabled;
                    let next_state = item.enabled;
                    let target_key = item.key.clone();

                    if let Some(tbl) = doc.as_table_mut().get_mut(&target_key) {
                        tbl["enabled"] = toml_edit::value(next_state);
                        let _ = fs::write(&manifest_path, doc.to_string());
                    }

                    status_msg = format!(
                        "Toggled {}: {}",
                        target_key,
                        if next_state { "Enabled" } else { "Disabled" }
                    );
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    for item in &mut items {
                        item.enabled = true;
                        if let Some(tbl) = doc.as_table_mut().get_mut(&item.key) {
                            tbl["enabled"] = toml_edit::value(true);
                        }
                    }
                    let _ = fs::write(&manifest_path, doc.to_string());
                    status_msg = "Applied 'All' preset (all 30 enabled)".into();
                }
                KeyCode::Char('m') | KeyCode::Char('M') => {
                    for item in &mut items {
                        item.enabled = item.key == "rust";
                        if let Some(tbl) = doc.as_table_mut().get_mut(&item.key) {
                            tbl["enabled"] = toml_edit::value(item.enabled);
                        }
                    }
                    let _ = fs::write(&manifest_path, doc.to_string());
                    status_msg = "Applied 'Minimal' preset (Rust only)".into();
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    let sys = ["rust", "cpp", "c", "go", "zig", "nim", "d", "fortran"];
                    for item in &mut items {
                        item.enabled = sys.contains(&item.key.as_str());
                        if let Some(tbl) = doc.as_table_mut().get_mut(&item.key) {
                            tbl["enabled"] = toml_edit::value(item.enabled);
                        }
                    }
                    let _ = fs::write(&manifest_path, doc.to_string());
                    status_msg = "Applied 'Systems' preset".into();
                }
                KeyCode::Char('w') | KeyCode::Char('W') => {
                    let web = [
                        "node",
                        "typescript",
                        "python",
                        "ruby",
                        "php",
                        "elixir",
                        "go",
                        "java",
                        "csharp",
                        "rust",
                    ];
                    for item in &mut items {
                        item.enabled = web.contains(&item.key.as_str());
                        if let Some(tbl) = doc.as_table_mut().get_mut(&item.key) {
                            tbl["enabled"] = toml_edit::value(item.enabled);
                        }
                    }
                    let _ = fs::write(&manifest_path, doc.to_string());
                    status_msg = "Applied 'Web' preset".into();
                }
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Backspace => {
                    break;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn list_languages() -> anyhow::Result<()> {
    let manifest_path = get_manifest_path();
    let (_, items) = load_languages()?;

    println!(
        "\n{BOLD}══════════════════════════════════════════════════════════════════════════════════{RESET}"
    );
    println!(
        " {BOLD}Cratera Multi-Language Runtimes & Toolchains ({}){RESET}",
        manifest_path
    );
    println!(
        "{BOLD}══════════════════════════════════════════════════════════════════════════════════{RESET}\n"
    );

    println!(
        " {BOLD}{:<3} {:<14} {:<18} {:<14} {:<16} STATUS{RESET}",
        "#", "KEY", "NAME", "SOURCE FILE", "INSTALL MODE"
    );
    println!(
        "{DIM}──────────────────────────────────────────────────────────────────────────────────{RESET}"
    );

    let mut enabled_count = 0;
    for (idx, item) in items.iter().enumerate() {
        let status_str = if item.enabled {
            enabled_count += 1;
            format!("{GREEN}● Enabled{RESET}")
        } else {
            format!("{DIM}○ Disabled{RESET}")
        };

        println!(
            " {:<3} {BOLD}{:<14}{RESET} {:<18} {:<14} {CYAN}{:<16}{RESET} {}",
            idx + 1,
            item.key,
            item.name,
            item.source,
            item.install_mode,
            status_str
        );
    }

    println!(
        "{DIM}──────────────────────────────────────────────────────────────────────────────────{RESET}"
    );
    println!(
        " {BOLD}Summary:{RESET} {GREEN}{enabled_count}{RESET} of {BOLD}{}{RESET} languages currently active in rootfs build.\n",
        items.len()
    );
    println!(
        " {DIM}Interactive cursor navigation:{RESET} Run 'cratera lang' to toggle with arrow keys & Enter."
    );
    println!(
        " {DIM}CLI toggle:{RESET}                    Run 'cratera lang enable <#|key>' or 'cratera lang disable <#|key>'\n"
    );

    Ok(())
}

fn set_languages_enabled(targets: &[String], enable: bool) -> anyhow::Result<()> {
    let manifest_path = get_manifest_path();
    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| anyhow::anyhow!("Failed to read {manifest_path}: {e}"))?;
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow::anyhow!("Failed to parse {manifest_path}: {e}"))?;

    let root_table = doc.as_table_mut();

    let ordered_keys: Vec<String> = root_table
        .iter()
        .filter_map(|(k, v)| {
            if v.as_table_like().and_then(|t| t.get("name")).is_some() {
                Some(k.to_string())
            } else {
                None
            }
        })
        .collect();

    let mut changed = Vec::new();
    let mut not_found = Vec::new();

    for t in targets {
        let t_clean = t.trim().trim_start_matches('#');
        let matched_key = if let Ok(idx) = t_clean.parse::<usize>() {
            if idx >= 1 && idx <= ordered_keys.len() {
                Some(ordered_keys[idx - 1].clone())
            } else {
                None
            }
        } else {
            let lower = t_clean.to_lowercase();
            if ordered_keys.contains(&lower) {
                Some(lower)
            } else {
                // Try matching by full name
                ordered_keys
                    .iter()
                    .find(|k| {
                        root_table
                            .get(k.as_str())
                            .and_then(|item| item.as_table_like())
                            .and_then(|tbl| tbl.get("name"))
                            .and_then(|n| n.as_str())
                            .map(|name| name.eq_ignore_ascii_case(t_clean))
                            .unwrap_or(false)
                    })
                    .cloned()
            }
        };

        if let Some(key) = matched_key {
            if let Some(item) = root_table.get_mut(&key) {
                item["enabled"] = toml_edit::value(enable);
                if !changed.contains(&key) {
                    changed.push(key);
                }
            }
        } else {
            not_found.push(t.clone());
        }
    }

    if !changed.is_empty() {
        fs::write(&manifest_path, doc.to_string())?;
        let action_str = if enable {
            format!("{GREEN}Enabled{RESET}")
        } else {
            format!("{YELLOW}Disabled{RESET}")
        };
        println!(
            "{BOLD}✓ {action_str} {}: {BOLD}{}{RESET}",
            changed.len(),
            changed.join(", ")
        );
        println!(
            "{DIM}Note: Rebuild rootfs with 'cratera build' to apply changes to guest disk.{RESET}"
        );
    }

    if !not_found.is_empty() {
        println!(
            "{RED}! Not found in manifest:{RESET} {}",
            not_found.join(", ")
        );
    }

    Ok(())
}

fn apply_preset(name: &str) -> anyhow::Result<()> {
    let top10 = [
        "python",
        "node",
        "rust",
        "cpp",
        "c",
        "go",
        "java",
        "csharp",
        "typescript",
        "ruby",
    ];
    let systems = [
        "rust", "cpp", "c", "go", "zig", "nim", "d", "fortran", "ada", "pascal",
    ];
    let web = [
        "node",
        "typescript",
        "python",
        "ruby",
        "php",
        "elixir",
        "go",
        "java",
        "csharp",
        "rust",
    ];
    let functional = [
        "haskell", "ocaml", "clojure", "scala", "erlang", "elixir", "fsharp",
    ];
    let scientific = ["python", "julia", "r", "fortran", "cpp", "c", "rust"];
    let minimal = ["rust"];

    let target_set: Option<&[&str]> = match name.to_lowercase().as_str() {
        "all" => None, // None means enable all
        "top10" | "top-10" => Some(&top10),
        "systems" | "sys" => Some(&systems),
        "web" => Some(&web),
        "functional" | "func" => Some(&functional),
        "scientific" | "sci" => Some(&scientific),
        "minimal" | "min" | "rust" => Some(&minimal),
        _ => {
            eprintln!("{RED}Unknown preset:{RESET} {name}");
            eprintln!(
                "Available presets: all, top10, systems, web, functional, scientific, minimal"
            );
            return Ok(());
        }
    };

    let manifest_path = get_manifest_path();
    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| anyhow::anyhow!("Failed to read {manifest_path}: {e}"))?;
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow::anyhow!("Failed to parse {manifest_path}: {e}"))?;

    let root_table = doc.as_table_mut();
    let mut enabled_keys = Vec::new();
    let mut disabled_count = 0;

    let keys: Vec<String> = root_table
        .iter()
        .filter_map(|(k, v)| {
            if v.as_table_like().and_then(|t| t.get("name")).is_some() {
                Some(k.to_string())
            } else {
                None
            }
        })
        .collect();

    for key in keys {
        let enable = match target_set {
            None => true,
            Some(set) => set.contains(&key.as_str()),
        };
        if let Some(item) = root_table.get_mut(&key) {
            item["enabled"] = toml_edit::value(enable);
            if enable {
                enabled_keys.push(key);
            } else {
                disabled_count += 1;
            }
        }
    }

    fs::write(&manifest_path, doc.to_string())?;

    println!("{BOLD}{GREEN}✓ Applied preset '{name}':{RESET}");
    println!(
        "  {BOLD}Active ({}):{RESET} {}",
        enabled_keys.len(),
        enabled_keys.join(", ")
    );
    println!("  {DIM}Disabled: {disabled_count} languages{RESET}");
    println!("  {DIM}Run 'cratera build' to rebuild guest rootfs with this preset.{RESET}\n");

    Ok(())
}
