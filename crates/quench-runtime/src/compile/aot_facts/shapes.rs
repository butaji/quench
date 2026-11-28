//! Whether the instances of a class keep their shape in the closed world (`AotFacts::class_fixed_cell`, `class_writable_cell`). An instance
//! departs its shape where a change reaches it that its union layout (`class_layout::unions_of`) does not name: a delete
//! or a store under a name the analysis cannot list (`Change::Any`), a builtin that defines properties on its arguments
//! (`Change::Define`), a store through a wild holder that may be one of its instances (`Census::wild`), or a builtin that
//! makes a property non-writable or an object non-extensible, sealed or frozen (`Scope::changes_descriptors`) handed one.
//! A named store of an instance is no departure: the union layout names it. The union layout itself is required, so the
//! fact is taken only in a closed world.

use super::census::Census;
use super::escapes::Escapes;
use super::flow::FunctionFlow;
use super::readers::callee_atom;
use super::scope::Scope;
use super::shape::Change;
use crate::bytecode::ResidualProgram;

/// What reaches the instances of a class, over the settled `flows` (`stability`). The caller requires the class's union layout,
/// which holds only in a closed world. With flags as class bits (departure-free cells), the two are separate facts.
#[derive(Clone, Copy)]
pub(super) struct Stability {
    /// No change moves or renames a slot: no shape change of an instance (`Change::Any`, a delete or computed store;
    /// `Change::Define`), and no wild store reaching one. Reads may trust the union's slots.
    pub(super) slots: bool,
    /// No builtin that changes descriptors (`freeze`, `seal`, `preventExtensions`, `defineProperty`, ...) is handed an
    /// instance: no flag transition reaches one, so stores may trust the slots too.
    pub(super) flags: bool,
}

pub(super) fn stability(flows: &[FunctionFlow], census: &Census, scope: &Scope, class: u32) -> Stability {
    let shaped = census.wild_reaches_instances(class) || flows.iter().flat_map(|flow| flow.shapes.iter()).any(|shape| {
        shape.owner.classes().is_some_and(|classes| classes.may_hold(class)) && matches!(shape.change, Change::Any | Change::Define)
    });
    let described = flows.iter().flat_map(|flow| flow.calls.iter()).any(|call| {
        callee_atom(call).is_some_and(|atom| scope.changes_descriptors(atom, call.arguments.len()))
            && call.arguments.iter().any(|argument| argument.cell.classes.may_hold(class))
    });
    Stability { slots: !shaped, flags: !described }
}

/// Whether every construction that reaches `class` is a direct `new` whose new_target is the class's own constructor. The
/// class's instances are born as its union cell only at a construction whose new_target is the class (the
/// `Constructed{C, true}` root, `provenance`), so any other construction of them breaks the fact:
/// - a derived class: its own `new` builds no instance in its own frame (`super()` does), and it has no union anyway. A parent
///   of a derived class is direct: a `super()` construction builds an object whose class word is the derived class, which the
///   lane never takes for the parent (its proven classes name the derived class);
/// - `Reflect.construct` (`Scope::reflect_construct`), and a Proxy's construct trap, which receives the proxy as new_target;
/// - code the flows do not follow: a closure that escapes to it may be constructed there (`escapes`). A closure stored under a
///   named property, or an accessor's function, is only called by code the flows do not record (a conversion, a property
///   access), never constructed, so neither is a reason; a class constructor is none either, since the constructions the flows
///   record are the direct ones.
/// The derived flag and the escapes are by function body, so the class is taken as `ClassIds::body`.
pub(super) fn direct_constructions_only(program: &ResidualProgram, census: &Census, scope: &Scope, escaped: &Escapes, class: u32) -> bool {
    let body = census.classes.body(class);
    if program.with_record(body as usize, |record| record.derived_constructor) { return false; }
    if scope.reflect_construct.is_some() { return false; }
    !escaped.functions.contains(&body)
}
