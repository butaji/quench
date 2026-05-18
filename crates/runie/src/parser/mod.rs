//! # Parser Module
//!
//! Parses `*.r.ts` and `*.r.tsx` files using SWC.

mod diagnostics;
#[cfg(test)]
mod integration_tests;
mod source_file;
pub mod swc_parser;
#[cfg(test)]
mod tests;

pub use diagnostics::ParseDiagnostics;
pub use source_file::{parse_file_from_str, SourceFile, SourceKind};

/// Check if a file path is a Rune source file (ending in .r.ts or .r.tsx).
pub fn is_rune_file(path: &std::path::Path) -> Option<(bool, SourceKind)> {
    let file_name = path.file_name()?.to_str()?;
    if file_name.ends_with(".r.ts") {
        Some((true, SourceKind::TypeScript))
    } else if file_name.ends_with(".r.tsx") {
        Some((true, SourceKind::Tsx))
    } else {
        None
    }
}

/// Parse a Rune source file using SWC.
///
/// # Errors
/// Returns an error if the file cannot be parsed.
pub fn parse_file(path: &std::path::Path) -> crate::Result<SourceFile> {
    // Check if it's a Rune file (.r.ts or .r.tsx)
    let Some((_, kind)) = is_rune_file(path) else {
        return Err(crate::ParseError::InvalidExtension(
            path.extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("unknown")
                .to_string(),
        )
        .into());
    };

    SourceFile::parse(path, kind).map_err(Into::into)
}

/// Scan a directory for all Rune source files.
pub fn scan_directory(dir: &std::path::Path) -> crate::Result<Vec<std::path::PathBuf>> {
    let mut sources = Vec::new();
    scan_directory_impl(dir, &mut sources)?;
    Ok(sources)
}

fn scan_directory_impl(
    dir: &std::path::Path,
    sources: &mut Vec<std::path::PathBuf>,
) -> crate::Result<()> {
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

        if is_rune_file(path).is_some() {
            sources.push(path.to_path_buf());
        }
    }

    Ok(())
}
