/// Filesystem tools: read, write, list directory.
use std::path::PathBuf;
use tracing::info;

use candor_core::error::CoreError;

use super::registry::{Tool, ToolContext, ToolOutput};

pub struct ReadFileTool;

#[async_trait::async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read contents of a file. Args: <path> [max_lines]"
    }

    async fn execute(&self, ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError> {
        let path = args
            .first()
            .ok_or_else(|| CoreError::Internal("read_file requires a path argument".into()))?;
        let max_lines: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(500);

        let full_path = PathBuf::from(&ctx.workdir).join(path);
        info!(path = %full_path.display(), "Reading file");

        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| CoreError::Io(format!("Cannot read {}: {e}", full_path.display())))?;

        let lines: Vec<&str> = content.lines().take(max_lines).collect();
        let truncated = if content.lines().count() > max_lines {
            format!("\n... (truncated, {} total lines)", content.lines().count())
        } else {
            String::new()
        };

        Ok(ToolOutput::ok(format!("{}{}", lines.join("\n"), truncated)))
    }
}

pub struct WriteFileTool;

#[async_trait::async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Write content to a file. Args: <path> <content>"
    }

    async fn execute(&self, ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError> {
        let path = args
            .first()
            .ok_or_else(|| CoreError::Internal("write_file requires a path argument".into()))?;
        let content = args.get(1).cloned().unwrap_or_default();

        let full_path = PathBuf::from(&ctx.workdir).join(path);
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| CoreError::Io(e.to_string()))?;
        }

        tokio::fs::write(&full_path, &content)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;

        info!(path = %full_path.display(), "File written");
        Ok(ToolOutput::ok(format!(
            "Written {} bytes to {}",
            content.len(),
            full_path.display()
        )))
    }
}

pub struct ListDirTool;

#[async_trait::async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }
    fn description(&self) -> &str {
        "List contents of a directory. Args: [path]"
    }

    async fn execute(&self, ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError> {
        let rel_path = args.first().map(|s| s.as_str()).unwrap_or(".");
        let full_path = PathBuf::from(&ctx.workdir).join(rel_path);

        let mut entries = tokio::fs::read_dir(&full_path)
            .await
            .map_err(|e| CoreError::Io(format!("Cannot list {}: {e}", full_path.display())))?;

        let mut listing = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            let file_type = entry.file_type().await.ok();
            let prefix = match file_type {
                Some(ft) if ft.is_dir() => "📁",
                Some(ft) if ft.is_symlink() => "🔗",
                _ => "📄",
            };
            listing.push(format!("{prefix} {name}"));
        }

        listing.sort();
        Ok(ToolOutput::ok(listing.join("\n")))
    }
}
