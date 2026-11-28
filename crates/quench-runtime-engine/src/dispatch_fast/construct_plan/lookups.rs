use super::*;

/// The Number a computed initializer store (`InitializerValue::Numeric`) gives its field:
/// `operator` of the parameter's `passed` value and `constant`, as the body's `Binary`
/// computes it on two Numbers; `None` for any other argument, whose operator may run guest code.
#[inline(always)]
pub(super) fn numeric_initializer(passed: Value, operator: u8, constant: f64) -> Option<Value> {
    match quench_runtime::vm::operations::static_number_binary(u32::from(operator), passed.as_number()?, constant)? {
        quench_runtime::bytecode::Constant::Number(number) => Some(Value::number(number)),
        _ => None,
    }
}

/// The value an initializer store (`InitializerStore`) gives its field from the parameter's
/// `passed` value: `fallback` in place of a falsy one where the store has it, else as passed.
#[inline(always)]
pub(crate) fn initializer_value(vm: &Vm, passed: Value, fallback: Option<Value>) -> Value {
    match fallback {
        Some(fallback) if !vm.truthy(passed) => fallback,
        _ => passed,
    }
}

/// The value initializer `store` gives its field from the parameters' values `argument`:
/// `None` where the store leaves it to the body (`add_initializer_fields_in_lane`). The running
/// program is the store's (`Vm::active_constant`).
#[inline(always)]
pub(crate) fn initializer_store_value(vm: &Vm, store: &quench_runtime::bytecode::InitializerStore, argument: impl Fn(u16) -> Value) -> Option<Value> {
    match store.value {
        quench_runtime::bytecode::InitializerValue::Parameter { parameter, fallback } => {
            Some(initializer_value(vm, argument(parameter), fallback.map(Value::number)))
        }
        quench_runtime::bytecode::InitializerValue::Constant(constant) => constant.value(|index| vm.active_constant(index)),
        quench_runtime::bytecode::InitializerValue::Numeric { parameter, operator, constant } => numeric_initializer(argument(parameter), operator, constant),
        quench_runtime::bytecode::InitializerValue::Construct(_) => None,
    }
}

/// `initialize_fields_in_lane` while `callee`'s plan holds: the finished receiver, its fields
/// the arguments the plan's parameters name. A plan holds while its callee's layout and
/// `prototype` do: the receiver's template is derived from those alone, and the epoch keeps
/// the adds it planned free of setters. Apart from the planning, whose frame is larger.
#[inline(always)]
pub(crate) fn planned_construction_in_lane(
    vm: &mut Vm,
    callee: Value,
    class: u32,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    planned_construction(vm, callee, class, construct_argument_in_lane(r, i))
}

/// `planned_construction_in_lane` of a plan of passed fields that fit the inline slots, where it
/// calls nothing out of line (`Heap::alloc_planned_inline_quick`); `None`, before any effect,
/// for anything else, which `planned_construction_in_lane` makes.
#[inline(always)]
pub(crate) fn planned_inline_quick_in_lane(vm: &mut Vm, callee: Value, class: u32, r: Registers, i: &LaneInstruction) -> Option<Value> {
    planned_inline_quick(vm, callee, class, construct_argument_in_lane(r, i))
}

/// `planned_inline_quick_in_lane` from the parameters' values `argument` gives: any construction
/// site's quick case, a lane record's or a lane body's (`body_calls::constructed_in_place`).
#[inline(always)]
pub(crate) fn planned_inline_quick(vm: &mut Vm, callee: Value, class: u32, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee, class)?;
    let plan = &vm.lane_stack.construct_plans[index];
    let len = usize::from(plan.fields);
    if plan.kind != ConstructPlanKind::Passed || len > INLINE_PROPERTY_COUNT {
        return None;
    }
    const _: () = assert!(INLINE_PROPERTY_COUNT == 2, "the inline slots are filled one by one");
    let mut inline = [Value::UNDEFINED; INLINE_PROPERTY_COUNT];
    if len > 0 { inline[0] = plan.passed_value(0, &argument); }
    if len > 1 { inline[1] = plan.passed_value(1, &argument); }
    let header = plan.header;
    vm.allocate_planned_inline_quick(header, inline, len)
}

/// `planned_construction_in_lane` from the parameters' values `argument` gives.
#[inline(always)]
pub(crate) fn planned_construction(vm: &mut Vm, callee: Value, class: u32, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee, class)?;
    // The plan in place: the allocation borrows the heap beside it, not a copy.
    let plan = &vm.lane_stack.construct_plans[index];
    if plan.kind != ConstructPlanKind::Passed {
        return defaulted_construction_in_lane(vm, index, argument);
    }
    let len = usize::from(plan.fields);
    if len <= INLINE_PROPERTY_COUNT {
        // Fields that fit the inline slots are those slots: no values array gathered first.
        const _: () = assert!(INLINE_PROPERTY_COUNT == 2, "the inline slots are filled one by one");
        let mut inline = [Value::UNDEFINED; INLINE_PROPERTY_COUNT];
        if len > 0 { inline[0] = plan.passed_value(0, &argument); }
        if len > 1 { inline[1] = plan.passed_value(1, &argument); }
        return Some(vm.allocate_planned_inline(plan.header, plan.prototype, inline, len));
    }
    Some(vm.allocate_planned_object(plan.header, plan.prototype, &quench_runtime::vm::tier_queries::planned_fields::<CONSTRUCT_PLAN_FIELDS>(len, |index| plan.passed_value(index, &argument)), len))
}

/// Whether the running lane's frame constructs in place (`LaneContext::script_constructs`),
/// which a body's constructions inherit (`body_construction_in_lane`).
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn context_constructs_in_lane(context: *mut ()) -> bool {
    let cx = context.cast::<LaneContext>();
    quench_runtime::staged::call_construct::concrete_admits_construct_run(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs })
}

/// A lane body's `new callee(...)` of the `argc` arguments at `arguments`, in `code`, the running
/// residual, as `materialized_construction_in_lane`; `None` where a collection is due, which the
/// lane runs, leaving the body at the construction before any effect.
///
/// # Safety
/// `arguments` are the body's `argc` registers.
pub(crate) unsafe fn body_construction_in_lane(vm: &mut Vm, code: &ResidualProgram, callee: Value, class: u32, arguments: *const Value, argc: u16) -> Option<Value> {
    if vm.collection_due() { return None; }
    // SAFETY: the caller passes the body's argument registers.
    unsafe { materialized_construction_in_lane(vm, code, callee, class, arguments, argc) }
}

/// `body_construction_in_lane` made whatever is due, for a materialization of a kept receiver
/// (`lane_region::virtual_objects`), which has no leave to take (DESIGN 4.7): the callee's plan
/// (`planned_construction`), else the cached adds of its initializing body (`initializer_construction`).
///
/// # Safety
/// `arguments` are the body's `argc` registers.
pub(crate) unsafe fn materialized_construction_in_lane(vm: &mut Vm, code: &ResidualProgram, callee: Value, class: u32, arguments: *const Value, argc: u16) -> Option<Value> {
    if !quench_runtime::staged::call_construct::concrete_unpending(vm.construct_target.is_some()) {
        return None;
    }
    // SAFETY: the caller passes the body's argument registers.
    let argument = |parameter: u16| if parameter < argc { unsafe { *arguments.add(usize::from(parameter)) } } else { Value::UNDEFINED };
    // A forwarding constructor's plan (`this.initialize(...)`) holds with its forwarding.
    let this = planned_construction(vm, callee, class, argument)
        .or_else(|| forwarded_fields_in_lane(vm, callee, class, argument))
        .or_else(|| initializer_construction(vm, code, callee, class, argument))?;
    note_allocation(vm);
    Some(this)
}

/// The receiver of a construction through forwarding constructor `callee` whose forwarded
/// initializer the body calls by its entry (`DirectCallee::forwarder`): allocated finished
/// from the plan while it holds, whose forwarding has no fields of its own. `None` leaves
/// the body to its helper path before any effect.
pub(crate) fn body_receiver_in_lane(vm: &mut Vm, callee: Value, class: u32) -> Option<Value> {
    if !quench_runtime::staged::call_construct::concrete_unpending(vm.construct_target.is_some()) || vm.collection_due() || vm.with_scope_entered {
        return None;
    }
    let index = holding_construct_plan_index(vm, callee, class)?;
    let plan = &vm.lane_stack.construct_plans[index];
    if plan.kind != ConstructPlanKind::Forwarding || plan.fields != 0 || plan.forwarded.is_none() {
        return None;
    }
    let plan = *plan;
    // A fixed class's receiver is its union cell (R4-F), as `allocate_receiver` makes it.
    let this = match plan.fixed {
        Some(_) => plan.allocate(vm, |_| Value::UNDEFINED),
        None => vm.allocate_planned_object(plan.header, plan.prototype, &[Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS], 0),
    };
    note_allocation(vm);
    Some(this)
}

/// The receiver `callee`'s cached adds finish where its body only stores its parameters and
/// primitive constants into `this` (`Function::initializer_stores`) and an in-place
/// construction admits it (`Vm::inline_constructible`): the static facts of
/// `ScriptCallTarget::initializes`, so a region with no lane run constructs as the lane does,
/// and the plan it leaves serves later constructions. Such a body runs no guest code, binds no
/// environment and reads no `arguments`. A miss leaves the unreachable receiver to the
/// collector. Out of line, so planned constructions keep their code.
#[inline(never)]
pub(super) fn initializer_construction(vm: &mut Vm, code: &ResidualProgram, callee: Value, class: u32, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let (program, function, _) = vm.same_realm_user_target(callee)?;
    if program != vm.active_program || function == super::super::ROOT_FUNCTION_ID
        || !Vm::inline_constructible(code.function_get(function as usize)?) {
        return None;
    }
    let (this, added) = initialize_fields_in_lane(vm, code, callee, class, callee, function, argument)?;
    added.then_some(this)
}

/// `planned_construction_in_lane` under the plan at `index` with defaulted fields, whose values
/// it computes first; `None` for a forwarding constructor's plan, which holds only with its
/// forwarding (`forwarded_fields_in_lane`). Out of line, so plans of plain fields keep their
/// code.
#[inline(never)]
pub(super) fn defaulted_construction_in_lane(vm: &mut Vm, index: usize, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let plan = &vm.lane_stack.construct_plans[index];
    if plan.kind == ConstructPlanKind::Forwarding {
        return None;
    }
    let mut values = plan.values(vm, argument)?;
    // A nested field holds while its root does; a fresh object's allocation never collects.
    if let Some(nested) = plan.nested {
        if !nested_root_holds(vm, &nested) { return None; }
        if nested.constructs {
            values[usize::from(nested.field)] = nested_construction(vm, nested.constructor, &nested.arguments)?;
        }
    }
    { let plan = vm.lane_stack.construct_plans[index]; Some(plan.allocate_values(vm, &values)) }
}

/// A construction's parameter `k` as a plain activation sees it: argument `k`, or undefined
/// when not passed.
#[inline(always)]
pub(crate) fn construct_argument_in_lane(r: Registers, i: &LaneInstruction) -> impl Fn(u16) -> Value + '_ {
    let argc = ImmediateLayout::argument_count(i.imm());
    move |parameter| if parameter < argc { r.get(i.c + parameter) } else { Value::UNDEFINED }
}

/// The construct plan cache entry of a construction of `callee` as class `class`, when `callee` names a heap cell.
/// The class is part of the key: two construct sites of one callee under different classes take different entries,
/// and a plan serves only the class it records (`holding_construct_plan_index`).
#[inline] pub(crate) fn construct_plan_index(callee: Value, class: u32) -> Option<usize> {
    callee.heap_index().map(|index| (index as usize ^ class as usize) & (CONSTRUCT_PLANS - 1))
}

/// `callee`'s construct plan of class `class` while it holds: the entry names `callee` and `class`, the prototype
/// layout keeps the plan's epoch and `callee`'s own layout still yields the planned
/// `prototype`, from which the receiver's template is derived.
#[inline(always)]
pub(crate) fn holding_construct_plan(vm: &Vm, callee: Value, class: u32) -> Option<ConstructPlan> {
    holding_construct_plan_index(vm, callee, class).map(|index| vm.lane_stack.construct_plans[index])
}

/// Whether `callee`'s plan of class `class` holds now (`holding_construct_plan_index`) and an allocation by it
/// would not refuse (`body_construction_in_lane`): no construction under way, no collection due.
/// A body that keeps a receiver as its field values (`compiled::ScalarConstructPlan`) tests it
/// where the construction would run, so the receiver a leave allocates is the plan's.
pub(crate) fn construct_plan_holds(vm: &Vm, callee: Value, class: u32) -> bool {
    vm.construct_target.is_none() && !vm.collection_due() && holding_construct_plan_index(vm, callee, class).is_some()
}

/// The construct plan cache index of `callee`'s plan of class `class` while it holds (`holding_construct_plan`).
#[inline(always)]
pub(crate) fn holding_construct_plan_index(vm: &Vm, callee: Value, class: u32) -> Option<usize> {
    let index = construct_plan_index(callee, class)?;
    let plan = &vm.lane_stack.construct_plans[index];
    (plan.callee == callee && plan.class == class
        && vm.lane_stack.construct_watches[index].as_ref().is_some_and(|watch| watch.is_published()))
    .then_some(index)
}

/// Records `resolution`, where `callee`'s forwarding read `function`, in plan `index`, which
/// then also holds only while `function` and the holder of its `apply` keep their layouts.
pub(crate) fn record_forwarded_resolution(vm: &mut Vm, index: usize, function: Value, resolution: ForwardedResolution) {
    vm.lane_stack.construct_plans[index].forwarded = Some(resolution);
    watch_construct_plan(vm, index, false, &[function, resolution.apply_holder]);
}

