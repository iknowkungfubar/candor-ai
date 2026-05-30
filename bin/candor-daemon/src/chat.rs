//! Conversational chat REPL for Candor AI.
//!
//! Provides a readline-style interface where users type tasks naturally,
//! see streaming phase progress (Observe → Think → Plan → Build → Execute → Verify → Learn),
//! and continue the conversation with full context retention.
//!
//! Two entry points:
//! - `run_chat()`: interactive readline REPL with history
//! - `run_listen()`: pipe-mode for AI agent integration (reads from stdin)

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Local;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

use candor_core::ideal::IdealStateArtifact;
use candor_core::state::AgentState;
use candor_orchestrator::OrchestratorEngine;

// ── Terminal styling ──

const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const MAGENTA: &str = "\x1b[35m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Phase display metadata: (name, icon, description)
const PHASES: &[(&str, &str, &str)] = &[
    ("Observe", "🔍", "Scanning project context and gathering information..."),
    ("Think",   "🧠", "Reasoning about the problem and analyzing context..."),
    ("Plan",    "📋", "Designing a step-by-step implementation plan..."),
    ("Build",   "🔧", "Writing code and generating artifacts..."),
    ("Execute", "⚡", "Running build, compiling, and performing actions..."),
    ("Verify",  "✅", "Verifying outputs against acceptance criteria..."),
    ("Learn",   "📝", "Documenting results and storing in memory..."),
];

/// O(1) phase index lookup by name.
#[cfg(test)]
fn phase_index(name: &str) -> Option<usize> {
    PHASES.iter().position(|(n, _, _)| *n == name)
}

// ── History ──

const HISTORY_FILE: &str = ".candor_history";

fn history_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(HISTORY_FILE)
}

fn load_history() -> Vec<String> {
    let path = history_path();
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect()
}

fn append_history(line: &str) {
    let path = history_path();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

// ── Interactive Chat REPL ──

/// Run the interactive readline-style chat REPL.
///
/// Prints a welcome banner, then loops:
/// 1. Read a line of user input
/// 2. If `/quit` or EOF, exit
/// 3. Create an ISA and spawn the agent in a background task
/// 4. Stream phase progress to the terminal
/// 5. Show the execution log summary
/// 6. Loop back for more input (retains conversation context)
pub async fn run_chat(
    orchestrator: Arc<Mutex<OrchestratorEngine>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Single persistent stdin reader (recreating BufReader would lose buffered data)
    let mut reader = BufReader::new(tokio::io::stdin());

    // ── Welcome banner ──
    print!(
        "\
         {CYAN}{BOLD}╔══════════════════════════════════════════════╗\n\
         ║        Candor AI — Conversational REPL        ║\n\
         ║     Type a task or `/quit` to exit.           ║\n\
         ╚══════════════════════════════════════════════╝{RESET}\n\n"
    );

    let mut conversation_round: u64 = 0;

    loop {
        conversation_round += 1;

        // ── Prompt ──
        let prompt = format!(
            "{CYAN}{BOLD}┌─[{RESET}candor{CYAN}]{BOLD}─({RESET}round {conversation_round}{CYAN}){RESET}\n\
             {CYAN}{BOLD}└─▶{RESET} "
        );
        print!("{prompt}");
        let _ = std::io::Write::flush(&mut std::io::stdout());

        // ── Read input ──
        let mut line_buf = String::new();
        let bytes_read = reader.read_line(&mut line_buf).await?;

        // EOF / Ctrl+D
        if bytes_read == 0 {
            println!("\n{GREEN}Goodbye!{RESET}");
            break;
        }

        let line = line_buf.trim().to_string();

        // ── Check for exit ──
        if line.eq_ignore_ascii_case("/quit")
            || line.eq_ignore_ascii_case("/exit")
            || line.eq_ignore_ascii_case("/q")
        {
            println!("\n{GREEN}Goodbye!{RESET}");
            break;
        }

        // ── Skip empty input ──
        if line.is_empty() {
            continue;
        }

        // ── Internal commands ──
        if line.starts_with('/') {
            handle_command(&line).await;
            continue;
        }

        // ── Save to history ──
        append_history(&line);

        // ── Execute the task with live streaming ──
        println!();
        let result = run_task_with_streaming(orchestrator.clone(), &line).await;

        match result {
            Ok(summary) => {
                println!(
                    "\n {GREEN}{BOLD}✓ Task completed successfully{RESET}  ({})\n",
                    summary.events_shown
                );
                if !summary.log_preview.is_empty() {
                    for entry in &summary.log_preview {
                        println!("   {DIM}{entry}{RESET}");
                    }
                    println!();
                }
            }
            Err(e) => {
                eprintln!(
                    "\n {RED}{BOLD}✗ Task failed:{RESET} {e}\n"
                );
            }
        }
    }

    Ok(())
}

// ── Listen (Pipe) Mode ──

/// Run in listen mode: reads tasks line-by-line from stdin (pipe or file).
///
/// Suitable for AI-agent integration:
/// - No prompts or banners
/// - Each non-empty line is treated as a task
/// - Phase transitions are printed (machine-parseable)
/// - EOF (/dev/null close) exits gracefully
pub async fn run_listen(
    orchestrator: Arc<Mutex<OrchestratorEngine>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf).await?;
        if bytes_read == 0 {
            break;
        }

        let line = line_buf.trim().to_string();
        if line.is_empty() {
            continue;
        }

        if line.eq_ignore_ascii_case("/quit") || line.eq_ignore_ascii_case("/exit") {
            break;
        }

        match run_task_with_streaming(orchestrator.clone(), &line).await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("FAILED: {e}");
            }
        }
    }

    Ok(())
}

// ── Task Execution with Streaming ──

struct TaskSummary {
    events_shown: String,
    log_preview: Vec<String>,
}

/// Run a task through the 7-phase agent pipeline while streaming
/// phase transitions and execution log events to stdout in real-time.
async fn run_task_with_streaming(
    orchestrator: Arc<Mutex<OrchestratorEngine>>,
    task: &str,
) -> Result<TaskSummary, Box<dyn std::error::Error>> {
    // ── Build ISA ──
    let isa = IdealStateArtifact {
        id: format!("chat-{}", uuid::Uuid::new_v4()),
        goal: task.to_string(),
        acceptance_criteria: vec![],
        constraints: vec![],
        expected_artifacts: vec![],
        phase_requirements: Default::default(),
        fully_autonomous: true,
    };

    // ── Clone state handle BEFORE locking the orchestrator ──
    // This gives us an independent handle to poll for progress
    // while `run_task` executes in the background.
    let state_handle: Arc<Mutex<AgentState>> = {
        let orch = orchestrator.lock().await;
        orch.graph_runner.state()
    };

    // ── Spawn the agent task in background ──
    let orch_clone = orchestrator.clone();
    let task_clone = task.to_string();
    let isa_clone = isa.clone();

    let agent_handle = tokio::spawn(async move {
        let mut orch = orch_clone.lock().await;
        orch.run_task(&task_clone, &isa_clone).await
    });

    // ── Print task header ──
    println!(" {CYAN}▶{RESET} {BOLD}Task:{RESET} {task}\n");

    // ── Stream phase progress by polling AgentState ──
    let mut last_phase: Option<String> = None;
    let mut last_log_len: usize = 0;
    let mut phase_start_times: Vec<(String, chrono::DateTime<Local>)> = Vec::new();

    loop {
        // Check if the agent finished
        if agent_handle.is_finished() {
            // Flush any remaining log entries before joining
            {
                let s = state_handle.lock().await;
                if s.execution_log.len() > last_log_len {
                    for event in &s.execution_log[last_log_len..] {
                        print_log_event(event);
                    }
                }
            }
            break;
        }

        let s = state_handle.lock().await;

        // ── Detect phase transitions ──
        let current_phase = s.current_phase.clone();
        if current_phase != last_phase {
            if let Some(ref phase_name) = current_phase {
                let now = Local::now();
                let elapsed = phase_start_times
                    .last()
                    .map(|(_, t)| {
                        let d = now - *t;
                        format_duration(d.num_seconds().max(0) as u64)
                    })
                    .unwrap_or_else(|| "0s".into());

                // Mark the previous phase as complete with timing
                if let Some(ref prev) = last_phase {
                    print_phase_complete(prev, &elapsed);
                }

                // Print the new phase header
                print_phase_start(phase_name);
                phase_start_times.push((phase_name.clone(), now));
                last_phase = current_phase;
            }
        }

        // ── Print new execution log entries (non-phase events) ──
        if s.execution_log.len() > last_log_len {
            for event in &s.execution_log[last_log_len..] {
                // Skip the events that are phase-logged (we handle those via current_phase)
                let is_phase_event = event.contains("Phase: ")
                    || event.contains("Phase: complete");
                if !is_phase_event {
                    print_log_event(event);
                }
            }
            last_log_len = s.execution_log.len();
        }

        drop(s);
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    // ── Mark the last phase as complete ──
    if let Some(ref last) = last_phase {
        let elapsed = phase_start_times
            .last()
            .map(|(_, t)| {
                let d = Local::now() - *t;
                format_duration(d.num_seconds().max(0) as u64)
            })
            .unwrap_or_else(|| "0s".into());
        print_phase_complete(last, &elapsed);
    }

    // ── Await the result ──
    let result = agent_handle.await.map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, format!("Agent task panicked: {e}"))
    })?;

    // ── Build summary ──
    let s = state_handle.lock().await;
    let log_len = s.execution_log.len();
    let log_preview: Vec<String> = s
        .execution_log
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|e| {
            let ts = extract_time(e);
            let msg = extract_message(e);
            format!("{DIM}{ts}{RESET} {msg}")
        })
        .collect();
    drop(s);

    match result {
        Ok(()) => Ok(TaskSummary {
            events_shown: format!("{log_len} events"),
            log_preview,
        }),
        Err(e) => Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))),
    }
}

// ── Rendering helpers ──

fn print_phase_start(name: &str) {
    let (icon, desc) = PHASES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, i, d)| (*i, *d))
        .unwrap_or(("●", "Executing..."));

    let now = Local::now().format("%H:%M:%S").to_string();
    println!(
        " {CYAN}┌─{BOLD}{icon} {name}{RESET}{DIM}  {desc}{RESET}"
    );
    println!(
        " {CYAN}│{RESET}  {DIM}at {now}{RESET}"
    );
}

fn print_phase_complete(name: &str, elapsed: &str) {
    let icon = PHASES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, i, _)| *i)
        .unwrap_or("●");

    let now = Local::now().format("%H:%M:%S").to_string();
    println!(
        " {GREEN}└─{BOLD}{icon} {name}{RESET}  {DIM}✓ done  ({elapsed})  at {now}{RESET}"
    );
}

fn print_log_event(event: &str) {
    let ts = extract_time(event);
    let msg = extract_message(event);
    let colored = colorize_message(msg);
    println!("   {DIM}{ts}{RESET} {colored}");
}

fn extract_time(event: &str) -> String {
    // Format: "[2026-05-30T12:34:56+00:00] message"
    if event.starts_with('[') {
        if let Some(end) = event.find(']') {
            let full = &event[1..end];
            // full is like "2026-05-30T12:34:56+00:00" or "2026-05-30T12:34:56Z"
            if let Some(t_pos) = full.find('T') {
                // Take exactly 8 characters after 'T' for HH:MM:SS
                let time_part: String = full[t_pos + 1..].chars().take(8).collect();
                if time_part.len() == 8 {
                    return time_part;
                }
            }
        }
    }
    "--:--:--".to_string()
}

fn extract_message(event: &str) -> &str {
    if let Some(end) = event.find(']') {
        return event[end + 1..].trim();
    }
    event
}

fn colorize_message(msg: &str) -> String {
    if msg.starts_with("Phase:") {
        format!("{CYAN}{msg}{RESET}")
    } else if msg.contains("complete") || msg.contains("PASSED") {
        format!("{GREEN}{msg}{RESET}")
    } else if msg.contains("FAILED") || msg.contains("Error") {
        format!("{RED}{msg}{RESET}")
    } else if msg.contains("warning") {
        format!("{YELLOW}{msg}{RESET}")
    } else if msg.starts_with("Observe:") || msg.starts_with("Think:") {
        format!("{DIM}{msg}{RESET}")
    } else if msg.starts_with("Plan:") || msg.starts_with("Build:") || msg.starts_with("Execute:") || msg.starts_with("Verify:") || msg.starts_with("Learn:") {
        format!("{MAGENTA}{msg}{RESET}")
    } else {
        format!("{DIM}{msg}{RESET}")
    }
}

fn format_duration(secs: u64) -> String {
    let mins = secs / 60;
    let secs_remainder = secs % 60;
    if mins > 0 {
        format!("{mins}m {secs_remainder}s")
    } else {
        format!("{secs_remainder}s")
    }
}

// ── Internal command handler ──

async fn handle_command(line: &str) {
    let parts: Vec<&str> = line.splitn(2, ' ').collect();
    let cmd = parts[0].to_lowercase();

    match cmd.as_str() {
        "/help" | "/h" => {
            println!(
                "{BOLD}Available commands:{RESET}\n\
                  {GREEN}/help{RESET}, {GREEN}/h{RESET}     Show this help\n\
                  {GREEN}/quit{RESET}, {GREEN}/q{RESET}     Exit the REPL\n\
                  {GREEN}/exit{RESET}          Exit the REPL\n\
                  {GREEN}/status{RESET}        Show current agent state\n\
                  {GREEN}/history{RESET}       Show recent history\n\
                  \n\
                  {DIM}Or just type any task description to run it!{RESET}"
            );
        }
        "/status" => {
            println!(
                "{YELLOW}Status information not available in chat mode.{RESET}\n\
                 Use the `--health` flag when starting candor."
            );
        }
        "/history" => {
            let h = load_history();
            if h.is_empty() {
                println!("{DIM}No history yet.{RESET}");
            } else {
                println!("{BOLD}Recent history:{RESET}");
                for (i, entry) in h.iter().rev().take(20).enumerate() {
                    println!("  {DIM}{}.{RESET} {entry}", h.len() - i);
                }
            }
        }
        _ => {
            println!("{YELLOW}Unknown command: {line}{RESET}");
            println!("Type {GREEN}/help{RESET} for available commands.");
        }
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_index_all() {
        for (i, (name, _, _)) in PHASES.iter().enumerate() {
            assert_eq!(phase_index(name), Some(i));
        }
        assert_eq!(phase_index("Nonexistent"), None);
    }

    #[test]
    fn test_extract_message_standard() {
        let msg = "[2026-05-30T12:34:56+00:00] Phase: Observe";
        assert_eq!(extract_message(msg), "Phase: Observe");
    }

    #[test]
    fn test_extract_message_no_brackets() {
        assert_eq!(extract_message("raw message"), "raw message");
    }

    #[test]
    fn test_extract_time_format() {
        let msg = "[2026-05-30T12:34:56+00:00] Task complete";
        let t = extract_time(&msg);
        assert_eq!(t, "12:34:56");
    }

    #[test]
    fn test_extract_time_no_brackets() {
        let msg = "raw message";
        let t = extract_time(&msg);
        assert_eq!(t, "--:--:--");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(65), "1m 5s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(0), "0s");
    }

    #[test]
    fn test_colorize_message() {
        let colored = colorize_message("Phase: Observe");
        assert!(colored.starts_with('\x1b'));

        let colored = colorize_message("Task complete");
        assert!(colored.contains("Task complete"));
    }
}
