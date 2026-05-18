//! # Switch Statement Emitter
//!
//! Emits Rust match expressions from TypeScript switch statements.

use super::types::{is_enum_type, to_rust_name};
use super::utils::to_pascal_case;
use super::{emit_expr, CodeEmitter};
use swc_ecma_ast::{Stmt, SwitchCase};

/// Emit a switch statement as Rust match.
pub fn emit_switch(emitter: &mut CodeEmitter, switch_stmt: &swc_ecma_ast::SwitchStmt) {
    // Determine if we're switching on a tag field (e.g., status.tag)
    let discriminant = &switch_stmt.discriminant;
    let (is_tag_access, type_name) = extract_tag_access_info(discriminant);

    if is_tag_access {
        // We're switching on .tag, so emit variant patterns
        emit_switch_as_enum_match(emitter, switch_stmt, type_name.as_deref());
    } else {
        // Regular switch on a value
        emitter.push_str("match ");
        emit_expr(emitter, discriminant);
        emitter.push_str(" {\n");
        emitter.inc_indent();

        for case in &switch_stmt.cases {
            emit_switch_case(emitter, case);
        }

        emitter.dec_indent();
        emitter.push_indent();
        emitter.push_str("}\n");
    }
}

/// Check if the discriminant is a tag access (e.g., msg.tag) and extract type name.
fn extract_tag_access_info(expr: &swc_ecma_ast::Expr) -> (bool, Option<String>) {
    if let swc_ecma_ast::Expr::Member(member) = expr {
        if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
            if prop.sym.as_ref() == "tag" {
                // Extract the type name from the object
                if let swc_ecma_ast::Expr::Ident(type_ident) = &*member.obj {
                    let name = type_ident.sym.as_ref();
                    // Check if this looks like a custom type
                    // Could be PascalCase (Status) or same as enum name (status)
                    // We infer the enum name by converting to PascalCase
                    let inferred_enum = to_pascal_case(name);
                    return (true, Some(inferred_enum));
                }
            }
        }
    }
    (false, None)
}

/// Emit switch as enum match when switching on .tag field.
fn emit_switch_as_enum_match(
    emitter: &mut CodeEmitter,
    switch_stmt: &swc_ecma_ast::SwitchStmt,
    type_name: Option<&str>,
) {
    // For a tagged union switch, we match on the variant directly
    // e.g., switch (status.tag) { case "Move": ... }
    // becomes match status { Status::Move => ... }

    emitter.push_str("match ");

    // Emit the discriminant (the enum variable, not the tag)
    if let swc_ecma_ast::Expr::Member(member) = &*switch_stmt.discriminant {
        emit_expr(emitter, &member.obj);
    } else {
        emit_expr(emitter, &switch_stmt.discriminant);
    }

    emitter.push_str(" {\n");
    emitter.inc_indent();

    for case in &switch_stmt.cases {
        emit_enum_case(emitter, case, type_name);
    }

    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit a switch case as enum variant pattern.
fn emit_enum_case(emitter: &mut CodeEmitter, case: &SwitchCase, type_name: Option<&str>) {
    emitter.push_indent();
    if let Some(test) = &case.test {
        emit_variant_pattern(emitter, test, type_name);
    } else {
        emitter.push_str("_");
    }
    emitter.push_str(" => {\n");

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

/// Emit a case test as enum variant pattern.
fn emit_variant_pattern(
    emitter: &mut CodeEmitter,
    test: &swc_ecma_ast::Expr,
    type_name: Option<&str>,
) {
    match test {
        swc_ecma_ast::Expr::Lit(lit) => {
            if let swc_ecma_ast::Lit::Str(s) = lit {
                // Convert "pending" to "TypeName::Pending"
                let s_str = format!("{:?}", s.value);
                let variant_name = to_pascal_case(s_str.trim_matches('"'));
                if let Some(ty) = type_name {
                    emitter.push_str(ty);
                    emitter.push_str("::");
                }
                emitter.push_str(&variant_name);
            } else {
                emit_expr(emitter, test);
            }
        }
        _ => emit_expr(emitter, test),
    }
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
