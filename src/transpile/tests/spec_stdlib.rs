//! Spec: JavaScript Standard Library Method Mappings
//!
//! Tests transpilation of common JS stdlib methods to Rust equivalents.

#[cfg(test)]
mod spec_stdlib_tests {
    use crate::transpile::hir::{Decl, Expr, ModuleItem, QuoteCodegen};
    use proc_macro2::TokenStream;
    use quote::ToTokens;

    fn parse_source(source: &str) -> Vec<ModuleItem> {
        let parser = crate::transpile::parser::TsParser::new();
        parser.parse_source(source).expect("parse failed").items
    }

    fn find_expr_in_var(source: &str) -> Expr {
        let items = parse_source(source);
        for item in &items {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                if let Some(ref expr) = v.init {
                    return (*expr).clone();
                }
            }
        }
        Expr::Invalid
    }

    fn find_call_expr(source: &str) -> Expr {
        let items = parse_source(source);
        for item in &items {
            if let ModuleItem::Decl(Decl::Function(ref f)) = item {
                if let Some(ref body) = f.body {
                    for stmt in &body.0 {
                        match stmt {
                            crate::transpile::hir::Stmt::Expr { expr } => return (*expr).clone(),
                            crate::transpile::hir::Stmt::Return { arg } => {
                                if let Some(expr) = arg {
                                    return (*expr).clone();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Expr::Invalid
    }

    fn codegen_expr(expr: &Expr) -> TokenStream {
        QuoteCodegen::default().gen_expr(expr)
    }

    fn assert_codegen_not_null(expr: &Expr, label: &str) {
        let tokens = codegen_expr(expr);
        let s = tokens.to_string();
        assert!(
            !s.contains("Value :: Null") && !s.contains("Value::Null"),
            "{}: codegen produced Value::Null: {}",
            label,
            s
        );
    }

    fn assert_codegen_contains(expr: &Expr, needle: &str, label: &str) {
        let tokens = codegen_expr(expr);
        let s = tokens.to_string();
        assert!(
            s.contains(needle),
            "{}: expected '{}' in codegen, got: {}",
            label,
            needle,
            s
        );
    }

    mod console_methods {
        use super::*;

        #[test]
        fn console_log() {
            let expr = find_call_expr("function f() { console.log('hello'); }");
            assert_codegen_contains(&expr, "println", "console.log");
        }

        #[test]
        fn console_error() {
            let expr = find_call_expr("function f() { console.error('error'); }");
            assert_codegen_contains(&expr, "eprintln", "console.error");
        }

        #[test]
        fn console_warn() {
            let expr = find_call_expr("function f() { console.warn('warning'); }");
            assert_codegen_contains(&expr, "eprintln", "console.warn");
        }

        #[test]
        fn console_info() {
            let expr = find_call_expr("function f() { console.info('info'); }");
            assert_codegen_contains(&expr, "println", "console.info");
        }

        #[test]
        fn console_table() {
            let expr = find_call_expr("function f() { console.table([1, 2, 3]); }");
            assert_codegen_contains(&expr, "println", "console.table");
        }

        #[test]
        fn console_assert() {
            let expr = find_call_expr("function f() { console.assert(true, 'msg'); }");
            assert_codegen_contains(&expr, "assert", "console.assert");
        }
    }

    mod math_methods {
        use super::*;

        #[test]
        fn math_pi() {
            let expr = find_expr_in_var("const x = Math.PI;");
            assert_codegen_contains(&expr, "PI", "Math.PI");
        }

        #[test]
        fn math_sqrt() {
            let expr = find_call_expr("function f() { return Math.sqrt(4); }");
            assert_codegen_not_null(&expr, "Math.sqrt");
        }

        #[test]
        fn math_pow() {
            let expr = find_call_expr("function f() { return Math.pow(2, 3); }");
            assert_codegen_not_null(&expr, "Math.pow");
        }

        #[test]
        fn math_abs() {
            let expr = find_call_expr("function f() { return Math.abs(-5); }");
            assert_codegen_not_null(&expr, "Math.abs");
        }

        #[test]
        fn math_floor() {
            let expr = find_call_expr("function f() { return Math.floor(3.7); }");
            assert_codegen_not_null(&expr, "Math.floor");
        }

        #[test]
        fn math_ceil() {
            let expr = find_call_expr("function f() { return Math.ceil(3.2); }");
            assert_codegen_not_null(&expr, "Math.ceil");
        }

        #[test]
        fn math_round() {
            let expr = find_call_expr("function f() { return Math.round(3.5); }");
            assert_codegen_not_null(&expr, "Math.round");
        }

        #[test]
        fn math_max() {
            let expr = find_call_expr("function f() { return Math.max(1, 5, 3); }");
            assert_codegen_not_null(&expr, "Math.max");
        }

        #[test]
        fn math_min() {
            let expr = find_call_expr("function f() { return Math.min(1, 5, 3); }");
            assert_codegen_not_null(&expr, "Math.min");
        }

        #[test]
        fn math_random() {
            let expr = find_call_expr("function f() { return Math.random(); }");
            assert_codegen_not_null(&expr, "Math.random");
        }
    }

    mod array_methods {
        use super::*;

        #[test]
        fn array_map() {
            let expr = find_call_expr("function f() { return arr.map(x => x * 2); }");
            assert_codegen_not_null(&expr, "array.map");
        }

        #[test]
        fn array_filter() {
            let expr = find_call_expr("function f() { return arr.filter(x => x > 0); }");
            assert_codegen_not_null(&expr, "array.filter");
        }

        #[test]
        fn array_reduce() {
            let expr = find_call_expr("function f() { return arr.reduce((acc, x) => acc + x, 0); }");
            assert_codegen_not_null(&expr, "array.reduce");
        }

        #[test]
        fn array_find() {
            let expr = find_call_expr("function f() { return arr.find(x => x > 0); }");
            assert_codegen_not_null(&expr, "array.find");
        }

        #[test]
        fn array_includes() {
            let expr = find_call_expr("function f() { return arr.includes(5); }");
            assert_codegen_not_null(&expr, "array.includes");
        }

        #[test]
        fn array_indexOf() {
            let expr = find_call_expr("function f() { return arr.indexOf(5); }");
            assert_codegen_not_null(&expr, "array.indexOf");
        }

        #[test]
        fn array_slice() {
            let expr = find_call_expr("function f() { return arr.slice(1, 3); }");
            assert_codegen_not_null(&expr, "array.slice");
        }

        #[test]
        fn array_push() {
            let expr = find_call_expr("function f() { return arr.push(5); }");
            assert_codegen_not_null(&expr, "array.push");
        }

        #[test]
        fn array_pop() {
            let expr = find_call_expr("function f() { return arr.pop(); }");
            assert_codegen_not_null(&expr, "array.pop");
        }

        #[test]
        fn array_shift() {
            let expr = find_call_expr("function f() { return arr.shift(); }");
            assert_codegen_not_null(&expr, "array.shift");
        }

        #[test]
        fn array_unshift() {
            let expr = find_call_expr("function f() { return arr.unshift(1); }");
            assert_codegen_not_null(&expr, "array.unshift");
        }

        #[test]
        fn array_concat() {
            let expr = find_call_expr("function f() { return arr.concat([4, 5]); }");
            assert_codegen_not_null(&expr, "array.concat");
        }

        #[test]
        fn array_join() {
            let expr = find_call_expr("function f() { return arr.join(', '); }");
            assert_codegen_not_null(&expr, "array.join");
        }

        #[test]
        fn array_reverse() {
            let expr = find_call_expr("function f() { return arr.reverse(); }");
            assert_codegen_not_null(&expr, "array.reverse");
        }

        #[test]
        fn array_sort() {
            let expr = find_call_expr("function f() { return arr.sort(); }");
            assert_codegen_not_null(&expr, "array.sort");
        }

        #[test]
        fn array_length() {
            let expr = find_expr_in_var("const x = arr.length;");
            assert_codegen_contains(&expr, "len", "array.length");
        }

        #[test]
        fn array_some() {
            let expr = find_call_expr("function f() { return arr.some(x => x > 0); }");
            assert_codegen_not_null(&expr, "array.some");
        }

        #[test]
        fn array_every() {
            let expr = find_call_expr("function f() { return arr.every(x => x > 0); }");
            assert_codegen_not_null(&expr, "array.every");
        }

        #[test]
        fn array_forEach() {
            let expr = find_call_expr("function f() { arr.forEach(x => console.log(x)); }");
            assert_codegen_not_null(&expr, "array.forEach");
        }
    }

    mod string_methods {
        use super::*;

        #[test]
        fn string_length() {
            let expr = find_expr_in_var("const x = str.length;");
            assert_codegen_contains(&expr, "len", "string.length");
        }

        #[test]
        fn string_slice() {
            let expr = find_call_expr("function f() { return str.slice(1, 3); }");
            assert_codegen_not_null(&expr, "string.slice");
        }

        #[test]
        fn string_substring() {
            let expr = find_call_expr("function f() { return str.substring(1, 3); }");
            assert_codegen_not_null(&expr, "string.substring");
        }

        #[test]
        fn string_split() {
            let expr = find_call_expr("function f() { return str.split(','); }");
            assert_codegen_not_null(&expr, "string.split");
        }

        #[test]
        fn string_trim() {
            let expr = find_call_expr("function f() { return str.trim(); }");
            assert_codegen_not_null(&expr, "string.trim");
        }

        #[test]
        fn string_toLowerCase() {
            let expr = find_call_expr("function f() { return str.toLowerCase(); }");
            assert_codegen_not_null(&expr, "string.toLowerCase");
        }

        #[test]
        fn string_toUpperCase() {
            let expr = find_call_expr("function f() { return str.toUpperCase(); }");
            assert_codegen_not_null(&expr, "string.toUpperCase");
        }

        #[test]
        fn string_includes() {
            let expr = find_call_expr("function f() { return str.includes('hello'); }");
            assert_codegen_not_null(&expr, "string.includes");
        }

        #[test]
        fn string_indexOf() {
            let expr = find_call_expr("function f() { return str.indexOf('hello'); }");
            assert_codegen_not_null(&expr, "string.indexOf");
        }

        #[test]
        fn string_replace() {
            let expr = find_call_expr("function f() { return str.replace('a', 'b'); }");
            assert_codegen_not_null(&expr, "string.replace");
        }

        #[test]
        fn string_startsWith() {
            let expr = find_call_expr("function f() { return str.startsWith('hello'); }");
            assert_codegen_not_null(&expr, "string.startsWith");
        }

        #[test]
        fn string_endsWith() {
            let expr = find_call_expr("function f() { return str.endsWith('world'); }");
            assert_codegen_not_null(&expr, "string.endsWith");
        }

        #[test]
        fn string_charAt() {
            let expr = find_call_expr("function f() { return str.charAt(0); }");
            assert_codegen_not_null(&expr, "string.charAt");
        }

        #[test]
        fn string_concat() {
            let expr = find_call_expr("function f() { return str.concat('abc'); }");
            assert_codegen_not_null(&expr, "string.concat");
        }

        #[test]
        fn string_repeat() {
            let expr = find_call_expr("function f() { return str.repeat(3); }");
            assert_codegen_not_null(&expr, "string.repeat");
        }

        #[test]
        fn string_toString() {
            let expr = find_call_expr("function f() { return str.toString(); }");
            assert_codegen_not_null(&expr, "string.toString");
        }
    }

    mod json_methods {
        use super::*;

        #[test]
        fn json_parse() {
            let expr = find_call_expr("function f() { return JSON.parse(text); }");
            assert_codegen_not_null(&expr, "JSON.parse");
        }

        #[test]
        fn json_stringify() {
            let expr = find_call_expr("function f() { return JSON.stringify(obj); }");
            assert_codegen_not_null(&expr, "JSON.stringify");
        }
    }

    mod promise_methods {
        use super::*;

        #[test]
        fn promise_all() {
            let expr = find_call_expr("function f() { return Promise.all([p1, p2]); }");
            assert_codegen_not_null(&expr, "Promise.all");
        }

        #[test]
        fn promise_race() {
            let expr = find_call_expr("function f() { return Promise.race([p1, p2]); }");
            assert_codegen_not_null(&expr, "Promise.race");
        }

        #[test]
        fn promise_allSettled() {
            let expr = find_call_expr("function f() { return Promise.allSettled([p1, p2]); }");
            assert_codegen_not_null(&expr, "Promise.allSettled");
        }
    }

    mod date_methods {
        use super::*;

        #[test]
        fn new_date() {
            let expr = find_call_expr("function f() { return new Date(); }");
            assert_codegen_not_null(&expr, "new Date");
        }

        #[test]
        fn date_now() {
            let expr = find_expr_in_var("const x = Date.now();");
            assert_codegen_not_null(&expr, "Date.now");
        }
    }

    mod static_members {
        use super::*;

        #[test]
        fn number_max_value() {
            let expr = find_expr_in_var("const x = Number.MAX_VALUE;");
            assert_codegen_contains(&expr, "MAX", "Number.MAX_VALUE");
        }

        #[test]
        fn number_min_value() {
            let expr = find_expr_in_var("const x = Number.MIN_VALUE;");
            assert_codegen_contains(&expr, "MIN", "Number.MIN_VALUE");
        }

        #[test]
        fn number_positive_infinity() {
            let expr = find_expr_in_var("const x = Number.POSITIVE_INFINITY;");
            assert_codegen_contains(&expr, "INFINITY", "Number.POSITIVE_INFINITY");
        }

        #[test]
        fn number_negative_infinity() {
            let expr = find_expr_in_var("const x = Number.NEGATIVE_INFINITY;");
            assert_codegen_contains(&expr, "NEG_INFINITY", "Number.NEGATIVE_INFINITY");
        }

        #[test]
        fn number_nan() {
            let expr = find_expr_in_var("const x = Number.NaN;");
            assert_codegen_contains(&expr, "NAN", "Number.NaN");
        }

        #[test]
        fn math_e() {
            let expr = find_expr_in_var("const x = Math.E;");
            assert_codegen_contains(&expr, "E", "Math.E");
        }

        #[test]
        fn math_sqrt2() {
            let expr = find_expr_in_var("const x = Math.SQRT2;");
            assert_codegen_contains(&expr, "SQRT2", "Math.SQRT2");
        }
    }

    mod new_expressions {
        use super::*;

        #[test]
        fn new_array() {
            let expr = find_call_expr("function f() { return new Array(1, 2, 3); }");
            assert_codegen_not_null(&expr, "new Array");
        }

        #[test]
        fn new_array_with_length() {
            let expr = find_call_expr("function f() { return new Array(5); }");
            assert_codegen_not_null(&expr, "new Array(length)");
        }

        #[test]
        fn new_object() {
            let expr = find_call_expr("function f() { return new Object(); }");
            assert_codegen_not_null(&expr, "new Object");
        }

        #[test]
        fn new_map() {
            let expr = find_call_expr("function f() { return new Map(); }");
            assert_codegen_not_null(&expr, "new Map");
        }

        #[test]
        fn new_set() {
            let expr = find_call_expr("function f() { return new Set(); }");
            assert_codegen_not_null(&expr, "new Set");
        }
    }
}