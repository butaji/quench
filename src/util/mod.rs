//! Shared utilities for runts
//!
//! Common helper functions used across modules.

/// Convert a string to snake_case
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i-1];
            let next = chars.get(i+1).copied();
            // Add underscore before uppercase if:
            // - Previous char is lowercase, OR
            // - Previous is uppercase AND next is lowercase (end of acronym)
            let add_underscore = c.is_uppercase() 
                && (prev.is_lowercase() 
                    || (prev.is_uppercase() && next.map(|n| n.is_lowercase()).unwrap_or(false)));
            if add_underscore {
                result.push('_');
            }
        }
        result.extend(c.to_lowercase());
    }
    result
}

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

/// Convert a string to camelCase
pub fn to_camel_case(s: &str) -> String {
    let pascal = to_pascal_case(s);
    let mut chars = pascal.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("HelloWorld"), "hello_world");
        assert_eq!(to_snake_case("HTMLParser"), "html_parser");  // Acronym stays uppercase
        assert_eq!(to_snake_case("simple"), "simple");
        assert_eq!(to_snake_case("myVariable"), "my_variable");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case("foo-bar"), "FooBar");
        assert_eq!(to_pascal_case("ok_err_pending"), "OkErrPending");
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("hello_world"), "helloWorld");
        assert_eq!(to_camel_case("FooBar"), "fooBar");
        assert_eq!(to_camel_case("simple"), "simple");
    }
}
