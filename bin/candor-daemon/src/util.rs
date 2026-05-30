/// Shared utility functions for the candor daemon.

use std::path::PathBuf;

/// Find a binary on PATH.
///
/// Checks each directory in `$PATH` for the named executable.
/// On Windows, also checks with `.exe` appended.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").as_ref().and_then(|paths| {
        std::env::split_paths(paths).find_map(|dir| {
            let full = dir.join(name);
            if full.is_file() {
                Some(full)
            } else {
                let with_ext = dir.join(format!("{}.exe", name));
                if with_ext.is_file() {
                    Some(with_ext)
                } else {
                    None
                }
            }
        })
    })
}
