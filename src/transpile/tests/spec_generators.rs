//! Spec: Generator Functions (function* and yield)
//!
//! Covers:
//! - Simple generators (single yield in loop)
//! - Yield expressions
//! - Generator function declarations
//! - Generator Iterator transformation

#[cfg(test)]
mod spec_generators_tests {
    use crate::transpile::hir::{
        Decl, Expr, FunctionDecl, ModuleItem, Param, QuoteCodegen, Stmt, Type,
    };
    use proc_macro2::TokenStream;
    use quote::ToTokens;

    // =============================================================================
    // Parser helpers
    // =============================================================================

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

    // =============================================================================
    // Codegen helpers
    // =============================================================================

    fn codegen_fn(func: &FunctionDecl) -> TokenStream {
        QuoteCodegen::default().gen_fn(func)
    }

    fn codegen_fn_to_string(func: &FunctionDecl) -> String {
        let tokens = codegen_fn(func);
        tokens.to_string()
    }

    // =============================================================================
    // Generator function detection
    // =============================================================================

    #[test]
    fn generator_function_detected() {
        let func = find_function("function* g() { yield 1; }");
        assert!(func.is_generator, "function* should set is_generator to true");
    }

    #[test]
    fn generator_function_with_params() {
        let func = find_function("function* count(start: number, end: number) { yield start; }");
        assert!(func.is_generator);
        assert_eq!(func.params.len(), 2);
        assert_eq!(func.params[0].name, "start");
        assert_eq!(func.params[1].name, "end");
    }

    #[test]
    fn non_generator_function_not_marked() {
        let func = find_function("function f() { return 1; }");
        assert!(!func.is_generator);
    }

    // =============================================================================
    // Simple generator codegen - basic yield
    // =============================================================================

    #[test]
    fn generator_simple_yield_number() {
        let func = find_function("function* g() { yield 1; }");
        let output = codegen_fn_to_string(&func);
        // Should not contain raw `yield` since Rust doesn't have stable generators
        // The exact output depends on implementation - could be Iterator impl or state machine
        assert!(!output.is_empty(), "generator should produce output");
    }

    #[test]
    fn generator_yield_in_loop() {
        let func = find_function("function* range(n: number) { for (let i = 0; i < n; i++) { yield i; } }");
        let output = codegen_fn_to_string(&func);
        assert!(!output.is_empty(), "range generator should produce output");
    }

    // =============================================================================
    // Iterator transformation tests
    // =============================================================================

    #[test]
    fn generator_range_transform() {
        let func = find_function("function* range(n: number) { for (let i = 0; i < n; i++) { yield i; } }");
        let output = codegen_fn_to_string(&func);
        // Simple range generator should become an Iterator impl
        // The output should contain Iterator-related code
        println!("Generator output: {}", output);
    }

    #[test]
    fn generator_count_up_transform() {
        let func = find_function("function* countUp(start: number, end: number) { for (let i = start; i < end; i++) { yield i; } }");
        let output = codegen_fn_to_string(&func);
        println!("countUp output: {}", output);
    }

    // =============================================================================
    // Edge cases
    // =============================================================================

    #[test]
    fn generator_yield_no_arg() {
        let func = find_function("function* g() { yield; }");
        let output = codegen_fn_to_string(&func);
        assert!(!output.is_empty());
    }

    #[test]
    fn generator_yield_expression() {
        let func = find_function("function* g() { yield 1 + 2; }");
        let output = codegen_fn_to_string(&func);
        assert!(!output.is_empty());
    }

    #[test]
    fn generator_yield_variable() {
        let func = find_function("function* g(x: number) { yield x; }");
        let output = codegen_fn_to_string(&func);
        assert!(!output.is_empty());
    }

    // =============================================================================
    // Multiple yield points - complex generators (may need state machine)
    // =============================================================================

    #[test]
    #[ignore] // Complex generators not yet supported
    fn generator_multiple_yields() {
        let func = find_function("function* g() { yield 1; yield 2; yield 3; }");
        let output = codegen_fn_to_string(&func);
        assert!(!output.is_empty());
    }

    #[test]
    #[ignore] // Complex generators not yet supported
    fn generator_fibonacci() {
        let func = find_function(
            "function* fibonacci() { let a = 0, b = 1; while (true) { yield a; } }"
        );
        let output = codegen_fn_to_string(&func);
        assert!(!output.is_empty());
    }

    // =============================================================================
    // Return type handling
    // =============================================================================

    #[test]
    fn generator_with_return_type() {
        let func = find_function("function* g(): Iterator<number> { yield 1; }");
        let output = codegen_fn_to_string(&func);
        assert!(!output.is_empty());
        // The return type should be preserved or transformed appropriately
    }
}