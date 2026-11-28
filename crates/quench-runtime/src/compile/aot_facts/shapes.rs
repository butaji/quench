//! Whether the instances of a class keep their shape in the closed world (`AotFacts::class_never_departs`). An instance
//! departs its shape where a change reaches it that its union layout (`class_layout::unions_of`) does not name: a delete
//! or a store under a name the analysis cannot list (`Change::Any`), a builtin that defines properties on its arguments
//! (`Change::Define`), a store through a wild holder that may be one of its instances (`Census::wild`), or a builtin that
//! makes a property non-writable or an object non-extensible, sealed or frozen (`Scope::changes_descriptors`) handed one.
//! A named store of an instance is no departure: the union layout names it. The union layout itself is required, so the
//! fact is taken only in a closed world.

use super::census::Census;
use super::escapes::Escapes;
use super::flow::FunctionFlow;
use super::gate::named_closures;
use super::readers::callee_atom;
use super::scope::Scope;
use super::shape::Change;
use crate::bytecode::ResidualProgram;

/// Whether instances of `class` never depart their union layout, over the settled `flows`: no shape change of an instance
/// (`Change::Any`, `Change::Define`), no wild store reaching one, and no builtin that changes the descriptors of an object
/// handed an instance. The caller requires the class's union layout, which holds only in a closed world.
pub(super) fn never_departs(flows: &[FunctionFlow], census: &Census, scope: &Scope, class: u32) -> bool {
    if census.wild_reaches_instances(class) {
        return false;
    }
    let shaped = flows.iter().flat_map(|flow| flow.shapes.iter()).any(|shape| {
        shape.owner.classes().is_some_and(|classes| classes.may_hold(class)) && matches!(shape.change, Change::Any | Change::Define)
    });
    let described = flows.iter().flat_map(|flow| flow.calls.iter()).any(|call| {
        callee_atom(call).is_some_and(|atom| scope.changes_descriptors(atom, call.arguments.len()))
            && call.arguments.iter().any(|argument| argument.cell.classes.may_hold(class))
    });
    !shaped && !described
}

/// Whether every construction that reaches `class` is a direct `new` whose new_target is the class's own constructor. The
/// class's instances are born as its union cell only at a construction whose new_target is the class (the
/// `Constructed{C, true}` root, `provenance`), so any other construction of them breaks the fact:
/// - a derived class's `super()` constructs its base with new_target = the derived class, so a parent of a derived class is
///   not direct, and a derived class is not direct either, since its own `new` builds no instance in its own frame. A derived
///   constructor the flows link to no parent makes every class not direct, as its base is unknown;
/// - `Reflect.construct` (`Scope::reflect_construct`), and a Proxy's construct trap, which receives the proxy as new_target;
/// - code the flows do not follow: a closure that escapes to it, a closure stored under a named property, or an accessor's
///   function. A class constructor is no reason on its own, since the constructions the flows record are the direct ones.
/// The flows' parent links and the derived flags are by function body, so the class is taken as `ClassIds::body`.
pub(super) fn direct_constructions_only(program: &ResidualProgram, flows: &[FunctionFlow], census: &Census, scope: &Scope, escaped: &Escapes, class: u32) -> bool {
    let body = census.classes.body(class);
    let derived = |function: usize| program.with_record(function, |record| record.derived_constructor);
    let links: Vec<(u32, u32)> = flows.iter()
        .flat_map(|flow| flow.parents.iter())
        .map(|&(child, parent)| (census.classes.body(child), census.classes.body(parent)))
        .collect();
    let unlinked_derived = (0..program.function_count())
        .any(|id| derived(id) && !links.iter().any(|&(child, _)| child == id as u32));
    if unlinked_derived || derived(body as usize) || links.iter().any(|&(_, parent)| parent == body) { return false; }
    if scope.reflect_construct.is_some() { return false; }
    if escaped.functions.contains(&body) || census.accessors.functions().any(|function| function == body) { return false; }
    !named_closures(flows).contains(&body)
}
