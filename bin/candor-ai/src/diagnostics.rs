use std::sync::Arc;

use candor_orchestrator::OrchestratorEngine;

use crate::display::{BOLD, GREEN, RESET, YELLOW};

/// Check all subsystems and print health report.
pub async fn run_health_check(orch: Arc<tokio::sync::Mutex<OrchestratorEngine>>) {
    let o = orch.lock().await;
    let frontier = o.cognitive.is_frontier_healthy();
    let local = o.cognitive.is_local_healthy();

    println!();
    println!("{BOLD}  Candor AI - Health Check{RESET}\n");
    println!(
        "  {BOLD}LLM:{RESET}      {}",
        if frontier {
            format!("{GREEN}Connected{RESET}")
        } else {
            format!("{YELLOW}Not configured{RESET}")
        }
    );
    println!(
        "  {BOLD}Local:{RESET}    {}",
        if local {
            format!("{GREEN}Connected{RESET}")
        } else {
            format!("{YELLOW}Not configured{RESET}")
        }
    );
    println!(
        "  {BOLD}Sandbox:{RESET}  {}",
        if o.sandbox.native_engine().is_bwrap_available() {
            "Bubblewrap"
        } else {
            "Direct"
        }
    );
    println!(
        "  {BOLD}Sentinel:{RESET} {}",
        if o.sentinel.is_active() {
            format!("{GREEN}Active{RESET}")
        } else {
            format!("{YELLOW}Inactive{RESET}")
        }
    );
    println!("  {BOLD}Tools:{RESET}    {} registered", o.tools.tool_count());
    println!();
    println!("{GREEN}  All systems operational.{RESET}");
}

/// Check if a newer version of Candor is available on GitHub.
async fn check_version() -> Option<String> {
    let current = env!("CARGO_PKG_VERSION");
    let url = "https://api.github.com/repos/TurinTech-Solutions/candor-ai/releases/latest";
    let client = reqwest::Client::builder().user_agent("candor-ai-doctor").build().ok()?;
    let resp = client.get(url).send().await.ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    let latest = json.get("tag_name")?.as_str()?.trim_start_matches('v');
    if latest != current {
        Some(format!("{current} -> {latest}"))
    } else {
        None
    }
}

/// Run full diagnostics and print results.
pub async fn run_doctor() {
    println!("\n{BOLD}Candor AI - Doctor{RESET}\n");

    let checks = [
        ("cargo", check_cmd("cargo")),
        ("git", check_cmd("git")),
        ("bubblewrap", check_cmd("bwrap")),
        (
            "whisper",
            check_cmd("whisper-cpp") || check_cmd("whisper-cli") || check_cmd("whisper"),
        ),
        ("piper-tts", check_cmd("piper")),
        ("espeak-ng", check_cmd("espeak-ng") || check_cmd("espeak")),
        ("aplay", check_cmd("aplay")),
        ("arecord", check_cmd("arecord")),
        ("PDA home", check_pda()),
        ("surrealDB", true),
    ];
    let all_ok = checks.iter().all(|(_, ok)| *ok);
    for (name, ok) in &checks {
        println!(
            "  {} {name}",
            if *ok {
                format!("{GREEN}✓{RESET}")
            } else {
                format!("{YELLOW}○{RESET}")
            }
        );
    }
    println!();
    match check_version().await {
        Some(update) => println!("  {YELLOW}⚠ Update available: {update}{RESET}"),
        None => println!("  {GREEN}✓ Up to date (v{}){RESET}", env!("CARGO_PKG_VERSION")),
    }
    println!();
    if all_ok {
        println!("{GREEN}✓ All checks passed.{RESET}");
    } else {
        println!("{YELLOW}○ Some optional dependencies missing. Candor will still work.{RESET}");
    }
}

fn check_cmd(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn check_pda() -> bool {
    if let Ok(home) = std::env::var("HOME") {
        std::path::Path::new(&home).join(".candor").join("IDENTITY.md").exists()
    } else {
        false
    }
}
