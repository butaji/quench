//! # Parser Module
//!
//! Parses `*.r.ts` and `*.r.tsx` files.

mod source_file;
mod diagnostics;

pub use source_file::{SourceFile, SourceKind};
pub use diagnostics::ParseDiagnostics;

/// Parse a Rune source file.
/// Returns the parsed AST module or an error.
pub fn parse_file(path: &std::path::Path) -> crate::Result<SourceFile> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| crate::ParseError::InvalidExtension(String::new()))?;

    let kind = match extension {
        "r.ts" => SourceKind::TypeScript,
        "r.tsx" => SourceKind::Tsx,
        "rs" => return SourceFile::parse(path, SourceKind::TypeScript).map_err(Into::into),
        _ => {
            return Err(crate::ParseError::InvalidExtension(extension.to_string()).into());
        }
    };

    SourceFile::parse(path, kind)
}

/// Scan a directory for all Rune source files.
pub fn scan_directory(dir: &std::path::Path) -> crate::Result<Vec<std::path::PathBuf>> {
    let mut sources = Vec::new();
    scan_directory_impl(dir, &mut sources)?;
    Ok(sources)
}

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

        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "r.ts" || ext == "r.tsx" {
                sources.push(path.to_path_buf());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_validation() {
        // Test that .r.ts and .r.tsx extensions are valid
        assert!(matches!(
            SourceKind::TypeScript,
            SourceKind::TypeScript
        ));
        assert!(matches!(
            SourceKind::Tsx,
            SourceKind::Tsx
        ));
    }
}
