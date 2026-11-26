//! Whether a program can receive source first known at execution. This is the
//! one decision for every producer (Node AOT, the test262 emitter): a program
//! that can reach dynamic source needs the interpreter and JIT linked beside its
//! native code, and one that cannot is linked execution-only.

use super::{constructors, NativeSourceKey};
use crate::Engine;
use oxc_ast::ast::{
    AssignmentTargetPropertyProperty, BinaryExpression, BindingProperty, ComputedMemberExpression, Expression,
    IdentifierReference, Program, StaticMemberExpression,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_syntax::operator::BinaryOperator;

const EVAL: &str = "eval";
const FUNCTION: &str = "Function";
const CONSTRUCTOR: &str = "constructor";
const REFLECT: &str = "Reflect";

/// Source obligations of one program: literal constructor sources to precompile,
/// and the offsets of every site that may receive other source at execution.
///
/// A program requires dynamic source when any list is nonempty. Sites:
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
///   `Function`; and any computed member or key whose name is not a literal
///   (it may evaluate to `constructor` or `eval`).
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
}

impl SourceDemands {
    pub fn requires_dynamic_source(&self) -> bool {
        !(self.eval.is_empty() && self.function.is_empty() && self.reflective.is_empty())
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

/// A member or key expression's hazard: literal names are exact, anything else may be any name.
fn expression_hazard(key: &Expression<'_>) -> Option<Hazard> {
    match key.get_inner_expression() {
        Expression::StringLiteral(text) => named(text.value.as_str()),
        Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => None,
        Expression::TemplateLiteral(template) => template.single_quasi().map_or(Some(Hazard::Reflective), |text| named(text.as_str())),
        _ => Some(Hazard::Reflective),
    }
}

struct Hazards<'s> {
    scoping: &'s Scoping,
    eval: Vec<u32>,
    reflective: Vec<u32>,
}

impl Hazards<'_> {
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
        self.note(expression_hazard(&member.expression), member.span.start);
        walk::walk_computed_member_expression(self, member);
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
        let mut hazards = Hazards { scoping, eval: Vec::new(), reflective: Vec::new() };
        hazards.visit_program(program);
        for sites in [&mut hazards.eval, &mut hazards.reflective] { sites.sort_unstable(); sites.dedup(); }
        SourceDemands { keys, eval: hazards.eval, function, reflective: hazards.reflective }
    }

    /// Whether a standalone Script or Module may receive source first known at
    /// execution. Source that does not parse cannot be proved closed.
    pub fn source_requires_dynamic_source(source: &str, kind: crate::SourceKind) -> bool {
        Self::standalone_source_demands(source, kind).is_none_or(|demands| demands.requires_dynamic_source())
    }

    /// The demands (`source_demands`) of a standalone Script or Module; `None` where it does not parse.
    pub fn standalone_source_demands(source: &str, kind: crate::SourceKind) -> Option<SourceDemands> {
        let allocator = oxc_allocator::Allocator::default();
        let source_type = match kind {
            crate::SourceKind::Module => oxc_span::SourceType::mjs(),
            crate::SourceKind::Script | crate::SourceKind::Eval => oxc_span::SourceType::script(),
        };
        let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
        if !parsed.diagnostics.is_empty() { return None; }
        let semantic = oxc_semantic::SemanticBuilder::new().build(&parsed.program);
        Some(Self::source_demands(&parsed.program, semantic.semantic.scoping(), true))
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
            "o[k]", "o[`${k}`]", "o['constructor']", "const { constructor: C } = f", "const { [k]: v } = o",
            "Reflect.construct(Function, [s])", "Reflect.apply(f, null, [])",
        ] {
            assert!(requires(source), "{source}");
        }
    }

    #[test]
    fn closed_programs_stay_execution_only() {
        for source in [
            "const o = { a: 1 }; o.a + o['b'] + [1, 2][0] + o[1];", "function f(eval, Function) { return eval(Function); }",
            "x instanceof Function", "'use strict'; const { a, b: c } = o; let g = 'eval';",
        ] {
            assert!(!requires(source), "{source}");
        }
        assert!(requires("function broken("));
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
