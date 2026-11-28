//! Construction in a lane: the planned fields a construction sets, the forwarded construction of a class constructor, and the forwarding resolution it takes.
use super::*;

/// Whether a construction at `i` may complete in the lane with a receiver its body finished
/// without an activation: into a register, into the activation that returns it, or from the
/// frame's own returning construct where `returning_call_admitted` admits it, as
/// `lane_script_construct` completes one.
#[inline(always)]
pub(in super::super) fn finishes_construction_in_lane(cx: *mut LaneContext, i: &LaneInstruction) -> bool {
    i.a & quench_runtime::bytecode::RETURN_REGISTER == 0
        || unsafe { vm(cx) }.lane_stack.has_frames()
        || returning_call_admitted(cx)
}

/// The receiver a forwarding constructor's holding plan finishes with its recorded fields
/// (`forwarded_fields_in_lane`), the first case `construct_keeping_activation_in_lane` tries, in a
/// function of its own: inlined there it ran inside that function's frame (six saved
/// registers and 1.3 KB of stack for the activation paths). `None` for any other case, which
/// that function then decides from the start.
#[inline(never)]
pub(in super::super) fn construct_planned_fields_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some())
        || !finishes_construction_in_lane(cx, i)
    {
        return None;
    }
    // SAFETY: `ip` is a record of the running view.
    let class = unsafe { site_class_in_lane(cx, ip, r.get(i.b)) };
    let this = forwarded_fields_in_lane(vm, r.get(i.b), class, construct_argument_in_lane(r, i))?;
    // Allocating the receiver can make a collection due.
    note_allocation(vm);
    Some(this)
}

/// `script_construct_keeping_miss`'s activation, under `script_construct_miss`'s admission:
/// the callee's registers and first record, or the receiver its body finished; the record
/// to continue at, or `None` for a construction of any target but a forwarding one. Out of
/// line: the call target it passes on lives in its own frame, not the handler's.
#[inline(never)]
pub(in super::super) fn construct_keeping_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<KeepingConstruction, Option<Ip>> {
    let vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    if !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some()) {
        return Err(Some(exit(ip)));
    }
    // SAFETY: `ip` is a record of the running view.
    let class = unsafe { site_class_in_lane(cx, ip, callee) };
    let finishes = finishes_construction_in_lane(cx, i);
    // A plan holding for `callee` fixes its function, so the plan alone answers before its
    // target is resolved.
    if finishes && let Some(this) = forwarded_fields_in_lane(vm, callee, class, construct_argument_in_lane(r, i)) {
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        return Ok(KeepingConstruction::Finished(this));
    }
    // Matched rather than filtered: the combinators moved the target through copies.
    let Some(target) = script_target_in_lane(cx, callee) else {
        return Err(None);
    };
    if !(target.constructs() && target.forwards()) {
        return Err(None);
    }
    // One look at the plan serves the receiver and the forwarding: a plan the receiver's
    // allocation leaves records no resolution yet.
    let plan = holding_construct_plan(vm, callee, class);
    let union = fixed_receiver_union(vm, callee, class);
    let Some(this) = planned_receiver_in_lane(vm, callee, class, plan.as_ref(), union) else {
        return Err(Some(exit(ip)));
    };
    note_allocation(vm);
    let arguments = ActivationArguments::Window(i.c);
    if let Some(method) = forwarded_method_of(vm, &target)
        && let Some(forwarded) = match resolved_forwarding_in_lane(cx, callee, plan.as_ref()) {
            Some(forwarded) => Some(forwarded),
            None => forwarded_construction_in_lane(cx, target, method, this),
        }
    {
        // A forwarded method that only initializes `this` from its parameters, which the
        // forwarding passes the construction's arguments as: its adds alone, with neither
        // activation, where the construction completes in the lane (`finishes_construction_in_lane`).
        // Its body completes with `undefined`, as the forwarding body then does, so
        // [[Construct]] yields `this`.
        if forwarded.initializes() && finishes {
            // SAFETY: the context's program outlives the lane run, and the forwarded target is
            // one of its functions (`script_target_in_lane`).
            let code = unsafe { &*(*cx).code };
            let argument = construct_argument_in_lane(r, i);
            let read = construct_plan::field_reader(vm, callee);
            if let Some(fields) = construct_plan::finish_forwarded_in_lane(vm, code, forwarded.env, this, forwarded.function, argument, read, union) {
                if let Some(fields) = fields {
                    record_forwarded_fields_in_lane(vm, callee, class, (forwarded.program(), forwarded.function), this, &fields);
                }
                return Ok(KeepingConstruction::Finished(this));
            }
        }
        note_construct_site(vm, i, callee, &forwarded);
        let forwarder = SkippedForwarder {
            function: target.function,
            callee,
            env: target.env,
            method,
        };
        return activate_keeping_in_lane::<false>(cx, r, ip, &forwarded, arguments, this, false, Some(forwarder))
            .map(|(registers, base)| KeepingConstruction::Activated(registers, base))
            .map_err(Some);
    }
    activate_keeping_in_lane::<false>(cx, r, ip, &target, arguments, this, true, None)
        .map(|(registers, base)| KeepingConstruction::Activated(registers, base))
        .map_err(Some)
}

/// The finished receiver of a forwarding constructor's construction under `callee`'s
/// holding plan, when it records the fields its forwarded initializer stores
/// (`record_forwarded_fields_in_lane`) and its forwarding still reads that initializer
/// (`resolved_forwarded_method`): the receiver that initializer's adds would finish, whose
/// body completes with `undefined`, as the forwarding body then does.
#[inline(always)]
pub(in super::super) fn forwarded_fields_in_lane(vm: &mut Vm, callee: Value, class: u32, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee, class)?;
    // The plan in place: the allocation borrows the heap beside it, not a copy.
    let plan = &vm.lane_stack.construct_plans[index];
    plan.forwarded?.initializer?;
    let mut values = [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS];
    plan.fill_values(vm, &mut values, argument)?;
    // A nested construction's field is a fresh object of its own plan; allocation never collects.
    if let Some(nested) = vm.lane_stack.construct_plans[index].nested {
        if !construct_plan::nested_root_holds(vm, &nested) { return None; }
        if nested.constructs {
            values[usize::from(nested.field)] = construct_plan::nested_construction(vm, nested.constructor, &nested.arguments)?;
        }
    }
    { let plan = vm.lane_stack.construct_plans[index]; Some(plan.allocate_values(vm, &values)) }
}

/// Records in `callee`'s holding plan of no fields, whose forwarding resolved
/// (`ConstructPlan::forwarded`), the fields `fields` the forwarded `initializer`'s adds gave
/// `this`, which that plan's receiver started as: later constructions allocate the finished
/// object (`forwarded_fields_in_lane`). The adds ran no guest code, so the plan's epoch still
/// holds.
#[inline(never)]
pub(in super::super) fn record_forwarded_fields_in_lane(
    vm: &mut Vm,
    callee: Value,
    class: u32,
    initializer: (ProgramId, u32),
    this: Value,
    fields: &ConstructPlan,
) {
    let Some(index) = holding_construct_plan_index(vm, callee, class) else {
        return;
    };
    let Some(shape) = vm.heap_reads().object(this).map(|object| object.shape()) else {
        return;
    };
    let plan = &mut vm.lane_stack.construct_plans[index];
    let Some(resolution) = plan.forwarded.filter(|_| plan.fields == 0) else {
        return;
    };
    *plan = plan.with_recorded(fields, shape, ForwardedResolution { initializer: Some(initializer), ..resolution });
    if let Some(nested) = fields.nested {
        watch_construct_plan(vm, index, false, nested.held());
    }
}

/// The method a forwarding constructor's body (`ScriptCallTarget::forwarded_method`) would
/// call on the receiver `this`, which a construction may activate in its place: the body's
/// two reads, `this.<method>` and its `apply`, as cached hits that run no guest code, the
/// intrinsic `Function.prototype.apply`, and a frameless target. `None` leaves the
/// construction to the constructor's own activation, which makes the reads itself.
#[inline(never)]
pub(in super::super) fn forwarded_construction_in_lane(
    cx: *mut LaneContext,
    constructor: ScriptCallTarget,
    method: quench_runtime::bytecode::ForwardedThisMethod,
    this: Value,
) -> Option<ScriptCallTarget> {
    // SAFETY: the context's program outlives the lane run, and the constructor belongs to it
    // (`Vm::resolve_script_call_target`), so the body's sites index the run's caches.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    // The tiers the lane's own reads of these sites take: a constructor every class shares
    // reads its method through the site's megamorphic entries, and `apply` lives on the
    // method's prototype, `Function.prototype`. Any other read takes the general cached one.
    // A read of `name` or `length` prepares its lookup (`Vm::reads_function_info`), which
    // only the general read does.
    let function = match (!vm.reads_function_info(method.method.0))
        .then(|| vm.polymorphic_field_hit(code, this, method.method.1))
        .flatten()
    {
        Some(function) => function,
        None => lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, this, method.method.0, method.method.1, 0)?,
    };
    let apply = match (!vm.reads_function_info(method.apply.0))
        .then(|| vm.cell_holder_field_hit(code, function, method.apply.1))
        .flatten()
    {
        Some(apply) => apply,
        None => lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, function, method.apply.0, method.apply.1, 0)?,
    };
    if !matches!(
        vm.function_record(apply),
        Some(FunctionRecord {
            kind: FunctionKind::Native(Native::FunctionApply),
            ..
        })
    ) || function == constructor.callee
    {
        return None;
    }
    script_target_in_lane(cx, function)
}

/// The method a forwarding constructor's construction activates, from `callee`'s plan's
/// recorded resolution (`resolved_forwarded_method`) as a frameless target, under `plan`,
/// `callee`'s holding plan. `None` resolves afresh (`forwarded_construction_in_lane`).
#[inline(always)]
pub(in super::super) fn resolved_forwarding_in_lane(
    cx: *mut LaneContext,
    callee: Value,
    plan: Option<&ConstructPlan>,
) -> Option<ScriptCallTarget> {
    let function = resolved_forwarded_method(callee, plan?)?;
    script_target_in_lane(cx, function)
}

/// The method `callee`'s forwarding reads under `plan`, its holding plan: the recorded
/// resolution's (`ConstructPlan::forwarded`), which the plan's dependency keeps (see
/// `ForwardedResolution`), unless it is `callee`.
#[inline(always)]
pub(in super::super) fn resolved_forwarded_method(callee: Value, plan: &ConstructPlan) -> Option<Value> {
    plan.forwarded.map(|resolution| resolution.method).filter(|&method| method != callee)
}

/// `forward_call_miss`'s activation: the callee's registers and first record, or the record
/// to continue at. Out of line: the call target it passes on lives in its own frame, not
/// the handler's.
#[inline(never)]
pub(in super::super) fn forward_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(Registers, Ip), Ip> {
    let window = ImmediateLayout::call_window_base(i.imm());
    if !unsafe { (*cx).script_calls }
        || ImmediateLayout::argument_count(i.imm()) != FORWARDED_APPLY_ARGUMENTS
        || r.get(window + 1) != Value::UNMATERIALIZED_ARGUMENTS
    {
        return Err(exit(ip));
    }
    let vm = unsafe { vm(cx) };
    if !matches!(
        vm.function_record(r.get(i.b)),
        Some(FunctionRecord {
            kind: FunctionKind::Native(Native::FunctionApply),
            ..
        })
    ) {
        return Err(exit(ip));
    }
    let Some(kept) = (unsafe { kept_arguments_in_lane(cx) }) else {
        return Err(exit(ip));
    };
    let Some(target) = script_target_in_lane(cx, r.get(i.c)) else {
        return Err(exit(ip));
    };
    let receiver = r.get(window);
    let Some(this) = call_this_in_lane(vm, &target, receiver) else {
        return Err(exit(ip));
    };
    activate_keeping_in_lane::<false>(cx, r, ip, &target, kept, this, false, None)
}
