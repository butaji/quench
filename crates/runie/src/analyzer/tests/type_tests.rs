//! TypeInfo tests

#[cfg(test)]
mod type_info_tests {
    use crate::analyzer::TypeInfo;

    #[test]
    fn test_type_info_integer() {
        let info = TypeInfo::Integer(42);
        assert!(info.is_integer());
        assert_eq!("i32", info.to_rust_type());
    }

    #[test]
    fn test_type_info_float() {
        let info = TypeInfo::Float;
        assert!(!info.is_integer());
        assert_eq!("f64", info.to_rust_type());
    }

    #[test]
    fn test_type_info_string() {
        let info = TypeInfo::String;
        assert_eq!("String", info.to_rust_type());
    }

    #[test]
    fn test_type_info_string_literal() {
        let info = TypeInfo::StringLiteral("hello".to_string());
        assert_eq!("&str", info.to_rust_type());
    }

    #[test]
    fn test_type_info_boolean() {
        let info = TypeInfo::Boolean;
        assert_eq!("bool", info.to_rust_type());
    }

    #[test]
    fn test_type_info_array() {
        let inner = Box::new(TypeInfo::Integer(0));
        let info = TypeInfo::Array(inner);
        assert_eq!("Vec<i32>", info.to_rust_type());
    }

    #[test]
    fn test_type_info_option() {
        let inner = Box::new(TypeInfo::String);
        let info = TypeInfo::Option(inner);
        assert_eq!("Option<String>", info.to_rust_type());
    }

    #[test]
    fn test_type_info_result() {
        let ok = Box::new(TypeInfo::Integer(0));
        let err = Box::new(TypeInfo::String);
        let info = TypeInfo::Result(ok, err);
        assert_eq!("Result<i32, String>", info.to_rust_type());
    }

    #[test]
    fn test_type_info_generic() {
        let info = TypeInfo::Generic("T".to_string());
        assert_eq!("T", info.to_rust_type());
    }
}
