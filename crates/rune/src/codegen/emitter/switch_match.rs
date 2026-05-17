//! # Switch Statement Emitter
//!
//! Emits Rust match expressions from TypeScript switch statements.

use super::{emit_expr, CodeEmitter};
use super::types::{is_enum_type, to_rust_name};
use swc_ecma_ast::{Stmt, SwitchCase};

/// Emit a switch statement as Rust match.
pub fn emit_switch(emitter: &mut CodeEmitter, switch_stmt: &swc_ecma_ast::SwitchStmt) {
    emitter.push_str("match ");
    emit_expr(emitter, &switch_stmt.discriminant);
    emitter.push_str(" {\n");
    emitter.inc_indent();

    for case in &switch_stmt.cases {
        emit_switch_case(emitter, case);
    }

    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit a single switch case.
pub fn emit_switch_case(emitter: &mut CodeEmitter, case: &SwitchCase) {
    emitter.push_indent();
    if let Some(test) = &case.test {
        emit_case_pattern_for_test(emitter, test);
    } else {
        emitter.push_str("_");
    }
    emitter.push_str(" {\n");

    emitter.inc_indent();
    for stmt in &case.cons {
        if !matches!(stmt, Stmt::Break(_)) {
            super::emit_single_stmt(emitter, stmt);
        }
    }
    emitter.dec_indent();

    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit case pattern for a test expression.
fn emit_case_pattern_for_test(emitter: &mut CodeEmitter, test: &swc_ecma_ast::Expr) {
    match test {
        swc_ecma_ast::Expr::Member(member) => {
            emit_tagged_variant_pattern(emitter, member);
        }
        swc_ecma_ast::Expr::Ident(ident) => {
            // For enum types, preserve PascalCase; for values, convert to snake_case
            let name = ident.sym.as_ref();
            if is_enum_type(name) {
                emitter.push_str(name);
            } else {
                emitter.push_str(&to_rust_name(name));
            }
        }
        swc_ecma_ast::Expr::Lit(lit) => {
            if let swc_ecma_ast::Lit::Str(s) = lit {
                emitter.push_str(&format!("{:?}", s.value));
            } else {
                emit_expr(emitter, test);
            }
        }
        _ => emit_expr(emitter, test),
    }
    emitter.push_str(" => ");
}

/// Emit a tagged variant pattern from member access.
fn emit_tagged_variant_pattern(emitter: &mut CodeEmitter, member: &swc_ecma_ast::MemberExpr) {
    if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
        let prop_name = prop.sym.as_ref();

        if let swc_ecma_ast::Expr::Ident(type_ident) = &*member.obj {
            let type_name = type_ident.sym.as_ref();
            if is_enum_type(type_name) {
                emitter.push_str(type_name);
            } else {
                emitter.push_str(&to_rust_name(type_name));
            }
            emitter.push_str("::");
            emitter.push_str(prop_name);
            return;
        }

        emit_expr(emitter, &member.obj);
        if prop_name == "tag" {
            emitter.push_str("::");
        } else {
            emitter.push_str(".");
            emitter.push_str(prop_name);
        }
    } else {
        emit_expr(emitter, &swc_ecma_ast::Expr::Member(member.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::emitter::CodeEmitter;

    #[test]
    fn test_emit_switch_basic() {
        let emitter = CodeEmitter::new();
        // Basic test that emitter is created correctly
        assert!(emitter.output().is_empty());
    }

    #[test]
    fn test_is_enum_type_in_switch() {
        // These should be recognized as enum types
        assert!(is_enum_type("Task"));
        assert!(is_enum_type("Message"));
        assert!(is_enum_type("Color"));
        assert!(is_enum_type("Filter"));
    }

    #[test]
    fn test_is_not_enum_type_in_switch() {
        // These should NOT be recognized as enum types
        assert!(!is_enum_type("task"));
        assert!(!is_enum_type("myVariable"));
        assert!(!is_enum_type("taskId"));
    }

    #[test]
    fn test_to_rust_name_in_switch() {
        // Conversion tests for switch cases
        // PascalCase names (enum types) are kept as-is
        assert_eq!("Task", to_rust_name("Task"));
        // camelCase/snake_case are converted to snake_case
        assert_eq!("task", to_rust_name("task"));
        assert_eq!("task_id", to_rust_name("taskId"));
    }
}

// Module emitter functions for module-level constructs.

/// Emit module-level code (types, functions, imports).
///
/// This function is part of the public API for module emission.
/// Actual module header writing is handled by RustEmitter::emit.
#[allow(clippy::unused_self, clippy::needless_pass_by_ref_mut)]
#[allow(dead_code)]
pub fn emit_module(_emitter: &mut super::RustEmitter, _source: &crate::parser::SourceFile) {
    // Module header is already written by RustEmitter::emit
    // This function handles any additional module-level processing
    // Types are emitted during AST walking
}

/// Write type definitions.
#[allow(dead_code)]
pub fn write_types(emitter: &mut super::RustEmitter, types: &[(String, crate::analyzer::TypeInfo)]) {
    for (_, info) in types {
        match info {
            crate::analyzer::TypeInfo::Struct(s) => {
                emitter.push_line(&s.to_rust());
                emitter.push_line("");
            }
            crate::analyzer::TypeInfo::Enum(e) => {
                emitter.push_line(&e.to_rust());
                emitter.push_line("");
            }
            crate::analyzer::TypeInfo::Function(_)
            | crate::analyzer::TypeInfo::Option(_)
            | crate::analyzer::TypeInfo::Result(_, _) => {}
            crate::analyzer::TypeInfo::Unknown
            | crate::analyzer::TypeInfo::Integer(_)
            | crate::analyzer::TypeInfo::Float
            | crate::analyzer::TypeInfo::String
            | crate::analyzer::TypeInfo::StringLiteral(_)
            | crate::analyzer::TypeInfo::Boolean
            | crate::analyzer::TypeInfo::Array(_)
            | crate::analyzer::TypeInfo::Generic(_) => {}
        }
    }
}

#[cfg(test)]
mod module_tests {
    use crate::analyzer::{EnumInfo, EnumVariant, StructInfo, TypeInfo};
    use crate::codegen::emitter::RustEmitter;
    use crate::parser::SourceFile;

    #[test]
    fn test_struct_info_to_rust() {
        let struct_info = StructInfo {
            name: "Task".to_string(),
            fields: vec![
                ("id".to_string(), TypeInfo::Integer(0)),
                ("title".to_string(), TypeInfo::String),
            ],
        };
        let rust = struct_info.to_rust();
        assert!(rust.contains("pub struct Task"));
        assert!(rust.contains("pub id: i32"));
        assert!(rust.contains("pub title: String"));
    }

    #[test]
    fn test_enum_info_to_rust() {
        let enum_info = EnumInfo {
            name: "Filter".to_string(),
            variants: vec![
                EnumVariant {
                    tag: "All".to_string(),
                    fields: vec![],
                },
                EnumVariant {
                    tag: "Active".to_string(),
                    fields: vec![],
                },
            ],
        };
        let rust = enum_info.to_rust();
        assert!(rust.contains("pub enum Filter"));
        assert!(rust.contains("All"));
        assert!(rust.contains("Active"));
    }

    #[test]
    fn test_enum_with_fields_to_rust() {
        let enum_info = EnumInfo {
            name: "Message".to_string(),
            variants: vec![
                EnumVariant {
                    tag: "Move".to_string(),
                    fields: vec![
                        ("x".to_string(), TypeInfo::Float),
                        ("y".to_string(), TypeInfo::Float),
                    ],
                },
                EnumVariant {
                    tag: "Quit".to_string(),
                    fields: vec![],
                },
            ],
        };
        let rust = enum_info.to_rust();
        assert!(rust.contains("Move"));
        assert!(rust.contains("x: f64"));
        assert!(rust.contains("y: f64"));
        assert!(rust.contains("Quit"));
    }

    #[test]
    fn test_rust_emitter_new() {
        let source = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: crate::parser::SourceKind::TypeScript,
            source: String::new(),
            name: String::from("test"),
            valid: true,
            errors: Vec::new(),
        };
        let analysis = crate::analyzer::AnalysisResult::default();
        let _emitter = RustEmitter::new(&source, &analysis);
        // Just verify it creates without panic
    }
}
