//! # Parser Module
//!
//! Parses `*.r.ts` and `*.r.tsx` files.

mod source_file;
mod diagnostics;

pub use source_file::{SourceFile, SourceKind};
pub use diagnostics::ParseDiagnostics;

/// Parse a Rune source file.
///
/// # Errors
/// Returns an error if the file cannot be parsed.
pub fn parse_file(path: &std::path::Path) -> crate::Result<SourceFile> {
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| crate::ParseError::InvalidExtension(String::new()))?;

    let kind = match extension {
        "r.ts" => SourceKind::TypeScript,
        "r.tsx" => SourceKind::Tsx,
        "rs" => return SourceFile::parse(path, SourceKind::TypeScript),
        _ => {
            return Err(crate::ParseError::InvalidExtension(extension.to_string()).into());
        }
    };

    SourceFile::parse(path, kind).map_err(Into::into)
}

/// Scan a directory for all Rune source files.
pub fn scan_directory(dir: &std::path::Path) -> crate::Result<Vec<std::path::PathBuf>> {
    let mut sources = Vec::new();
    scan_directory_impl(dir, &mut sources)?;
    Ok(sources)
}

#[allow(clippy::unnecessary_wraps)]
fn scan_directory_impl(dir: &std::path::Path, sources: &mut Vec<std::path::PathBuf>) -> crate::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in walkdir::WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if let Some(ext) = path.extension().and_then(std::ffi::OsStr::to_str) {
            if ext == "r.ts" || ext == "r.tsx" {
                sources.push(path.to_path_buf());
            }
        }
    }

    Ok(())
}
