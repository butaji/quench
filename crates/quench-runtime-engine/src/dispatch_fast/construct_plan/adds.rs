//! A forwarded or chain initializer's adds made on a receiver from the caches, without guest code.
use super::*;

/// The adds an initializing body (`ScriptCallTarget::initializes`) of `function` makes to
/// `this`, each the parameter its store names (`argument`) as a cached add that runs no guest
/// code, passed to `each` in order. `false` at the first add the caches do not answer: the
/// adds made so far are the body's own first stores of the same values, which an activation
/// of the body then makes again as stores, leaving the object as the activation alone would.
#[inline(always)]
pub(in super::super) fn add_initializer_fields_in_lane(
    vm: &mut Vm,
    code: &ResidualProgram,
    this: Value,
    function: u32,
    argument: impl Fn(u16) -> Value,
    nested: impl FnMut(&mut Vm, &quench_runtime::bytecode::NestedConstruct) -> Option<Value>,
    each: impl FnMut(&quench_runtime::bytecode::InitializerStore, Value),
) -> bool {
    let Some(stores) = code.function(function as usize).initializer_stores(&code.constants) else {
        return false;
    };
    add_stores_in_lane(vm, code, this, stores, argument, nested, each)
}

/// `add_initializer_fields_in_lane` of `stores`, a body's or a class chain's (`chain_stores`).
#[inline(always)]
pub(in super::super) fn add_stores_in_lane(
    vm: &mut Vm,
    code: &ResidualProgram,
    this: Value,
    stores: &[quench_runtime::bytecode::InitializerStore],
    argument: impl Fn(u16) -> Value,
    mut nested: impl FnMut(&mut Vm, &quench_runtime::bytecode::NestedConstruct) -> Option<Value>,
    mut each: impl FnMut(&quench_runtime::bytecode::InitializerStore, Value),
) -> bool {
    for store in stores {
        let value = match &store.value {
            quench_runtime::bytecode::InitializerValue::Construct(construct) => nested(vm, construct),
            _ => initializer_store_value(vm, store, &argument),
        };
        let Some(value) = value else {
            return false;
        };
        if !vm.field_add_atom_eligible(store.atom) || !vm.primary_field_add_hit(code, this, store.atom, value, store.site) {
            return false;
        }
        each(store, value);
    }
    true
}
