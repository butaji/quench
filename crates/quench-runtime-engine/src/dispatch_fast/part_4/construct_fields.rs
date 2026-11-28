//! Field initialization of a construction in a lane, and the planned receiver and fixed receiver union it takes.
use super::*;

/// The body of [[Construct]] for a constructor that only stores its parameters into fields
/// of `this` (`ScriptCallTarget::initializes`): its receiver
/// (`Vm::inline_construct_receiver`), and whether every store was a cached add
/// (`Vm::primary_field_add_hit`), which runs no guest code, so the body completed. After a
/// miss the constructor activates on the receiver: its body repeats the stores made so far
/// as stores of the same values to the own data properties they added, which leaves the
/// object as one activation would. A body whose adds all hit leaves its `ConstructPlan`,
/// with which a later construct allocates the finished object at once
/// (`planned_construction_in_lane`), which comes first. Out of line, so the construct keeps
/// its tail dispatch.
#[inline(never)]
pub(in super::super) fn initialize_fields_in_lane(
    vm: &mut Vm,
    code: &ResidualProgram,
    callee: Value,
    class: u32,
    constructor: Value,
    function: u32,
    argument: impl Fn(u16) -> Value,
) -> Option<(Value, bool)> {
    let stores = code.function(function as usize).initializer_stores(&code.constants)?;
    let env = vm.same_realm_user_target(callee).map_or(Value::UNDEFINED, |(_, _, env)| env);
    let read = construct_plan::field_reader(vm, callee);
    initialize_stores_in_lane(vm, callee, class, constructor, env, |vm, this, fixed, nested, each| {
        add_stores_in_lane(vm, code, this, stores, argument, read, fixed, nested, each)
    })
}

/// The receiver `Vm::inline_construct_receiver` allocates for a constructor that forwards its
/// arguments (`script_construct_keeping_miss`): from `callee`'s holding plan `plan` where it
/// has no fields, else from the template, which leaves that plan. `script_construct_miss` sends a
/// forwarding target here before it tests `ScriptCallTarget::initializes`, so a callee's plan
/// is only ever this one or `initialize_fields_in_lane`'s.
pub(in super::super) fn planned_receiver_in_lane(
    vm: &mut Vm,
    callee: Value,
    class: u32,
    plan: Option<&ConstructPlan>,
    union: Option<quench_runtime::vm::engine_types::FixedUnion>,
) -> Option<Value> {
    if let Some(plan) = plan.filter(|plan| plan.fields == 0) {
        return Some(plan.allocate(vm, |_| Value::UNDEFINED));
    }
    let template = vm.inline_construct_template(callee, class)?;
    let (prototype, shape) = (template.proto(), template.shape());
    // A fixed class's receiver is its union cell from birth (R4-F), as `initialize_stores_in_lane` makes it.
    let fixed = union.map(|union| quench_runtime::vm::engine_types::FixedCellPlan { shape: union.shape, slots: [0; CONSTRUCT_PLAN_FIELDS] });
    let this = match union {
        Some(union) => vm.allocate_fixed_cell(union.shape, prototype, &[], &[]),
        None => vm.allocate_object(template),
    };
    if let Some(index) = construct_plan_index(callee, class)
        && let Some(callee_layout) = vm.function_prototype_layout(callee)
    {
        vm.lane_stack.construct_plans[index] = ConstructPlan {
            callee,
            callee_layout,
            prototype,
            shape: fixed.map_or(shape, |fixed| fixed.shape),
            kind: ConstructPlanKind::Forwarding,
            fixed,
            class,
            ..EMPTY_CONSTRUCT_PLAN
        }
        .finish();
        watch_construct_plan(vm, index, true, &[callee, prototype]);
    }
    Some(this)
}

/// The union of a forwarding constructor's receiver `callee` when its class is fixed (R4-F, `Vm::fixed_union`), from the
/// template of `callee`'s prototype: the receiver is then the union cell (`planned_receiver_in_lane`).
pub(in super::super) fn fixed_receiver_union(vm: &mut Vm, callee: Value, class: u32) -> Option<quench_runtime::vm::engine_types::FixedUnion> {
    let template = vm.inline_construct_template(callee, class)?;
    vm.fixed_union(callee, class, template.shape())
}
