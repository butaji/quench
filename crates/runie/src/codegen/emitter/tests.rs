//! # Emitter Tests
//!
//! Tests for code generation from TypeScript to Rust.

#[cfg(test)]
mod utils_tests {
    use crate::codegen::emitter::utils::{escape_rust_keyword, to_pascal_case, to_snake_case};

    #[test]
    fn test_to_snake_case_simple() {
        assert_eq!("hello", to_snake_case("hello"));
        assert_eq!("world", to_snake_case("world"));
    }

    #[test]
    fn test_to_snake_case_camel() {
        assert_eq!("hello_world", to_snake_case("helloWorld"));
        assert_eq!("foo_bar", to_snake_case("fooBar"));
        assert_eq!("my_variable_name", to_snake_case("myVariableName"));
    }

    #[test]
    fn test_to_snake_case_pascal() {
        assert_eq!("hello_world", to_snake_case("HelloWorld"));
        assert_eq!("foo_bar", to_snake_case("FooBar"));
    }

    #[test]
    fn test_to_snake_case_already_snake() {
        assert_eq!("hello_world", to_snake_case("hello_world"));
    }

    #[test]
    fn test_to_pascal_case_simple() {
        assert_eq!("Hello", to_pascal_case("hello"));
        assert_eq!("World", to_pascal_case("world"));
    }

    #[test]
    fn test_to_pascal_case_snake() {
        assert_eq!("HelloWorld", to_pascal_case("hello_world"));
        assert_eq!("FooBar", to_pascal_case("foo_bar"));
    }

    #[test]
    fn test_to_pascal_case_camel() {
        assert_eq!("HelloWorld", to_pascal_case("helloWorld"));
    }

    #[test]
    fn test_escape_rust_keyword() {
        assert_eq!("r#type", escape_rust_keyword("type"));
        assert_eq!("r#fn", escape_rust_keyword("fn"));
        assert_eq!("r#let", escape_rust_keyword("let"));
        assert_eq!("r#impl", escape_rust_keyword("impl"));
    }

    #[test]
    fn test_escape_rust_keyword_not_needed() {
        assert_eq!("hello", escape_rust_keyword("hello"));
        assert_eq!("world", escape_rust_keyword("world"));
    }
}

#[cfg(test)]
mod type_tests {
    use crate::codegen::emitter::types::{is_enum_type, to_rust_name};

    #[test]
    fn test_is_enum_type() {
        assert!(is_enum_type("Task"));
        assert!(is_enum_type("Color"));
        assert!(is_enum_type("Message"));
        assert!(!is_enum_type("task"));
        assert!(!is_enum_type("myVariable"));
    }

    #[test]
    fn test_to_rust_name() {
        assert_eq!("Task", to_rust_name("Task"));
        assert_eq!("task", to_rust_name("task"));
        assert_eq!("my_function", to_rust_name("myFunction"));
    }
}
