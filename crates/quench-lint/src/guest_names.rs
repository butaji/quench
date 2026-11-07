//! No special cases for particular programs: the engine never tests a guest program's own
//! names (a function's, a property's, a binding's) against a string written in Quench.
//! Such a test makes code fast or correct for one benchmark or library and nothing else.
//!
//! Every function outside tests is counted for `guest-name`: a comparison (`==`, `!=`, a
//! `match` arm, `matches!`) of a string literal with an expression that reads guest names
//! out of the program's own code (`atoms[..]`, `field_name(..)`). The names the language gives
//! meaning to ([`LANGUAGE_NAMES`]) are exempt. Every count is a violation: unlike the size
//! measures, `oversized.txt` holds no allowance for one.

use quote::ToTokens;
use syn::visit::Visit;

use crate::shape::Measures;

/// Expressions that read a name out of the guest program's code. A built-in's dispatch on a
/// property key it defines reads the key at run time instead, and is not counted.
const GUEST_NAME_READS: &[&str] = &["atoms [", "atoms . get (", "field_name (", "field_is ("];

/// Names whose meaning the language defines, which semantics may test for.
const LANGUAGE_NAMES: &[&str] = &[
    "arguments", "eval", "constructor", "prototype", "__proto__", "length", "name", "value",
    "done", "next", "return", "throw", "then", "get", "set", "default", "use strict", "async",
    "await", "yield", "let", "static", "undefined", "NaN", "Infinity", "globalThis",
];

/// The measure every guest-name test is recorded under, by subject.
pub const MEASURE: &str = "guest-name ";

/// Records, by `guest-name <subject>`, how often a function tests a guest name.
pub fn scan(subject: &str, body: &syn::Block, measures: &mut Measures) {
    if crate::in_test_path(subject) {
        return;
    }
    let mut tests = Tests(0);
    tests.visit_block(body);
    if tests.0 > 0 {
        measures.insert(format!("{MEASURE}{subject}"), tests.0);
    }
}

struct Tests(usize);

fn reads_guest_name(expr: &impl ToTokens) -> bool {
    let text = expr.to_token_stream().to_string();
    GUEST_NAME_READS.iter().any(|read| text.contains(read))
}

/// The program-specific string literals among `tokens`.
fn specific_literals(tokens: &impl ToTokens) -> usize {
    tokens.to_token_stream().into_iter().map(|token| match token {
        proc_macro2::TokenTree::Group(group) => specific_literals(&group.stream()),
        proc_macro2::TokenTree::Literal(literal) => syn::parse_str::<syn::LitStr>(&literal.to_string())
            .is_ok_and(|text| !text.value().is_empty() && !LANGUAGE_NAMES.contains(&text.value().as_str()))
            .into(),
        _ => 0,
    }).sum()
}

/// Whether `expr` compares anything with a program-specific string literal.
fn compares_literal(expr: &syn::Expr) -> bool {
    struct Finds(bool);
    impl<'ast> Visit<'ast> for Finds {
        fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
            self.0 |= matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_))
                && specific_literals(&binary.left) + specific_literals(&binary.right) > 0;
            syn::visit::visit_expr_binary(self, binary);
        }
    }
    let mut finds = Finds(false);
    finds.visit_expr(expr);
    finds.0
}

impl<'ast> Visit<'ast> for Tests {
    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        if matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
            let (left, right) = (&*binary.left, &*binary.right);
            let tested = (reads_guest_name(left) && specific_literals(right) > 0)
                || (reads_guest_name(right) && specific_literals(left) > 0);
            self.0 += usize::from(tested);
        }
        syn::visit::visit_expr_binary(self, binary);
    }

    fn visit_expr_match(&mut self, expr: &'ast syn::ExprMatch) {
        if reads_guest_name(&expr.expr) {
            self.0 += expr.arms.iter().filter(|arm| specific_literals(&arm.pat) > 0).count();
        }
        syn::visit::visit_expr_match(self, expr);
    }

    /// A helper that tests a guest name against its argument (`field_is(.., "name")`).
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let tester = call.func.to_token_stream().to_string().ends_with("field_is");
        self.0 += usize::from(tester && call.args.iter().any(|arg| specific_literals(arg) > 0));
        syn::visit::visit_expr_call(self, call);
    }

    /// A name read and then tested in a closure (`atoms.get(atom).is_some_and(|name| name == "x")`).
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if reads_guest_name(&call.receiver) {
            let mut inner = Tests(0);
            call.args.iter().for_each(|arg| inner.visit_expr(arg));
            let compares = call.args.iter().any(|arg| compares_literal(arg));
            self.0 += usize::from(inner.0 == 0 && compares);
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("matches") && reads_guest_name(&mac.tokens) {
            self.0 += usize::from(specific_literals(&mac.tokens) > 0);
        }
    }

    /// A nested function is its own subject.
    fn visit_item(&mut self, _: &'ast syn::Item) {}
}
