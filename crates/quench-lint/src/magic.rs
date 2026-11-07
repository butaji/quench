//! Zero magic numbers: every representation bit, spec boundary, protocol value, sentinel,
//! threshold and non-obvious numeric rule has a named owner or derives from a named layout.
//!
//! Every function outside tests is counted for `magic`: a numeric literal in its body other
//! than zero and one (the ordinary index and the arithmetic identities). A literal that names
//! itself (a `const` or `static` item, an enum discriminant) is not in a body and is not
//! counted; neither is a literal inside a macro call, whose tokens are not parsed. The count
//! joins the shrink-only measures in `oversized.txt`: a new literal fails, an old one may only
//! be given its name.

use syn::visit::Visit;

use crate::shape::Measures;

/// Records, by `magic <subject>`, how many unnamed numbers a function's body spells.
pub fn scan(subject: &str, body: &syn::Block, measures: &mut Measures) {
    if crate::in_test_path(subject) {
        return;
    }
    let mut literals = Literals(0);
    literals.visit_block(body);
    if literals.0 > 0 {
        measures.insert(format!("magic {subject}"), literals.0);
    }
}

struct Literals(usize);

/// Whether a literal spells zero or one, which carry no policy of their own.
fn is_identity(literal: &syn::Lit) -> bool {
    match literal {
        syn::Lit::Int(int) => int.base10_parse::<u128>().is_ok_and(|value| value <= 1),
        syn::Lit::Float(float) => float.base10_parse::<f64>().is_ok_and(|value| value == 0.0 || value == 1.0),
        _ => true,
    }
}

impl<'ast> Visit<'ast> for Literals {
    fn visit_lit(&mut self, literal: &'ast syn::Lit) {
        self.0 += usize::from(!is_identity(literal));
    }

    /// A nested item (a local `const`, a nested function) names its own values.
    fn visit_item(&mut self, _: &'ast syn::Item) {}

    /// Type positions (`[u8; 4]`) are layouts, counted where the layout is declared.
    fn visit_type(&mut self, _: &'ast syn::Type) {}
}
