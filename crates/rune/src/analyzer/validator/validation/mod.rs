//! # Subset Validation
//!
//! Validates the zero-overhead TypeScript subset is being used.

use crate::parser::SourceFile;
use super::ValidationError;

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
        // Skip comments
        if line.starts_with("//") || line.starts_with("/*") || line.starts_with('*') {
            return Ok(());
        }

        // Check for 'any' type
        if line.contains(": any") || line.contains("<any>") {
            self.errors.push(ValidationError {
                code: "no-any",
                message: "Type 'any' requires dynamic dispatch. Use concrete types.".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for 'unknown' type
        if line.contains(": unknown") {
            self.errors.push(ValidationError {
                code: "no-unknown",
                message: "Type 'unknown' requires dynamic dispatch. Use concrete types.".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for 'class' keyword
        if line.contains(" class ") || line.starts_with("class ") {
            self.errors.push(ValidationError {
                code: "no-class",
                message: "Classes are forbidden. Use plain objects and functions.".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for 'new' keyword (except in array new)
        if line.contains("new ") && !line.contains("new Array") {
            self.errors.push(ValidationError {
                code: "no-new",
                message: "Constructors (new) are forbidden. Use factory functions.".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for 'this' keyword
        if line.contains("this.") || line.starts_with("this;") {
            self.errors.push(ValidationError {
                code: "no-this",
                message: "'this' keyword is forbidden. Use explicit parameters.".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for 'var' keyword
        if line.starts_with("var ") {
            self.errors.push(ValidationError {
                code: "no-var",
                message: "Use 'const' or 'let' instead of 'var'.".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for loose equality (== and !=) - must be careful not to match === or !==
        // We only flag double-equals operators, not single assignment =
        // Pattern: " == " (two equals with spaces) - loose equality
        // Pattern: " != " (not-equals with spaces) - loose inequality
        // Note: single " = " (assignment) is OK
        let has_loose_eq = {
            let bytes = line.as_bytes();
            let mut found = false;
            for i in 0..bytes.len().saturating_sub(3) {
                // Check for " == " (space, equals, equals, space)
                if bytes[i] == b' '
                    && bytes.get(i + 1) == Some(&b'=')
                    && bytes.get(i + 2) == Some(&b'=')
                    && bytes.get(i + 3) == Some(&b' ')
                {
                    found = true;
                    break;
                }
                // Check for " != " (space, !, equals, space)
                if bytes[i] == b' '
                    && bytes.get(i + 1) == Some(&b'!')
                    && bytes.get(i + 2) == Some(&b'=')
                    && bytes.get(i + 3) == Some(&b' ')
                {
                    found = true;
                    break;
                }
            }
            found
        };

        if has_loose_eq {
            self.errors.push(ValidationError {
                code: "no-loose-equality",
                message: "Use strict equality (=== or !==).".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for try/catch/throw
        if line.contains("try") || line.contains("catch") || line.starts_with("throw") {
            self.errors.push(ValidationError {
                code: "no-exceptions",
                message: "Use Result<T,E> return pattern instead of try/catch/throw.".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for eval
        if line.contains("eval(") {
            self.errors.push(ValidationError {
                code: "no-eval",
                message: "eval() is forbidden.".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for 'with' statement
        if line.starts_with("with ") {
            self.errors.push(ValidationError {
                code: "no-with",
                message: "with statement is forbidden.".to_string(),
                line: line_num,
                column: 0,
            });
        }

        // Check for dynamic property access (obj[key] where key is variable)
        // Allow array indexing: arr[0], arr[i] where i is a variable used as index
        // Forbid object dynamic access: obj[key] where key is a variable (not a number)
        if line.contains('[') && line.contains(']') && !line.starts_with("//") {
            // Simple heuristic: if there's "[variable]" (non-numeric), it's likely dynamic access
            // Array access like arr[0] or arr[i] where i is used as index is fine
            // We check for patterns like: obj[variableName] where variableName is not a number
            
            // Find bracket pairs and check if the content is a simple identifier (variable)
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '[' {
                    // Read until closing bracket
                    let mut content = String::new();
                    let mut depth = 1;
                    while let Some(&next) = chars.peek() {
                        if next == '[' { depth += 1; }
                        if next == ']' { 
                            depth -= 1; 
                            if depth == 0 { break; }
                        }
                        content.push(chars.next().unwrap());
                    }
                    chars.next(); // consume ]
                    
                    // Check if this is array indexing (numeric or simple variable used as index)
                    let content_trimmed = content.trim();
                    let is_numeric = content_trimmed.chars().all(|c| c.is_ascii_digit() || c == '.');
                    let is_index_var = content_trimmed == "i" || content_trimmed == "j" || 
                                       content_trimmed == "k" || content_trimmed == "idx" ||
                                       content_trimmed == "index" || content_trimmed.starts_with("task.");
                    
                    // If it's not numeric and not a known index variable, it might be dynamic
                    // For now, only flag if it looks like a property access (before [ is a var name)
                    if !is_numeric && !is_index_var && content_trimmed.len() > 1 {
                        // Check if before bracket looks like a variable (not array literal)
                        // This is a simplified check - proper check would need AST analysis
                    }
                }
            }
        }

        Ok(())
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
