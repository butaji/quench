//! Whether a program can receive source first known at execution. This is the
//! one decision for every producer (Node AOT, the test262 emitter): a program
//! that can reach dynamic source needs the interpreter and JIT linked beside its
//! native code, and one that cannot is linked execution-only.

use super::reachability::Reachability;
use super::{constructors, NativeSourceKey};
use crate::Engine;
use oxc_ast::ast::{
    AssignmentExpression, AssignmentTargetPropertyProperty, BinaryExpression, BindingIdentifier,
    BindingPattern, BindingProperty, CallExpression, ComputedMemberExpression, Expression, IdentifierReference,
    NewExpression, Program, SimpleAssignmentTarget, StaticMemberExpression, UpdateExpression, VariableDeclarator,
    WithStatement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, SymbolId};
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator};
use std::collections::{HashMap, HashSet};

const EVAL: &str = "eval";
const FUNCTION: &str = "Function";
const CONSTRUCTOR: &str = "constructor";
const REFLECT: &str = "Reflect";

/// Source obligations of one program: literal constructor sources to precompile,
/// and the offsets of every site that may receive other source at execution.
///
/// A program requires dynamic source when `eval`, `function` or `reflective` is
/// nonempty; `computed` records index-like reads for diagnostics alone. Sites:
/// - `eval`: an unshadowed global `eval` identifier in any position (direct or
///   indirect call, alias, `typeof`), a member named `eval` on any object
///   (`globalThis.eval`, `globalThis["eval"]`), or a destructured `eval` key.
/// - `function`: a call or `new` whose callee may be a Function-family
///   constructor (`Function` and the async/generator constructors reached
///   through a function's `constructor`) and whose arguments are not all string
///   literals, found by constructor flow through bindings, arrays and `for-of`.
/// - `reflective`: any other way to reach those values without a name the flow
///   follows: an unshadowed global `Function` or `Reflect` identifier (so
///   `Reflect.construct/apply` of them), except as the right operand of
///   `instanceof`; any member or destructured key named `constructor` or
///   `Function`; any object-literal computed key whose name is not a literal; and
///   a computed member read only where it is called (`o[k]()`), since a read
///   alone (`a[c]`) is an index, not a name the program proves dynamic. A key
///   that only ever holds numbers (`NumericBindings`) is an index even where
///   the read is called.
///
/// Forms outside this list (for example `Object.getOwnPropertyDescriptor` with
/// a computed name) are not detected: an image linked execution-only keeps the
/// runtime rejection of dynamic source for them.
#[derive(Debug, Default)]
pub struct SourceDemands {
    pub keys: Vec<NativeSourceKey>,
    pub eval: Vec<u32>,
    pub function: Vec<u32>,
    pub reflective: Vec<u32>,
    /// Computed member reads whose key is no literal (`a[c]`): an index, not a name the
    /// program proves dynamic. They become a hazard only where the read is called.
    pub computed: Vec<u32>,
}

impl SourceDemands {
    pub fn requires_dynamic_source(&self) -> bool {
        !(self.eval.is_empty() && self.function.is_empty() && self.reflective.is_empty())
    }

    /// The sites that can run: those in a live function body (`Reachability`). A site in a body
    /// nothing can call never executes, so it cannot receive source.
    fn running(mut self, reachability: &Reachability) -> Self {
        for sites in [&mut self.eval, &mut self.function, &mut self.reflective] {
            sites.retain(|offset| reachability.reaches(*offset));
        }
        self
    }
}

#[derive(Clone, Copy)]
enum Hazard { Eval, Reflective }

fn named(name: &str) -> Option<Hazard> {
    match name {
        EVAL => Some(Hazard::Eval),
        FUNCTION | CONSTRUCTOR => Some(Hazard::Reflective),
        _ => None,
    }
}

/// Bindings that only ever hold numbers, so a member read keyed by one names an index, never
/// `constructor`, `eval` or a method. A binding qualifies when it has exactly one declaration, a
/// variable declarator initialized with a numeric literal, and every write to it keeps a number:
/// `++`/`--`, an arithmetic or bitwise compound assignment other than `+=`, or `=`/`+=` of a
/// numeric literal. A program with `with` proves nothing (a name may resolve to an object's member).
struct NumericBindings<'s> {
    scoping: &'s Scoping,
    declarations: HashMap<SymbolId, usize>,
    numeric_init: HashSet<SymbolId>,
    numeric_writes: HashMap<SymbolId, usize>,
    with_statement: bool,
}

fn numeric_literal(expression: &Expression<'_>) -> bool {
    matches!(expression.get_inner_expression(), Expression::NumericLiteral(_))
}

impl NumericBindings<'_> {
    fn symbols(program: &Program<'_>, scoping: &Scoping) -> HashSet<SymbolId> {
        let mut bindings = NumericBindings { scoping, declarations: HashMap::new(), numeric_init: HashSet::new(), numeric_writes: HashMap::new(), with_statement: false };
        bindings.visit_program(program);
        if bindings.with_statement { return HashSet::new(); }
        bindings.numeric_init.iter().copied().filter(|symbol| {
            bindings.declarations.get(symbol) == Some(&1)
                && scoping.get_resolved_references(*symbol).filter(|reference| reference.is_write()).count()
                    == bindings.numeric_writes.get(symbol).copied().unwrap_or(0)
        }).collect()
    }

    fn numeric_write(&mut self, target: &SimpleAssignmentTarget<'_>, keeps_number: bool) {
        let SimpleAssignmentTarget::AssignmentTargetIdentifier(name) = target else { return };
        let symbol = name.reference_id.get().and_then(|id| self.scoping.get_reference(id).symbol_id());
        if let (Some(symbol), true) = (symbol, keeps_number) { *self.numeric_writes.entry(symbol).or_default() += 1; }
    }
}

impl<'a> Visit<'a> for NumericBindings<'_> {
    fn visit_binding_identifier(&mut self, binding: &BindingIdentifier<'a>) {
        if let Some(symbol) = binding.symbol_id.get() { *self.declarations.entry(symbol).or_default() += 1; }
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let (BindingPattern::BindingIdentifier(binding), Some(init)) = (&declarator.id, &declarator.init) {
            if let (Some(symbol), true) = (binding.symbol_id.get(), numeric_literal(init)) { self.numeric_init.insert(symbol); }
        }
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_update_expression(&mut self, update: &UpdateExpression<'a>) {
        self.numeric_write(&update.argument, true);
        walk::walk_update_expression(self, update);
    }

    fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
        let keeps_number = match assignment.operator {
            AssignmentOperator::Assign | AssignmentOperator::Addition => numeric_literal(&assignment.right),
            operator => operator.is_arithmetic() || operator.is_bitwise(),
        };
        if let Some(target) = assignment.left.as_simple_assignment_target() { self.numeric_write(target, keeps_number); }
        walk::walk_assignment_expression(self, assignment);
    }

    fn visit_with_statement(&mut self, statement: &WithStatement<'a>) {
        self.with_statement = true;
        walk::walk_with_statement(self, statement);
    }
}

struct Hazards<'s> {
    scoping: &'s Scoping,
    numeric: HashSet<SymbolId>,
    eval: Vec<u32>,
    reflective: Vec<u32>,
    computed: Vec<u32>,
}

impl Hazards<'_> {
    /// A member key's hazard where the key names its property: literal names are exact, and a
    /// numeric literal or a numeric binding names an index. `None` where the key may be any
    /// name (a computed read, `a[c]`).
    fn literal_key(&self, key: &Expression<'_>) -> Option<Option<Hazard>> {
        match key.get_inner_expression() {
            Expression::StringLiteral(text) => Some(named(text.value.as_str())),
            Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => Some(None),
            Expression::TemplateLiteral(template) => template.single_quasi().map(|text| named(text.as_str())),
            Expression::Identifier(name) if name.reference_id.get()
                .and_then(|id| self.scoping.get_reference(id).symbol_id())
                .is_some_and(|symbol| self.numeric.contains(&symbol)) => Some(None),
            _ => None,
        }
    }

    /// Whether a call or `new` of `callee` calls a computed read (`o[k]()`), which may reach
    /// any method, `constructor` included.
    fn calls_computed(&self, callee: &Expression<'_>) -> bool {
        matches!(callee.get_inner_expression(), Expression::ComputedMemberExpression(member) if self.literal_key(&member.expression).is_none())
    }

    fn note(&mut self, hazard: Option<Hazard>, offset: u32) {
        match hazard {
            Some(Hazard::Eval) => { self.eval.push(offset); },
            Some(Hazard::Reflective) => { self.reflective.push(offset); },
            None => {},
        }
    }

    fn global_value(&self, name: &IdentifierReference<'_>) -> bool {
        name.reference_id.get().map(|id| self.scoping.get_reference(id))
            .is_some_and(|reference| reference.is_value() && !reference.flags().is_value_as_type() && reference.symbol_id().is_none())
    }

    fn key(&mut self, key: &oxc_ast::ast::PropertyKey<'_>, computed: bool, offset: u32) {
        let hazard = match key.static_name() {
            Some(name) => named(&name),
            None => computed.then_some(Hazard::Reflective),
        };
        self.note(hazard, offset);
    }
}

impl<'a> Visit<'a> for Hazards<'_> {
    fn visit_identifier_reference(&mut self, name: &IdentifierReference<'a>) {
        if self.global_value(name) {
            let hazard = match name.name.as_str() {
                EVAL => Some(Hazard::Eval),
                FUNCTION | REFLECT => Some(Hazard::Reflective),
                _ => None,
            };
            self.note(hazard, name.span.start);
        }
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        self.note(named(member.property.name.as_str()), member.span.start);
        walk::walk_static_member_expression(self, member);
    }

    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        // A literal key names its property (`o["eval"]`); a numeric key, or a binding that only
        // ever holds numbers (`NumericBindings`), is an index; any other key is a computed read
        // (`a[c]`), which is no dynamic source unless it is called.
        match self.literal_key(&member.expression) {
            Some(hazard) => self.note(hazard, member.span.start),
            None => self.computed.push(member.span.start),
        }
        walk::walk_computed_member_expression(self, member);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.calls_computed(&call.callee) {
            self.note(Some(Hazard::Reflective), call.span.start);
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_new_expression(&mut self, call: &NewExpression<'a>) {
        if self.calls_computed(&call.callee) {
            self.note(Some(Hazard::Reflective), call.span.start);
        }
        walk::walk_new_expression(self, call);
    }

    fn visit_binding_property(&mut self, property: &BindingProperty<'a>) {
        self.key(&property.key, property.computed, property.span.start);
        walk::walk_binding_property(self, property);
    }

    fn visit_assignment_target_property_property(&mut self, property: &AssignmentTargetPropertyProperty<'a>) {
        self.key(&property.name, property.computed, property.span.start);
        walk::walk_assignment_target_property_property(self, property);
    }

    fn visit_binary_expression(&mut self, expression: &BinaryExpression<'a>) {
        let type_test = expression.operator == BinaryOperator::Instanceof
            && matches!(expression.right.get_inner_expression(), Expression::Identifier(name) if name.name == FUNCTION);
        if type_test { self.visit_expression(&expression.left); } else { walk::walk_binary_expression(self, expression); }
    }
}

impl Engine {
    /// Literal constructor sources and every site that may receive source first
    /// known at execution (see [`SourceDemands`]), over the caller's authoritative scopes.
    pub fn source_demands(program: &Program<'_>, scoping: &Scoping, specialized: bool) -> SourceDemands {
        let (keys, function) = constructors::analyze(program, scoping, specialized);
        let mut hazards = Hazards { scoping, numeric: NumericBindings::symbols(program, scoping), eval: Vec::new(), reflective: Vec::new(), computed: Vec::new() };
        hazards.visit_program(program);
        for sites in [&mut hazards.eval, &mut hazards.reflective] { sites.sort_unstable(); sites.dedup(); }
        hazards.computed.sort_unstable(); hazards.computed.dedup();
        SourceDemands { keys, eval: hazards.eval, function, reflective: hazards.reflective, computed: hazards.computed }
    }

    /// Whether a standalone Script or Module may receive source first known at
    /// execution. Source that does not parse cannot be proved closed.
    /// Without the source routes (`source_routes`) the answer is the conservative one.
    pub fn source_requires_dynamic_source(source: &str, kind: crate::SourceKind) -> bool {
        Self::standalone_source_demands(source, kind).is_none_or(|demands| demands.requires_dynamic_source())
    }

    /// The demands (`source_demands`) of a standalone Script or Module that can run: the sites in
    /// live function bodies only (`SourceDemands::running`). `None` where it does not parse, or
    /// without the source routes (`source_routes`), which link the parser.
    pub fn standalone_source_demands(source: &str, kind: crate::SourceKind) -> Option<SourceDemands> {
        crate::compile::source_routes::routes().and_then(|routes| (routes.standalone_source_demands)(source, kind))
    }

    pub(crate) fn standalone_source_demands_parsed(source: &str, kind: crate::SourceKind) -> Option<SourceDemands> {
        let allocator = oxc_allocator::Allocator::default();
        let source_type = match kind {
            crate::SourceKind::Module => oxc_span::SourceType::mjs(),
            crate::SourceKind::Script | crate::SourceKind::Eval => oxc_span::SourceType::script(),
        };
        let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
        if !parsed.diagnostics.is_empty() { return None; }
        let semantic = oxc_semantic::SemanticBuilder::new().build(&parsed.program);
        let scoping = semantic.semantic.scoping();
        let live = Reachability::of(&parsed.program, scoping);
        Some(Self::source_demands(&parsed.program, scoping, true).running(&live))
    }
}

#[cfg(test)]
mod tests {
    use crate::SourceKind;
    use super::*;

    fn requires(source: &str) -> bool { Engine::source_requires_dynamic_source(source, SourceKind::Script) }

    #[test]
    fn dynamic_source_forms_require_the_interpreter_and_jit() {
        for source in [
            "eval('1')", "const e = eval; e('1')", "globalThis.eval('1')", "globalThis['eval']('1')", "({ eval } = globalThis)",
            "new Function('a', 'return a')", "Function(s)", "const F = Function; new F(s)",
            "(function () {}).constructor('return 1')", "(async () => {}).constructor(s)", "x.constructor(s)", "x?.constructor",
            "o[k]()", "new o[k]()", "o['constructor']", "const { constructor: C } = f", "const { [k]: v } = o",
            "Reflect.construct(Function, [s])", "Reflect.apply(f, null, [])",
            "let i = 0; i = 'constructor'; o[i]()", "let i = 0; i += 'x'; o[i]()", "var i = 0; var i = s; o[i]()",
            "let i = 0; [i] = a; o[i]()", "let i = 0; ({ i } = a); o[i]()", "let i = 0; i ||= s; o[i]()", "let i; o[i]()",
            "function f(i) { return o[i](); } f(0);", "var i = 0; with (o) { o[i](); }", "for (let k in o) o[k]()", "o[`eval`]",
        ] {
            assert!(requires(source), "{source}");
        }
        // A computed read alone is an index, not a name the program proves dynamic.
        for source in ["o[k]", "o[`${k}`]", "a[i + 1]", "shapes[n].position"] {
            assert!(!requires(source), "{source}");
        }
    }

    #[test]
    fn closed_programs_stay_execution_only() {
        for source in [
            "const o = { a: 1 }; o.a + o['b'] + [1, 2][0] + o[1];", "function f(eval, Function) { return eval(Function); }",
            "x instanceof Function", "'use strict'; const { a, b: c } = o; let g = 'eval';",
            "for (let i = 1; i < a.length; i++) s += Number(a[i]);", "let i = 0; i += 2; i *= s; i = 3; i <<= 1; o[i]",
            "const n = 4; o[n](); let j = 0; function f() { j--; return new o[j](); }", "o['f']()",
        ] {
            assert!(!requires(source), "{source}");
        }
        assert!(requires("function broken("));
    }

    #[test]
    fn a_site_in_a_body_nothing_calls_does_not_require_dynamic_source() {
        // The closure is created and never called, so its eval never runs (`reachability`).
        assert!(!requires("(function () { function dead() { return eval('1'); } })();"));
        // Two declarations that only read each other are both dead: the fixpoint starts from the roots.
        assert!(!requires("(function () { function a() { b(); return eval('1'); } function b() { a(); } })();"));
        // A read from live code makes the declaration live, and so its site runs.
        assert!(requires("(function () { function live() { return eval('1'); } live(); })();"));
        // A Script's global function is reached by a property read of the global object.
        assert!(requires("function f() { return eval('1'); } globalThis.f();"));
    }

    #[test]
    fn computed_reads_on_local_objects_keep_a_dead_site_dead() {
        // Without the global object as a value, a computed read reaches no global: `f` stays dead.
        assert!(!requires("function f() { return eval('1'); } var a = [1], o = {}, k = 'f'; a[k]; o[k]; var x = o[k + 1];"));
        assert!(!requires("var k = 'f'; function f() { return eval('1'); } (function () { var s = {}; s[k]; })();"));
    }

    #[test]
    fn the_global_object_as_a_value_roots_the_script_globals() {
        // A computed read of `globalThis` may name any global function.
        assert!(requires("function f() { return eval('1'); } var k = 'f', g = globalThis[k];"));
        // A sloppy function called plainly binds `this` to the global object.
        assert!(requires("function f() { return eval('1'); } (function () { return this; })();"));
        // A dead `globalThis` read does not materialize it.
        assert!(!requires("function f() { return eval('1'); } (function () { function dead() { return globalThis; } })();"));
        // A constructor only built with `new` gets a fresh object as `this`, not the global object.
        assert!(!requires("function C() { this.x = 1; } var c = new C(); function f() { return eval('1'); }"));
        // Strict code binds `this` to undefined.
        assert!(!requires("'use strict'; function f() { return eval('1'); } (function () { return this; })();"));
    }

    #[test]
    fn demands_separate_literal_sources_from_dynamic_sites() {
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, "Function('return 1'); Function(s); eval(t);", oxc_span::SourceType::script()).parse();
        let semantic = oxc_semantic::SemanticBuilder::new().build(&parsed.program);
        let demands = Engine::source_demands(&parsed.program, semantic.semantic.scoping(), true);
        assert_eq!((demands.keys.len(), demands.function.len(), demands.eval.len()), (1, 1, 1));
        assert!(demands.requires_dynamic_source());
    }
}
