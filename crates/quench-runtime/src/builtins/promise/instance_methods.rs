//! Promise instance methods
//!
//! Instance methods (then, catch, finally) are defined in constructor.rs
//! to avoid circular dependencies with callback processing logic.

pub use super::constructor::promise_then_impl;
