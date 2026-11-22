//! Nested construction in a forwarded initializer's plan: `this.f = new C(<constants>)`.
use super::*;
use quench_runtime::bytecode::{ImmediateLayout, InitializerValue, NestedConstruct, NESTED_ARGUMENTS, NESTED_CHAIN};

pub(in super::super) use quench_runtime::vm::engine_types::NestedField;

#[quench_macros::ext(NestedFieldOps1)]
impl NestedField {
    /// The objects the constructor was read through, which the plan depends on.
    pub(in super::super) fn held(&self) -> &[Value] {
        &self.held[..usize::from(self.depth)]
    }
}

/// Whether the capture a nested construction's chain starts from still holds the object the
/// plan read the constructor through: a binding any assignment can change, so each use reads it.
#[inline(always)]
pub(in super::super) fn nested_root_holds(vm: &mut Vm, nested: &NestedField) -> bool {
    capture_value(vm, nested.env, nested.capture) == Some(nested.root)
}

fn capture_value(vm: &mut Vm, env: Value, capture: u32) -> Option<Value> {
    let (slots, root) = vm.walked_capture_slots(env, false, ImmediateLayout::capture_depth(capture))?;
    // SAFETY: as `Vm::walked_capture_slots`; nothing below collects.
    vm.plain_capture(unsafe { &*slots }, root, ImmediateLayout::capture_slot(capture))
}

/// A fresh object of `constructor` as its own plans build it from `arguments`, which run no
/// guest code: `None` where it has no holding plan.
pub(in super::super) fn nested_construction(vm: &mut Vm, constructor: Value, arguments: &[Value; NESTED_ARGUMENTS]) -> Option<Value> {
    let argument = |parameter: u16| arguments.get(usize::from(parameter)).copied().unwrap_or(Value::UNDEFINED);
    planned_construction(vm, constructor, argument).or_else(|| super::super::forwarded_fields_in_lane(vm, constructor, argument))
}

/// The value `construct` gives its field, built from the constructor its capture and chain
/// name, read through data properties, with the field the plan then holds for it. `env` is the
/// initializer's closure environment.
pub(in super::super) fn nested_construct_in_lane(vm: &mut Vm, env: Value, construct: &NestedConstruct) -> Option<(Value, NestedField)> {
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
/// none runs guest code: the plan of the fields they gave it (`None` where it cannot hold one),
/// nested constructions included, or the outer `None` where an add cannot be applied (the construction then activates).
pub(in super::super) fn finish_forwarded_in_lane(
    vm: &mut Vm,
    code: &ResidualProgram,
    env: Value,
    this: Value,
    function: u32,
    argument: impl Fn(u16) -> Value,
) -> Option<Option<ConstructPlan>> {
    let mut fields = Some(EMPTY_CONSTRUCT_PLAN);
    let nested = std::cell::Cell::new(None);
    let applied = add_initializer_fields_in_lane(vm, code, this, function, argument,
        |vm, construct| {
            let (built, field) = nested_construct_in_lane(vm, env, construct)?;
            nested.set(Some(field));
            Some(built)
        },
        |store, value| {
            fields = fields.and_then(|fields| match store.value {
                InitializerValue::Construct(_) => fields.with_nested(nested.take()?),
                _ => fields.with_field(store, value),
            });
        });
    applied.then_some(fields)
}

#[quench_macros::ext(ConstructPlanOps2)]
impl ConstructPlan {
    /// This plan extended by a field `nested` gives a fresh object, or the value its chain
    /// read as a constant, `None` once it names all it can or already has such a field.
    pub(in super::super) fn with_nested(mut self, nested: NestedField) -> Option<Self> {
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
