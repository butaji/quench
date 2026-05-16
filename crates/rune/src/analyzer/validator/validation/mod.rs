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

        // Check for loose equality (== and !=)
        if line.contains(" == ") || line.contains(" != ") || line.contains("== ") || line.contains("!= ") {
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

        // Check for dynamic property access (obj[key])
        if line.contains('[') && line.contains(']') && !line.starts_with("//") {
            if let Some(idx) = line.find('[') {
                let before = &line[..idx].trim();
                if !before.is_empty()
                    && !before.ends_with('(')
                    && !before.ends_with('[')
                    && !before.ends_with('{')
                    && !before.ends_with('<')
                {
                    self.errors.push(ValidationError {
                        code: "no-dynamic-access",
                        message: "Dynamic property access (obj[key]) is forbidden. Use Map<K,V>."
                            .to_string(),
                        line: line_num,
                        column: idx as u32,
                    });
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
