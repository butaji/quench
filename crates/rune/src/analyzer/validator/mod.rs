//! # Validator Module
//!
//! Validates the zero-overhead TypeScript subset.

mod validation;

pub use validation::{SubsetValidator, ValidationError};
