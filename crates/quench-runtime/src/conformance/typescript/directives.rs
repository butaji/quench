//! TypeScript test case directive parsing
//!
//! Parses comments like `// @target: es2015` from test case sources.
#![allow(unknown_lints, clippy::function_length, renamed_and_removed_lints)]

use std::collections::HashMap;

/// Parsed directives from a TypeScript test case
#[derive(Debug, Default, Clone)]
pub struct Directives {
    /// @target: ES version (es5, es2015, es2016, etc.)
    pub target: Option<String>,
    /// @module: module system (commonjs, amd, umd, system, es2015, etc.)
    pub module: Option<String>,
    /// @jsx: JSX mode (preserve, react, react-native)
    pub jsx: Option<String>,
    /// @noEmit: if true, TypeScript emits nothing
    pub no_emit: bool,
    /// @emitDeclarationOnly: if true, only .d.ts is emitted
    pub emit_declaration_only: bool,
    /// @strict: strict mode flag
    pub strict: Option<bool>,
    /// @filename: markers (multi-file cases)
    pub filenames: Vec<String>,
    /// Raw key-value directives
    pub raw: HashMap<String, String>,
}

impl Directives {
    /// Parse directives from TypeScript source code
        pub fn parse(source: &str) -> Self {
        let mut directives = Self::default();
        
        for line in source.lines() {
            let line = line.trim();
            
            // Match // @key: value or // @key
            if !line.starts_with("//") {
                continue;
            }
            
            let content = line.trim_start_matches("//").trim();
            
            if let Some(stripped) = content.strip_prefix('@') {
                if let Some((key, value)) = stripped.split_once(':') {
                    let key = key.trim();
                    let value = value.trim();
                    
                    match key {
                        "target" => directives.target = Some(value.to_string()),
                        "module" => directives.module = Some(value.to_string()),
                        "jsx" => directives.jsx = Some(value.to_string()),
                        "strict" => directives.strict = Some(value == "true"),
                        _ => { directives.raw.insert(key.to_string(), value.to_string()); }
                    }
                } else {
                    // No value, treat as boolean flag
                    match stripped.trim() {
                        "noEmit" => directives.no_emit = true,
                        "emitDeclarationOnly" => directives.emit_declaration_only = true,
                        _ => { directives.raw.insert(stripped.trim().to_string(), String::new()); }
                    }
                }
            }
            
            // Also parse // @filename: name
            if let Some(name) = content.strip_prefix("@filename:") {
                directives.filenames.push(name.trim().to_string());
            }
        }
        
        directives
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_target() {
        let source = "// @target: es2015";
        let dirs = Directives::parse(source);
        assert_eq!(dirs.target, Some("es2015".to_string()));
    }

    #[test]
    fn test_parse_module() {
        let source = "// @module: commonjs";
        let dirs = Directives::parse(source);
        assert_eq!(dirs.module, Some("commonjs".to_string()));
    }

    #[test]
    fn test_parse_jsx() {
        let source = "// @jsx: preserve";
        let dirs = Directives::parse(source);
        assert_eq!(dirs.jsx, Some("preserve".to_string()));
    }

    #[test]
    fn test_parse_no_emit() {
        let source = "// @noEmit";
        let dirs = Directives::parse(source);
        assert!(dirs.no_emit);
    }

    #[test]
    fn test_parse_emit_declaration_only() {
        let source = "// @emitDeclarationOnly";
        let dirs = Directives::parse(source);
        assert!(dirs.emit_declaration_only);
    }

    #[test]
    fn test_parse_strict() {
        let source = "// @strict: true";
        let dirs = Directives::parse(source);
        assert_eq!(dirs.strict, Some(true));
    }

    #[test]
    fn test_parse_filename() {
        let source = "// @filename: a.ts\nexport const x = 1;";
        let dirs = Directives::parse(source);
        assert_eq!(dirs.filenames, vec!["a.ts"]);
    }

    #[test]
    fn test_parse_multiple() {
        let source = r#"// @target: es2015
// @module: commonjs
// @jsx: preserve
// @strict: false
const x: number = 1;
"#;
        let dirs = Directives::parse(source);
        assert_eq!(dirs.target, Some("es2015".to_string()));
        assert_eq!(dirs.module, Some("commonjs".to_string()));
        assert_eq!(dirs.jsx, Some("preserve".to_string()));
        assert_eq!(dirs.strict, Some(false));
    }

    #[test]
    fn test_parse_empty_source() {
        let source = "const x: number = 1;";
        let dirs = Directives::parse(source);
        assert!(dirs.target.is_none());
        assert!(!dirs.no_emit);
    }
}
