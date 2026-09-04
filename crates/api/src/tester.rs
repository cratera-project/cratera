use cratera_common::HarnessResult;
use cratera_executor::{ExecutorConfig, FirecrackerExecutor};
use std::path::Path;

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub async fn run_test(args: &[String]) -> anyhow::Result<()> {
    println!("\n{BOLD}═══════════════════════════════════════════════════════════════{RESET}");
    println!(" {BOLD}Cratera In-Guest Hardware MicroVM Smoke Tester{RESET}");
    println!("{BOLD}═══════════════════════════════════════════════════════════════{RESET}\n");

    let kvm = Path::new("/dev/kvm");
    if !kvm.exists() {
        eprintln!("{RED}Error:{RESET} /dev/kvm not found. Hardware virtualization required.");
        return Ok(());
    }

    let cfg = ExecutorConfig::try_from_env().map_err(anyhow::Error::msg)?;
    if !cfg.firecracker.exists() {
        eprintln!(
            "{RED}Error:{RESET} Firecracker binary missing at: {}",
            cfg.firecracker.display()
        );
        eprintln!("Run ./scripts/fetch-runtime.sh to download runtime assets.");
        return Ok(());
    }
    if !cfg.kernel.exists() {
        eprintln!(
            "{RED}Error:{RESET} Guest kernel missing at: {}",
            cfg.kernel.display()
        );
        return Ok(());
    }
    if !cfg.rootfs.exists() {
        eprintln!(
            "{RED}Error:{RESET} Rootfs image missing at: {}",
            cfg.rootfs.display()
        );
        eprintln!("Run ./scripts/build-rootfs.sh or 'cratera build' to generate rootfs.");
        return Ok(());
    }

    let executor = FirecrackerExecutor::new(cfg.clone());
    let target_filter = args.first().map(|s| s.to_lowercase());

    let test_snippets = [
        (
            "rust",
            "fn main() { println!(\"Hello from isolated Rust!\"); }",
        ),
        (
            "python",
            "nums = [1, 2, 3, 4, 5]\nprint(f\"Hello from Python! Sum={sum(nums)}\")",
        ),
        ("node", "console.log('Hello from Node ' + process.version);"),
        (
            "cpp",
            "#include <iostream>\nint main() { std::cout << \"Hello from C++20!\" << std::endl; return 0; }",
        ),
        (
            "c",
            "#include <stdio.h>\nint main() { printf(\"Hello from C!\\n\"); return 0; }",
        ),
        (
            "go",
            "package main\nimport \"fmt\"\nfunc main() { fmt.Println(\"Hello from Go!\") }",
        ),
    ];

    let mut tested = 0;
    let mut passed = 0;
    let mut failed = 0;

    for (lang_key, snippet) in test_snippets {
        if let Some(ref filter) = target_filter
            && filter != "all"
            && !filter.split(',').any(|f| f.trim() == lang_key)
        {
            continue;
        }

        let resolved = match cfg.languages.resolve(Some(lang_key)) {
            Some(r) => r,
            None => continue,
        };

        tested += 1;
        print!("  -> Testing {BOLD}{:<12}{RESET} ... ", resolved.name);
        std::io::Write::flush(&mut std::io::stdout())?;

        match executor
            .run_harness(snippet.to_string(), 5000, Some(resolved))
            .await
        {
            Ok(outcome) => {
                let res = HarnessResult::from_job(outcome.job.clone(), outcome.wall_ms);
                if res.verdict.is_accepted() {
                    passed += 1;
                    let mem_str = if outcome.job.run_rss_kb > 0 {
                        format!(", RAM: {:.1}MB", outcome.job.run_rss_kb as f64 / 1024.0)
                    } else {
                        String::new()
                    };
                    println!(
                        "{GREEN}{BOLD}PASSED{RESET} {DIM}(Boot: {}ms, Run: {}μs{}){RESET}",
                        outcome.boot_ms, outcome.job.run_ms, mem_str
                    );
                } else {
                    failed += 1;
                    println!("{RED}{BOLD}FAILED{RESET} (verdict: {:?})", res.verdict);
                    if let Some(err) = &res.compilation_error {
                        println!(
                            "     {DIM}Compiler Error: {}{RESET}",
                            err.lines().next().unwrap_or("")
                        );
                    }
                    if let Some(err) = &res.stderr {
                        println!(
                            "     {DIM}Stderr: {}{RESET}",
                            err.lines().next().unwrap_or("")
                        );
                    }
                }
            }
            Err(e) => {
                failed += 1;
                println!("{RED}{BOLD}ERROR{RESET}: {e}");
            }
        }
    }

    if tested == 0 {
        println!(
            "{YELLOW}No matching languages found to test.{RESET} (Filter: {:?})",
            target_filter
        );
    } else {
        println!("\n{BOLD}───────────────────────────────────────────────────────────────{RESET}");
        println!(
            " Summary: {GREEN}{passed} passed{RESET}, {RED}{failed} failed{RESET} out of {tested} tested."
        );
        println!("{BOLD}───────────────────────────────────────────────────────────────{RESET}\n");
    }

    Ok(())
}
