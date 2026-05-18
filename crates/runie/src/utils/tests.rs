//! # Utils Tests
//!
//! Tests for shared utilities.

#[cfg(test)]
mod utils_tests {
    use crate::utils::*;

    #[test]
    fn test_to_snake_case_simple() {
        assert_eq!("hello", to_snake_case("hello"));
        assert_eq!("world", to_snake_case("world"));
    }

    #[test]
    fn test_to_snake_case_camel() {
        assert_eq!("hello_world", to_snake_case("helloWorld"));
        assert_eq!("foo_bar", to_snake_case("fooBar"));
    }

    #[test]
    fn test_to_snake_case_pascal() {
        assert_eq!("hello_world", to_snake_case("HelloWorld"));
        assert_eq!("foo_bar", to_snake_case("FooBar"));
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

    #[test]
    fn test_escape_keyword() {
        assert_eq!("r#type", escape_keyword("type"));
        assert_eq!("r#async", escape_keyword("async"));
    }

    #[test]
    fn test_is_enum_type() {
        assert!(is_enum_type("Task"));
        assert!(is_enum_type("Color"));
        assert!(!is_enum_type("task"));
        assert!(!is_enum_type("myVariable"));
    }

    #[test]
    fn test_to_rust_name() {
        assert_eq!("Task", to_rust_name("Task"));
        assert_eq!("task", to_rust_name("task"));
    }

    #[test]
    fn test_escape_rust_keyword_for_module() {
        assert_eq!("r#type", escape_rust_keyword_for_module("type"));
        assert_eq!("r#mod", escape_rust_keyword_for_module("mod"));
    }
}

#[cfg(test)]
mod source_location_tests {
    use crate::SourceLocation;

    #[test]
    fn test_source_location_new() {
        let loc = SourceLocation::new("main.r.ts", 10, 5);
        assert_eq!("main.r.ts", loc.file);
        assert_eq!(10, loc.line);
        assert_eq!(5, loc.column);
    }

    #[test]
    fn test_source_location_display() {
        let loc = SourceLocation::new("main.r.ts", 10, 5);
        let display = format!("{}", loc);
        assert!(display.contains("main.r.ts"));
        assert!(display.contains("10"));
        assert!(display.contains('5'));
    }

    #[test]
    fn test_source_location_default() {
        let loc = SourceLocation::default();
        assert_eq!("", loc.file);
        assert_eq!(0, loc.line);
        assert_eq!(0, loc.column);
    }
}

#[cfg(test)]
mod error_tests {
    use crate::{ParseError, RunieError};

    #[test]
    fn test_parse_error_not_found() {
        let error = ParseError::NotFound("main.r.ts".to_string());
        assert!(error.to_string().contains("not found"));
        assert!(error.to_string().contains("main.r.ts"));
    }

    #[test]
    fn test_parse_error_invalid_extension() {
        let error = ParseError::InvalidExtension("main.js".to_string());
        let msg = error.to_string();
        assert!(msg.contains("main.js") || msg.contains("extension"));
    }

    #[test]
    fn test_parse_error_parse() {
        let error = ParseError::Parse("unexpected token".to_string());
        assert!(error.to_string().contains("Parse"));
        assert!(error.to_string().contains("unexpected token"));
    }

    #[test]
    fn test_rune_error_analysis() {
        let error = RunieError::Analysis {
            location: "main.r.ts:10".to_string(),
            message: "type mismatch".to_string(),
        };
        assert!(error.to_string().contains("main.r.ts"));
        assert!(error.to_string().contains("type mismatch"));
    }

    #[test]
    fn test_rune_error_codegen() {
        let error = RunieError::Codegen("test error".to_string());
        assert!(error.to_string().contains("test error"));
    }

    #[test]
    fn test_rune_error_io() {
        let error = RunieError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn test_rune_result_ok() {
        let result: crate::Result<i32> = Ok(42);
        assert!(result.is_ok());
        if let Ok(val) = result {
            assert_eq!(42, val);
        }
    }

    #[test]
    fn test_rune_result_err() {
        let result: crate::Result<i32> = Err(RunieError::Codegen("test".to_string()));
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod known_struct_tests {
    // Removed KnownStruct tests - the enum was project-specific and has been deleted.
}
