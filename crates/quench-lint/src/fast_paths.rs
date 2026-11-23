//! The fast path is its own small function and the general fallback an out-of-line call
//! (principle P4, TOD-2022/2023): a large `#[inline(always)]` body, or one that runs the whole
//! staged interpreter (`staged::step`), is pasted into every caller and every monomorph.
//!
//! Every function outside tests is counted for:
//! - `inline_always_lines`: an `#[inline(always)]` function over [`MAX_INLINE_ALWAYS_LINES`] lines;
//! - `inline_always_step`: a `staged::step` call inside an `#[inline(always)]` function;
//! - `inline_always_count`: how many `#[inline(always)]` functions the tree holds, one measure.
//!
//! The counts join the shrink-only measures in `oversized.txt`.

use syn::visit::Visit;

use crate::shape::Measures;

/// Above this, an always-inlined body is a slow path pasted into its callers.
pub const MAX_INLINE_ALWAYS_LINES: usize = 40;
const TOTAL: &str = "inline_always_count total";

/// Whether `attrs` hold `#[inline(always)]`.
pub fn always_inlined(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("inline")
            && attr.parse_args::<syn::Ident>().is_ok_and(|argument| argument == "always")
    })
}

/// Records the measures of function `subject`, `lines` long, with `attrs` and `body`.
pub fn scan(subject: &str, lines: usize, attrs: &[syn::Attribute], body: &syn::Block, measures: &mut Measures) {
    if !always_inlined(attrs) {
        return;
    }
    *measures.entry(TOTAL.to_owned()).or_insert(0) += 1;
    if lines > MAX_INLINE_ALWAYS_LINES {
        measures.insert(format!("inline_always_lines {subject}"), lines);
    }
    let mut steps = StagedSteps(0);
    steps.visit_block(body);
    if steps.0 > 0 {
        measures.insert(format!("inline_always_step {subject}"), steps.0);
    }
}

/// Counts calls of `staged::step` (any path ending in `staged::step`).
struct StagedSteps(usize);

impl<'ast> Visit<'ast> for StagedSteps {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func {
            let segments: Vec<String> = path.path.segments.iter().map(|segment| segment.ident.to_string()).collect();
            if segments.ends_with(&["staged".to_owned(), "step".to_owned()]) {
                self.0 += 1;
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}
