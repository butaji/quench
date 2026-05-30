//! Shared utilities for runts
//!
//! Common helper functions used across modules.

/// Convert a string to PascalCase
pub fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .flat_map(|part| part.split('_'))
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}
