//! No bytecode-shape fingerprints: the engine never recognizes a function by the exact form
//! its bytecode happens to take. A test of how many instructions a function has, of which
//! instruction stands at a fixed position, or of which numbered register an instruction
//! names, matches the code one compiler version emits for one source text and nothing else
//! with the same meaning; a specializer decides from semantics (data flow, types, effects).
//!
//! Every function outside tests is counted for `fingerprint`:
//! - an equality (`==`, `!=`, `matches!`, `contains`) of a code length (`code.len()`,
//!   `instructions.len()`) with a nonzero literal or a constant;
//! - an instruction read at a literal position past the entry (`code[3]`, `code.get(3)`,
//!   `function_instruction(function, 3)`);
//! - an equality of a register an instruction names (`register_a()`, `result_register()`,
//!   `register_index()`, ...) with a literal; a parameter's register is asked by its index.
//!
//! The count joins the shrink-only measures in `oversized.txt`, which lists none: every
//! fingerprint fails.

use quote::ToTokens;
use syn::visit::Visit;

use crate::shape::Measures;

/// Receivers whose elements are a function's instructions.
const CODE: &[&str] = &["code", "instructions", "wide"];
/// Accessors of the registers an instruction names.
const REGISTERS: &[&str] = &["register_a", "register_b", "register_c", "result_register", "register_index"];
/// Calls that read one instruction of a function by position.
const POSITIONAL_READS: &[&str] = &["function_instruction"];

/// Records, by `fingerprint <subject>`, how many shape tests a function's body makes.
pub fn scan(subject: &str, body: &syn::Block, measures: &mut Measures) {
    if crate::in_test_path(subject) {
        return;
    }
    let mut tests = Tests(0);
    tests.visit_block(body);
    if tests.0 > 0 {
        measures.insert(format!("fingerprint {subject}"), tests.0);
    }
}

struct Tests(usize);

/// The last path segment or method an expression ends with (`function.code` gives `code`).
fn ends_with(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(path) => path.path.segments.last().map(|segment| segment.ident.to_string()),
        syn::Expr::Field(field) => match &field.member {
            syn::Member::Named(name) => Some(name.to_string()),
            syn::Member::Unnamed(_) => None,
        },
        syn::Expr::MethodCall(call) => Some(call.method.to_string()),
        syn::Expr::Reference(reference) => ends_with(&reference.expr),
        syn::Expr::Paren(paren) => ends_with(&paren.expr),
        syn::Expr::Try(attempt) => ends_with(&attempt.expr),
        _ => None,
    }
}

/// Whether an expression holds a function's instructions: a `code`, `instructions` or `wide`
/// field or accessor, or a local `instructions` (a local `code` is as often bytes or text).
fn is_code(expr: &syn::Expr) -> bool {
    let local = matches!(expr, syn::Expr::Path(path) if path.path.is_ident("instructions"));
    local || (!matches!(expr, syn::Expr::Path(_)) && ends_with(expr).is_some_and(|name| CODE.contains(&name.as_str())))
}

/// Whether an expression is a function's code length: `<code>.len()`.
fn is_code_length(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::MethodCall(call) if call.method == "len" => is_code(&call.receiver),
        syn::Expr::Reference(reference) => is_code_length(&reference.expr),
        syn::Expr::Paren(paren) => is_code_length(&paren.expr),
        _ => false,
    }
}

/// Whether an expression is a register an instruction names: `<instruction>.register_a()`.
fn is_register(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::MethodCall(call) => REGISTERS.contains(&call.method.to_string().as_str()),
        syn::Expr::Paren(paren) => is_register(&paren.expr),
        _ => false,
    }
}

/// Whether an expression is an integer literal, optionally as `Some(literal)`.
fn is_literal(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(_), .. }) => true,
        syn::Expr::Call(call) => {
            call.func.to_token_stream().to_string() == "Some" && call.args.len() == 1 && is_literal(&call.args[0])
        }
        syn::Expr::Paren(paren) => is_literal(&paren.expr),
        _ => false,
    }
}

/// Whether an expression is an integer literal other than zero: a position past the entry,
/// where execution starts and which every function has.
fn is_position(expr: &syn::Expr) -> bool {
    is_literal(expr) && is_fixed_count(expr)
}

/// Whether an expression names a fixed count: a nonzero literal or a constant.
fn is_fixed_count(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(int), .. }) => int.base10_digits() != "0",
        syn::Expr::Path(path) => path.path.segments.last().is_some_and(|segment| {
            let name = segment.ident.to_string();
            name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        }),
        syn::Expr::Paren(paren) => is_fixed_count(&paren.expr),
        _ => false,
    }
}

impl Tests {
    /// Counts an equality between `left` and `right` that pins a length or a register.
    fn equality(&mut self, left: &syn::Expr, right: &syn::Expr) {
        let pins = |measure: &syn::Expr, value: &syn::Expr| {
            (is_code_length(measure) && is_fixed_count(value)) || (is_register(measure) && is_literal(value))
        };
        self.0 += usize::from(pins(left, right) || pins(right, left));
    }
}

impl<'ast> Visit<'ast> for Tests {
    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        if crate::is_equality(&binary.op) {
            self.equality(&binary.left, &binary.right);
        }
        syn::visit::visit_expr_binary(self, binary);
    }

    fn visit_expr_index(&mut self, index: &'ast syn::ExprIndex) {
        self.0 += usize::from(is_code(&index.expr) && is_position(&index.index));
        syn::visit::visit_expr_index(self, index);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let positional = is_code(&call.receiver) && call.method == "get" && call.args.first().is_some_and(is_position);
        let pinned = call.method == "contains" && call.args.first().is_some_and(is_code_length);
        self.0 += usize::from(positional || pinned);
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let reads = ends_with(&call.func).is_some_and(|name| POSITIONAL_READS.contains(&name.as_str()));
        self.0 += usize::from(reads && call.args.iter().nth(1).is_some_and(is_position));
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("matches")
            && let Ok(expr) = mac.parse_body_with(|input: syn::parse::ParseStream<'_>| {
                let expr = input.parse::<syn::Expr>()?;
                input.parse::<proc_macro2::TokenStream>()?;
                Ok(expr)
            })
        {
            self.0 += usize::from(is_code_length(&expr) || is_register(&expr));
        }
    }

    /// A nested item is its own subject.
    fn visit_item(&mut self, _: &'ast syn::Item) {}
}
