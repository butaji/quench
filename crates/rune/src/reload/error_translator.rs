//! # Error Translator
//!
//! Translates Rust compiler errors back to TypeScript line numbers.

use std::collections::HashMap;
use std::path::Path;

/// Parsed location from rustc output.
struct ParsedLocation {
    file: String,
    line: u32,
    column: u32,
    message: String,
}

/// A translated error with TypeScript location.
#[derive(Debug, Clone)]
pub struct TranslatedError {
    /// File path
    pub file: String,
    /// Line number in source file
    pub line: u32,
    /// Column number
    pub column: u32,
    /// Error message
    pub message: String,
    /// Original Rust error
    pub original: String,
}

impl std::fmt::Display for TranslatedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.file, self.line, self.column, self.message
        )
    }
}

/// Translates Rust errors to TypeScript source locations.
#[derive(Default)]
pub struct ErrorTranslator {
    /// Source map: generated file -> original source file
    source_map: HashMap<String, String>,
    /// Line map: generated line -> source line (per file)
    line_maps: HashMap<String, Vec<u32>>,
}

impl ErrorTranslator {
    /// Create a new error translator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            source_map: HashMap::new(),
            line_maps: HashMap::new(),
        }
    }

    /// Register a mapping from generated file to source file.
    pub fn register_mapping(&mut self, generated: &Path, source: &Path) {
        let gen_str = generated.to_string_lossy().to_string();
        let src_str = source.to_string_lossy().to_string();
        self.source_map.insert(gen_str, src_str);
    }

    /// Register a line mapping for a file.
    pub fn register_line_map(&mut self, file: &str, lines: Vec<u32>) {
        self.line_maps.insert(file.to_string(), lines);
    }

    /// Translate a Rust error message to a TypeScript location.
    #[must_use]
    pub fn translate(&self, rust_error: &str) -> TranslatedError {
        if let Some(parsed) = self.parse_rustc_location(rust_error) {
            let source_file = self
                .source_map
                .get(parsed.file.as_str())
                .cloned()
                .unwrap_or_else(|| parsed.file.clone());

            let source_line = self
                .line_maps
                .get(parsed.file.as_str())
                .and_then(|map| map.get(parsed.line as usize - 1).copied())
                .unwrap_or(parsed.line);

            TranslatedError {
                file: source_file,
                line: source_line,
                column: parsed.column,
                message: self.translate_message(&parsed.message),
                original: rust_error.to_string(),
            }
        } else {
            TranslatedError {
                file: "unknown".to_string(),
                line: 0,
                column: 0,
                message: self.translate_message(rust_error),
                original: rust_error.to_string(),
            }
        }
    }

    /// Parse location info from rustc error output.
    fn parse_rustc_location(&self, rust_error: &str) -> Option<ParsedLocation> {
        self.try_parse_multiline_format(rust_error)
            .or_else(|| self.try_parse_single_line_format(rust_error))
    }

    fn try_parse_multiline_format(&self, rust_error: &str) -> Option<ParsedLocation> {
        let idx = rust_error.find("-->")?;
        let before_arrow = rust_error[..idx].trim();
        let after_arrow = rust_error[idx + 3..].trim();

        let message = before_arrow.lines().last().unwrap_or(before_arrow).to_string();
        self.parse_location_parts(after_arrow, message)
    }

    fn try_parse_single_line_format(&self, rust_error: &str) -> Option<ParsedLocation> {
        let parts: Vec<&str> = rust_error.split("--> ").collect();
        if parts.len() < 2 {
            return None;
        }
        let location = parts[1].trim();
        let message = parts[0].trim().to_string();
        self.parse_location_parts(location, message)
    }

    fn parse_location_parts(&self, location: &str, message: String) -> Option<ParsedLocation> {
        let loc_parts: Vec<&str> = location.split(':').collect();
        if loc_parts.len() < 3 {
            return None;
        }
        Some(ParsedLocation {
            file: loc_parts[0].trim().to_string(),
            line: loc_parts[1].trim().parse().unwrap_or(1),
            column: loc_parts[2].trim().parse().unwrap_or(0),
            message,
        })
    }

    /// Translate Rust error messages to TypeScript-focused messages.
    #[must_use]
    fn translate_message(&self, message: &str) -> String {
        let msg = message.trim();
        self.try_translate_move_error(msg)
            .or_else(|| self.try_translate_borrow_error(msg))
            .or_else(|| self.try_translate_trait_error(msg))
            .or_else(|| self.try_translate_type_mismatch(msg))
            .or_else(|| self.try_translate_integer_division_warning(msg))
            .unwrap_or_else(|| msg.to_string())
    }

    fn try_translate_move_error(&self, msg: &str) -> Option<String> {
        if msg.contains("cannot move") {
            Some("Move error: value was already moved. Use .clone() to explicitly copy.".into())
        } else {
            None
        }
    }

    fn try_translate_borrow_error(&self, msg: &str) -> Option<String> {
        if msg.contains("borrow of moved value") {
            Some("Borrow error: value was moved. Consider using a reference or .clone().".into())
        } else {
            None
        }
    }

    fn try_translate_trait_error(&self, msg: &str) -> Option<String> {
        if msg.contains("does not implement") {
            Some(format!("Type error: {}", msg.lines().next().unwrap_or(msg)))
        } else {
            None
        }
    }

    fn try_translate_type_mismatch(&self, msg: &str) -> Option<String> {
        if msg.contains("expected") && msg.contains("found") {
            Some(self.extract_type_mismatch(msg))
        } else {
            None
        }
    }

    fn try_translate_integer_division_warning(&self, msg: &str) -> Option<String> {
        if msg.contains("integer") && msg.contains("division") {
            Some(format!(
                "Warning: {} (Integer division produces i32 result, not f64)",
                msg
            ))
        } else {
            None
        }
    }

    /// Extract type mismatch information from error message.
    #[must_use]
    fn extract_type_mismatch(&self, msg: &str) -> String {
        if let Some(exp_idx) = msg.find("expected ") {
            let exp_rest = &msg[exp_idx + 9..];
            let exp_end = exp_rest.find(' ').unwrap_or(exp_rest.len());
            let expected = &exp_rest[..exp_end];

            if let Some(found_idx) = msg.find("found ") {
                let found_rest = &msg[found_idx + 6..];
                let found_end = found_rest.find(' ').unwrap_or(found_rest.len());
                let found = &found_rest[..found_end];

                return format!("Type mismatch: expected `{}`, found `{}`", expected, found);
            }
        }
        msg.to_string()
    }

    /// Translate all errors in a rustc output.
    #[must_use]
    pub fn translate_all(&self, output: &str) -> Vec<TranslatedError> {
        output
            .lines()
            .filter(|line| line.starts_with("error") || line.starts_with("warning"))
            .map(|line| self.translate(line))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_translation() {
        let mut translator = ErrorTranslator::new();
        translator.register_line_map("test.rs", vec![1, 5, 10, 15]);

        let error = "error[E0382]: borrow of moved value: `x` --> src/main.rs:10:5";
        let result = translator.translate(error);

        assert_eq!(result.line, 10);
    }

    #[test]
    fn test_parse_multiline_format() {
        let translator = ErrorTranslator::new();
        let input = "error[E0382]: borrow of moved value: `s`\n  --> src/main.r.ts:15:10";
        let result = translator.parse_rustc_location(input);

        assert!(result.is_some());
        let loc = result.unwrap();
        assert_eq!(loc.file, "src/main.r.ts");
        assert_eq!(loc.line, 15);
        assert_eq!(loc.column, 10);
        assert!(loc.message.contains("borrow of moved value"));
    }

    #[test]
    fn test_parse_single_line_format() {
        let translator = ErrorTranslator::new();
        let input = "error[E0505]: cannot move out --> lib.rs:42:8";
        let result = translator.parse_rustc_location(input);

        assert!(result.is_some());
        let loc = result.unwrap();
        assert_eq!(loc.file, "lib.rs");
        assert_eq!(loc.line, 42);
        assert_eq!(loc.column, 8);
    }

    #[test]
    fn test_parse_location_with_complex_path() {
        let translator = ErrorTranslator::new();
        let input = "error[E0601]: --> crates/app/src/main.r.ts:5:3";
        let result = translator.parse_rustc_location(input);

        assert!(result.is_some());
        let loc = result.unwrap();
        assert_eq!(loc.file, "crates/app/src/main.r.ts");
        assert_eq!(loc.line, 5);
    }

    #[test]
    fn test_parse_invalid_format_returns_none() {
        let translator = ErrorTranslator::new();
        let input = "some random text without arrow";
        let result = translator.parse_rustc_location(input);

        assert!(result.is_none());
    }

    #[test]
    fn test_translate_move_error() {
        let translator = ErrorTranslator::new();
        let error = "error[E0382]: cannot move out --> file.rs:1:1";
        let result = translator.translate(error);

        assert!(result.message.contains("Move error"));
        assert!(result.message.contains(".clone()"));
    }

    #[test]
    fn test_translate_borrow_error() {
        let translator = ErrorTranslator::new();
        let error = "error[E0382]: borrow of moved value --> file.rs:1:1";
        let result = translator.translate(error);

        assert!(result.message.contains("Borrow error"));
    }

    #[test]
    fn test_translate_trait_error() {
        let translator = ErrorTranslator::new();
        let error = "error[E0277]: Foo does not implement Bar --> file.rs:1:1";
        let result = translator.translate(error);

        assert!(result.message.contains("Type error"));
    }

    #[test]
    fn test_translate_type_mismatch() {
        let translator = ErrorTranslator::new();
        let error = "error[E0308]: expected i32 found String --> file.rs:1:1";
        let result = translator.translate(error);

        assert!(result.message.contains("Type mismatch"));
        assert!(result.message.contains("i32"));
        assert!(result.message.contains("String"));
    }

    #[test]
    fn test_translate_integer_division_warning() {
        let translator = ErrorTranslator::new();
        let error = "warning: integer division --> file.rs:1:1";
        let result = translator.translate(error);

        assert!(result.message.contains("Integer division"));
        assert!(result.message.contains("i32"));
    }

    #[test]
    fn test_translate_all_filters_errors_and_warnings() {
        let translator = ErrorTranslator::new();
        let output = "error[E0001]: first error\nnote: some note\nwarning[E0002]: some warning\ninfo: some info";
        let results = translator.translate_all(output);

        assert_eq!(results.len(), 2);
        assert!(results[0].message.contains("first error"));
        assert!(results[1].message.contains("some warning"));
    }

    #[test]
    fn test_source_mapping() {
        let mut translator = ErrorTranslator::new();
        translator.register_mapping(
            Path::new("target/generated/main.rs"),
            Path::new("src/main.r.ts"),
        );

        let error = "error[E0001]: test --> target/generated/main.rs:10:5";
        let result = translator.translate(error);

        assert_eq!(result.file, "src/main.r.ts");
    }

    #[test]
    fn test_line_mapping() {
        let mut translator = ErrorTranslator::new();
        translator.register_line_map("test.rs", vec![1, 5, 10, 15, 20]);

        let error = "error[E0001]: test --> test.rs:3:1";
        let result = translator.translate(error);

        assert_eq!(result.line, 10);
    }

    #[test]
    fn test_unknown_file_when_no_mapping() {
        let translator = ErrorTranslator::new();
        let error = "error[E0001]: test --> unknown.rs:10:5";
        let result = translator.translate(error);

        assert_eq!(result.file, "unknown.rs");
        assert_eq!(result.line, 10);
    }
}
