use crate::display::{GREEN, BOLD, RESET};

const CANDOR_TOML_CONTENT: &str = r#"[server]
host = "127.0.0.1"
port = 31337
checkpoint_dir = "/tmp/candor-checkpoints"
max_iterations = 100

[sandbox]
scratchpad_dir = "/tmp/agent_scratchpad"
default_timeout_secs = 15
default_memory_mb = 256

[inference]
# anthropic_api_key = "sk-ant-..."
# openai_api_key = "sk-..."
embedding_model = "all-MiniLM-L6-v2"
embedding_dim = 384

[memory]
backend = "mem"
compaction_token_limit = 135000
"#;

/// Bootstrap a new candor project in the given directory.
pub fn init_project(dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&path)?;
    std::fs::write(path.join("candor.toml"), CANDOR_TOML_CONTENT)?;
    std::fs::write(path.join(".gitignore"), "/target/\n.env\n/tmp/\n")?;
    println!("{GREEN}✓{RESET} Project initialized at {BOLD}{}{RESET}", path.display());
    println!("  candor task \"build something\"");
    Ok(())
}