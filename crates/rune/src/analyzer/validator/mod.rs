//! # Subset Validator
//!
//! Validates the zero-overhead TypeScript subset.

mod validation;

pub use validation::SubsetValidator;

/// Validation error.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub code: &'static str,
    pub message: String,
    pub line: u32,
    pub column: u32,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: [{}] {}",
            self.line, self.column, self.code, self.message
        )
    }
}
