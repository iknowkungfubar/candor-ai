/// Filesystem tools: read, write, list directory.
use std::path::{Path, PathBuf};
use tracing::info;

use candor_core::error::CoreError;

use super::registry::{Tool, ToolContext, ToolOutput};

/// Validate that a resolved file path is within the allowed workdir.
/// Uses canonicalize to prevent path traversal via `..` or symlinks.
/// NOTE: This function requires the path to exist on disk (for canonicalize).
async fn validate_file_path(resolved: &Path, workdir: &Path) -> Result<(), CoreError> {
    // Canonicalize both paths to resolve symlinks and `..` components
    let canonical_resolved = tokio::fs::canonicalize(resolved).await.map_err(|e| {
        CoreError::Io(format!(
            "Path validation failed for {}: {e}",
            resolved.display()
        ))
    })?;

    let canonical_workdir = tokio::fs::canonicalize(workdir)
        .await
        .map_err(|e| CoreError::Io(format!("Workdir canonicalization failed: {e}")))?;

    if !canonical_resolved.starts_with(&canonical_workdir) {
        return Err(CoreError::Io(format!(
            "Path traversal denied: {} escapes the working directory {}",
            resolved.display(),
            canonical_workdir.display()
        )));
    }
    Ok(())
}

/// Validate that a parent directory (for a file-to-be-created) is within workdir.
/// Canonicalizes the parent dir (which must exist) to prevent path traversal.
async fn validate_parent_path(parent: &Path, workdir: &Path) -> Result<(), CoreError> {
    let canonical_parent = tokio::fs::canonicalize(parent).await.map_err(|e| {
        CoreError::Io(format!(
            "Parent directory validation failed for {}: {e}",
            parent.display()
        ))
    })?;

    let canonical_workdir = tokio::fs::canonicalize(workdir)
        .await
        .map_err(|e| CoreError::Io(format!("Workdir canonicalization failed: {e}")))?;

    if !canonical_parent.starts_with(&canonical_workdir) {
        return Err(CoreError::Io(format!(
            "Path traversal denied: {} escapes the working directory {}",
            parent.display(),
            canonical_workdir.display()
        )));
    }
    Ok(())
}

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

        // Validate path is within workdir (prevents path traversal)
        validate_file_path(&full_path, Path::new(&ctx.workdir)).await?;

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

        // Validate parent directory is within workdir (prevents path traversal).
        // We canonicalize the parent because the file itself does not exist yet.
        if let Some(parent) = full_path.parent() {
            validate_parent_path(parent, Path::new(&ctx.workdir)).await?;
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

        // Validate path is within workdir (prevents path traversal)
        validate_file_path(&full_path, Path::new(&ctx.workdir)).await?;

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
