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
        let parts: Vec<&str> = rust_error.split("--> ").collect();
        if parts.len() < 2 {
            return None;
        }

        let location = parts[1].trim();
        let message = parts[0].trim().to_string();

        let loc_parts: Vec<&str> = location.split(':').collect();
        if loc_parts.len() < 3 {
            return None;
        }

        let file = loc_parts[0].trim().to_string();
        let line: u32 = loc_parts[1].trim().parse().unwrap_or(1);
        let column: u32 = loc_parts[2].trim().parse().unwrap_or(0);

        Some(ParsedLocation {
            file,
            line,
            column,
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
}
