//! # Utilities
//!
//! Shared utility functions for the emitter.

/// Convert camelCase/snake_case to snake_case.
#[must_use]
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 && !chars[i - 1].is_uppercase() {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else if *c == '-' {
            result.push('_');
        } else {
            result.push(*c);
        }
    }

    // Handle consecutive uppercase (e.g., "URLParser" -> "url_parser")
    let mut final_result = String::new();
    for (i, c) in result.chars().enumerate() {
        if i > 0 && i < result.len() - 1 {
            let prev = result.chars().nth(i - 1);
            let next = result.chars().nth(i + 1);
            if c.is_uppercase() && prev.is_some_and(|p| !p.is_uppercase()) {
                final_result.push('_');
            }
            if c.is_uppercase() && next.is_some_and(|n| n.is_uppercase()) {
                // keep as is
            } else if c.is_uppercase() {
                final_result.push('_');
            }
        }
        final_result.push(c.to_ascii_lowercase());
    }

    final_result.trim_matches('_').to_string()
}
