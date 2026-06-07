//! Spec: Async Runtime Patterns
//!
//! Tests for async runtime pattern transpilation:
//! - fetch() -> reqwest::get()
//! - setTimeout/setInterval -> tokio::time::sleep/interval
//! - new Promise() -> tokio::sync::oneshot::channel()
//! - for await (... of stream) -> while let Some(x) = stream.next().await

#[cfg(test)]
mod spec_async_runtime_tests {
    use crate::transpile::hir::{Decl, Expr, FunctionDecl, ModuleItem, QuoteCodegen, Stmt};
    use proc_macro2::TokenStream;
    use quote::ToTokens;

    fn parse_source(source: &str) -> Vec<ModuleItem> {
        let parser = crate::transpile::parser::TsParser::new();
        parser.parse_source(source).expect("parse failed").items
    }

    fn find_function(source: &str) -> FunctionDecl {
        let items = parse_source(source);
        for item in &items {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                return f.clone();
            }
        }
        panic!("no function found in: {}", source);
    }

    fn find_expr_in_var(source: &str) -> Expr {
        let items = parse_source(source);
        for item in &items {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                if let Some(init) = &v.init {
                    return init.clone();
                }
            }
            if let ModuleItem::Stmt(Stmt::Variable(v)) = item {
                if let Some(init) = &v.init {
                    return init.clone();
                }
            }
        }
        panic!("no expression found in: {}", source);
    }

    fn codegen_expr(expr: &Expr) -> TokenStream {
        QuoteCodegen::default().gen_expr(expr)
    }

    fn codegen_fn(func: &FunctionDecl) -> TokenStream {
        QuoteCodegen::default().gen_fn(func)
    }

    fn to_string(ts: &TokenStream) -> String {
        ts.to_string()
            .replace(" :: ", "::")
            .replace(" . ", ".")
            .replace("await ?", "await?")
            .replace("next ()", "next()")
            .replace("println !", "println!")
    }

    // fetch() -> reqwest::get()

    #[test]
    fn fetch_basic() {
        let expr = find_expr_in_var(r#"const r = fetch("https://api.example.com/data");"#);
        let s = to_string(&codegen_expr(&expr));
        assert!(s.contains("reqwest::get"), "{}", s);
        assert!(s.contains(".await?"), "{}", s);
    }

    #[test]
    fn fetch_with_var() {
        let expr = find_expr_in_var("const r = fetch(url);");
        let s = to_string(&codegen_expr(&expr));
        assert!(s.contains("reqwest::get"), "{}", s);
    }

    #[test]
    #[ignore = "async fetch patterns not implemented in compile path"]
    fn fetch_in_async_fn() {
        let func = find_function(r#"async function f() { return await fetch("url"); }"#);
        let s = to_string(&codegen_fn(&func));
        assert!(s.contains("reqwest::get"), "{}", s);
    }

    // setTimeout/setInterval -> tokio timers
    // Note: setTimeout/setInterval as bare expression statements cannot be parsed
    // because find_expr_in_var expects variable declarations. These require
    // statement-level expression handling or a different test approach.

    #[test]
    #[ignore = "setTimeout as bare expression cannot be parsed - find_expr_in_var expects variable declarations"]
    fn set_timeout_basic() {
        let expr = find_expr_in_var(r#"setTimeout(() => console.log("done"), 1000);"#);
        let s = to_string(&codegen_expr(&expr));
        assert!(s.contains("tokio::time::sleep"), "{}", s);
        assert!(s.contains("from_millis"), "{}", s);
    }

    #[test]
    #[ignore = "setInterval as bare expression cannot be parsed - find_expr_in_var expects variable declarations"]
    fn set_interval_basic() {
        let expr = find_expr_in_var(r#"setInterval(() => console.log("tick"), 1000);"#);
        let s = to_string(&codegen_expr(&expr));
        assert!(s.contains("tokio::time::interval"), "{}", s);
    }

    // new Promise() -> tokio::sync::oneshot::channel()

    #[test]
    #[ignore = "Promise constructor not implemented in compile path"]
    fn new_promise_basic() {
        let expr = find_expr_in_var("const p = new Promise((resolve, reject) => { resolve(1); });");
        let s = to_string(&codegen_expr(&expr));
        assert!(s.contains("tokio::sync::oneshot::channel"), "{}", s);
        assert!(s.contains("tokio::spawn"), "{}", s);
    }

    #[test]
    fn promise_resolve() {
        let items = parse_source("const p = Promise.resolve(1);");
        assert!(!items.is_empty());
    }

    #[test]
    fn promise_reject() {
        let items = parse_source("const p = Promise.reject(new Error('fail'));");
        assert!(!items.is_empty());
    }

    #[test]
    fn promise_all() {
        let items = parse_source("const p = Promise.all([p1, p2]);");
        assert!(!items.is_empty());
    }

    #[test]
    fn promise_race() {
        let items = parse_source("const p = Promise.race([p1, p2]);");
        assert!(!items.is_empty());
    }

    // for await (... of stream) -> tokio_stream

    #[test]
    #[ignore = "for await not implemented in compile path"]
    fn for_await_basic() {
        let source = r#"async function f() { for await (const chunk of stream) { console.log(chunk); } }"#;
        let func = find_function(source);
        let s = to_string(&codegen_fn(&func));
        assert!(s.contains("while let Some"), "{}", s);
        assert!(s.contains(".next().await"), "{}", s);
    }

    #[test]
    #[ignore = "for await not implemented in compile path"]
    fn for_await_with_body() {
        let source = r#"async function processAll() { for await (const item of asyncIter) { console.log(item); } }"#;
        let func = find_function(source);
        let s = to_string(&codegen_fn(&func));
        assert!(s.contains("while let Some"), "{}", s);
    }

    #[test]
    fn regular_for_of() {
        let source = r#"function f() { for (const item of items) { console.log(item); } }"#;
        let func = find_function(source);
        let s = to_string(&codegen_fn(&func));
        assert!(!s.contains("while let Some"), "{}", s);
    }

    // Combined async patterns

    #[test]
    #[ignore = "async fetch patterns not implemented in compile path"]
    fn async_fetch_and_await() {
        let source = r#"async function fetchData() { const r = await fetch("url"); return r.json(); }"#;
        let func = find_function(source);
        let s = to_string(&codegen_fn(&func));
        assert!(s.contains("reqwest::get"), "{}", s);
    }

    #[test]
    #[ignore = "new Promise callback with setTimeout requires complex Promise constructor implementation - current Promise handling does not process callback arguments"]
    fn async_with_timer() {
        let source = r#"async function delayed() { await new Promise(resolve => setTimeout(resolve, 1000)); }"#;
        let func = find_function(source);
        let s = to_string(&codegen_fn(&func));
        assert!(s.contains("tokio::time::sleep"), "{}", s);
    }
}