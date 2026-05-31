/// PDA (Personal Digital Assistant) module.
///
/// Manages the `~/.candor/` home directory:
/// - IDENTITY.md         — who the user is (name, role, preferences, goals)
/// - DA_IDENTITY.md      — the DA's personality (name, voice, tone, style)
/// - MEMORY/
///   - WORK/<slug>/     — active task state (ISA.md, notes)
///   - LEARNING/        — meta-patterns extracted from completed sessions
///   - KNOWLEDGE/       — typed entities (People, Companies, Ideas, Projects)
///
/// Memory is git-backed: every write auto-commits to a local git repo
/// at ~/.candor/ (initialized on first use).
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

/// Errors from the PDA system.
#[derive(Debug)]
pub enum PdaError {
    Io(std::io::Error),
    Git(String),
    NotFound(String),
    AlreadyExists(String),
}

impl std::fmt::Display for PdaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Git(e) => write!(f, "Git error: {e}"),
            Self::NotFound(e) => write!(f, "Not found: {e}"),
            Self::AlreadyExists(e) => write!(f, "Already exists: {e}"),
        }
    }
}

impl std::error::Error for PdaError {}

impl From<std::io::Error> for PdaError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Resolve the PDA home directory (~/.candor).
pub fn pda_home() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".candor")
    } else {
        PathBuf::from("/tmp/candor-pda")
    }
}

/// Initialize the PDA home directory structure.
/// Called on first use (idempotent — safe to call repeatedly).
pub async fn init() -> Result<(), PdaError> {
    let home = pda_home();
    let dirs = [
        home.join("MEMORY").join("WORK"),
        home.join("MEMORY").join("LEARNING"),
        home.join("MEMORY").join("KNOWLEDGE"),
    ];
    for d in &dirs {
        tokio::fs::create_dir_all(d).await?;
    }

    // Create default IDENTITY.md if it doesn't exist.
    let identity_path = home.join("IDENTITY.md");
    if !identity_path.exists() {
        let default_identity = r#"# Identity

## Name
<!-- Your name or handle -->

## Role
<!-- What you do -->

## Goals
<!-- What you're working toward -->

## Preferences
<!-- Communication style, timezone, work hours, etc. -->

## Values
<!-- What matters to you -->
"#;
        let mut f = tokio::fs::File::create(&identity_path).await?;
        f.write_all(default_identity.as_bytes()).await?;
    }

    // Create default DA_IDENTITY.md if it doesn't exist.
    let da_path = home.join("DA_IDENTITY.md");
    if !da_path.exists() {
        let default_da = r#"# Digital Assistant Identity

## Name
Candor

## Voice
Professional, warm, direct. Uses natural language. Avoids jargon.

## Personality
- Honest and direct — tells you what you need to hear, not what you want
- Proactive — suggests next actions without being asked
- Concise — respects your time with focused responses
- Learning — adapts to your patterns and preferences over time

## Core Directives
1. Always be truthful, even when the truth is inconvenient
2. Prioritize your goals and values above efficiency metrics
3. Protect your privacy — never share identity or memory data
4. Continuously improve through reflection and learning

## Communication Style
- Technical when appropriate, plain language by default
- Uses bullet points for lists, paragraphs for narrative
- Asks clarifying questions when instructions are ambiguous
- Flags assumptions before acting on them
"#;
        let mut f = tokio::fs::File::create(&da_path).await?;
        f.write_all(default_da.as_bytes()).await?;
    }

    // Initialize git repo in ~/.candor/ if not already a repo.
    init_git_repo(&home).await?;

    Ok(())
}

/// Initialize git repo for version-controlled memory.
async fn init_git_repo(home: &PathBuf) -> Result<(), PdaError> {
    use std::process::Stdio;

    // Check if already a git repo.
    let status = tokio::process::Command::new("git")
        .arg("rev-parse")
        .arg("--git-dir")
        .current_dir(home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|e| PdaError::Git(format!("git not found: {e}")))?;

    if status.success() {
        return Ok(()); // Already a repo
    }

    // Initialize new repo.
    let out = tokio::process::Command::new("git")
        .arg("init")
        .current_dir(home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| PdaError::Git(format!("git init failed: {e}")))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(PdaError::Git(format!("git init: {stderr}")));
    }

    // Set user config for the repo if not already set globally.
    let has_global_user = tokio::process::Command::new("git")
        .args(["config", "--global", "user.name"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);

    if !has_global_user {
        // Set local git config for this repo so commits work.
        tokio::process::Command::new("git")
            .args(["config", "user.name", "Candor PDA"])
            .current_dir(home)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .ok();
        tokio::process::Command::new("git")
            .args(["config", "user.email", "pda@candor.local"])
            .current_dir(home)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .ok();
    }

    // Initial commit.
    tokio::process::Command::new("git")
        .args(["add", "."])
        .current_dir(home)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| PdaError::Git(format!("git add failed: {e}")))?;

    let _ = tokio::process::Command::new("git")
        .args(["commit", "-m", "chore: initialize PDA home"])
        .current_dir(home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    Ok(())
}

/// Git-commit any pending changes in the PDA home.
pub async fn auto_commit(message: &str) -> Result<(), PdaError> {
    let home = pda_home();
    use std::process::Stdio;

    let _ = tokio::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    tokio::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", message])
        .current_dir(&home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .ok();

    Ok(())
}

/// Get the path to IDENTITY.md.
pub fn identity_path() -> PathBuf {
    pda_home().join("IDENTITY.md")
}

/// Get the path to DA_IDENTITY.md.
pub fn da_identity_path() -> PathBuf {
    pda_home().join("DA_IDENTITY.md")
}

/// Read and return the user's IDENTITY.md content.
pub async fn read_identity() -> Result<String, PdaError> {
    let path = identity_path();
    if !path.exists() {
        return Err(PdaError::NotFound(
            "IDENTITY.md not found. Run `candor pda init` first.".into(),
        ));
    }
    Ok(tokio::fs::read_to_string(&path).await?)
}

/// Read and return the DA's identity content.
pub async fn read_da_identity() -> Result<String, PdaError> {
    let path = da_identity_path();
    if !path.exists() {
        return Err(PdaError::NotFound(
            "DA_IDENTITY.md not found. Run `candor pda init` first.".into(),
        ));
    }
    Ok(tokio::fs::read_to_string(&path).await?)
}

/// Write a memory note to MEMORY/LEARNING/<slug>.md with git auto-commit.
#[allow(dead_code)]
pub async fn write_learning(slug: &str, content: &str) -> Result<(), PdaError> {
    let path = pda_home()
        .join("MEMORY")
        .join("LEARNING")
        .join(format!("{slug}.md"));
    tokio::fs::write(&path, content).await?;
    auto_commit(&format!("learn: {slug}")).await?;
    Ok(())
}

/// Write a knowledge entity to MEMORY/KNOWLEDGE/<slug>.md with git auto-commit.
#[allow(dead_code)]
pub async fn write_knowledge(slug: &str, entity_type: &str, content: &str) -> Result<(), PdaError> {
    let path = pda_home()
        .join("MEMORY")
        .join("KNOWLEDGE")
        .join(format!("{slug}.md"));
    let frontmatter = format!(
        "---\ntype: {entity_type}\ncreated: {created}\n---\n\n{content}",
        created = chrono::Utc::now().to_rfc3339()
    );
    tokio::fs::write(&path, &frontmatter).await?;
    auto_commit(&format!("knowledge: {slug} ({entity_type})")).await?;
    Ok(())
}

/// Start a new work session: create MEMORY/WORK/<slug>/ISA.md.
pub async fn start_work(slug: &str, goal: &str) -> Result<(), PdaError> {
    let dir = pda_home().join("MEMORY").join("WORK").join(slug);
    if dir.exists() {
        return Err(PdaError::AlreadyExists(format!(
            "Work session '{slug}' already exists. Use a different slug or archive the existing one."
        )));
    }
    tokio::fs::create_dir_all(&dir).await?;

    let isa = format!(
        r#"# ISA: {slug}

## Problem
<!-- What problem are you solving? -->

## Vision
<!-- What does success look like? -->

## Goal
{goal}

## Acceptance Criteria
- [ ]

## Decisions

## Changelog
- {date}: Created
"#,
        date = chrono::Utc::now().format("%Y-%m-%d")
    );

    tokio::fs::write(dir.join("ISA.md"), &isa).await?;
    auto_commit(&format!("work: start {slug}")).await?;
    Ok(())
}

/// List all active work sessions.
pub async fn list_work() -> Result<Vec<String>, PdaError> {
    let dir = pda_home().join("MEMORY").join("WORK");
    let mut entries = tokio::fs::read_dir(&dir).await?;
    let mut slugs = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            slugs.push(name.to_string());
        }
    }
    slugs.sort();
    Ok(slugs)
}

/// Count markdown files in a memory directory (LEARNING or KNOWLEDGE).
async fn count_memory_files(dir: &std::path::Path) -> Result<usize, PdaError> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut count = 0;
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() {
            count += 1;
        }
    }
    Ok(count)
}

/// Get the number of uncommitted changes in the PDA git repo.
async fn git_uncommitted_count(home: &std::path::Path) -> usize {
    use std::process::Stdio;
    let out = tokio::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(home)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await;
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() { 0 } else { s.lines().count() }
        }
        Err(_) => 0,
    }
}

/// Check PDA status.
pub async fn status() -> Result<String, PdaError> {
    let home = pda_home();
    let exists = tokio::fs::try_exists(&home).await.unwrap_or(false);

    let mut report = String::new();
    report.push_str(&format!("PDA Home: {}\n", home.display()));
    report.push_str(&format!(
        "Initialized: {}\n",
        if exists { "✅" } else { "❌" }
    ));

    if exists {
        let identity = tokio::fs::try_exists(identity_path())
            .await
            .unwrap_or(false);
        let da = tokio::fs::try_exists(da_identity_path())
            .await
            .unwrap_or(false);
        report.push_str(&format!(
            "IDENTITY.md: {}\n",
            if identity { "✅" } else { "❌" }
        ));
        report.push_str(&format!(
            "DA_IDENTITY.md: {}\n",
            if da { "✅" } else { "❌" }
        ));

        let work_count = list_work().await?.len();
        report.push_str(&format!("Work sessions: {work_count}\n"));

        let learning_count = count_memory_files(&home.join("MEMORY").join("LEARNING")).await?;
        report.push_str(&format!("Learning entries: {learning_count}\n"));

        let dirty = git_uncommitted_count(&home).await;
        if dirty == 0 {
            report.push_str("Memory: ✅ clean\n");
        } else {
            report.push_str(&format!("Memory: {dirty} uncommitted changes\n"));
        }
    }

    Ok(report)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tokio::sync::Mutex;

    /// Serializes PDA tests — HOME env var is process-global.
    static TEST_LOCK: Mutex<()> = Mutex::const_new(());

    async fn with_pda<F, Fut, T>(f: F) -> T
    where
        F: FnOnce(PathBuf) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let _guard = TEST_LOCK.lock().await;

        let tmp = std::env::temp_dir().join(format!("candor-pda-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();

        let orig_home = std::env::var("HOME").ok();
        // SAFETY: Mutex ensures single-threaded env access.
        unsafe {
            std::env::set_var("HOME", &tmp);
        }

        let result = f(tmp.clone()).await;

        match orig_home {
            Some(h) => unsafe {
                std::env::set_var("HOME", h);
            },
            None => unsafe {
                std::env::remove_var("HOME");
            },
        }
        drop(_guard);
        let _ = tokio::fs::remove_dir_all(&tmp).await;

        result
    }

    #[tokio::test]
    async fn test_init_creates_directories() {
        with_pda(|tmp| async move {
            let pda = tmp.join(".candor");
            assert!(!pda.exists(), "home should not exist before init");
            init().await.unwrap();
            assert!(pda.exists());
            assert!(pda.join("IDENTITY.md").exists());
            assert!(pda.join("DA_IDENTITY.md").exists());
            assert!(pda.join("MEMORY").join("WORK").exists());
            assert!(pda.join("MEMORY").join("LEARNING").exists());
            assert!(pda.join("MEMORY").join("KNOWLEDGE").exists());
            assert!(pda.join(".git").exists());
        })
        .await;
    }

    #[tokio::test]
    async fn test_init_idempotent() {
        with_pda(|_| async move {
            init().await.unwrap();
            init().await.unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn test_read_identity_defaults() {
        with_pda(|_| async move {
            init().await.unwrap();
            let id = read_identity().await.unwrap();
            assert!(id.contains("## Name"));
            assert!(id.contains("## Goals"));
        })
        .await;
    }

    #[tokio::test]
    async fn test_read_da_identity_defaults() {
        with_pda(|_| async move {
            init().await.unwrap();
            let da = read_da_identity().await.unwrap();
            assert!(da.contains("## Name"));
            assert!(da.contains("Candor"));
        })
        .await;
    }

    #[tokio::test]
    async fn test_identity_not_found() {
        with_pda(|_| async move {
            let err = read_identity().await.unwrap_err().to_string();
            assert!(err.contains("IDENTITY.md"));
        })
        .await;
    }

    #[tokio::test]
    async fn test_da_identity_not_found() {
        with_pda(|_| async move {
            let err = read_da_identity().await.unwrap_err().to_string();
            assert!(err.contains("DA_IDENTITY.md"));
        })
        .await;
    }

    #[tokio::test]
    async fn test_start_work_creates_isa() {
        with_pda(|tmp| async move {
            init().await.unwrap();
            start_work("test-session", "Run a test").await.unwrap();
            let isa = tmp
                .join(".candor")
                .join("MEMORY")
                .join("WORK")
                .join("test-session")
                .join("ISA.md");
            assert!(isa.exists());
            let content = tokio::fs::read_to_string(isa).await.unwrap();
            assert!(content.contains("test-session"));
            assert!(content.contains("Run a test"));
        })
        .await;
    }

    #[tokio::test]
    async fn test_start_work_duplicate_slug() {
        with_pda(|_| async move {
            init().await.unwrap();
            start_work("dup", "first").await.unwrap();
            let err = start_work("dup", "second").await.unwrap_err().to_string();
            assert!(err.contains("already exists"));
        })
        .await;
    }

    #[tokio::test]
    async fn test_list_work() {
        with_pda(|_| async move {
            init().await.unwrap();
            assert!(list_work().await.unwrap().is_empty());
            start_work("alpha", "first").await.unwrap();
            start_work("beta", "second").await.unwrap();
            let slugs = list_work().await.unwrap();
            assert_eq!(slugs.len(), 2);
            assert!(slugs.contains(&"alpha".to_string()));
            assert!(slugs.contains(&"beta".to_string()));
        })
        .await;
    }

    #[tokio::test]
    async fn test_status_after_init() {
        with_pda(|_| async move {
            init().await.unwrap();
            let s = status().await.unwrap();
            assert!(s.contains("PDA Home"));
            assert!(s.contains("IDENTITY.md: ✅"));
            assert!(s.contains("DA_IDENTITY.md: ✅"));
            assert!(s.contains("Work sessions: 0"));
        })
        .await;
    }

    #[tokio::test]
    async fn test_status_with_work() {
        with_pda(|_| async move {
            init().await.unwrap();
            start_work("active", "do it").await.unwrap();
            assert!(status().await.unwrap().contains("Work sessions: 1"));
        })
        .await;
    }

    #[tokio::test]
    async fn test_write_learning() {
        with_pda(|tmp| async move {
            init().await.unwrap();
            write_learning("test-pat", "Learned something")
                .await
                .unwrap();
            let f = tmp
                .join(".candor")
                .join("MEMORY")
                .join("LEARNING")
                .join("test-pat.md");
            assert!(f.exists());
            assert!(
                tokio::fs::read_to_string(f)
                    .await
                    .unwrap()
                    .contains("Learned something")
            );
        })
        .await;
    }

    #[tokio::test]
    async fn test_write_knowledge() {
        with_pda(|tmp| async move {
            init().await.unwrap();
            write_knowledge("entity", "Idea", "great idea")
                .await
                .unwrap();
            let f = tmp
                .join(".candor")
                .join("MEMORY")
                .join("KNOWLEDGE")
                .join("entity.md");
            assert!(f.exists());
            let c = tokio::fs::read_to_string(f).await.unwrap();
            assert!(c.contains("type: Idea"));
            assert!(c.contains("great idea"));
        })
        .await;
    }

    #[tokio::test]
    async fn test_git_auto_commit() {
        with_pda(|tmp| async move {
            init().await.unwrap();
            write_learning("git-test", "auto-commit").await.unwrap();
            let out = std::process::Command::new("git")
                .args(["log", "--oneline"])
                .current_dir(&tmp.join(".candor"))
                .output()
                .unwrap();
            let log = String::from_utf8_lossy(&out.stdout);
            assert!(!log.is_empty(), "git log should have entries");
            assert!(
                log.contains("learn:"),
                "git log should contain learn commit: {log}"
            );
        })
        .await;
    }

    #[test]
    fn test_pda_home_ends_with_candor() {
        let h = pda_home();
        assert!(
            h.to_string_lossy().ends_with(".candor"),
            "got: {}",
            h.display()
        );
    }
}
