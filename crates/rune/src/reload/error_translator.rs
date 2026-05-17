//! # Error Translator
//!
//! Translates Rust compiler errors back to TypeScript line numbers.

use std::collections::HashMap;
use std::path::Path;

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
        // Try to extract file path and line number from rustc output
        let parts: Vec<&str> = rust_error.split("-->").collect();

        if parts.len() >= 2 {
            let location = parts[1].trim();
            let message = parts[0].trim();

            // Parse location: file:line:col
            let loc_parts: Vec<&str> = location.split(':').collect();
            if loc_parts.len() >= 3 {
                let file = loc_parts[0].trim();
                let line: u32 = loc_parts[1].trim().parse().unwrap_or(1);
                let col: u32 = loc_parts[2].trim().parse().unwrap_or(0);

                let source_file = self
                    .source_map
                    .get(file)
                    .cloned()
                    .unwrap_or_else(|| file.to_string());

                let source_line = self
                    .line_maps
                    .get(file)
                    .and_then(|map| map.get(line as usize - 1).copied())
                    .unwrap_or(line);

                return TranslatedError {
                    file: source_file,
                    line: source_line,
                    column: col,
                    message: self.translate_message(message),
                    original: rust_error.to_string(),
                };
            }
        }

        TranslatedError {
            file: "unknown".to_string(),
            line: 0,
            column: 0,
            message: self.translate_message(rust_error),
            original: rust_error.to_string(),
        }
    }

    /// Translate Rust error messages to TypeScript-focused messages.
    #[must_use]
    fn translate_message(&self, message: &str) -> String {
        let msg = message.trim();

        if msg.contains("cannot move") {
            return "Move error: value was already moved. Use .clone() to explicitly copy."
                .to_string();
        }
        if msg.contains("borrow of moved value") {
            return "Borrow error: value was moved. Consider using a reference or .clone()."
                .to_string();
        }
        if msg.contains("does not implement") {
            return format!("Type error: {}", msg.lines().next().unwrap_or(msg));
        }
        if msg.contains("expected") && msg.contains("found") {
            return self.extract_type_mismatch(msg);
        }
        if msg.contains("integer") && msg.contains("division") {
            return format!(
                "Warning: {} (Integer division produces i32 result, not f64)",
                msg
            );
        }

        msg.to_string()
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
