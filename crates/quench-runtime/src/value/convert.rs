//! Value conversion utilities — re-exports from coerce, compare, and primitive modules.
//!
//! Canonical spec ops live in:
//!   - `coerce.rs`   — to_js_string, to_bool, to_number, to_uint32
//!   - `compare.rs`  — strict_eq, loose_eq, same_value
//!   - `primitive.rs` — to_primitive, to_object

// Re-export from coerce
pub use crate::value::coerce::{
    simple_string_value, to_bool, to_js_string, to_number, to_number_unchecked, to_uint32,
    try_to_number,
};

// Re-export from compare
pub use crate::value::compare::{loose_eq, same_value, strict_eq};

// Re-export from primitive
pub use crate::value::primitive::PrimitiveHint;
pub use crate::value::primitive::{to_object, to_primitive};

#[cfg(test)]
#[cfg(test)]
mod tests;
