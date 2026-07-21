//! Private helper functions for object operations.

pub mod destructuring;
pub mod member;
pub mod proxy;

// Re-export all items from submodules.
pub use destructuring::*;
pub use member::*;
pub use proxy::*;

#[cfg(test)]
mod member_tests;
