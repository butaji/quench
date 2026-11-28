//! Whether the instances of a class keep their shape in the closed world (`AotFacts::class_never_departs`). An instance
//! departs its shape where a change reaches it that its union layout (`class_layout::unions_of`) does not name: a delete
//! or a store under a name the analysis cannot list (`Change::Any`), a builtin that defines properties on its arguments
//! (`Change::Define`), a store through a wild holder that may be one of its instances (`Census::wild`), or a builtin that
//! makes a property non-writable or an object non-extensible, sealed or frozen (`Scope::changes_descriptors`) handed one.
//! A named store of an instance is no departure: the union layout names it. The union layout itself is required, so the
//! fact is taken only in a closed world.

use super::census::Census;
use super::flow::FunctionFlow;
use super::readers::callee_atom;
use super::scope::Scope;
use super::shape::Change;

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
