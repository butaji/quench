//! # Subset Validator
//!
//! Validates that TypeScript code uses only the zero-overhead subset.

mod rules;

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
    pub fn new() -> Self {
        Self { complexity: 0 }
    }

    /// Validate an entire module.
    #[allow(unused)]
    pub fn validate_module(&mut self, _module: &()) -> crate::Result<()> {
        Ok(())
    }
}
