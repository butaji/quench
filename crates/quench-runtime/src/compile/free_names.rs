//! The names each function's subtree reads or writes from outside itself.
//!
//! A function compiled lazily (TOD-196) still decides its enclosing functions' layouts at
//! load: they keep in their environments exactly the bindings some nested function captures.
//! Those captures are the bindings, declared in scopes around the function, that an
//! identifier inside its subtree resolves to. Semantic analysis gives each reference its
//! symbol, and each symbol its declaring scope, so the set follows from the tree without
//! compiling the function's code.

use oxc_ast::ast::{ArrowFunctionExpression, Function, IdentifierReference, Program};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, Semantic};
use oxc_syntax::scope::{ScopeFlags, ScopeId};
use rustc_hash::FxHashMap;

/// The names a function's subtree resolves outside it, by the function's source start:
/// sorted and without repeats, with the names the source declares nowhere.
pub(super) type FreeNames = FxHashMap<u32, Box<[String]>>;

/// The free names of every function and arrow of `program`.
pub(super) fn free_names<'a>(semantic: &Semantic<'a>, program: &Program<'a>) -> FreeNames {
    let mut collector = Collector { scoping: semantic.scoping(), open: Vec::new(), found: FxHashMap::default() };
    collector.visit_program(program);
    collector
        .found
        .into_iter()
        .map(|(start, mut names)| {
            names.sort_unstable();
            names.dedup();
            (start, names.into_boxed_slice())
        })
        .collect()
}

struct Collector<'s> {
    scoping: &'s Scoping,
    /// The functions around the visit, outermost first: source start, own scope, and whether
    /// it is an arrow, which has no `arguments` of its own.
    open: Vec<(u32, ScopeId, bool)>,
    found: FxHashMap<u32, Vec<String>>,
}

impl Collector<'_> {
    fn enter(&mut self, start: u32, scope: ScopeId, arrow: bool) {
        self.open.push((start, scope, arrow));
        self.found.entry(start).or_default();
    }
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.enter(function.span.start, function.scope_id(), false);
        walk::walk_function(self, function, flags);
        self.open.pop();
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.enter(arrow.span.start, arrow.scope_id(), true);
        walk::walk_arrow_function_expression(self, arrow);
        self.open.pop();
    }

    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        // A name the source declares nowhere is free in every open function: it may still
        // be bound around the source (an eval's enclosing scopes), or be a global.
        let declared = self
            .scoping
            .get_reference(identifier.reference_id())
            .symbol_id()
            .map(|symbol| self.scoping.symbol_scope_id(symbol));
        // Every open function the declaring scope lies outside of reads the name from
        // outside; the innermost one holding the declaration and those around it do not.
        // An undeclared `arguments` is the nearest non-arrow function's own object.
        let own_arguments = declared.is_none() && identifier.name == "arguments";
        for &(start, scope, arrow) in self.open.iter().rev() {
            if declared.is_some_and(|declared| self.scoping.scope_ancestors(declared).any(|ancestor| ancestor == scope))
                || (own_arguments && !arrow)
            {
                break;
            }
            self.found.get_mut(&start).expect("an open function has an entry").push(identifier.name.to_string());
        }
    }
}
