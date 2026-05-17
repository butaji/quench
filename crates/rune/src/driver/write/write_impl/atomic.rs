//! # Atomic File Writer
//!
//! Writes files atomically using temp file + rename pattern.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Atomic file writer - writes to temp file then renames.
pub fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let tmp_path = parent.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));

    let mut file = fs::File::create(&tmp_path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp_path, path)
}

/// Write content to a file, creating directories as needed.
#[allow(dead_code)]
pub fn write_with_dirs(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(path, content)
}
