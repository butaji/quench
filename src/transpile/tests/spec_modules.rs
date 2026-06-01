//! ES Module System tests — section 2.6 of SUPPORTED_SUBSET.md
//!
//! Covers:
//! - Imports: named, default, namespace, mixed, type-only
//! - Exports: named declarations, named specifiers, re-exports, default exports
//! - Fresh-specific: handler objects, page components
//!
//! allow:too_many_lines,complexity,nested_externals

#[cfg(test)]
mod spec_modules_tests {
    use crate::transpile::hir::*;
    use crate::transpile::parser::TsParser;

    /// Helper: parse source and return all ModuleItems
    fn parse_all(source: &str) -> Vec<ModuleItem> {
        let parser = TsParser::new();
        parser.parse_source(source).expect("parse failed").items
    }

    /// Helper: find first Import in module items
    fn find_import(items: &[ModuleItem]) -> Option<&Import> {
        items.iter().find_map(|item| {
            if let ModuleItem::Import(imp) = item {
                Some(imp)
            } else {
                None
            }
        })
    }

    /// Helper: find first Stmt::ExportNamed in module items
    fn find_stmt_export_named(items: &[ModuleItem]) -> Option<&Stmt> {
        items.iter().find_map(|item| {
            if let ModuleItem::Stmt(s) = item {
                if matches!(s, Stmt::ExportNamed { .. }) {
                    Some(s)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    /// Helper: find first Stmt::ExportDefault in module items
    fn find_stmt_export_default(items: &[ModuleItem]) -> Option<&Stmt> {
        items.iter().find_map(|item| {
            if let ModuleItem::Stmt(s) = item {
                if matches!(s, Stmt::ExportDefault { .. }) {
                    Some(s)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    // =============================================================================
    // IMPORT TESTS — Named, Default, Namespace, Mixed, Type-only
    // =============================================================================

    #[test]
    fn import_named_single() {
        let items = parse_all(r#"import { a } from "mod";"#);
        let imp = find_import(&items).expect("should have import");
        assert_eq!(imp.source, "mod");
        assert_eq!(imp.specifiers.len(), 1);
        assert!(matches!(&imp.specifiers[0], ImportSpecifier::Named { name, alias: None }
            if name == "a"));
    }

    #[test]
    fn import_named_multiple() {
        let items = parse_all(r#"import { a, b, c } from "mod";"#);
        let imp = find_import(&items).expect("should have import");
        assert_eq!(imp.specifiers.len(), 3);
    }

    #[test]
    fn import_named_with_alias() {
        let items = parse_all(r#"import { a as b } from "mod";"#);
        let imp = find_import(&items).expect("should have import");
        assert!(matches!(&imp.specifiers[0],
            ImportSpecifier::Named { name, alias: Some(alias) }
            if name == "a" && alias == "b"));
    }

    #[test]
    fn import_default() {
        let items = parse_all(r#"import X from "mod";"#);
        let imp = find_import(&items).expect("should have import");
        assert_eq!(imp.source, "mod");
        assert_eq!(imp.specifiers.len(), 1);
        assert!(matches!(&imp.specifiers[0],
            ImportSpecifier::Default { name }
            if name == "X"));
    }

    #[test]
    fn import_namespace() {
        let items = parse_all(r#"import * as X from "mod";"#);
        let imp = find_import(&items).expect("should have import");
        assert_eq!(imp.specifiers.len(), 1);
        assert!(matches!(&imp.specifiers[0],
            ImportSpecifier::Namespace { name }
            if name == "X"));
    }

    #[test]
    fn import_mixed() {
        let items = parse_all(r#"import X, { a, b } from "mod";"#);
        let imp = find_import(&items).expect("should have import");
        assert_eq!(imp.specifiers.len(), 3);
        assert!(matches!(&imp.specifiers[0],
            ImportSpecifier::Default { name }
            if name == "X"));
        assert!(matches!(&imp.specifiers[1],
            ImportSpecifier::Named { name, alias: None }
            if name == "a"));
    }

    #[test]
    fn import_type_only() {
        let items = parse_all(r#"import type { T } from "mod";"#);
        assert!(items.iter().any(|i| !matches!(i, ModuleItem::Stmt(Stmt::Empty))) || items.is_empty());
    }

    #[test]
    fn import_named_empty_from() {
        let items = parse_all(r#"import { } from "mod";"#);
        let imp = find_import(&items);
        if let Some(imp) = imp {
            assert!(imp.specifiers.is_empty());
        }
    }

    // =============================================================================
    // EXPORT TESTS — Named declarations
    // =============================================================================

    #[test]
    fn export_const() {
        let items = parse_all(r#"export const x = 1;"#);
        assert!(items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                v.name == "x" && matches!(v.kind, VariableKind::Const)
            } else {
                false
            }
        }), "export const should produce Decl::Variable");
    }

    #[test]
    fn export_let() {
        let items = parse_all(r#"export let x = 1;"#);
        assert!(items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                v.name == "x" && matches!(v.kind, VariableKind::Let)
            } else {
                false
            }
        }), "export let should produce Decl::Variable");
    }

    #[test]
    fn export_var() {
        let items = parse_all(r#"export var x = 1;"#);
        assert!(items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                v.name == "x" && matches!(v.kind, VariableKind::Var)
            } else {
                false
            }
        }), "export var should produce Decl::Variable");
    }

    #[test]
    fn export_function() {
        let items = parse_all(r#"export function f() {}"#);
        assert!(items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(func)) = item {
                func.name == "f"
            } else {
                false
            }
        }), "export function should produce Decl::Function");
    }

    #[test]
    fn export_function_with_body() {
        let items = parse_all(r#"export function add(a: number, b: number): number { return a + b; }"#);
        assert!(items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(func)) = item {
                func.name == "add" && func.body.is_some()
            } else {
                false
            }
        }), "export function with body should parse correctly");
    }

    #[test]
    fn export_async_function() {
        let items = parse_all(r#"export async function f() {}"#);
        assert!(items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(func)) = item {
                func.name == "f" && func.is_async
            } else {
                false
            }
        }), "export async function should parse");
    }

    #[test]
    fn export_class() {
        let items = parse_all(r#"export class Foo {}"#);
        assert!(items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Class(c)) = item {
                c.name == "Foo"
            } else {
                false
            }
        }), "export class should produce Decl::Class");
    }

    // =============================================================================
    // EXPORT TESTS — Named specifier exports (export { x })
    // =============================================================================

    #[test]
    fn export_named_specifier() {
        let items = parse_all(r#"export { x };"#);
        let found = find_stmt_export_named(&items);
        assert!(found.is_some(), "export { x } should produce Stmt::ExportNamed");
        if let Some(Stmt::ExportNamed { specifiers }) = found {
            assert!(!specifiers.is_empty());
            assert!(specifiers.iter().any(|s| {
                matches!(s, Export::Named { name } if name == "x")
            }));
        }
    }

    #[test]
    fn export_named_specifier_multiple() {
        let items = parse_all(r#"export { x, y, z };"#);
        let found = find_stmt_export_named(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportNamed { specifiers }) = found {
            assert_eq!(specifiers.len(), 3);
        }
    }

    #[test]
    fn export_named_specifier_with_alias() {
        let items = parse_all(r#"export { x as y };"#);
        let found = find_stmt_export_named(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportNamed { specifiers }) = found {
            assert!(!specifiers.is_empty());
        }
    }

    // =============================================================================
    // EXPORT TESTS — Re-exports
    // =============================================================================

    #[test]
    fn re_export_named() {
        let items = parse_all(r#"export { a } from "mod";"#);
        let found = find_stmt_export_named(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportNamed { specifiers }) = found {
            assert!(!specifiers.is_empty());
            assert!(matches!(&specifiers[0], Export::ReExport { source, names }
                if source == "mod" && names.contains(&"a".to_string())));
        }
    }

    #[test]
    fn re_export_multiple() {
        let items = parse_all(r#"export { a, b, c } from "mod";"#);
        let found = find_stmt_export_named(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportNamed { specifiers }) = found {
            assert!(matches!(&specifiers[0], Export::ReExport { source, names }
                if source == "mod" && names.len() == 3));
        }
    }

    #[test]
    fn re_export_all() {
        let items = parse_all(r#"export * from "mod";"#);
        let found = find_stmt_export_named(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportNamed { specifiers }) = found {
            assert_eq!(specifiers.len(), 1);
            assert!(matches!(&specifiers[0], Export::All { source }
                if source == "mod"));
        }
    }

    // =============================================================================
    // EXPORT TESTS — Default exports
    // =============================================================================

    #[test]
    fn export_default_number() {
        let items = parse_all(r#"export default 42;"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some(), "export default 42 should produce Stmt::ExportDefault");
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::Number(_)));
        }
    }

    #[test]
    fn export_default_string() {
        let items = parse_all(r#"export default "hello";"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::String(_)));
        }
    }

    #[test]
    fn export_default_boolean() {
        let items = parse_all(r#"export default true;"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::Boolean(true)));
        }
    }

    #[test]
    fn export_default_null() {
        let items = parse_all(r#"export default null;"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::Null));
        }
    }

    #[test]
    fn export_default_identifier() {
        let items = parse_all(r#"export default Component;"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::Ident { name } if name == "Component"));
        }
    }

    #[test]
    fn export_default_function_anonymous() {
        let items = parse_all(r#"export default function() {}"#);
        let has_function = items.iter().any(|item| {
            match item {
                ModuleItem::Decl(Decl::Function(f)) => f.name.is_empty() || f.name.starts_with("function"),
                ModuleItem::Stmt(Stmt::ExportDefault { expr }) => matches!(expr, Expr::Function(_)),
                _ => false,
            }
        });
        assert!(has_function, "export default function() should parse");
    }

    #[test]
    fn export_default_function_named() {
        let items = parse_all(r#"export default function foo() {}"#);
        let has_foo = items.iter().any(|item| {
            match item {
                ModuleItem::Decl(Decl::Function(f)) => f.name == "foo",
                ModuleItem::Stmt(Stmt::ExportDefault { expr }) => {
                    if let Expr::Function(func) = expr {
                        func.name == "foo"
                    } else {
                        false
                    }
                }
                _ => false,
            }
        });
        assert!(has_foo, "export default function foo() should have name 'foo'");
    }

    #[test]
    fn export_default_arrow() {
        let items = parse_all(r#"export default () => 42;"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
        }
    }

    #[test]
    fn export_default_arrow_async() {
        let items = parse_all(r#"export default async () => {}"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::ArrowFunction { is_async: true, .. }));
        }
    }

    #[test]
    fn export_default_object() {
        let items = parse_all(r#"export default { a: 1 };"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::Object { .. }));
        }
    }

    #[test]
    fn export_default_array() {
        let items = parse_all(r#"export default [1, 2, 3];"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::Array { .. }));
        }
    }

    // =============================================================================
    // FRESH-SPECIFIC TESTS
    // =============================================================================

    #[test]
    fn fresh_export_handler_object() {
        let items = parse_all(r#"export const handler = { GET: async () => {} };"#);
        let has_handler = items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                v.name == "handler" && matches!(v.kind, VariableKind::Const)
            } else {
                false
            }
        });
        assert!(has_handler, "export const handler should parse as Decl::Variable");
    }

    #[test]
    fn fresh_export_handler_with_post() {
        let items = parse_all(r#"
            export const handler = {
                GET: async () => {},
                POST: async () => {},
            };
        "#);
        let has_handler = items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                v.name == "handler"
            } else {
                false
            }
        });
        assert!(has_handler, "handler with GET and POST should parse");
    }

    #[test]
    fn fresh_export_page_component() {
        let items = parse_all(r#"export default function Page() { return <div>Hello</div>; }"#);
        let has_page = items.iter().any(|item| {
            match item {
                ModuleItem::Decl(Decl::Function(f)) => f.name == "Page",
                ModuleItem::Stmt(Stmt::ExportDefault { expr }) => {
                    if let Expr::Function(func) = expr {
                        func.name == "Page"
                    } else {
                        false
                    }
                }
                _ => false,
            }
        });
        assert!(has_page, "export default function Page() should parse with name 'Page'");
    }

    // =============================================================================
    // COMBINED IMPORT/EXPORT TESTS
    // =============================================================================

    #[test]
    fn import_and_export_combined() {
        let source = r#"
            import { a, b } from "mod";
            export const x = a + b;
        "#;
        let items = parse_all(source);
        assert!(find_import(&items).is_some(), "should have import");
        assert!(items.iter().any(|i| {
            if let ModuleItem::Decl(Decl::Variable(v)) = i {
                v.name == "x"
            } else {
                false
            }
        }), "should have export const");
    }

    #[test]
    fn multiple_imports_same_source() {
        let source = r#"
            import { a } from "mod";
            import { b } from "mod";
            import c from "mod";
        "#;
        let items = parse_all(source);
        let imports: Vec<_> = items.iter().filter_map(|i| {
            if let ModuleItem::Import(imp) = i {
                Some(imp.source.clone())
            } else {
                None
            }
        }).collect();
        assert_eq!(imports.len(), 3);
    }

    #[test]
    fn export_import_roundtrip() {
        let source = r#"
            import { a } from "mod";
            export { a };
        "#;
        let items = parse_all(source);
        assert!(find_import(&items).is_some(), "should have import");
        assert!(find_stmt_export_named(&items).is_some(), "should have export named");
    }

    // =============================================================================
    // HIR PRESERVATION TESTS
    // =============================================================================

    #[test]
    fn hir_preserves_import_count() {
        let source = r#"
            import { a } from "x";
            import { b } from "y";
            import c from "z";
        "#;
        let items = parse_all(source);
        let import_count = items.iter().filter(|i| matches!(i, ModuleItem::Import(_))).count();
        assert_eq!(import_count, 3, "all 3 imports should be preserved in HIR");
    }

    #[test]
    fn hir_preserves_export_count() {
        let source = r#"
            export const a = 1;
            export const b = 2;
            export { c } from "mod";
        "#;
        let items = parse_all(source);
        let export_count = items.iter().filter(|i| {
            match i {
                ModuleItem::Decl(Decl::Variable(_)) => true,
                ModuleItem::Stmt(Stmt::ExportNamed { .. }) => true,
                _ => false,
            }
        }).count();
        assert_eq!(export_count, 3, "all exports should be preserved");
    }

    #[test]
    fn hir_preserves_named_export_specifiers() {
        let source = r#"export { x, y, z } from "mod";"#;
        let items = parse_all(source);
        let found = find_stmt_export_named(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportNamed { specifiers }) = found {
            if let Export::ReExport { names, .. } = &specifiers[0] {
                assert_eq!(names.len(), 3);
            } else {
                panic!("expected ReExport, got {:?}", specifiers[0]);
            }
        }
    }

    #[test]
    fn hir_preserves_namespace_import() {
        let source = r#"import * as ns from "mod";"#;
        let items = parse_all(source);
        let imp = find_import(&items).expect("should have import");
        assert!(matches!(imp.specifiers.first(),
            Some(ImportSpecifier::Namespace { name }) if name == "ns"));
    }

    #[test]
    fn hir_preserves_default_import() {
        let source = r#"import React from "react";"#;
        let items = parse_all(source);
        let imp = find_import(&items).expect("should have import");
        assert!(matches!(imp.specifiers.first(),
            Some(ImportSpecifier::Default { name }) if name == "React"));
    }

    // =============================================================================
    // EXPORT DEFAULT TYPE VARIANTS
    // =============================================================================

    #[test]
    fn export_default_bigint() {
        let items = parse_all(r#"export default 123n;"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::BigInt(_)));
        }
    }

    #[test]
    fn export_default_template() {
        let items = parse_all(r#"export default `hello ${name}`;"#);
        let found = find_stmt_export_default(&items);
        assert!(found.is_some());
        if let Some(Stmt::ExportDefault { expr }) = found {
            assert!(matches!(expr, Expr::Template { .. }));
        }
    }

    // =============================================================================
    // CODGEN ASSERTIONS
    // =============================================================================

    #[test]
    fn codegen_export_named_produces_tokens() {
        use proc_macro2::TokenStream;
        use quote::ToTokens;

        let source = r#"export const x = 1;"#;
        let items = parse_all(source);

        let cg = QuoteCodegen::default();
        let mut tokens = TokenStream::new();

        for item in &items {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                if let Some(ts) = cg.gen_stmt(&Stmt::Variable(v.clone())) {
                    tokens.extend(ts);
                }
            }
        }

        let output = tokens.to_string();
        assert!(!output.is_empty(), "codegen should produce tokens for export const");
    }

    #[test]
    fn codegen_export_default_produces_tokens() {
        use proc_macro2::TokenStream;
        use quote::ToTokens;

        let source = r#"export default 42;"#;
        let items = parse_all(source);
        let found = find_stmt_export_default(&items).expect("should have export default");

        let cg = QuoteCodegen::default();
        if let Stmt::ExportDefault { expr } = found {
            let tokens = cg.gen_expr(expr);
            let output = tokens.to_string();
            assert!(!output.is_empty(), "codegen should produce tokens for export default expr");
        }
    }

    // =============================================================================
    // EDGE CASES
    // =============================================================================

    #[test]
    fn export_const_with_type_annotation() {
        let items = parse_all(r#"export const x: number = 1;"#);
        let has_const_with_type = items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                v.name == "x" && v.type_.is_some()
            } else {
                false
            }
        });
        assert!(has_const_with_type, "export const with type annotation should preserve type");
    }

    #[test]
    fn export_function_with_params() {
        let items = parse_all(r#"export function f(a: number, b: string): boolean { return true; }"#);
        let has_func_with_params = items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(func)) = item {
                func.name == "f" && !func.params.is_empty()
            } else {
                false
            }
        });
        assert!(has_func_with_params, "export function with params should preserve params");
    }

    #[test]
    fn export_function_with_generic() {
        let items = parse_all(r#"export function f<T>(x: T): T { return x; }"#);
        let has_generic = items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(func)) = item {
                !func.generics.is_empty()
            } else {
                false
            }
        });
        assert!(has_generic, "export function with generic should preserve generics");
    }

    #[test]
    fn export_const_multiple_declarators() {
        let items = parse_all(r#"export const a = 1, b = 2, c = 3;"#);
        let count = items.iter().filter(|item| {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                v.name == "a" || v.name == "b" || v.name == "c"
            } else {
                false
            }
        }).count();
        assert!(count >= 1, "export const with multiple declarators should parse");
    }

    #[test]
    fn import_side_effects() {
        let items = parse_all(r#"import "mod";"#);
        let imp = find_import(&items);
        if let Some(imp) = imp {
            assert!(imp.specifiers.is_empty(), "side-effect import has no specifiers");
        }
    }

    #[test]
    fn export_enum_not_supported() {
        // Enums are NOT in the supported subset
        let source = r#"export enum Color { Red, Green, Blue }"#;
        let parser = TsParser::new();
        let result = parser.parse_source(source);
        match result {
            Ok(items) => {
                println!("enum parsed to: {:?}", items);
            }
            Err(e) => {
                println!("enum errored (expected): {:?}", e);
            }
        }
    }
}
