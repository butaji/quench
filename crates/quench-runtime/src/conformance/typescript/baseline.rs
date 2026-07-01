//! TypeScript baseline file lookup and parsing
//!
//! TypeScript emits JavaScript baselines alongside test cases. This module
//! handles finding the correct baseline and extracting the JS code from it.

use std::path::{Path, PathBuf};

/// Find the baseline JS file for a TypeScript test case.
///
/// TypeScript emits baselines to `tests/typescript/tests/baselines/reference/`.
///
/// The baseline file is named after the test case with `.js` extension.
/// For configuration-specific baselines, it may include suffixes like:
/// - `test.es2015.js`
/// - `test.es2015.commonjs.js`
pub fn find_baseline(ts_path: &Path) -> Option<String> {
    // Baselines live at tests/typescript/tests/baselines/reference/
    // The conformance root is .../tests/cases/conformance, so go up two levels.
    let conformance_root = ts_path
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "conformance"))?;

    let ts_stem = ts_path.file_stem()?.to_string_lossy();
    let baselines_root = conformance_root
        .parent()?
        .parent()?
        .join("baselines")
        .join("reference");

    let patterns: Vec<PathBuf> = vec![
        baselines_root.join(format!("{}.js", ts_stem)),
        baselines_root.join(format!("{}.es2015.js", ts_stem)),
        baselines_root.join(format!("{}.es2015.commonjs.js", ts_stem)),
        baselines_root.join(format!("{}.es2015.esm.js", ts_stem)),
        baselines_root.join(format!("{}.commonjs.js", ts_stem)),
        baselines_root.join(format!("{}.es2015.react.js", ts_stem)),
    ];

    for pattern in &patterns {
        if pattern.exists() {
            if let Ok(content) = std::fs::read_to_string(pattern) {
                return Some(content);
            }
        }
    }

    None
}

/// Extract JavaScript code from a TypeScript baseline file.
///
/// TypeScript baseline files have a specific format:
///
/// ```text
/// //// [tests/cases/conformance/path/to/test.ts] ////
///
/// //// [test.ts]
/// // original TypeScript source (in comments)
/// var x: number = 1;
///
/// //// [test.js]
/// "use strict";
/// var x = 1;
/// ```
///
/// This function extracts the JavaScript section.
pub fn extract_js_from_baseline(baseline: &str) -> Result<String, String> {
    // Find the last //// [*.js] marker
    let marker_pattern = "//// [";
    let js_marker = ".js]";
    
    // Find all JS markers
    let mut js_start = None;
    for (i, line) in baseline.lines().enumerate() {
        if line.contains(marker_pattern) && line.contains(js_marker) && !line.contains(".ts]") {
            js_start = Some(i);
        }
    }
    
    let start = match js_start {
        Some(idx) => idx + 1, // Skip the marker line
        None => return Err("No JavaScript section found in baseline".to_string()),
    };
    
    // Collect lines until we hit another marker or EOF
    let mut js_lines = Vec::new();
    for line in baseline.lines().skip(start) {
        // Stop at the next marker
        if line.starts_with("////") {
            break;
        }
        js_lines.push(line);
    }
    
    // Remove leading/trailing empty lines
    while js_lines.first().is_some_and(|l| l.trim().is_empty()) {
        js_lines.remove(0);
    }
    while js_lines.last().is_some_and(|l| l.trim().is_empty()) {
        js_lines.pop();
    }
    
    Ok(js_lines.join("\n"))
}

/// Split a source file into multiple units based on @filename markers.
///
/// TypeScript test cases can contain `// @filename:` markers that split
/// a single file into multiple logical files for multi-file test cases.
///
/// ```typescript
/// // @filename: a.ts
/// export const x = 1;
///
/// // @filename: b.ts
/// import { x } from "./a";
/// console.log(x);
/// ```
pub fn split_units(source: &str, _directives: &super::directives::Directives) -> Vec<(String, String)> {
    let mut units = Vec::new();
    let mut current_name = String::new();
    let mut current_lines = Vec::new();
    let mut in_unit = false;
    
    for line in source.lines() {
        // Check for @filename marker
        let trimmed = line.trim();
        if trimmed.starts_with("// @filename:") || trimmed.starts_with("//@filename:") {
            // Save previous unit
            if in_unit && !current_lines.is_empty() {
                units.push((current_name.clone(), current_lines.join("\n")));
            }
            
            // Start new unit
            current_name = trimmed
                .trim_start_matches("// @filename:")
                .trim_start_matches("//@filename:")
                .trim()
                .to_string();
            current_lines.clear();
            in_unit = true;
        } else if in_unit {
            current_lines.push(line);
        }
    }
    
    // Don't forget the last unit
    if in_unit && !current_lines.is_empty() {
        units.push((current_name, current_lines.join("\n")));
    }
    
    // If no units found, treat the whole source as one unit
    if units.is_empty() {
        units.push(("main.ts".to_string(), source.to_string()));
    }
    
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_js_from_baseline() {
        let baseline = r#"//// [tests/cases/conformance/path/to/test.ts] ////

//// [test.ts]
const x: number = 1;

//// [test.js]
"use strict";
var x = 1;
"#;
        let result = extract_js_from_baseline(baseline).unwrap();
        assert!(result.contains("\"use strict\""));
        assert!(result.contains("var x = 1"));
    }

    #[test]
    fn test_split_units() {
        let source = r#"// @filename: a.ts
export const x = 1;

// @filename: b.ts
import { x } from "./a";
console.log(x);
"#;
        use super::super::directives::Directives;
        let units = split_units(source, &Directives::default());
        
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].0, "a.ts");
        assert!(units[0].1.contains("export const x"));
        assert_eq!(units[1].0, "b.ts");
        assert!(units[1].1.contains("import"));
    }

    #[test]
    fn test_split_units_no_markers() {
        let source = "const x: number = 1;";
        use super::super::directives::Directives;
        let units = split_units(source, &Directives::default());
        
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].0, "main.ts");
        assert!(units[0].1.contains("const x"));
    }
}
