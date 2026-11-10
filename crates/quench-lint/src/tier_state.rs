//! A tier's lifecycle state changes in one place: `vm/tier_state.rs`. Every other function
//! asks the state a question (a `match`, a predicate) or sends it an event
//! (`Tier::transition`, `Tier::apply`); none builds a state or assigns one.
//!
//! Every function outside tests and outside the module is counted for `tier-state`: an
//! expression that names a state of a `Tier` (`Tier::Warming { .. }`, `JsTier::Cold`), and an
//! assignment whose target is a tier (a place named `tier`, or ending in `_tier`). A pattern
//! naming a state is a question, and is not counted. Every count is a violation: unlike the
//! size measures, `oversized.txt` holds no allowance for one.

use syn::visit::Visit;

use crate::shape::Measures;

/// The module that owns every transition.
const OWNER: &str = "crates/quench-runtime/src/vm/tier_state";

/// The measure every outside change of a tier's state is recorded under, by subject.
pub const MEASURE: &str = "tier-state ";

/// The states of a `Tier`, which are named by no other enum of the engine.
const STATES: &[&str] = &["Warming", "Queued", "Compiling", "Speculative", "GenericInterpreted"];
/// The states a path names only through a `Tier` or its alias, as other enums have them too.
const SHARED_STATES: &[&str] = &["Cold", "Generic"];

/// Records, by `tier-state <subject>`, how often a function builds or assigns a tier's state.
pub fn scan(subject: &str, body: &syn::Block, measures: &mut Measures) {
    if crate::in_test_path(subject) || subject.starts_with(OWNER) {
        return;
    }
    let mut changes = Changes(0);
    changes.visit_block(body);
    if changes.0 > 0 {
        measures.insert(format!("{MEASURE}{subject}"), changes.0);
    }
}

struct Changes(usize);

/// Whether `path` names a state of a tier: `Tier::Warming`, `RegionTier::Cold`.
fn names_state(path: &syn::Path) -> bool {
    let mut segments = path.segments.iter().rev();
    let (Some(state), Some(owner)) = (segments.next(), segments.next()) else { return false };
    let owner = owner.ident.to_string();
    let named = |names: &[&str]| names.iter().any(|name| state.ident == name);
    let tier = owner == "Tier" || owner.ends_with("Tier");
    (tier && (named(STATES) || named(SHARED_STATES))) || (owner == "Self" && named(STATES))
}

/// Whether `target` is a place a tier is stored in.
fn is_tier_place(target: &syn::Expr) -> bool {
    let name = match target {
        syn::Expr::Field(field) => match &field.member {
            syn::Member::Named(ident) => ident.to_string(),
            syn::Member::Unnamed(_) => return false,
        },
        syn::Expr::Unary(unary) => return is_tier_place(&unary.expr),
        syn::Expr::Paren(paren) => return is_tier_place(&paren.expr),
        syn::Expr::Path(path) => path.path.get_ident().map(ToString::to_string).unwrap_or_default(),
        _ => return false,
    };
    name == "tier" || name.ends_with("_tier")
}

impl<'ast> Visit<'ast> for Changes {
    fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
        self.0 += usize::from(names_state(&expr.path));
    }

    fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
        self.0 += usize::from(names_state(&expr.path));
        syn::visit::visit_expr_struct(self, expr);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func {
            self.0 += usize::from(names_state(&path.path));
        }
        call.args.iter().for_each(|arg| self.visit_expr(arg));
    }

    fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
        self.0 += usize::from(is_tier_place(&assign.left));
        syn::visit::visit_expr_assign(self, assign);
    }

    /// A pattern naming a state asks a question, as a path pattern shares an expression's type.
    fn visit_pat(&mut self, _: &'ast syn::Pat) {}

    /// A nested function is its own subject.
    fn visit_item(&mut self, _: &'ast syn::Item) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(source: &str) -> usize {
        let function: syn::ItemFn = syn::parse_str(source).expect("a function");
        let mut changes = Changes(0);
        changes.visit_block(&function.block);
        changes.0
    }

    #[test]
    fn building_or_assigning_a_state_is_a_change() {
        assert_eq!(changes("fn f() { let t = Tier::Warming { entries: 0 }; }"), 1);
        assert_eq!(changes("fn f() { *self.tier = JsTier::Cold; }"), 2);
        assert_eq!(changes("fn f(&mut self) { self.region_tier = other; }"), 1);
        assert_eq!(changes("fn f() -> Self { Self::Queued { past } }"), 1);
    }

    #[test]
    fn asking_and_sending_events_are_not() {
        assert_eq!(changes("fn f() { match tier { Tier::Cold | Tier::Warming { .. } => 1, _ => 2 }; }"), 0);
        assert_eq!(changes("fn f() { tier.apply(Event::DUE, &TIERS); matches!(tier, Tier::Queued { .. }); }"), 0);
        assert_eq!(changes("fn f() { let tiers = other; self.count = 1; Self::Cold; }"), 0);
    }
}
