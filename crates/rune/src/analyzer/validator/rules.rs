//! # Validation Rules
//!
//! Individual validation rules for the TypeScript subset.

/// Subset validator placeholder.
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
