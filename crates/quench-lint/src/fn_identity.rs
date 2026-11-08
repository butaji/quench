//! `fn-identity`: an equality test on a function's address, counted per file outside tests.
//! LLVM merges functions whose bodies are identical (generic instances whose parameter is only
//! a tag), so one address can stand for several handlers and the test silently matches the
//! wrong one. What a record does belongs in the record's data (an explicit kind), never in
//! its handler's address. The counts join the shrink-only measures in `oversized.txt`.

use quote::ToTokens;
use syn::visit::Visit;

use crate::shape::Measures;

/// Fields that hold a dispatch handler's address.
const HANDLER_FIELDS: &[&str] = &["handler"];

pub fn scan(file: &str, syntax: &syn::File, measures: &mut Measures) {
    let mut tests = Tests(0);
    tests.visit_file(syntax);
    if tests.0 > 0 {
        measures.insert(format!("fn-identity {file}"), tests.0);
    }
}

struct Tests(usize);

/// Whether `ty` erases a function to its address: an untyped pointer (`*const ()`), a bare
/// `fn` type, or a handler type.
fn erases_function(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Ptr(pointer) => matches!(&*pointer.elem, syn::Type::Tuple(unit) if unit.elems.is_empty()),
        syn::Type::BareFn(_) => true,
        syn::Type::Path(path) => path.path.segments.last().is_some_and(|last| last.ident.to_string().ends_with("Handler")),
        _ => false,
    }
}

/// Whether `expr` is a function's address: a value cast to an erased function type
/// (`f::<T> as *const ()`, `f as Handler as *const ()`), or a handler field.
fn is_address(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Cast(cast) => erases_function(&cast.ty) || is_address(&cast.expr),
        syn::Expr::Field(field) => matches!(&field.member, syn::Member::Named(name) if HANDLER_FIELDS.iter().any(|field| name == field)),
        syn::Expr::Paren(inner) => is_address(&inner.expr),
        _ => false,
    }
}

impl<'ast> Visit<'ast> for Tests {
    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        self.0 += usize::from(crate::is_equality(&binary.op) && (is_address(&binary.left) || is_address(&binary.right)));
        syn::visit::visit_expr_binary(self, binary);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        self.0 += usize::from(call.func.to_token_stream().to_string().ends_with("fn_addr_eq"));
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !crate::shape::is_test(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !crate::shape::is_test(&item.attrs) {
            syn::visit::visit_item_fn(self, item);
        }
    }
}
