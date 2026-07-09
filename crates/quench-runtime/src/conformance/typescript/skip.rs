//! TypeScript conformance test skip rules
//!
//! Determines which test cases should be skipped based on their
//! directives and file paths.
#![allow(unknown_lints, clippy::function_length, clippy::complexity, renamed_and_removed_lints)]

use std::path::Path;

use super::directives::Directives;

/// Determine if a test case should be skipped.
///
/// Returns `Some(reason)` if the case should be skipped, or `None` if it should run.
pub fn should_skip(path: &Path, directives: &Directives) -> Option<String> {
    // Skip if @noEmit is set
    if directives.no_emit {
        return Some("@noEmit: true - TypeScript emits nothing".to_string());
    }

    // Skip if @emitDeclarationOnly is set
    if directives.emit_declaration_only {
        return Some("@emitDeclarationOnly: true - only .d.ts emitted".to_string());
    }

    // Skip unsupported module systems
    if let Some(ref module) = directives.module {
        match module.as_str() {
            "amd" | "umd" | "system" | "node16" | "nodenext" | "none" => {
                return Some(format!("@module: {} - unsupported module system", module));
            }
            _ => {}
        }
    }

    // Skip JSX react cases (we don't have React runtime)
    if let Some(ref jsx) = directives.jsx {
        if jsx == "react" || jsx == "react-native" {
            return Some(format!("@jsx: {} - React runtime not available", jsx));
        }
    }

    // Skip cases in specific directories
    let path_str = path.to_string_lossy().to_lowercase();
    
    let skip_dirs = [
        "types/",
        "interfaces/",
        "symbols/",
        "namespaces/",
        "decorators/",
        "ambient/",
        "constEnums/",
        "declarationEmit/",
        "additionalChecks/",
    ];
    
    for dir in &skip_dirs {
        if path_str.contains(dir) {
            return Some(format!("In skipped directory: {}", dir.trim_end_matches("/")));
        }
    }

    // Skip .errors.txt files (already filtered at the caller)
    // Skip .d.ts files
    if path.extension().is_some_and(|ext| ext == "d.ts") {
        return Some("Type declaration file (.d.ts)".to_string());
    }

    // Skip .tsx files for now (JSX support)
    if path.extension().is_some_and(|ext| ext == "tsx") {
        return Some(".tsx file - JSX support not yet implemented".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skip_no_emit() {
        let directives = Directives {
            no_emit: true,
            ..Default::default()
        };
        let reason = should_skip(Path::new("test.ts"), &directives);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("@noEmit"));
    }

    #[test]
    fn test_skip_emit_declaration_only() {
        let directives = Directives {
            emit_declaration_only: true,
            ..Default::default()
        };
        let reason = should_skip(Path::new("test.ts"), &directives);
        assert!(reason.is_some());
    }

    #[test]
    fn test_skip_amd_module() {
        let directives = Directives {
            module: Some("amd".to_string()),
            ..Default::default()
        };
        let reason = should_skip(Path::new("test.ts"), &directives);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("amd"));
    }

    #[test]
    fn test_skip_react_jsx() {
        let directives = Directives {
            jsx: Some("react".to_string()),
            ..Default::default()
        };
        let reason = should_skip(Path::new("test.ts"), &directives);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("react"));
    }

    #[test]
    fn test_skip_types_directory() {
        let directives = Directives::default();
        let reason = should_skip(Path::new("tests/types/test.ts"), &directives);
        assert!(reason.is_some());
    }

    #[test]
    fn test_skip_tsx() {
        let directives = Directives::default();
        let reason = should_skip(Path::new("test.tsx"), &directives);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("JSX"));
    }

    #[test]
    fn test_no_skip_normal() {
        let directives = Directives::default();
        let reason = should_skip(Path::new("expressions/binaryOperators/test.ts"), &directives);
        assert!(reason.is_none());
    }
}
