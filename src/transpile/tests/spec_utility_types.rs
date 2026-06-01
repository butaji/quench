//! Tests for TypeScript Utility Types
//!
//! - Partial<T> -> all fields become Option<T>
//! - Pick<T, K> -> struct with only selected fields
//! - Omit<T, K> -> struct without specified fields
//! - Record<K, V> -> HashMap
//! - keyof T -> enum with field names
//! - ReturnType<T> -> extracts return type
//! - Parameters<T> -> extracts param types as tuple

#[cfg(test)]
mod spec_utility_types_tests {
    use crate::transpile::hir::*;
    use crate::transpile::hir::type_to_rust::{TypeToRust, OutputKind, RustType};
    use crate::transpile::hir::QuoteCodegen;
    use quote::ToTokens;

    fn type_to_rust_name(ty: &Type) -> String {
        TypeToRust::new(OutputKind::String).convert(ty).type_name()
    }

    fn codegen_produces_output(ty: &Type) -> bool {
        !QuoteCodegen::default().gen_type(ty).is_empty()
    }

    // Partial<T> tests
    mod partial {
        use super::*;

        #[test]
        fn test_partial_converts_to_struct() {
            let ty = Type::Partial {
                inner: Box::new(Type::Object {
                    members: vec![
                        TypeMember { key: "x".into(), type_: Type::Number, optional: false, readonly: false },
                    ],
                }),
            };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            assert!(matches!(rust_type, RustType::Struct(_)));
        }

        #[test]
        fn test_partial_codegen_produces_output() {
            let ty = Type::Partial {
                inner: Box::new(Type::Object {
                    members: vec![TypeMember { key: "x".into(), type_: Type::Number, optional: false, readonly: false }],
                }),
            };
            assert!(codegen_produces_output(&ty));
        }
    }

    // Pick<T, K> tests
    mod pick {
        use super::*;

        #[test]
        fn test_pick_single_key() {
            let ty = Type::Pick {
                inner: Box::new(Type::Object {
                    members: vec![
                        TypeMember { key: "x".into(), type_: Type::Number, optional: false, readonly: false },
                        TypeMember { key: "y".into(), type_: Type::Number, optional: false, readonly: false },
                    ],
                }),
                keys: vec!["x".into()],
            };
            let name = type_to_rust_name(&ty);
            assert!(name.contains("x:"));
            assert!(!name.contains("y:"));
        }

        #[test]
        fn test_pick_multiple_keys() {
            let ty = Type::Pick {
                inner: Box::new(Type::Object {
                    members: vec![
                        TypeMember { key: "x".into(), type_: Type::Number, optional: false, readonly: false },
                        TypeMember { key: "y".into(), type_: Type::Number, optional: false, readonly: false },
                        TypeMember { key: "z".into(), type_: Type::Number, optional: false, readonly: false },
                    ],
                }),
                keys: vec!["x".into(), "y".into()],
            };
            let name = type_to_rust_name(&ty);
            assert!(name.contains("x:"));
            assert!(name.contains("y:"));
            assert!(!name.contains("z:"));
        }
    }

    // Omit<T, K> tests
    mod omit {
        use super::*;

        #[test]
        fn test_omit_single_key() {
            let ty = Type::Omit {
                inner: Box::new(Type::Object {
                    members: vec![
                        TypeMember { key: "x".into(), type_: Type::Number, optional: false, readonly: false },
                        TypeMember { key: "y".into(), type_: Type::Number, optional: false, readonly: false },
                        TypeMember { key: "z".into(), type_: Type::Number, optional: false, readonly: false },
                    ],
                }),
                keys: vec!["z".into()],
            };
            let name = type_to_rust_name(&ty);
            assert!(name.contains("x:"));
            assert!(name.contains("y:"));
            assert!(!name.contains("z:"));
        }
    }

    // Record<K, V> tests
    mod record {
        use super::*;

        #[test]
        fn test_record_to_hashmap() {
            let ty = Type::Record {
                key: Box::new(Type::String),
                value: Box::new(Type::Number),
            };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            assert!(matches!(rust_type, RustType::HashMap(_, _)));
        }

        #[test]
        fn test_record_type_name() {
            let ty = Type::Record {
                key: Box::new(Type::String),
                value: Box::new(Type::Number),
            };
            let name = type_to_rust_name(&ty);
            assert!(name.contains("HashMap"));
            assert!(name.contains("String"));
            assert!(name.contains("f64"));
        }
    }

    // keyof T tests
    mod keyof {
        use super::*;

        #[test]
        fn test_keyof_converts_to_enum() {
            let ty = Type::KeyOf {
                inner: Box::new(Type::Object {
                    members: vec![
                        TypeMember { key: "x".into(), type_: Type::Number, optional: false, readonly: false },
                        TypeMember { key: "y".into(), type_: Type::Number, optional: false, readonly: false },
                    ],
                }),
            };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            assert!(matches!(rust_type, RustType::Enum(_)));
        }
    }

    // ReturnType<T> tests
    mod return_type {
        use super::*;

        #[test]
        fn test_return_type_extracts_return() {
            let ty = Type::ReturnType {
                inner: Box::new(Type::Function {
                    params: vec![Type::Number],
                    ret: Box::new(Type::String),
                }),
            };
            let name = type_to_rust_name(&ty);
            assert_eq!(name, "String");
        }
    }

    // Parameters<T> tests
    mod parameters {
        use super::*;

        #[test]
        fn test_parameters_extracts_params() {
            let ty = Type::Parameters {
                inner: Box::new(Type::Function {
                    params: vec![Type::String, Type::Number],
                    ret: Box::new(Type::Void),
                }),
            };
            let name = type_to_rust_name(&ty);
            assert!(name.contains("String"));
            assert!(name.contains("f64"));
        }
    }

    // Readonly<T> tests
    mod readonly {
        use super::*;

        #[test]
        fn test_readonly_passes_through() {
            let ty = Type::Readonly {
                inner: Box::new(Type::Object {
                    members: vec![
                        TypeMember { key: "x".into(), type_: Type::Number, optional: false, readonly: false },
                    ],
                }),
            };
            assert!(codegen_produces_output(&ty));
        }
    }

    // Integration test
    #[test]
    fn test_all_utility_types_produce_output() {
        let types = vec![
            Type::Partial { inner: Box::new(Type::Object { members: vec![] }) },
            Type::Pick { inner: Box::new(Type::Object { members: vec![] }), keys: vec![] },
            Type::Omit { inner: Box::new(Type::Object { members: vec![] }), keys: vec![] },
            Type::Record { key: Box::new(Type::String), value: Box::new(Type::Number) },
            Type::KeyOf { inner: Box::new(Type::Object { members: vec![] }) },
            Type::ReturnType { inner: Box::new(Type::Function { params: vec![], ret: Box::new(Type::Void) }) },
            Type::Parameters { inner: Box::new(Type::Function { params: vec![], ret: Box::new(Type::Void) }) },
            Type::Readonly { inner: Box::new(Type::Object { members: vec![] }) },
        ];
        for ty in types {
            assert!(codegen_produces_output(&ty), "Type {:?} should produce output", ty);
        }
    }
}
