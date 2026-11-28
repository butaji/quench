//! Nested construction in a forwarded initializer's plan: `this.f = new C(<constants>)`.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::{ImmediateLayout, NestedConstruct, NESTED_ARGUMENTS, NESTED_CHAIN};

pub(crate) use quench_runtime::vm::engine_types::NestedField;

#[quench_macros::ext(NestedFieldOps1)]
impl NestedField {
    /// The objects the constructor was read through, which the plan depends on.
    pub(crate) fn held(&self) -> &[Value] {
        &self.held[..usize::from(self.depth)]
    }
}

/// Whether the capture a nested construction's chain starts from still holds the object the
/// plan read the constructor through: a binding any assignment can change, so each use reads it.
#[inline(always)]
pub(crate) fn nested_root_holds(vm: &mut Vm, nested: &NestedField) -> bool {
    capture_value(vm, nested.env, nested.capture) == Some(nested.root)
}

fn capture_value(vm: &mut Vm, env: Value, capture: u32) -> Option<Value> {
    let (slots, root) = vm.walked_capture_slots(env, false, ImmediateLayout::capture_depth(capture))?;
    // SAFETY: as `Vm::walked_capture_slots`; nothing below collects.
    vm.plain_capture(unsafe { &*slots }, root, ImmediateLayout::capture_slot(capture))
}

/// A fresh object of `constructor` as its own plans build it from `arguments`, which run no
/// guest code: `None` where it has no holding plan. A nested construction has no construct site
/// of its own in the bytecode, so its class is the constructor's body (`Vm::ordinary_class`).
pub(crate) fn nested_construction(vm: &mut Vm, constructor: Value, arguments: &[Value; NESTED_ARGUMENTS]) -> Option<Value> {
    let argument = |parameter: u16| arguments.get(usize::from(parameter)).copied().unwrap_or(Value::UNDEFINED);
    let class = vm.ordinary_class(constructor);
    planned_construction(vm, constructor, class, argument)
}

/// The value `construct` gives its field, built from the constructor its capture and chain
/// name, read through data properties, with the field the plan then holds for it. `env` is the
/// initializer's closure environment.
pub(crate) fn nested_construct_in_lane(vm: &mut Vm, env: Value, construct: &NestedConstruct) -> Option<(Value, NestedField)> {
    let root = capture_value(vm, env, construct.capture)?;
    let mut held = [Value::UNDEFINED; NESTED_CHAIN];
    let mut constructor = root;
    for (slot, atom) in held.iter_mut().zip(&construct.chain[..usize::from(construct.depth)]) {
        *slot = constructor;
        // A read the plan keeps is of own data properties, whose slots its watch holds.
        constructor = if construct.constructs { vm.plain_named_get(constructor, *atom) } else { vm.plain_own_data(constructor, *atom) }?;
    }
    let field = NestedField { field: 0, constructor, arguments: [Value::UNDEFINED; NESTED_ARGUMENTS], env, capture: construct.capture, root, held, depth: construct.depth, constructs: construct.constructs };
    if !construct.constructs {
        return Some((constructor, field));
    }
    let mut arguments = [Value::UNDEFINED; NESTED_ARGUMENTS];
    for (argument, constant) in arguments.iter_mut().zip(&construct.arguments[..usize::from(construct.count)]) {
        *argument = constant.value(|index| vm.active_constant(index))?;
    }
    let built = nested_construction(vm, constructor, &arguments)?;
    Some((built, NestedField { arguments, ..field }))
}

/// The forwarded initializer's adds on `this`, which a construction completes in the lane when
/// none runs guest code: `None` where an add cannot be applied (the construction then activates).
/// A fixed class's receiver takes its stores in its union slots.
pub(crate) fn finish_forwarded_in_lane(
    vm: &mut Vm,
    code: &ResidualProgram,
    env: Value,
    this: Value,
    function: u32,
    argument: impl Fn(u16) -> Value,
    read: impl Fn(u32) -> bool,
    union: Option<quench_runtime::vm::engine_types::FixedUnion>,
) -> Option<()> {
    // Each store's name and value in store order: a fixed receiver's slot writes.
    let mut atoms = [0; quench_runtime::tier_surface::MAX_CELL_FIELDS];
    let mut values = [Value::UNDEFINED; quench_runtime::tier_surface::MAX_CELL_FIELDS];
    let mut stored = 0;
    let applied = add_initializer_fields_in_lane(vm, code, this, function, argument, read, union.is_some(),
        |vm, construct| nested_construct_in_lane(vm, env, construct).map(|(built, _)| built),
        |store, value| {
            if let (Some(atom), Some(stored_value)) = (atoms.get_mut(stored), values.get_mut(stored)) {
                *atom = store.atom;
                *stored_value = value;
            }
            stored += 1;
        });
    if !applied {
        return None;
    }
    let Some(union) = union else {
        return Some(());
    };
    for (&atom, &value) in atoms.iter().zip(&values).take(stored) {
        let slot = union.slot_of(atom).expect("a fixed class's store names one of its union slots");
        vm.store_fixed_cell_slot(this, slot, atom, value);
    }
    Some(())
}

#[quench_macros::ext(ConstructPlanOps2)]
impl ConstructPlan {
    /// This plan extended by a field `nested` gives a fresh object, or the value its chain
    /// read as a constant, `None` once it names all it can or already has such a field.
    pub(crate) fn with_nested(mut self, nested: NestedField) -> Option<Self> {
        (self.nested.is_none() && usize::from(self.fields) < CONSTRUCT_PLAN_FIELDS).then_some(())?;
        if !nested.constructs {
            self.constant |= 1 << self.fields;
            self.fallbacks[usize::from(self.fields)] = nested.constructor;
        }
        self.nested = Some(NestedField { field: self.fields, ..nested });
        self.fields += 1;
        Some(self)
    }
}
