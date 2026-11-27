//! Which function bodies of a program can run. The closed-world gate (`compile::aot_facts::gate`) asks whether a
//! site that may receive code at run time (an `eval`, a `Function` constructor) can execute, not whether the
//! source contains one. This module answers that with a least fixpoint over the function nodes of the program.
//!
//! Soundness: a function body runs only if its closure is created by code that runs and the closure is called.
//! - A closure is created where its expression appears (a function expression, an arrow, a method), so such a node
//!   is live exactly when its enclosing body is live. Top-level code is live.
//! - A nested declaration is called only through a read of its binding, and a read inside a dead body never runs.
//!   The fixpoint starts from the roots and only grows, so a body is dead only when no run can reach it.
//! - A top-level declaration is a root where the host can reach it without a read the program makes of its
//!   binding: every top-level declaration of a Module (its exports, which the host imports), and a Script's global
//!   function, which the program reaches as a property of the global object. A global read by name is an
//!   identifier reference; a global read by property (`globalThis.f`, `globalThis["f"]`, `const { f } = o`) is a
//!   property name, so a Script's declaration is also a root when its name is one of the program's property names,
//!   and every top-level Script declaration is a root when the program makes a computed read it cannot name
//!   (`o[k]`), which may yield any global.
//! - Code that new code (`eval`, `Function`) could reach is not counted here: a live site in a reachable body
//!   opens the world (`gate::world`), so no run of a Closed program adds code that reads the globals.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, SymbolId};
use oxc_syntax::scope::ScopeFlags;
use std::collections::{HashMap, HashSet};

/// One function-like node: its span, the function body it encloses, and what its declaration names.
struct Node {
    start: u32,
    end: u32,
    /// The index of the enclosing function node; `None` for top-level code.
    parent: Option<usize>,
    /// A function declaration's binding; `None` for expressions, arrows and methods.
    binding: Option<SymbolId>,
    /// The name of a top-level Script declaration (`None` elsewhere): a global the program may reach by property.
    global_name: Option<String>,
    /// A declaration at top level (a Module's exports, or a Script's globals).
    top_declaration: bool,
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
    /// Each read of a binding, with the innermost function body it occurs in (`None` for top level).
    reads: Vec<(SymbolId, Option<usize>)>,
    /// Every property name the program reads by name (`o.f`, `o["f"]`, `const { f } = o`).
    names: HashSet<String>,
    /// Whether the program makes a computed read whose key it cannot name (`o[k]`).
    dynamic_read: bool,
}

impl Collect<'_> {
    fn enter(&mut self, start: u32, end: u32, binding: Option<SymbolId>, global_name: Option<String>, top_declaration: bool) {
        let parent = self.open.last().copied();
        self.nodes.push(Node { start, end, parent, binding, global_name, top_declaration });
        self.open.push(self.nodes.len() - 1);
    }

    fn property_name(&mut self, key: &PropertyKey<'_>) {
        if let Some(name) = key.static_name() { self.names.insert(name.to_string()); }
    }
}

impl<'a> Visit<'a> for Collect<'_> {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let top = self.open.is_empty();
        let declaration = function.is_declaration();
        let binding = if declaration { function.id.as_ref().and_then(|id| id.symbol_id.get()) } else { None };
        let global_name = match &function.id {
            Some(id) if declaration && top => Some(id.name.to_string()),
            _ => None,
        };
        self.enter(function.span.start, function.span.end, binding, global_name, declaration && top);
        walk::walk_function(self, function, flags);
        self.open.pop();
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.enter(arrow.span.start, arrow.span.end, None, None, false);
        walk::walk_arrow_function_expression(self, arrow);
        self.open.pop();
    }

    fn visit_identifier_reference(&mut self, name: &IdentifierReference<'a>) {
        let read = name.reference_id.get().map(|id| self.scoping.get_reference(id))
            .filter(|reference| reference.flags().is_read())
            .and_then(|reference| reference.symbol_id());
        if let Some(symbol) = read {
            self.reads.push((symbol, self.open.last().copied()));
        }
        walk::walk_identifier_reference(self, name);
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        self.names.insert(member.property.name.to_string());
        walk::walk_static_member_expression(self, member);
    }

    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        match member.expression.get_inner_expression() {
            Expression::StringLiteral(text) => { self.names.insert(text.value.to_string()); }
            Expression::NumericLiteral(_) => {}
            _ => self.dynamic_read = true,
        }
        walk::walk_computed_member_expression(self, member);
    }

    fn visit_binding_property(&mut self, property: &BindingProperty<'a>) {
        self.property_name(&property.key);
        walk::walk_binding_property(self, property);
    }

    fn visit_assignment_target_property_property(&mut self, property: &AssignmentTargetPropertyProperty<'a>) {
        self.property_name(&property.name);
        walk::walk_assignment_target_property_property(self, property);
    }
}

impl Reachability {
    /// The live bodies of `program`, over its `scoping` (the same scopes the demands are read from).
    pub(super) fn of(program: &Program<'_>, scoping: &Scoping) -> Self {
        let mut collect = Collect { scoping, nodes: Vec::new(), open: Vec::new(), reads: Vec::new(), names: HashSet::new(), dynamic_read: false };
        collect.visit_program(program);
        let mut reads: HashMap<SymbolId, Vec<Option<usize>>> = HashMap::new();
        for (symbol, body) in collect.reads { reads.entry(symbol).or_default().push(body); }
        let nodes = collect.nodes;
        // A Module's declarations are its exports and its scope; a Script's global is named by a property read
        // of the global object (`names`), or by any computed read that may yield one (`dynamic_read`).
        let module = program.source_type.is_module();
        let root: Vec<bool> = nodes.iter().map(|node| {
            node.top_declaration && (module || collect.dynamic_read || node.global_name.as_ref().is_some_and(|name| collect.names.contains(name)))
        }).collect();
        let mut live = vec![false; nodes.len()];
        loop {
            let mut changed = false;
            for index in 0..nodes.len() {
                if live[index] { continue; }
                let node = &nodes[index];
                let enclosing_live = node.parent.is_none_or(|parent| live[parent]);
                if !enclosing_live { continue; }
                let needed = root[index] || match node.binding {
                    None => true,
                    Some(symbol) => reads.get(&symbol).is_some_and(|bodies| bodies.iter().any(|body| body.is_none_or(|body| live[body]))),
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
