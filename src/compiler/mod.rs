//! TuiBridge TSX/TS Compiler
//!
//! Transforms React+Ink TSX source files into TuiBridge-compatible JavaScript.

pub mod jsx;
pub mod shim;

use anyhow::{Context, Result};
use std::path::Path;

use jsx::{extract_import_aliases, remove_imports, transform_jsx};

/// Compile TSX/TS source to TuiBridge-compatible JavaScript
pub fn compile_tsx(source: &str, _filename: &str) -> Result<String> {
    let mut result = source.to_string();

    let aliases = extract_import_aliases(&result);
    result = remove_imports(&result);

    if !aliases.is_empty() {
        result = format!("// TuiBridge auto-generated aliases\n{};\n\n{}", aliases, result);
    }

    let before = result.len();
    result = transform_jsx(&result);
    eprintln!("JSX transform: {} chars -> {} chars", before, result.len());
    Ok(result)
}

/// Compile TS/JS source (no JSX transformation)
pub fn compile_ts(source: &str, _filename: &str) -> Result<String> {
    let mut result = source.to_string();

    let aliases = extract_import_aliases(&result);
    result = remove_imports(&result);

    if !aliases.is_empty() {
        result = format!("// TuiBridge auto-generated aliases\n{};\n\n{}", aliases, result);
    }
    Ok(result)
}

/// Compile a file from disk
pub fn compile_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {:?}", path))?;

    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("input.tsx");

    if filename.ends_with(".tsx") || filename.ends_with(".jsx") {
        compile_tsx(&source, filename)
    } else {
        compile_ts(&source, filename)
    }
}

/// Compile and immediately run in rquickjs context
pub fn compile_and_run(source: &str, filename: &str) -> Result<String> {
    let js = compile_tsx(source, filename)?;
    Ok(js)
}

// ===================================================================
// Tests
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_imports() {
        let source = r#"import { useState } from "react";
import { render, Box } from "ink";
render(<Box />);"#;
        let result = compile_tsx(source, "test.tsx").unwrap();
        assert!(!result.contains("from \"react\""));
        assert!(!result.contains("from \"ink\""));
        assert!(result.contains("render"));
    }

    #[test]
    fn test_simple_jsx() {
        let source = r#"<Box />"#;
        let result = compile_tsx(source, "test.tsx").unwrap();
        assert!(result.contains("ink.createElement"));
        assert!(result.contains("\"Box\""));
    }

    #[test]
    fn test_jsx_with_attrs() {
        let source = r#"<Box flexDirection="column" padding={2} />"#;
        let result = compile_tsx(source, "test.tsx").unwrap();
        assert!(result.contains("ink.createElement"));
        assert!(result.contains("flexDirection"));
    }

    #[test]
    fn test_import_aliases() {
        let source = r#"import { useState, useEffect } from "react";
import { Box, Text, render } from "ink";
render(<Box />);"#;
        let result = compile_tsx(source, "test.tsx").unwrap();
        assert!(result.contains("const useState = ink.useState"), "Missing useState alias");
        assert!(result.contains("const Box = ink.Box"), "Missing Box alias");
        assert!(result.contains("const render = ink.render"), "Missing render alias");
    }

    #[test]
    fn test_nested_jsx_transformation() {
        let source = r#"import { Box, Text } from "ink";
render(<Box><Text>Hello</Text></Box>);"#;
        let result = compile_tsx(source, "test.tsx").unwrap();
        assert!(result.contains("ink.createElement(\"Box\","), "Missing Box createElement");
        assert!(result.contains("ink.createElement(\"Text\","), "Missing Text createElement");
    }

    #[test]
    fn test_sibling_elements() {
        let source = r#"<Box><Text>A</Text><Text>B</Text></Box>"#;
        let result = compile_tsx(source, "test.tsx").unwrap();
        eprintln!("Input: {}", source);
        eprintln!("Output:\n{}", result);
        // Verify proper sibling structure with 2 Text elements
        assert!(result.contains("ink.createElement(\"Box\","), "Missing Box");
        assert!(result.contains("ink.createElement(\"Text\","), "Missing Text elements");
        // Should have 2 Text createElement calls
        let count = result.matches("ink.createElement(\"Text\"").count();
        assert_eq!(count, 2, "Expected 2 Text elements, got {}", count);
    }

    #[test]
    fn test_text_with_expression() {
        let source = r#"<Text>Count: {count}</Text>"#;
        let result = compile_tsx(source, "test.tsx").unwrap();
        eprintln!("Input: {}", source);
        eprintln!("Output:\n{}", result);
        assert!(result.contains("ink.createElement(\"Text\","), "Missing Text");
    }
}
