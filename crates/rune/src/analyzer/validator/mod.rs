//! # Subset Validator
//!
//! Validates that TypeScript code uses only the zero-overhead subset.

mod rules;

use crate::parser::SourceFile;

/// Validation error with source location.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Location in source
    pub location: String,
    /// Error message
    pub message: String,
    /// Error code
    pub code: &'static str,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Validates the zero-overhead TypeScript subset.
#[derive(Debug, Default)]
pub struct SubsetValidator {
    /// Current depth for complexity tracking
    #[allow(unused)]
    complexity: usize,
}

impl SubsetValidator {
    /// Create a new validator.
    #[must_use]
    pub const fn new() -> Self {
        Self { complexity: 0 }
    }

    /// Validate an entire source file.
    ///
    /// # Errors
    /// Returns an error if validation fails.
    pub fn validate(&self, source: &SourceFile) -> Result<(), ValidationError> {
        let lines: Vec<&str> = source.source.lines().collect();

        for (line_num, line) in lines.iter().enumerate() {
            let line_num = u32::try_from(line_num).unwrap_or(0) + 1;
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") {
                continue;
            }

            // Check for forbidden patterns
            self.check_line(trimmed, line_num, &source.path.display().to_string())?;
        }

        Ok(())
    }

    /// Check a single line for forbidden patterns.
    #[allow(clippy::too_many_lines)]
    fn check_line(
        &self,
        line: &str,
        line_num: u32,
        file: &str,
    ) -> Result<(), ValidationError> {
        let location = format!("{file}:{line_num}:1");

        // Check for forbidden patterns
        let forbidden_checks: &[(&str, &str, &str)] = &[
            (": any", "no_any", "Type 'any' requires dynamic dispatch. Use concrete types."),
            (": unknown", "no_unknown", "Type 'unknown' requires dynamic dispatch. Use concrete types."),
            ("var ", "no_var", "Use 'const' or 'let' instead of 'var'."),
            (" == ", "no_loose_equality", "Use '===' for strict equality."),
            ("try", "no_exceptions", "Use Result<T,E> return pattern instead of try/catch/throw."),
            ("catch", "no_exceptions", "Use Result<T,E> return pattern instead of try/catch/throw."),
            ("throw", "no_exceptions", "Use Result<T,E> return pattern instead of try/catch/throw."),
            ("class ", "no_classes", "Classes and prototype inheritance are forbidden. Use plain objects and functions."),
            ("this.", "no_this", "Use plain functions instead of class methods with 'this'."),
            ("eval(", "no_eval", "Dynamic scoping via eval() is forbidden."),
            ("with ", "no_with", "Dynamic scoping via 'with' is forbidden."),
            ("typeof ", "no_typeof", "Runtime type inspection via typeof is forbidden."),
            ("instanceof", "no_instanceof", "Runtime type inspection via instanceof is forbidden."),
            ("delete ", "no_delete", "Use ownership and explicit drops instead of delete."),
        ];

        for (pattern, code, message) in forbidden_checks {
            if line.contains(pattern) {
                return Err(ValidationError {
                    location,
                    message: message.to_string(),
                    code,
                });
            }
        }

        // Check for dynamic property access
        if line.contains("][") || line.contains("[key]") {
            return Err(ValidationError {
                location,
                message: "Use Map<K,V> for dynamic keys or arrays for fixed indices.".to_string(),
                code: "no_dynamic_access",
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forbidden_any() {
        let validator = SubsetValidator::new();
        let source = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: crate::parser::SourceKind::TypeScript,
            source: "function foo(x: any) {}".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: vec![],
        };
        assert!(validator.validate(&source).is_err());
    }

    #[test]
    fn test_valid_function() {
        let validator = SubsetValidator::new();
        let source = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: crate::parser::SourceKind::TypeScript,
            source: "function add(a: number, b: number): number { return a + b; }".to_string(),
            name: "test".to_string(),
            valid: true,
            errors: vec![],
        };
        assert!(validator.validate(&source).is_ok());
    }
}
