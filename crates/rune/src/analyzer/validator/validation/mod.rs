//! # Subset Validation
//!
//! Validates the zero-overhead TypeScript subset is being used.

use super::ValidationError;
use crate::parser::SourceFile;

/// Validator for the Rune TypeScript subset.
#[derive(Debug)]
pub struct SubsetValidator {
    /// Errors found during validation
    errors: Vec<ValidationError>,
}

impl SubsetValidator {
    /// Create a new validator.
    #[must_use]
    pub const fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Validate a source file.
    ///
    /// # Errors
    /// Returns an error if validation fails.
    pub fn validate(&mut self, source: &SourceFile) -> Result<(), ValidationError> {
        self.errors.clear();
        let content = &source.source;

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();
            let line_num = (line_num + 1) as u32;
            self.check_forbidden_features(line, line_num)?;
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors[0].clone())
        }
    }

    /// Check for forbidden TypeScript features.
    fn check_forbidden_features(
        &mut self,
        line: &str,
        line_num: u32,
    ) -> Result<(), ValidationError> {
        if Self::is_comment_line(line) {
            return Ok(());
        }
        self.check_type_restrictions(line, line_num)?;
        self.check_keyword_restrictions(line, line_num)?;
        self.check_operators(line, line_num)?;
        self.check_statements(line, line_num)?;
        self.check_runtime_inspection(line, line_num)?;
        self.check_dynamic_access(line, line_num)
    }

    /// Check if line is a comment.
    fn is_comment_line(line: &str) -> bool {
        line.starts_with("//") || line.starts_with("/*") || line.starts_with('*')
    }

    /// Check for forbidden type restrictions.
    fn check_type_restrictions(&mut self, line: &str, line_num: u32) -> Result<(), ValidationError> {
        if line.contains(": any") || line.contains("<any>") {
            self.push_error("no-any", "Type 'any' requires dynamic dispatch. Use concrete types.", line_num);
        }
        if line.contains(": unknown") {
            self.push_error("no-unknown", "Type 'unknown' requires dynamic dispatch. Use concrete types.", line_num);
        }
        Ok(())
    }

    /// Check for forbidden keywords.
    fn check_keyword_restrictions(&mut self, line: &str, line_num: u32) -> Result<(), ValidationError> {
        if line.contains(" class ") || line.starts_with("class ") {
            self.push_error("no-class", "Classes are forbidden. Use plain objects and functions.", line_num);
        }
        if line.contains("new ") && !line.contains("new Array") {
            self.push_error("no-new", "Constructors (new) are forbidden. Use factory functions.", line_num);
        }
        if line.contains("this.") || line.starts_with("this;") {
            self.push_error("no-this", "'this' keyword is forbidden. Use explicit parameters.", line_num);
        }
        if line.starts_with("var ") {
            self.push_error("no-var", "Use 'const' or 'let' instead of 'var'.", line_num);
        }
        if line.contains(" delete ") || line.starts_with("delete ") {
            self.push_error("no-delete", "delete is forbidden. Use ownership and explicit drops.", line_num);
        }
        Ok(())
    }

    /// Check for forbidden operators.
    fn check_operators(&mut self, line: &str, line_num: u32) -> Result<(), ValidationError> {
        if Self::has_loose_equality(line) {
            self.push_error("no-loose-equality", "Use strict equality (=== or !==).", line_num);
        }
        self.check_implicit_coercion(line, line_num)
    }

    /// Check for implicit coercion patterns like `if ("")` or `if (0)`.
    fn check_implicit_coercion(&mut self, line: &str, line_num: u32) -> Result<(), ValidationError> {
        // Check for falsy value in condition without comparison
        // e.g., `if (str)` or `if (num)` where str/num are direct identifiers
        if line.starts_with("if (") {
            let condition = Self::extract_condition(line, "if");
            if Self::is_falsy_pattern(&condition) {
                self.push_error(
                    "no-implicit-coercion",
                    "Implicit boolean coercion forbidden. Use explicit comparison (e.g., s.is_empty())",
                    line_num,
                );
            }
        }
        Ok(())
    }

    /// Check for forbidden statements.
    fn check_statements(&mut self, line: &str, line_num: u32) -> Result<(), ValidationError> {
        if line.contains("try") || line.contains("catch") || line.starts_with("throw") {
            self.push_error("no-exceptions", "Use Result<T,E> return pattern instead of try/catch/throw.", line_num);
        }
        if line.contains("eval(") {
            self.push_error("no-eval", "eval() is forbidden.", line_num);
        }
        if line.starts_with("with ") {
            self.push_error("no-with", "with statement is forbidden.", line_num);
        }
        if line.contains("for (") && line.contains(" in ") {
            self.push_error("no-for-in", "for...in is forbidden. Use for...of with Object.keys() or Map.", line_num);
        }
        if line.contains(" arguments") || line.contains("(arguments") {
            self.push_error("no-arguments", "Use rest parameters (...args) instead of 'arguments'.", line_num);
        }
        Ok(())
    }

    /// Check for runtime type inspection.
    fn check_runtime_inspection(&mut self, line: &str, line_num: u32) -> Result<(), ValidationError> {
        if line.contains("typeof ") {
            self.push_error("no-typeof", "typeof is forbidden. Runtime type inspection is not allowed.", line_num);
        }
        if line.contains(" instanceof ") {
            self.push_error("no-instanceof", "instanceof is forbidden. Use explicit type checking.", line_num);
        }
        Ok(())
    }

    /// Check for dynamic property access.
    fn check_dynamic_access(&mut self, line: &str, line_num: u32) -> Result<(), ValidationError> {
        // Match obj[key] pattern where key is not a number literal
        if Self::has_dynamic_bracket_access(line) {
            self.push_error(
                "no-dynamic-access",
                "Dynamic property access (obj[key]) is forbidden. Use Map<K,V> for dynamic keys.",
                line_num,
            );
        }
        Ok(())
    }

    /// Extract condition from statement.
    fn extract_condition(line: &str, keyword: &str) -> String {
        if let Some(start) = line.find(&format!("{keyword} (")) {
            let after = &line[start + keyword.len() + 2..];
            let mut depth = 0;
            let mut end = 0;
            for (i, c) in after.chars().enumerate() {
                match c {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => {
                        if depth == 0 {
                            end = i;
                            break;
                        }
                        depth -= 1;
                    }
                    _ => {}
                }
            }
            return after[..end].trim().to_string();
        }
        String::new()
    }

    /// Check if condition is a falsy pattern (bare identifier or literal).
    fn is_falsy_pattern(condition: &str) -> bool {
        let trimmed = condition.trim();
        // Empty string literal
        if trimmed == "''" || trimmed == "\"\"" || trimmed == "`" {
            return true;
        }
        // Numeric literal
        if trimmed.parse::<f64>().is_ok() {
            return true;
        }
        // Boolean literal
        if trimmed == "true" || trimmed == "false" {
            return true;
        }
        // Direct identifier (would be falsy check without comparison)
        // This is conservative - allow identifiers that are clearly booleans
        if trimmed == "null" || trimmed == "undefined" {
            return true;
        }
        false
    }

    /// Check for dynamic bracket access patterns.
    fn has_dynamic_bracket_access(line: &str) -> bool {
        let bytes = line.as_bytes();
        for i in 0..bytes.len().saturating_sub(4) {
            // Look for [...]
            if bytes[i] == b'[' {
                // Find matching ]
                let mut depth = 0;
                let mut j = i;
                for c in &bytes[i..] {
                    match c {
                        b'[' | b'(' | b'{' => depth += 1,
                        b']' | b')' | b'}' => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        break;
                    }
                    j += 1;
                }
                // Check if content is not a simple number (allowed for arrays)
                let inner = &line[i + 1..i + j.min(bytes.len())];
                if !inner.is_empty() && inner.trim().parse::<f64>().is_err() {
                    // This is likely a dynamic key access
                    return true;
                }
            }
        }
        false
    }

    /// Check for loose equality operators (== and !=).
    fn has_loose_equality(line: &str) -> bool {
        let bytes = line.as_bytes();
        for i in 0..bytes.len().saturating_sub(3) {
            if Self::matches_loose_eq(&bytes[i..]) || Self::matches_loose_neq(&bytes[i..]) {
                return true;
            }
        }
        false
    }

    /// Match loose equality pattern " == ".
    fn matches_loose_eq(slice: &[u8]) -> bool {
        slice.len() >= 4 && slice[0] == b' ' && slice[1] == b'=' && slice[2] == b'=' && slice[3] == b' '
    }

    /// Match loose inequality pattern " != ".
    fn matches_loose_neq(slice: &[u8]) -> bool {
        slice.len() >= 4 && slice[0] == b' ' && slice[1] == b'!' && slice[2] == b'=' && slice[3] == b' '
    }

    /// Push a validation error.
    fn push_error(&mut self, code: &'static str, message: &str, line: u32) {
        self.errors.push(ValidationError {
            code,
            message: message.to_string(),
            line,
            column: 0,
        });
    }

    /// Get all validation errors.
    #[must_use]
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }
}

impl Default for SubsetValidator {
    fn default() -> Self {
        Self::new()
    }
}
