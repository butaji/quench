//! Which function bodies of a program can run. The closed-world gate (`compile::aot_facts::gate`) asks whether a
//! site that may receive code at run time (an `eval`, a `Function` constructor) can execute, not whether the
//! source contains one. This module answers that with a least fixpoint over the function nodes of the program.
//!
//! Soundness: a function body runs only if its closure is created by code that runs and the closure is called.
//! - A closure is created where its expression appears (a function expression, an arrow, a method), so such a node
//!   is live exactly when its enclosing body is live. Top-level code is live.
//! - A nested declaration is called only through a read of its binding, and a read inside a dead body never runs.
//!   The fixpoint starts from the roots and only grows, so a body is dead only when no run can reach it.
//! - A top-level declaration of a Module is a root: the host imports it. A top-level Script declaration is a global;
//!   it is reached by name (an identifier read, counted above) or as a property of the global object. The program
//!   holds the global object as a value only at a site that materializes it (`global_object_live`); once it does,
//!   every top-level Script declaration is a root, since a computed read (`g[k]`) may name any of them. A computed
//!   read on any other object cannot reach a global, so it roots nothing.
//! - The global object is materialized by a live site that yields it: an unresolved `globalThis`, `window`,
//!   `self`, `global` or `frames`; a top-level `this` in a Script; or the `this` of a sloppy function that is
//!   called without a receiver, which binds the global object. A sloppy function's `this` counts only where the
//!   function's value is read other than as a `new` callee (a constructor built with `new` gets a fresh object);
//!   an anonymous sloppy function may be called plainly, so its `this` always counts. Strict code (a class body,
//!   a `"use strict"` scope, a Module) binds `this` to `undefined`, so its `this` never does.
//! - Code that new code (`eval`, `Function`) could reach is not counted here: a live site in a reachable body
//!   opens the world (`gate::world`), so no run of a Closed program adds code that reads the globals.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, SymbolId};
use oxc_syntax::scope::ScopeFlags;
use std::collections::{HashMap, HashSet};

/// The unresolved names a host binds to the global object (`window`, `self`, `frames` in browsers and their kin).
const GLOBAL_OBJECT_NAMES: [&str; 5] = ["globalThis", "window", "self", "global", "frames"];

/// One function-like node: its span, the function body it encloses, and what its declaration names.
struct Node {
    start: u32,
    end: u32,
    /// The index of the enclosing function node; `None` for top-level code.
    parent: Option<usize>,
    /// A function declaration's binding; `None` for expressions, arrows and methods.
    binding: Option<SymbolId>,
    /// A declaration at top level (a Module's exports, or a Script's globals).
    top_declaration: bool,
    /// An arrow takes its `this` from the enclosing function.
    arrow: bool,
    /// Strict code, where a plain call binds `this` to `undefined`.
    strict: bool,
}

/// A read of a binding: the body it occurs in (`None` for top level), and whether it is a `new` callee.
struct Read {
    symbol: SymbolId,
    body: Option<usize>,
    construct: bool,
}

/// The live function bodies of one program (`of`).
pub(super) struct Reachability {
    nodes: Vec<Node>,
    live: Vec<bool>,
}

struct Collect<'s> {
    scoping: &'s Scoping,
    nodes: Vec<Node>,
    /// The function nodes whose bodies the walk is inside, innermost last.
    open: Vec<usize>,
    reads: Vec<Read>,
    /// The start offsets of the identifiers that are `new` callees.
    new_callees: HashSet<u32>,
    /// The body of each read of an unresolved global-object name (`GLOBAL_OBJECT_NAMES`).
    global_names: Vec<Option<usize>>,
    /// The non-arrow body of each `this` (`None` for top level).
    this_bodies: Vec<Option<usize>>,
}

impl Collect<'_> {
    fn enter(&mut self, span: oxc_span::Span, binding: Option<SymbolId>, top_declaration: bool, arrow: bool, strict: bool) {
        let parent = self.open.last().copied();
        self.nodes.push(Node { start: span.start, end: span.end, parent, binding, top_declaration, arrow, strict });
        self.open.push(self.nodes.len() - 1);
    }

    fn strict(&self, scope: &std::cell::Cell<Option<oxc_syntax::scope::ScopeId>>) -> bool {
        scope.get().is_some_and(|id| self.scoping.scope_flags(id).is_strict_mode())
    }
}

impl<'a> Visit<'a> for Collect<'_> {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let top = self.open.is_empty();
        let declaration = function.is_declaration();
        let binding = if declaration { function.id.as_ref().and_then(|id| id.symbol_id.get()) } else { None };
        let strict = self.strict(&function.scope_id);
        self.enter(function.span, binding, declaration && top, false, strict);
        walk::walk_function(self, function, flags);
        self.open.pop();
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        let strict = self.strict(&arrow.scope_id);
        self.enter(arrow.span, None, false, true, strict);
        walk::walk_arrow_function_expression(self, arrow);
        self.open.pop();
    }

    fn visit_new_expression(&mut self, new: &NewExpression<'a>) {
        if let Expression::Identifier(callee) = new.callee.get_inner_expression() {
            self.new_callees.insert(callee.span.start);
        }
        walk::walk_new_expression(self, new);
    }

    fn visit_this_expression(&mut self, this: &ThisExpression) {
        let body = self.open.iter().rev().copied().find(|&index| !self.nodes[index].arrow);
        self.this_bodies.push(body);
        walk::walk_this_expression(self, this);
    }

    fn visit_identifier_reference(&mut self, name: &IdentifierReference<'a>) {
        let reference = name.reference_id.get().map(|id| self.scoping.get_reference(id));
        if let Some(reference) = reference.filter(|reference| reference.flags().is_read()) {
            let body = self.open.last().copied();
            match reference.symbol_id() {
                Some(symbol) => {
                    let construct = self.new_callees.contains(&name.span.start);
                    self.reads.push(Read { symbol, body, construct });
                }
                None if GLOBAL_OBJECT_NAMES.contains(&name.name.as_str()) => self.global_names.push(body),
                None => {}
            }
        }
        walk::walk_identifier_reference(self, name);
    }
}

impl Reachability {
    /// The live bodies of `program`, over its `scoping` (the same scopes the demands are read from).
    pub(super) fn of(program: &Program<'_>, scoping: &Scoping) -> Self {
        let mut collect = Collect {
            scoping, nodes: Vec::new(), open: Vec::new(), reads: Vec::new(), new_callees: HashSet::new(),
            global_names: Vec::new(), this_bodies: Vec::new(),
        };
        collect.visit_program(program);
        let mut by_symbol: HashMap<SymbolId, Vec<(Option<usize>, bool)>> = HashMap::new();
        for read in &collect.reads { by_symbol.entry(read.symbol).or_default().push((read.body, read.construct)); }
        let module = program.source_type.is_module();
        let nodes = collect.nodes;
        let mut live = vec![false; nodes.len()];
        loop {
            // The global object is materialized by a live site; a live global makes every Script declaration a root.
            // Both grow with `live`, so the loop is monotone and ends at the least fixpoint.
            let materialized = global_object_live(module, &collect.global_names, &collect.this_bodies, &nodes, &live, &by_symbol);
            let mut changed = false;
            for index in 0..nodes.len() {
                if live[index] { continue; }
                let node = &nodes[index];
                if !node.parent.is_none_or(|parent| live[parent]) { continue; }
                let root = node.top_declaration && (module || materialized);
                let needed = root || match node.binding {
                    None => true,
                    Some(symbol) => by_symbol.get(&symbol).is_some_and(|reads| reads.iter().any(|(body, _)| body.is_none_or(|body| live[body]))),
                };
                if needed { live[index] = true; changed = true; }
            }
            if !changed { break; }
        }
        Self { nodes, live }
    }

    /// Whether code at `offset` can run: the innermost function body containing it is live. Top-level code (no
    /// enclosing body) always runs. An offset at a body's own first byte is in the enclosing code, not the body.
    pub(super) fn reaches(&self, offset: u32) -> bool {
        self.nodes.iter().enumerate()
            .filter(|(_, node)| node.start < offset && offset < node.end)
            .min_by_key(|(_, node)| node.end - node.start)
            .is_none_or(|(index, _)| self.live[index])
    }
}

/// Whether a live site yields the global object: an unresolved global-object name, a top-level `this` of a Script,
/// or the `this` of a sloppy function that a plain call can reach.
fn global_object_live(
    module: bool,
    global_names: &[Option<usize>],
    this_bodies: &[Option<usize>],
    nodes: &[Node],
    live: &[bool],
    by_symbol: &HashMap<SymbolId, Vec<(Option<usize>, bool)>>,
) -> bool {
    let live_at = |body: Option<usize>| body.is_none_or(|body| live[body]);
    global_names.iter().any(|&body| live_at(body))
        || this_bodies.iter().any(|&body| match body {
            None => !module,
            Some(index) => {
                let node = &nodes[index];
                live[index] && !node.strict && match node.binding {
                    None => true,
                    Some(symbol) => by_symbol.get(&symbol).is_some_and(|reads| reads.iter().any(|&(body, construct)| !construct && live_at(body))),
                }
            }
        })
}
