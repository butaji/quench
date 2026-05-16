//! # Parser Module - SWC Integration
//!
//! Parses `*.r.ts` and `*.r.tsx` files into valid TypeScript AST.
//! Enforces file extension validation and UTF-8 encoding.

mod source_file;
mod diagnostics;

pub use source_file::{SourceFile, SourceKind};
pub use diagnostics::ParseDiagnostics;

/// Parse a Rune source file using SWC.
/// Returns the parsed AST module or an error.
pub fn parse_file(path: &std::path::Path) -> crate::Result<SourceFile> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| crate::ParseError::InvalidExtension(String::new()))?;

    let kind = match extension {
        "r.ts" => SourceKind::TypeScript,
        "r.tsx" => SourceKind::Tsx,
        "rs" => return Ok(SourceFile::from_rust_file(path)?),
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
        assert!(matches!(
            parse_file(std::path::Path::new("test.r.ts")),
            Ok(SourceFile {
                kind: SourceKind::TypeScript,
                ..
            })
        ));

        assert!(matches!(
            parse_file(std::path::Path::new("test.r.tsx")),
            Ok(SourceFile {
                kind: SourceKind::Tsx,
                ..
            })
        ));

        assert!(parse_file(std::path::Path::new("test.ts")).is_err());
        assert!(parse_file(std::path::Path::new("test.js")).is_err());
    }
}
