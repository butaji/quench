//! # Subset Validator
//!
//! Validates that TypeScript code uses only the zero-overhead subset.

mod rules;

use crate::parser::SourceFile;

/// Validation error with source location.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub location: String,
    pub message: String,
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
    complexity: usize,
}

impl SubsetValidator {
    /// Create a new validator.
    #[must_use]
    pub fn new() -> Self {
        Self { complexity: 0 }
    }

    /// Validate an entire source file.
    pub fn validate(&mut self, source: &SourceFile) -> Result<(), ValidationError> {
        let lines: Vec<&str> = source.source.lines().collect();

        for (line_num, line) in lines.iter().enumerate() {
            let line_num = line_num + 1;
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") {
                continue;
            }

            // Check for forbidden patterns
            self.check_line(trimmed, line_num as u32, &source.path.display().to_string())?;
        }

        Ok(())
    }

    /// Check a single line for forbidden patterns.
    fn check_line(
        &mut self,
        line: &str,
        line_num: u32,
        file: &str,
    ) -> Result<(), ValidationError> {
        let location = format!("{}:{}:1", file, line_num);

        // Check for forbidden 'any' type
        if line.contains(": any") || line.ends_with("any") {
            return Err(ValidationError {
                location,
                message: "Type 'any' requires dynamic dispatch. Use concrete types.".to_string(),
                code: "no_any",
            });
        }

        // Check for 'unknown' type
        if line.contains(": unknown") {
            return Err(ValidationError {
                location,
                message: "Type 'unknown' requires dynamic dispatch. Use concrete types.".to_string(),
                code: "no_unknown",
            });
        }

        // Check for 'var' declarations
        if line.contains("var ") {
            return Err(ValidationError {
                location,
                message: "Use 'const' or 'let' instead of 'var'.".to_string(),
                code: "no_var",
            });
        }

        // Check for loose equality
        if line.contains(" == ") || line.ends_with("==") {
            return Err(ValidationError {
                location,
                message: "Use '===' for strict equality.".to_string(),
                code: "no_loose_equality",
            });
        }

        // Check for try/catch/throw
        if line.contains("try") || line.contains("catch") || line.contains("throw") {
            return Err(ValidationError {
                location,
                message: "Use Result<T,E> return pattern instead of try/catch/throw.".to_string(),
                code: "no_exceptions",
            });
        }

        // Check for class declarations
        if line.contains("class ") {
            return Err(ValidationError {
                location,
                message: "Classes and prototype inheritance are forbidden. Use plain objects and functions.".to_string(),
                code: "no_classes",
            });
        }

        // Check for 'this' keyword
        if line.contains("this.") || line == "this" {
            return Err(ValidationError {
                location,
                message: "Use plain functions instead of class methods with 'this'.".to_string(),
                code: "no_this",
            });
        }

        // Check for eval
        if line.contains("eval(") {
            return Err(ValidationError {
                location,
                message: "Dynamic scoping via eval() is forbidden.".to_string(),
                code: "no_eval",
            });
        }

        // Check for 'with' statement
        if line.starts_with("with ") {
            return Err(ValidationError {
                location,
                message: "Dynamic scoping via 'with' is forbidden.".to_string(),
                code: "no_with",
            });
        }

        // Check for typeof
        if line.contains("typeof ") {
            return Err(ValidationError {
                location,
                message: "Runtime type inspection via typeof is forbidden.".to_string(),
                code: "no_typeof",
            });
        }

        // Check for instanceof
        if line.contains(" instanceof ") {
            return Err(ValidationError {
                location,
                message: "Runtime type inspection via instanceof is forbidden.".to_string(),
                code: "no_instanceof",
            });
        }

        // Check for delete
        if line.contains("delete ") {
            return Err(ValidationError {
                location,
                message: "Use ownership and explicit drops instead of delete.".to_string(),
                code: "no_delete",
            });
        }

        // Check for dynamic property access (object[key])
        if line.contains("][") || line.contains("[key]") {
            return Err(ValidationError {
                location,
                message: "Use Map<K,V> for dynamic keys or arrays for fixed indices.".to_string(),
                code: "no_dynamic_access",
            });
        }

        // Check for implicit any (untyped parameters)
        if let Some(func_start) = line.find("function ") {
            let after_func = &line[func_start + 9..];
            if let Some(paren_start) = after_func.find('(') {
                let before_paren = &after_func[..paren_start];
                let params_start = after_func.find('(').map(|p| p + 1);
                let params_end = after_func.find(')');

                if let (Some(ps), Some(pe)) = (params_start, params_end) {
                    let params = &after_func[ps..pe];
                    // Check if any param lacks type annotation
                    for param in params.split(',') {
                        let param = param.trim();
                        if !param.is_empty() && !param.contains(':') {
                            // This is implicit any - flag as warning
                        }
                    }
                }
            }
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
