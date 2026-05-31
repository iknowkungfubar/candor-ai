/// Personal automation agents for the PDA system.
///
/// Provides scheduled, background agents for everyday life:
/// - **Morning Digest** — Daily briefing from your identity, knowledge, and work state
/// - **Monitor Agent** — Proactive checks on work sessions, learning, and knowledge gaps
///
/// These agents run via the existing cron infrastructure (delegate to Hermes cron
/// or run as one-shot tasks).
use crate::pda;

/// Build the system prompt for a morning digest agent.
///
/// Reads the user's identity and DA identity, then constructs a prompt
/// that asks the LLM to produce a concise daily briefing.
pub async fn morning_digest_prompt() -> Result<String, String> {
    let identity = pda::read_identity().await.map_err(|e| format!("{e}"))?;
    let da_identity = pda::read_da_identity().await.map_err(|e| format!("{e}"))?;

    let work_slugs = pda::list_work().await.map_err(|e| format!("{e}"))?;

    let mut work_summary = String::new();
    if work_slugs.is_empty() {
        work_summary.push_str("No active work sessions.\n");
    } else {
        work_summary.push_str("Active work sessions:\n");
        for slug in &work_slugs {
            work_summary.push_str(&format!("  - {slug}\n"));
        }
    }

    Ok(format!(
        r#"{da_identity}

## User Identity
{identity}

## Current State
{work_summary}

## Task
Generate a brief morning digest (3-5 bullet points) covering:
1. What you're working on today — prioritize active work sessions
2. Any decisions or blockers you should be aware of
3. A suggestion for the most impactful next action

Keep it concise — this will be read aloud via TTS.
"#,
    ))
}

/// Build the system prompt for a monitoring agent.
///
/// Scans the PDA state and suggests actions:
/// - Check if any work sessions are stale (>7 days no updates)
/// - Check for knowledge entries that need updating
/// - Suggest learning entries from recent patterns
pub async fn monitor_prompt() -> Result<String, String> {
    let da_identity = pda::read_da_identity().await.map_err(|e| format!("{e}"))?;

    let work_slugs = pda::list_work().await.map_err(|e| format!("{e}"))?;

    let mut analysis = String::new();
    for slug in &work_slugs {
        let work_dir = pda::pda_home()
            .join("MEMORY")
            .join("WORK")
            .join(slug);
        let isa_path = work_dir.join("ISA.md");
        if isa_path.exists() {
            if let Ok(meta) = tokio::fs::metadata(&isa_path).await {
                if let Ok(modified) = meta.modified() {
                    if let Ok(elapsed) = modified.elapsed() {
                        let days = elapsed.as_secs_f64() / 86400.0;
                        if days > 7.0 {
                            analysis.push_str(&format!(
                                "  - ⚠️  '{slug}' is stale (last activity {:.0} days ago)\n",
                                days
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(format!(
        r#"{da_identity}

## PDA Scan
Active work sessions: {work_count}
{analysis}

## Task
Analyze the PDA state and suggest:
1. Any work sessions that need attention
2. Patterns worth capturing as LEARNING entries
3. Knowledge entries to create or update

Respond in 2-3 bullet points.
"#,
        work_count = work_slugs.len()
    ))
}
