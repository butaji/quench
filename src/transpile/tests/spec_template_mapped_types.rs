//! Tests for template literal types and mapped types
//!
//! allow:too_many_lines,complexity

#[cfg(test)]
mod template_mapped_type_tests {
    use crate::transpile::hir::*;
    use crate::transpile::hir::type_to_rust::{TypeToRust, OutputKind, RustType};

    // =========================================================================
    // Template Literal Types
    // =========================================================================

    #[test]
    fn test_template_type_hir_representation() {
        // Verify Type::Template exists and has correct structure
        let ty = Type::Template {
            parts: vec![
                TemplatePart::String("hello_".to_string()),
                TemplatePart::String("_world".to_string()),
            ],
            values: vec![],
        };
        if let Type::Template { parts, values } = &ty {
            assert_eq!(parts.len(), 2);
            assert_eq!(values.len(), 0);
        } else {
            panic!("Expected Type::Template");
        }
    }

    #[test]
    fn test_template_type_with_dynamic_parts() {
        // Template type with type placeholders
        let ty = Type::Template {
            parts: vec![
                TemplatePart::String("on".to_string()),
            ],
            values: vec![Type::String],
        };
        if let Type::Template { parts, values } = &ty {
            assert_eq!(parts.len(), 1);
            assert_eq!(values.len(), 1);
        } else {
            panic!("Expected Type::Template");
        }
    }

    #[test]
    fn test_codegen_template_literal_all_static() {
        // Template type with all static parts resolves to StringLiteral
        let ty = Type::Template {
            parts: vec![
                TemplatePart::String("on".to_string()),
            ],
            values: vec![],
        };
        let converter = TypeToRust::new(OutputKind::String);
        let rust_type = converter.convert(&ty);
        assert!(matches!(rust_type, RustType::StringLiteral(_)));
    }

    #[test]
    fn test_codegen_template_literal_with_dynamic() {
        // Template type with dynamic parts erases to Value
        let ty = Type::Template {
            parts: vec![
                TemplatePart::String("on".to_string()),
                TemplatePart::Type(Type::String), // dynamic
            ],
            values: vec![Type::String],
        };
        let converter = TypeToRust::new(OutputKind::String);
        let rust_type = converter.convert(&ty);
        assert!(matches!(rust_type, RustType::Value));
    }

    #[test]
    fn test_template_type_equality() {
        let ty1 = Type::Template {
            parts: vec![TemplatePart::String("a".to_string())],
            values: vec![],
        };
        let ty2 = Type::Template {
            parts: vec![TemplatePart::String("a".to_string())],
            values: vec![],
        };
        assert_eq!(ty1, ty2);
    }

    // =========================================================================
    // Mapped Types
    // =========================================================================

    #[test]
    fn test_mapped_type_hir_representation() {
        let ty = Type::Mapped {
            from: Box::new(Type::String),
            to: Box::new(Type::Boolean),
        };
        if let Type::Mapped { from, to } = &ty {
            assert!(matches!(**from, Type::String));
            assert!(matches!(**to, Type::Boolean));
        } else {
            panic!("Expected Type::Mapped");
        }
    }

    #[test]
    fn test_codegen_mapped_type() {
        // Mapped types become HashMap in Rust
        let ty = Type::Mapped {
            from: Box::new(Type::String),
            to: Box::new(Type::Number),
        };
        let converter = TypeToRust::new(OutputKind::String);
        let rust_type = converter.convert(&ty);
        assert!(matches!(rust_type, RustType::HashMap(_, _)));
    }

    #[test]
    fn test_mapped_type_equality() {
        let ty1 = Type::Mapped {
            from: Box::new(Type::String),
            to: Box::new(Type::Number),
        };
        let ty2 = Type::Mapped {
            from: Box::new(Type::String),
            to: Box::new(Type::Number),
        };
        assert_eq!(ty1, ty2);
    }

    // =========================================================================
    // Type::Template with TemplatePart::Type
    // =========================================================================

    #[test]
    fn test_template_part_type_variant() {
        let part = TemplatePart::Type(Type::Number);
        if let TemplatePart::Type(t) = &part {
            assert!(matches!(t, Type::Number));
        } else {
            panic!("Expected TemplatePart::Type");
        }
    }

    #[test]
    fn test_template_part_string_variant() {
        let part = TemplatePart::String("hello".to_string());
        if let TemplatePart::String(s) = &part {
            assert_eq!(s, "hello");
        } else {
            panic!("Expected TemplatePart::String");
        }
    }

    // =========================================================================
    // Codegen verification for existing Type variants used in template/mapped types
    // =========================================================================

    #[test]
    fn test_codegen_mapped_string_to_number() {
        let ty = Type::Mapped {
            from: Box::new(Type::String),
            to: Box::new(Type::Number),
        };
        let converter = TypeToRust::new(OutputKind::String);
        let rust_type = converter.convert(&ty);
        let name = rust_type.type_name();
        assert!(name.contains("HashMap"), "Expected HashMap, got: {}", name);
    }

    #[test]
    fn test_codegen_mapped_complex_types() {
        let ty = Type::Mapped {
            from: Box::new(Type::KeyOf {
                inner: Box::new(Type::Ref { name: "Props".to_string(), generics: vec![] }),
            }),
            to: Box::new(Type::Index {
                obj: Box::new(Type::Ref { name: "Props".to_string(), generics: vec![] }),
                index: Box::new(Type::Ref { name: "P".to_string(), generics: vec![] }),
            }),
        };
        let converter = TypeToRust::new(OutputKind::String);
        let rust_type = converter.convert(&ty);
        // Mapped should produce HashMap
        assert!(matches!(rust_type, RustType::HashMap(_, _)));
    }

    // =========================================================================
    // Round-trip HIR -> Rust type name -> HIR consistency
    // =========================================================================

    #[test]
    fn test_type_name_for_template_static() {
        let ty = Type::Template {
            parts: vec![TemplatePart::String("onClick".to_string())],
            values: vec![],
        };
        let converter = TypeToRust::new(OutputKind::String);
        let rust_type = converter.convert(&ty);
        let name = rust_type.type_name();
        // Static template should produce a string literal
        assert_eq!(name, "\"onClick\"", "got: {}", name);
    }

    #[test]
    fn test_type_name_for_template_dynamic() {
        let ty = Type::Template {
            parts: vec![TemplatePart::String("on".to_string())],
            values: vec![Type::String],
        };
        let converter = TypeToRust::new(OutputKind::String);
        let rust_type = converter.convert(&ty);
        let name = rust_type.type_name();
        // Dynamic template should produce Value
        assert_eq!(name, "Value", "got: {}", name);
    }

    #[test]
    fn test_type_name_for_mapped() {
        let ty = Type::Mapped {
            from: Box::new(Type::String),
            to: Box::new(Type::Boolean),
        };
        let converter = TypeToRust::new(OutputKind::String);
        let rust_type = converter.convert(&ty);
        let name = rust_type.type_name();
        // Mapped should produce HashMap
        assert!(name.contains("HashMap"), "got: {}", name);
    }
}