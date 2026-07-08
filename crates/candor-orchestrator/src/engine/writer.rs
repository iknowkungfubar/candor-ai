use std::path::PathBuf;

// ── Code writer helper ──

/// Parse LLM output containing FILE: markers and write files to disk.
/// Returns the number of files written.
pub(super) async fn write_code_files(output: &str, workdir: &str) -> usize {
    let mut count = 0;
    let mut path: Option<String> = None;
    let mut code = String::new();
    let mut in_block = false;

    for line in output.lines() {
        if line.starts_with("### FILE:") || line.starts_with("## FILE:") {
            if flush_file(&path, &code, workdir).await {
                count += 1;
            }
            path = Some(
                line.trim_start_matches("### FILE:")
                    .trim_start_matches("## FILE:")
                    .trim()
                    .to_string(),
            );
            code.clear();
            in_block = false;
        } else if line.trim() == "```" {
            in_block = !in_block;
        } else if in_block {
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(line);
        }
    }
    if flush_file(&path, &code, workdir).await {
        count += 1;
    }
    count
}

/// Returns `true` if a file was actually written.
async fn flush_file(path: &Option<String>, code: &str, workdir: &str) -> bool {
    if let Some(p) = path
        && !code.is_empty()
        && !p.is_empty()
    {
        let full = std::path::PathBuf::from(workdir).join(p);
        let _ = tokio::fs::create_dir_all(full.parent().unwrap()).await;
        let _ = tokio::fs::write(&full, code).await;
        true
    } else {
        false
    }
}

/// Resolve the PDA home directory (~/.candor) or fall back to /tmp.
pub(super) fn dirs_or_default() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".candor")
    } else {
        PathBuf::from("/tmp/candor")
    }
}

/// Convert a string into a filesystem-safe slug.
/// Replaces spaces with hyphens, removes other non-alphanumeric chars.
pub(super) fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .trim_matches('-')
        .to_string()
}
