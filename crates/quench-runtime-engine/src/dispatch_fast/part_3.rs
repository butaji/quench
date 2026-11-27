//! Functions of `dispatch_fast` in a codegen unit of their own (B1: a staged edit recompiles only the units that inline it).
#![allow(unused_imports)]
use super::*;

/// The plain activation (`push_passed_in_lane`) of a rest body of the running program that keeps
/// nothing (`TARGET_GATHERS_KEEPING` clear), passed the `argc` values at `passed` (its window
/// of `r`, or the elements of the array a spread call passes): the fixed parameters copied as
/// any call's are, and the rest parameter's register the array of the values past them, read
/// in place, or `undefined` for a rest view that keeps the values after its registers.
#[inline(always)]
pub(super) fn activate_gathering_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if (returns && !returning_call_admitted(cx)) || vm.with_scope_entered {
        return Err(exit(ip));
    }
    let view = target.forms & TARGET_REST_VIEW != 0;
    let room = usize::from(target.room) + if view { usize::from(argc) } else { 0 };
    if !quench_runtime::staged::call_keeping::concrete_admits_keeping(
        vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get(), vm.lane_stack.values.room(), room,
    ) {
        return Err(script_call_frame_in_lane(ip, room));
    }
    let fixed = target.params - 1;
    if view {
        // A rest view keeps every value passed after its registers, as a keeping activation
        // does, and its rest register holds `undefined` (`LaneFrame::rest_view`).
        let callee_registers = fill_window_in_lane(vm, target, passed, argc.min(fixed));
        callee_registers.set(fixed, Value::UNDEFINED);
        let count = usize::from(target.registers);
        // SAFETY: the room checked above holds the kept values past the window.
        unsafe {
            quench_runtime::staged::call_keeping::keep_passed(
                &mut quench_runtime::staged::Concrete::new(std::ptr::null()), callee_registers.0.add(count).expose_provenance() as u64,
                passed.expose_provenance() as u64, argc,
            );
            vm.lane_stack.values.set_top(callee_registers.0.add(count + usize::from(argc) + lane_values::CONTEXT));
        }
        return Ok(enter_window_in_lane::<false, false>(cx, r, ip, target, callee_registers, argc, this, false, None, Some(target.registers)));
    }
    // The entry, which callers read in place, is copied before the rest array allocates.
    let target = &{ *target };
    let rest = vm.rest_parameter_array(unsafe {
        std::slice::from_raw_parts(passed.add(usize::from(fixed.min(argc))), usize::from(argc.saturating_sub(fixed)))
    });
    let callee_registers = fill_window_in_lane(vm, target, passed, argc.min(fixed));
    callee_registers.set(fixed, rest);
    Ok(enter_window_in_lane::<false, false>(cx, r, ip, target, callee_registers, argc.min(target.params), this, false, None, None))
}

/// Whether a construction at `i` may complete in the lane with a receiver its body finished
/// without an activation: into a register, into the activation that returns it, or from the
/// frame's own returning construct where `returning_call_admitted` admits it, as
/// `lane_script_construct` completes one.
#[inline(always)]
pub(super) fn finishes_construction_in_lane(cx: *mut LaneContext, i: &LaneInstruction) -> bool {
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
pub(super) fn construct_planned_fields_in_lane(cx: *mut LaneContext, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some())
        || !finishes_construction_in_lane(cx, i)
    {
        return None;
    }
    let this = forwarded_fields_in_lane(vm, r.get(i.b), construct_argument_in_lane(r, i))?;
    // Allocating the receiver can make a collection due.
    note_allocation(vm);
    Some(this)
}

/// `script_construct_keeping_miss`'s activation, under `script_construct_miss`'s admission:
/// the callee's registers and first record, or the receiver its body finished; the record
/// to continue at, or `None` for a construction of any target but a forwarding one. Out of
/// line: the call target it passes on lives in its own frame, not the handler's.
#[inline(never)]
pub(super) fn construct_keeping_activation_in_lane(
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
    let finishes = finishes_construction_in_lane(cx, i);
    // A plan holding for `callee` fixes its function, so the plan alone answers before its
    // target is resolved.
    if finishes && let Some(this) = forwarded_fields_in_lane(vm, callee, construct_argument_in_lane(r, i)) {
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
    let plan = holding_construct_plan(vm, callee);
    let Some(this) = planned_receiver_in_lane(vm, callee, plan.as_ref()) else {
        return Err(Some(exit(ip)));
    };
    note_allocation(vm);
    let arguments = ActivationArguments::Window(i.c);
    if let Some(&method) = vm.lane_stack.forwarded_methods.get(target.forwarded_method as usize)
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
            if let Some(fields) = construct_plan::finish_forwarded_in_lane(vm, code, forwarded.env, this, forwarded.function, argument) {
                if let Some(fields) = fields {
                    record_forwarded_fields_in_lane(vm, callee, (forwarded.program(), forwarded.function), this, &fields);
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
pub(super) fn forwarded_fields_in_lane(vm: &mut Vm, callee: Value, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee)?;
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
pub(super) fn record_forwarded_fields_in_lane(
    vm: &mut Vm,
    callee: Value,
    initializer: (ProgramId, u32),
    this: Value,
    fields: &ConstructPlan,
) {
    let Some(index) = holding_construct_plan_index(vm, callee) else {
        return;
    };
    let Some(shape) = vm.object_data(this).map(|object| object.shape()) else {
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
pub(super) fn forwarded_construction_in_lane(
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
    let forwarded = script_target_in_lane(cx, function)?;
    if let Some(resolution) = forwarded_resolution(vm, constructor.callee, method, function, apply) {
        let index = construct_plan_index(constructor.callee)?;
        record_forwarded_resolution(vm, index, function, resolution);
    }
    Some(forwarded)
}

/// Where a forwarding constructor's reads of `this.<method>` and its `apply`, which gave
/// `function` and `apply`, resolved under `callee`'s plan of no fields while it holds
/// (`ForwardedResolution`): own data properties of the plan's prototype and of the method's
/// prototype, neither shadowed by the receiver template or the method, in shapes that fix
/// their slots. `None` records nothing.
pub(super) fn forwarded_resolution(
    vm: &Vm,
    callee: Value,
    method: quench_runtime::bytecode::ForwardedThisMethod,
    function: Value,
    apply: Value,
) -> Option<ForwardedResolution> {
    let plan = holding_construct_plan(vm, callee).filter(|plan| plan.fields == 0)?;
    let data_slot = |owner: &quench_runtime::tier_surface::Object, atom| {
        let shape = owner.shape();
        if vm.shape_is_dictionary(shape) {
            return None;
        }
        let slot = vm.shape_slot(shape, atom)?;
        // SAFETY: the slot exists in the non-dictionary shape that names it.
        let value = unsafe { vm.heap_reads().property_get_shaped(owner, slot) }?;
        (!value.is_deleted()).then_some((u16::try_from(slot).ok()?, value))
    };
    let data = |owner: Value, atom, slot: usize| {
        let key = quench_runtime::vm::PropertyKey::string(atom);
        vm.property_attributes(owner, key).is_some_and(|attributes| !attributes.accessor)
            && vm.object_data(owner).is_some_and(|object| vm.shape_slot(object.shape(), atom) == Some(slot))
    };
    if vm.shape_slot(plan.shape, method.method.0).is_some() {
        return None;
    }
    let prototype = vm.heap_reads().object_record(plan.prototype)?;
    let (method_slot, found) = data_slot(prototype, method.method.0)?;
    let FunctionRecord { object, .. } = vm.function_record(function)?;
    if found != function || vm.shape_slot(object.shape(), method.apply.0).is_some() {
        return None;
    }
    let apply_holder = object.proto();
    let (apply_slot, found) = data_slot(vm.heap_reads().object(apply_holder)?, method.apply.0)?;
    (found == apply
        && data(plan.prototype, method.method.0, usize::from(method_slot))
        && data(apply_holder, method.apply.0, usize::from(apply_slot)))
    .then_some(ForwardedResolution {
        method: function,
        apply_holder,
        initializer: None,
    })
}

/// The method a forwarding constructor's construction activates, from `callee`'s plan's
/// recorded resolution (`resolved_forwarded_method`) as a frameless target, under `plan`,
/// `callee`'s holding plan. `None` resolves afresh (`forwarded_construction_in_lane`).
#[inline(always)]
pub(super) fn resolved_forwarding_in_lane(
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
pub(super) fn resolved_forwarded_method(callee: Value, plan: &ConstructPlan) -> Option<Value> {
    plan.forwarded.map(|resolution| resolution.method).filter(|&method| method != callee)
}

/// `forward_call_miss`'s activation: the callee's registers and first record, or the record
/// to continue at. Out of line: the call target it passes on lives in its own frame, not
/// the handler's.
#[inline(never)]
pub(super) fn forward_activation_in_lane(
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

/// The values the running function kept for its forwarded `arguments`: a forwarding
/// activation's, after its registers, or a frame's `original_arguments`.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(super) unsafe fn kept_arguments_in_lane(cx: *mut LaneContext) -> Option<ActivationArguments> {
    let vm = unsafe { vm(cx) };
    if unsafe { (*cx).registers }.env().is_deleted() {
        let frame = &vm.frames[unsafe { (*cx).frame() }];
        return Some(ActivationArguments::Kept(
            frame.original_arguments.as_ptr(),
            frame.original_arguments.len(),
        ));
    }
    let activation = vm.lane_stack.top_frame()?;
    let script = activation.script().filter(|script| script.forwards)?;
    let registers = usize::from(unsafe { &*(*cx).code }.function(activation.function() as usize).registers);
    // SAFETY: the activation's window holds its registers, then its kept values.
    let values = unsafe { activation.window_ptr().add(registers) };
    Some(ActivationArguments::Kept(values, usize::from(script.argc)))
}

/// The values the running function kept for its unmaterialized `arguments` binding, in the
/// local slot `slot` (`kept_arguments_in_lane`): `None` where a frame's binding was
/// materialized, which the general path then reads.
///
/// # Safety
/// `cx` is the running lane's context, and `slot` its function's validated local slot.
#[inline(always)]
pub(super) unsafe fn unmaterialized_arguments_in_lane(
    cx: *mut LaneContext,
    slot: usize,
) -> Option<(*const Value, usize)> {
    if unsafe { (*cx).registers }.env().is_deleted()
        && unsafe { vm(cx).read_validated_local((*cx).frame(), slot) }
            != Value::UNMATERIALIZED_ARGUMENTS
    {
        return None;
    }
    match unsafe { kept_arguments_in_lane(cx) }? {
        ActivationArguments::Kept(values, count) => Some((values, count)),
        ActivationArguments::Window(_) => None,
    }
}

/// The running keeping activation's kept-value count and its function, when the lane runs
/// one (`ScriptActivation::forwards`): its values follow its registers in its window, which
/// `r` names. `None` for a frame or any other activation.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(super) unsafe fn keeping_activation_in_lane(cx: *mut LaneContext) -> Option<(u16, u32)> {
    if unsafe { (*cx).registers }.env().is_deleted() {
        return None;
    }
    let activation = unsafe { vm(cx) }.lane_stack.top_frame()?;
    let script = activation.script().filter(|script| script.forwards)?;
    Some((script.argc, activation.function()))
}

/// The count of the running function's `arguments` binding in local `slot`, for the
/// `ArgumentsLength` row (`Frame::arguments_length`): a keeping activation's kept-value count.
/// `KEPT`: the view proved `slot` its function's kept arguments (`Function::kept_arguments`).
/// `None` leaves the count to the row's general step, which reads the frame's or window's binding.
///
/// # Safety
/// `cx` is the running lane's context, and `slot` its function's validated local slot.
#[inline(always)]
pub(super) unsafe fn arguments_count<const KEPT: bool>(cx: *mut LaneContext, slot: u16) -> Option<Value> {
    if let Some((argc, function)) = unsafe { keeping_activation_in_lane(cx) }
        // SAFETY: the context's program outlives the lane run.
        && (KEPT || unsafe { &*(*cx).code }.function(function as usize).arguments_slot == Some(slot))
    {
        return Some(Value::integer(i32::from(argc)));
    }
    None
}

/// The keeping activation's `arguments` binding a local read of its slot takes (`Frame::arguments_binding`): a
/// frame run reads its own binding as the general path does (`Vm::read_validated_local`, which materializes it);
/// a frameless activation's is the unmaterialized marker a forwarding read passes on (`OBJECT` false), or the
/// object made from the values it kept (`OBJECT` true, `Vm::activation_arguments_object`), which the keeping
/// activation's last read makes. `None` where the object cannot be made: the row leaves to its miss.
/// Subsumes the hand loads `lane_load_forwarded_arguments` and `lane_load_kept_arguments_object` of the lane.
///
/// # Safety
/// `cx` is the running lane's context, and `r` its register file.
pub(super) unsafe fn kept_arguments_binding<const OBJECT: bool>(cx: *mut (), r: Registers, slot: u16) -> Option<Value> {
    let cx = cx.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    // The forwarding check: a frame run's registers hold the deleted environment marker.
    if unsafe { (*cx).registers }.env().is_deleted() {
        // SAFETY: validated bytecode bounds the slot by Function.locals, and the frame is the running one.
        return Some(unsafe { vm.read_validated_local((*cx).frame(), usize::from(slot)) });
    }
    if !OBJECT {
        return Some(Value::UNMATERIALIZED_ARGUMENTS);
    }
    let Some((function, Some(script))) =
        vm.lane_stack.top_frame().map(|activation| (activation.function(), activation.script()))
    else {
        return None;
    };
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(function as usize);
    // A keeping activation's values follow its registers (`activate_keeping_in_lane`).
    let kept = (0..script.argc)
        .map(|index| r.get(body.registers + index))
        .collect::<SmallVec<[Value; GATHERED_INLINE_VALUES]>>();
    let context = CallContext::user_function(function, script.callee);
    let arguments = vm.activation_arguments_object(body, context, &kept).ok()?;
    note_allocation(vm);
    Some(arguments)
}

/// The frameless target of a lane call to `callee`, resolved on first use.
/// Whether `callee` is the target of a `new` whose `new.target` the general path still
/// holds (`Vm::construct_target`): a lane activation would not bind it.
#[inline(always)]
pub(super) fn constructing_in_lane(vm: &Vm, callee: Value) -> bool {
    vm.construct_target
        .is_some_and(|target| target.callee == callee)
}

#[inline(always)]
pub(super) fn script_target_in_lane(cx: *mut LaneContext, callee: Value) -> Option<ScriptCallTarget> {
    script_target_in_in_lane(cx, callee, &mut None).copied()
}

/// `script_target_in_lane`'s target, read in place where the cache holds it, else resolved
/// into `resolved`. The cache's entry holds until the next resolution or collection.
#[inline(always)]
pub(super) fn script_target_in_in_lane(
    cx: *mut LaneContext,
    callee: Value,
    resolved: &mut Option<ScriptCallTarget>,
) -> Option<&ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // The paths besides a plain call (constructions, getters, forwarded and applied calls)
    // activate callees of the running program alone. Another program's callee, once
    // resolved, is held apart (`cross_call_entry`) for the callers that activate across
    // programs; resolving it again would only find it so.
    // The cached entry is tested and used in place; `script_call_holds` admitted it for the
    // active program already.
    if let Some(entry) = vm.script_call_entry(callee) {
        // SAFETY: the entry is read before anything else touches the cache.
        let target = unsafe { &*entry };
        return admits_activation_in_lane(target).then_some(target);
    }
    // A cached entry or cross entry is admitted by its link's compare, which refuses under
    // any condition (no script calls, a construction pending); a resolve makes no compare, so
    // the conditions refuse it here, once, for every unlinked caller.
    if vm.lane_stack.refuses_any() || vm.cross_call_entry(callee).is_some() {
        return None;
    }
    let active = vm.active_program;
    *resolved = vm.resolve_script_call_target(unsafe { &*(*cx).code }, callee)
        .filter(|target| target.program() == active && admits_activation_in_lane(target));
    resolved.as_ref()
}

/// The instance environment the running Wasm function reads its bindings from: the
/// export's (`LaneContext::instance`) above a JavaScript call of it, else its frame's.
#[inline(always)]
pub(super) fn instance_env_in_lane(cx: *mut LaneContext) -> Value {
    // SAFETY: the context outlives the lane run.
    let instance = unsafe { (*cx).instance };
    if !instance.is_deleted() {
        return instance;
    }
    unsafe { vm(cx).frames[(*cx).frame()].env }
}

/// Make `program` the lane's running program, as a call into or a return to one of its
/// functions makes it the general loop's executing one: the residual, lane views and
/// constants its records read, and the VM's active program, whose cache layout its sites
/// index.
#[inline(always)]
pub(super) fn enter_program_in_lane(cx: *mut LaneContext, program: ProgramId) {
    let vm = unsafe { vm(cx) };
    let context = vm.program_context_in_lane(program);
    vm.activate_context(program, context);
    // The innermost plain activation's caller program (`PlainFrames::program`) is the program
    // the next plain push runs under. An emitted direct call bumps the plain depth without
    // writing it, so after a crossing push (whose side record keeps its own caller program)
    // the word must name the entered program, or a plain frame pushed above it by emitted
    // code reads its caller as the crossing frame's caller program.
    vm.lane_stack.plain.program.set(program);
    // SAFETY: the context outlives the lane run; the store keeps the program's residual,
    // views and constants for it, and its context names them.
    unsafe {
        (*cx).code = (*context).code;
        (*cx).program = program;
        (*cx).views = (*context).views;
        (*cx).constants = (*context).constants;
    }
}

/// Activate `target`, which keeps no passed values (callers route those to
/// `activate_keeping_in_lane`), for the call record at `ip` with the arguments from register
/// `arguments` on: the activation's registers and the callee's
/// first record, or the record to continue at when the call takes the general path.
#[inline(always)]
pub(super) fn activate_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
    constructs: bool,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    let argc = ImmediateLayout::argument_count(i.imm());
    // A call that returns its callee's completion completes its caller too, which only an
    // activation's return does in the lane. A host permitting proper tail calls
    // sends strict terminal calls to the general dispatch path instead.
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns && !returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    if vm.with_scope_entered {
        return Err(exit(ip));
    }
    // Values past the parameters stay observable (legacy `f.arguments`, a frame made of the
    // activation): the keeping activation holds them after the registers. A callee no legacy
    // reflection observes is passed its parameters alone, as its frame would keep them.
    if argc > target.params && (!target.drops_extra_arguments() || target.forwards()) {
        // A call keeping every value it was passed, a rest body's array aside, takes the plain
        // push with the values after its registers.
        if !constructs && target.entry != CallEntry::Gathers {
            return activate_keeping_window_in_lane::<true>(cx, r, ip, target, arguments, this);
        }
        return activate_keeping_in_lane::<true>(
            cx,
            r,
            ip,
            target,
            ActivationArguments::Window(arguments),
            this,
            constructs,
            None,
        );
    }
    if !activation_room_in_lane(vm, target) {
        return Err(script_call_frame_in_lane(ip, usize::from(target.room)));
    }
    // SAFETY: the call's window of `r` holds its `argc` values.
    let passed = unsafe { r.0.add(usize::from(arguments)).cast_const() };
    Ok(push_passed_in_lane::<true, false>(cx, r, ip, target, passed, argc.min(target.params), this, constructs))
}

/// Whether the lane stack takes `target`'s activation: capacity, the guest-call budget and
/// a due collection (`LaneStack::activation_limit`).
#[inline(always)]
pub(super) fn activation_room_in_lane(vm: &Vm, target: &ScriptCallTarget) -> bool {
    vm.lane_stack.admits_activations(1)
        && quench_runtime::staged::call::concrete_fits(vm.lane_stack.values.room() as u64, u64::from(target.room))
}

/// Whether `target` takes `argc` arguments and the lane stack holds it: `staged::call::admit_room` run on words.
#[inline(always)]
pub(super) fn admits_window_in_lane(vm: &Vm, target: &ScriptCallTarget, argc: u16) -> bool {
    quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(argc)) && activation_room_in_lane(vm, target)
}

/// Where the lane continues after a frameless activation's tier prologue (`tier_prologue_in_lane`): at the record
/// a compiled body's result completes (`Vm::js_tier_activation_entry`), result in its register, else at its first
/// record. Shared by the prologue's staged row (`EntryHost`) and its hand body.
#[inline(always)]
pub(super) fn tier_prologue_resumed(cx: *mut LaneContext, r: Registers, view: Carried, function: u32, params: usize) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the activation just set the context to its own function's.
    let base = unsafe { (*cx).base };
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view);
    // A body warming between attempts has nothing to enter: the entry is counted and its first record runs.
    if let Some(own) = own && crate::compiled::admission::lane_bodies_enrolled(vm) && own.counted_between_attempts(false) { return Resumed::At(r, base, Carried::script_in(cx, view)); }
    // A function of a program with a loaded AOT image enters the image's body at its first entry.
    // SAFETY: the prologue is this record, written when the tier claimed the function (`claim_for_tier`).
    if let Some(own) = own && vm.images_run_in_lane() && vm.programs.ahead_images(unsafe { (*cx).program }, unsafe { &*(*cx).code }).is_some() {
        let prologue = own.prologue.get().cast_const();
        return body_calls::enter_published_body(cx, prologue, r, view, function, unsafe { (*prologue).b } != 0);
    }
    // SAFETY: the activation's registers hold its parameters from its first.
    let parameters = unsafe { std::slice::from_raw_parts(r.0, params) };
    let entered = own
        .and_then(|own| tier_direct_in_lane(vm, own, parameters))
        .or_else(|| tier_enter_in_lane(cx, function, own, parameters));
    if let Some((pc, register, value)) = entered {
        r.set(register, value);
        // SAFETY: the tier returns a PC of the function, whose records are contiguous.
        return Resumed::At(r, unsafe { base.add(pc) }, Carried::script_in(cx, view));
    }
    Resumed::At(r, base, Carried::script_in(cx, view))
}

/// The protocol row of a frameless activation's tier prologue record (`TierPrologue`): the staged prologue
/// row (`PrologueFrame::tier_prologue`), with `tier_prologue_in_lane` as the body it completes where it leaves.
pub(super) const fn tier_prologue_row() -> Handler {
    protocol_row!({ quench_runtime::bytecode::TIER_PROLOGUE as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::TIER_PROLOGUE as u16 })
}

/// The protocol row of a specialized body's prologue record (`BodyEntry`): the staged prologue row
/// (`PrologueFrame::body_entry`), with `body_entry_in_lane` as the body it completes where it leaves.
pub(super) const fn body_entry_row() -> Handler {
    protocol_row!({ quench_runtime::bytecode::BODY_ENTRY as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::BODY_ENTRY as u16 })
}

/// The receiver word a body is entered with: the activation's `this` (bound by `direct_call_this_in_lane`)
/// where the body reads it (`LaneView::binds_this`), else `DELETED`, at which its `LoadThis` leaves.
#[inline(always)]
pub(super) fn entry_receiver_in_lane(cx: *mut LaneContext, binds_this: bool) -> Value {
    // SAFETY: the context is the running activation's, or the leaf's callee.
    if binds_this { unsafe { this_in_lane(cx, (*cx).registers) }.unwrap_or(Value::DELETED) } else { Value::DELETED }
}

/// Where the lane continues after a run of `method`, `own`'s body, that ran on the
/// activation `cx` holds, with registers `r`, and ended at `exit`: a returned run completes
/// the activation (no leave to resolve, no frame to materialize), any other leave resumes
/// where it names (`body_left_in_lane`).
#[inline(always)]
pub(super) fn body_concluded_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, own: &LaneView, method: &crate::compiled::LaneMethod, left: crate::compiled::LaneExit, function: u32) -> Resumed {
    let base = unsafe { (*cx).base };
    if let crate::compiled::LaneExit::Returned { pc, value } = left {
        record_end_in_lane(cx, own, method, left);
        // SAFETY: as `leave_counts_in_lane`; a completed run carries no warming callee.
        unsafe { (*cx).callee_warming = false };
        note_body_end_in_lane(cx, own, left, |_| {});
        // SAFETY: the record that completes the activation is one of its function's.
        return returned_in_lane(cx, unsafe { base.add(pc) }, value);
    }
    let counts = leave_counts_in_lane(cx, left);
    let (resumed, feedback) = body_left_in_lane(cx, r, own, method, left);
    // Counted once the leave is resolved: a retirement or reoptimization moves the body.
    if counts { note_body_end_in_lane(cx, own, feedback, |left| note_leave_in_lane(cx, function, left)) } else { note_warming_leave_in_lane(cx, own) }
    body_resumed_in_lane(cx, ip, resumed)
}

/// Where the lane continues after a lane body's resolved leave `resumed`, which left at `ip`.
#[inline(always)]
pub(super) fn body_resumed_in_lane(cx: *mut LaneContext, ip: Ip, resumed: LaneBodyLeft) -> Resumed {
    match resumed {
        LaneBodyLeft::Terminated => Resumed::Exit(exit(ip)),
        LaneBodyLeft::Resume(_, resume, _) if body_calls::left_lane(resume) => Resumed::Exit(resume),
        LaneBodyLeft::Resume(registers, resume, carried) => Resumed::At(registers, resume, carried),
        LaneBodyLeft::Raised(record) => Resumed::Exit(raised_exit_in_lane(cx, record)),
        LaneBodyLeft::Completed(record, value) => returned_in_lane(cx, record, value),
    }
}

/// `lane_return_value!` as the resumption it continues at.
#[inline(always)]
pub(super) fn returned_in_lane(cx: *mut LaneContext, record: Ip, value: Value) -> Resumed {
    complete_plain_activation_in_lane::<NOT_LEAF>(cx, value).unwrap_or_else(|| return_value_general_in_lane(cx, record, value))
}

/// The tier prologue's run of the compiled body `own` keeps for a body that reads only its
/// parameters (`LaneView::tier_direct`): the `Return` to continue at, its register and the result.
#[inline(always)]
pub(super) fn tier_direct_in_lane(vm: &mut Vm, own: &LaneView, parameters: &[Value]) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
    let direct = own.tier_direct.take()?;
    let entered = vm.run_direct_tier_entry(&direct, parameters);
    if !direct.retired() { own.tier_direct.set(Some(direct)); }
    entered
}

/// The tier prologue's entry where no direct run completes the activation (`Vm::js_tier_activation_entry`).
/// A completed entry keeps the direct run the compiled body admits; once the tier declines the
/// function, its activations start at the first record again (`LaneView::release_from_tier`).
/// A lane body runs on the activation itself, so it takes the function from the frame-entering
/// tier; a program with a loaded AOT image entered its body already (`tier_prologue_resumed`).
#[cold]
#[inline(never)]
pub(super) fn tier_enter_in_lane(cx: *mut LaneContext, function: u32, own: Option<&LaneView>, parameters: &[Value]) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the activation just set the context to its own function's.
    let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
    if crate::compiled::admission::lane_bodies_enrolled(vm) && let Some(own) = own {
        if !specialize_in_lane(cx, own, function, false, parameters, None) { own.release_from_tier(); }
        return None;
    }
    // A function only lane bodies claimed (`claim_for_tier`) has no JS-tier entry to try.
    let may_claim = vm.js_tier_may_claim(code, program, function);
    if may_claim && let Some(entered) = vm.js_tier_activation_entry(code, program, function, parameters) {
        if let Some(own) = own && let Some(direct) = vm.direct_tier_entry(code, program, function) { own.tier_direct.set(Some(direct)); }
        return Some(entered);
    }
    if let Some(own) = own && !may_claim { own.release_from_tier(); }
    None
}

/// The script targets the call record at `pc` of `view` linked (`call_link`), newest first:
/// its latest and its history (`LaneStack::call_sites`), each where a plain call of it binds
/// an ordinary `this` and activates in place: what a lane body inlines (`lane_unit::unit`).
pub(crate) fn site_targets_in_lane(vm: &Vm, view: &LaneView, pc: u32) -> Vec<crate::compiled::CallTarget> {
    // SAFETY: the view holds a record per PC; reading its link changes nothing.
    let Some(record) = view.records.get(pc as usize).map(|record| unsafe { &*record.get() }) else { return Vec::new() };
    let history = vm.lane_stack.call_sites.borrow();
    // The callees the site resolved or memoized, in the order first seen (`LaneStack::note_site_callee`).
    let recorded = history.get(&std::ptr::from_ref(&record.call_link).addr());
    // A collection since the site linked retires its entry; the callee's own entry of this
    // generation, where a call resolved it again, is the same target. A linked entry another
    // site's callee took since names no callee of this site.
    let latest = vm.lane_stack.script_targets.entries.get(link_index(record.call_link.get())).and_then(|linked| {
        if linked.admission.of_generation(vm.lane_stack.admission) {
            Some(linked)
        } else {
            // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
            vm.script_call_entry(linked.callee).map(|target| unsafe { &*target })
        }
    }).filter(|target| {
        target.program() == vm.active_program
            && target.entry.activates_registers_only()
            && target.this_binding.binds_receiver()
            && recorded.is_none_or(|recorded| recorded.iter().any(|site| site.identity() == Some(target.callee)))
    });
    match latest {
        Some(latest) => {
            let latest = crate::compiled::CallTarget {
                callee: crate::compiled::CalleeBinding::Identity(latest.callee), function: latest.function };
            std::iter::once(latest)
                .chain(recorded.into_iter().flatten().rev().copied().filter(|target| *target != latest))
                .take(crate::compiled::tier_policy::POLYMORPHISM)
                .collect()
        }
        // A site that runs only in its function's lane body, or that a read memo answers,
        // links nothing current: the callees it recorded, else the body's own inlined calls
        // (`lane_unit::InlinedCall`), are its targets.
        None => match recorded {
            Some(recorded) => recorded.iter().rev().copied().collect(),
            None => view.body_view_in_lane().map_or_else(Vec::new, |method| {
                method.inlined.iter()
                    .filter(|frame| frame.parent.is_none() && frame.call == pc)
                    .map(|frame| frame.target)
                    .collect()
            }),
        },
    }
}

/// `site_targets_in_lane` of any function of the running program, by its view.
pub(super) fn unit_targets_in_lane(cx: *mut LaneContext, function: u32, pc: u32) -> Vec<crate::compiled::CallTarget> {
    // SAFETY: the program's views outlive the lane run.
    let Some(view) = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view) else {
        return Vec::new();
    };
    site_targets_in_lane(unsafe { vm(cx) }, view, pc)
}

/// The mean trips of the loop at `header` of any function of the running program per native
/// run of it (`LoopTier::mean_trip`): what its call sites are weighed by (`lane_unit::with_trips`).
pub(super) fn unit_trips_in_lane(cx: *mut LaneContext, function: u32, header: u32) -> Option<u64> {
    // SAFETY: the program's views outlive the lane run.
    let view = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)?;
    for site in &view.loops {
        if site.header == header {
            return site.tier.borrow().mean_trip();
        }
    }
    None
}

/// The dense capture slots of `callee`'s environment (`Vm::callee_capture_slots`).
pub(super) fn callee_captures_in_lane(cx: *mut LaneContext, callee: Value) -> (usize, usize) {
    unsafe { vm(cx) }.callee_capture_slots(callee)
}

/// Records the forwarded initializer the construction at `i` ran, which a body may call by
/// its entry (`construct_callee_in_lane`).
pub(super) fn note_construct_site(vm: &Vm, i: &LaneInstruction, constructor: Value, forwarded: &ScriptCallTarget) {
    vm.lane_stack.note_site_callee(&i.call_link, crate::compiled::CallTarget {
        callee: crate::compiled::CalleeBinding::Forwarded { through: constructor, callee: forwarded.callee },
        function: forwarded.function,
    });
}

/// Whether a lane body's run may call bodies by their compiled entries: the lane may make
/// their activations should it leave inside one (`body_call_helper`'s own conditions).
pub(crate) fn direct_calls_in_lane(cx: *mut LaneContext) -> bool {
    let vm = unsafe { vm(cx) };
    let script_calls = unsafe { (*cx).script_calls };
    // A direct callee's registers follow the caller's window, where an activation that keeps its
    // locals there (`context_locals_in_window_in_lane`) has them: such a run calls through the lane.
    script_calls && !vm.with_scope_entered && vm.construct_target.is_none() && !unsafe { context_locals_in_window_in_lane(cx as usize) }
}

/// Whether a run of `own`'s specialized body may start: the one thing per run it cannot
/// prove at specialization, that the lane can make the activation of a call it inlines
/// should it leave inside one (`body_inlined_activation_in_lane`). `pending` activations are
/// the running body's own, which its call has not pushed yet (`enter_cell_body_in_lane`).
#[inline(always)]
pub(super) fn body_admits_in_lane(cx: *mut LaneContext, method: &crate::compiled::LaneMethod, pending: usize) -> bool {
    room_admits_in_lane(cx, method.recovery_space(), pending)
}

/// Whether the lane has the room `room` names: the activations and values a leave inside an inlined
/// callee of native code about to run makes (`body_inlined_activation_in_lane`), past the `pending`
/// activations the running body's own call has not pushed.
#[inline(always)]
pub(super) fn room_admits_in_lane(cx: *mut LaneContext, room: LaneRecoverySpace, pending: usize) -> bool {
    let vm = unsafe { vm(cx) };
    // The values past the stack's top hold the frame slots the rows write past a lane activation's
    // registers (`LaneRecoverySpace::reaching`), whether or not the code inlines an activation.
    (room.activations == 0 && (!vm.lane_stack.has_frames() || vm.lane_stack.values.room() >= room.values))
        || (vm.lane_stack.activation_limit.get().saturating_sub(vm.lane_stack.activation_count() + pending) >= room.activations
            && vm.lane_stack.values.room() >= room.values
            && unsafe { (*cx).script_calls }
            && !vm.with_scope_entered)
}

/// A compiled region's recovery demand may exceed the calls the lane has observed.
/// Materialize before growing either Vec: live lane windows borrow their storage.
/// The caller must exit immediately and resume from its original semantic record.
#[cold]
#[inline(never)]
pub(super) fn body_prepare_recovery_in_lane(cx: *mut LaneContext, method: &crate::compiled::LaneMethod) -> bool {
    room_prepare_in_lane(cx, method.recovery_space())
}

/// Grows the lane stacks to the room `room` names (`room_admits_in_lane`), materializing the lane's
/// activations first; whether it grew them. The caller must exit at once and resume from its
/// original semantic record.
#[cold]
#[inline(never)]
pub(super) fn room_prepare_in_lane(cx: *mut LaneContext, room: LaneRecoverySpace) -> bool {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } || vm.with_scope_entered { return false; }
    let values = vm.lane_stack.values.room() < room.values;
    let activations = vm.lane_stack.frame_capacity() - vm.lane_stack.activation_count() < room.activations;
    if !values && !activations { return false; }
    vm.materialize_lane_activations(cx);
    if values { vm.lane_stack.reserve_values(room.values); }
    if activations { vm.lane_stack.records_mut().reserve(room.activations); }
    vm.lane_stack.refresh_activation_limit();
    true
}

/// The activation native code that left inside inlined `frame` of `run`, at its callee's
/// PC `pc`, continues in: the activations the calls of the frame and the frames around it
/// make (`push_activation_in_lane`), outermost first, each holding the registers native code
/// held for it (`Vm::scratch_in_lane`); the innermost's record at `pc`. Each frame's call
/// guard proved its callee the inlined one, whose resolution is a function of it; a cache
/// that dropped its entry resolves it again.
pub(super) fn body_inlined_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    base: Ip,
    run: InlinedRun<'_>,
    frame: usize,
    pc: u32,
) -> (Registers, Ip, Carried) {
    let frames = run.frames;
    if run.framed {
        // Only the frames' own reach: the scratch window may be wider (another code's), past this storage.
        Vm::adopt_window_in_lane(r.0.cast_const(), usize::from(run.caller_registers), crate::compiled::window_width(frames, run.caller_registers));
    }
    let mut chain = vec![frame];
    while let Some(parent) = frames[*chain.last().expect("a chain has a frame")].parent {
        chain.push(parent);
    }
    let (mut registers, mut records) = (r, base);
    let mut view = std::ptr::null::<LaneView>();
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    for (depth, &index) in chain.iter().enumerate().rev() {
        let call = frames[index];
        let vm = unsafe { vm(cx) };
        // Cache eviction is not invalidation of an immutable callee binding: a lifecycle
        // edge retires the body first (`ProgramStore::retire_dead_native_calls`). Resolve
        // only when recovery needs the activation, rather than scanning at entry.
        // A frame bound by its function (closed world) names no closure: its callee register holds it,
        // live through the frame (`lane_region`'s liveness).
        let parent = call.parent_base(frames);
        let callee = call.target.identity().unwrap_or_else(|| registers.get(call.callee_register - parent));
        let target = vm.script_call_entry(callee)
            // SAFETY: copy the current cache row before any cache mutation.
            .map(|target| unsafe { *target })
            .or_else(|| vm.resolve_call_target_in_lane(code, callee, true))
            .unwrap_or_else(|| panic!("an inlined callee resolves: {:#x} (frame {index} of {}, callee register {}, parent {parent}, function {}, call {})",
                callee.to_bits(), frames.len(), call.callee_register, call.target.function, call.call));
        // SAFETY: the record is the call's in its parent's view.
        let (target, record) = (&target, unsafe { records.add(call.call as usize) });
        let this = call_this_in_lane(vm, target, registers.get(call.receiver - parent)).expect("a direct-object receiver binds itself");
        // The frame's own window and count: a call through `call` passes past its receiver
        // (`lane_unit::InlinedCall::function_call`).
        // SAFETY: the call's window of `registers` holds its values.
        let passed = unsafe { registers.0.add(usize::from(call.window - parent)).cast_const() };
        let (callee, _) = push_passed_in_lane::<false, false>(cx, registers, record, target, passed, call.arguments, this, false);
        // The frame's registers live where it stands, the leave's PC or its inner frame's
        // call; the exit wrote only those (`lane_region`), the rest keep the activation's.
        let at = match depth.checked_sub(1) { Some(inner) => frames[chain[inner]].call, None => pc };
        let live = code.register_root_masks(call.target.function);
        let scratch = usize::from(call.base - run.caller_registers);
        Vm::scratch_in_lane(scratch, usize::from(call.registers), |register, value| {
            if live.is_none_or(|live| live.is_live(at as usize, register)) {
                callee.set(register, value);
            }
        });
        // The activation's records start at the context's base, past any tier prologue its
        // entry record is.
        (registers, records, view) = (callee, unsafe { (*cx).base }, target.view);
    }
    // SAFETY: the innermost activation's view holds a record for each of its function's PCs.
    let resume = unsafe { records.add(pc as usize) };
    (registers, resume, carried_at_in_lane(cx, view, registers, pc))
}

/// What an activation carries into the record at `pc` of its function's `view`: the script
/// state, and the accumulator where the record reads one.
pub(super) fn carried_at_in_lane(cx: *mut LaneContext, view: *const LaneView, registers: Registers, pc: u32) -> Carried {
    let mut carried = Carried::script(cx);
    // SAFETY: a resolved target's view outlives the lane run.
    if let Some(&register) = unsafe { &*view }.accumulator_registers.get(pc as usize)
        && register != NO_ACCUMULATOR
    {
        carried.acc = Slot::of(registers.get(register));
    }
    carried
}

/// A lane body's call at `call_pc` of the running activation (`lane_region`'s `Call`): the
/// activation `script_call_prove_miss`'s common form makes, then the callee's own lane body. A
/// callee whose body returns completes the call (`call_return::step_activation_return_of`), the result in the
/// call's register; one that leaves, or has no body, leaves the lane at its record in that
/// activation (`LaneContext::callee_resume`). A call the common form does not make misses,
/// before any of its effects.
///
/// # Safety
/// `context` is the running lane's context, whose activation's registers hold the caller's
/// state; nothing the call runs allocates or collects.
/// A lane body's read of a capture `packed` names (depth in its high half, slot in its low),
/// by the lane's own `LoadCapture` lookup (`read_capture_value`): its value, or
/// `Value::DELETED` for none or an uninitialized binding, which leaves the body at the read.
/// `ArgumentsLength` of a lane body's entry function (`lane_arguments_length`): the count a
/// keeping activation kept, else the count an unmaterialized binding answers. `DELETED` where
/// the lane's own handler must read the materialized object, which the body's leave reaches.
pub(crate) unsafe extern "C" fn body_arguments_length_in_lane(_vm: u64, _residual: u64, context: u64, slot: u64) -> u64 {
    if context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext;
    let value = match unsafe { keeping_activation_in_lane(cx) } {
        Some((argc, _)) => Value::integer(i32::from(argc)),
        None => match unsafe { unmaterialized_arguments_in_lane(cx, slot as usize) } {
            Some((_, count)) => Value::number(count as f64),
            None => Value::DELETED,
        },
    };
    value.to_bits()
}

/// `ArgumentGet` of that binding at an integer index a keeping activation kept or an
/// unmaterialized binding holds; any other key leaves at the record.
pub(crate) unsafe extern "C" fn body_argument_get_in_lane(_vm: u64, _residual: u64, context: u64, slot: u64, index: Value) -> u64 {
    if context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext;
    if let Some((argc, function)) = unsafe { keeping_activation_in_lane(cx) }
        && let Some(index) = index.as_int().and_then(|index| u16::try_from(index).ok()).filter(|index| *index < argc)
    {
        // SAFETY: the context's program outlives the lane run, and the keeping activation
        // holds its `argc` values after the function's registers (`staged::call_keeping::keep_passed`).
        let registers = unsafe { &*(*cx).code }.function(function as usize).registers;
        return unsafe { (*cx).registers }.get(registers + index).to_bits();
    }
    match unsafe { unmaterialized_arguments_in_lane(cx, slot as usize) } {
        Some((values, count)) => index
            .as_int()
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < count)
            // SAFETY: `values` holds `count` kept values, and the index is below it.
            .map_or(Value::DELETED, |index| unsafe { *values.add(index) })
            .to_bits(),
        None => Value::DELETED.to_bits(),
    }
}

pub(crate) unsafe extern "C" fn body_capture_in_lane(_vm: u64, _residual: u64, context: u64, packed: u64) -> u64 {
    if context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext;
    let (depth, slot) = ((packed >> u16::BITS) as u16, packed as u16);
    let value = match native_frames::innermost_callee_env(cx) {
        Some(env) => {
            let vm = unsafe { vm(cx) };
            // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
            vm.walked_capture_slots(env, false, depth).and_then(|(slots, root)| vm.plain_capture(unsafe { &*slots }, root, slot))
        }
        None => read_capture_value::<true>(cx, depth, slot),
    };
    value
        .filter(|value| value.to_bits() & quench_runtime::value::ValueLayout::TAG_MASK != quench_runtime::value::ValueLayout::INTERNAL_TAG)
        .unwrap_or(Value::DELETED)
        .to_bits()
}

/// A lane body's store of `value` to the capture `packed` names (depth in its high half,
/// slot in its low), by the lane's own stores (`store_own_capture`, else
/// `store_to_capture`): 1 where it stored, else 0, which leaves the body at the store,
/// before it.
///
/// # Safety
/// As `body_capture_in_lane`.
pub(crate) unsafe extern "C" fn body_capture_store_in_lane(_vm: u64, _residual: u64, context: u64, packed: u64, value: Value) -> u64 {
    if context == 0 { return 0; }
    let cx = context as usize as *mut LaneContext;
    let (depth, slot, value) = ((packed >> u16::BITS) as u16, packed as u16, value);
    let stored = match native_frames::innermost_callee_env(cx) {
        Some(env) => {
            // SAFETY: the context's program outlives the lane run.
            let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
            store_to_environment(unsafe { vm(cx) }, code, program, env, depth, slot, value)
        }
        None => (depth == 0 && store_own_capture(cx, slot, value)) || store_to_capture(cx, depth, slot, value),
    };
    u64::from(stored)
}

/// Whether a body's run that ended at `exit` counts toward its retirement and
/// reoptimization: a leave into a callee that only lacked a body while warming toward one
/// (`LaneContext::callee_warming`) does not, as the next run may find the callee's body. Only
/// a `Callee` leave reads the flag: the callee's own code may run nested bodies before the
/// leave resolves, whose other ends must not take it.
pub(super) fn leave_counts_in_lane(cx: *mut LaneContext, exit: crate::compiled::LaneExit) -> bool {
    // SAFETY: the running lane's context, read and reset by the run that left into the callee.
    exit != crate::compiled::LaneExit::Callee || !unsafe { std::mem::take(&mut (*cx).callee_warming) }
}

/// With diagnostics, counts a leave of `own`'s body into a warming callee, which
/// `leave_counts_in_lane` keeps from its feedback, in the view's report.
pub(super) fn note_warming_leave_in_lane(cx: *mut LaneContext, own: &LaneView) {
    if crate::compiled::admission::diagnosing(unsafe { vm(cx) }) {
        own.tier().body_stats.warming_leaves.set(own.tier().body_stats.warming_leaves.get() + 1);
    }
}

/// Counts a run of `own`'s body that ended at `exit` on its typed edge (`ExitTable::cross`), and in the diagnostics leave table.
#[inline(always)]
pub(super) fn record_end_in_lane(cx: *mut LaneContext, own: &LaneView, method: &crate::compiled::LaneMethod, exit: crate::compiled::LaneExit) {
    crate::compiled::fallback_sites::body_ended(cx, method, exit);
    method.exits.cross(exit);
    if crate::compiled::admission::diagnosing(unsafe { vm(cx) }) {
        own.tier().body_stats.ended(exit, &method.exits, &method.inlined, crate::compiled::take_lane_exit_line());
    }
}

/// `LaneView::note_lane_body_end`, counting the retirement it makes in the VM's tier events.
#[inline(always)]
pub(super) fn note_body_end_in_lane(cx: *mut LaneContext, own: &LaneView, exit: crate::compiled::LaneExit, left_new: impl FnOnce((Option<u32>, usize))) {
    if own.note_lane_body_end(exit, false, left_new) {
        unsafe { vm(cx) }.programs.tier_events().record(Event::Retired);
    }
}

/// Counts an entry whose body the lane could not leave inside of (`body_admits_in_lane`): in the VM's
/// tier events and, with diagnostics, the view's report.
#[cold]
pub(super) fn body_refused_in_lane(vm: &Vm, own: &LaneView) {
    vm.programs.tier_events().record(Event::Refused);
    if crate::compiled::admission::diagnosing(vm) {
        own.tier().body_stats.refused.set(own.tier().body_stats.refused.get() + 1);
    }
}

pub(super) fn body_left_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    own: &LaneView,
    method: &crate::compiled::LaneMethod,
    exit: crate::compiled::LaneExit,
) -> (LaneBodyLeft, crate::compiled::LaneExit) {
    record_end_in_lane(cx, own, method, exit);
    if exit == crate::compiled::LaneExit::Terminated {
        // A fault can bypass the native exit's register snapshots. Discard its
        // native frames instead of materializing stale homes for guest execution.
        native_frames::discard_frames(cx);
        return (LaneBodyLeft::Terminated, exit);
    }
    crate::compiled::admission::retire_after_native(unsafe { vm(cx) }, || !matches!(exit, crate::compiled::LaneExit::Returned { .. }) || native_frames::left_native_frames(cx));
    let feedback = method.feedback(exit, unsafe { &*(*cx).code });
    (frames_resumed_in_lane(cx, r, (own, Some(method)), exit), feedback)
}

/// The activation a leave resumes in: the one the leaving body ran in for a native call's callee, else
/// the innermost frame a native-frame advance made the running one, where there is one.
pub(super) fn resumed_activation_in_lane<'a>(
    cx: *mut LaneContext,
    r: Registers,
    (own, root): (&'a LaneView, Option<&'a crate::compiled::LaneMethod>),
    callee: bool,
) -> (Registers, &'a LaneView, Option<&'a crate::compiled::LaneMethod>) {
    if callee {
        return (r, own, root);
    }
    match native_frames::advance_into_frames(cx) {
        // SAFETY: the advance made the innermost frame the running activation.
        Some((own, method)) => (unsafe { (*cx).registers }, own, Some(method)),
        None => (r, own, root),
    }
}

/// Where the lane continues after native code of the activation with registers `r`, a body of
/// `root`'s or a region's, left with `exit`: in the innermost of the native frames it left inside, which
/// the lane links and continues in first (`native_frames::advance_into_frames`; a leave into a callee's
/// activation already linked them and went on in it), then at the record `exit` names there, or the
/// innermost's completion. `own` is the activation's view; `root` the body that ran, where one did,
/// whose inlined frames an `exit` that was not inside a native frame names.
pub(super) fn frames_resumed_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    (own, root): (&LaneView, Option<&crate::compiled::LaneMethod>),
    exit: crate::compiled::LaneExit,
) -> LaneBodyLeft {
    let callee = matches!(exit, crate::compiled::LaneExit::Callee);
    let (r, own, method) = resumed_activation_in_lane(cx, r, (own, root), callee);
    // SAFETY: the running activation's records start at the context's base.
    let base = unsafe { (*cx).base };
    own.resumed_leave(cx, (root, method), exit, match exit.resumed() {
        // SAFETY: the record that raised is one of the activation's function.
        crate::compiled::LaneExit::Threw { pc } => LaneBodyLeft::Raised(unsafe { base.add(pc) }),
        crate::compiled::LaneExit::Terminated | crate::compiled::LaneExit::Entry | crate::compiled::LaneExit::Stub => unreachable!("termination leaves before recovery; an entry leave or an unentered call resumes at its first record"),
        crate::compiled::LaneExit::At(pc) => {
            body_resume_at_in_lane(cx, r, own, loop_body::header_for_tier_up(cx, own, pc).unwrap_or(pc))
        }
        crate::compiled::LaneExit::Inlined { frame, pc } => {
            let method = method.expect("a leave inside an inlined callee is a lane body's");
            let (registers, resume, carried) = body_inlined_activation_in_lane(cx, r, base, InlinedRun::of_body(method), frame, pc);
            LaneBodyLeft::Resume(registers, resume, carried)
        }
        // A native call made its callee's activation and left the lane there.
        crate::compiled::LaneExit::Callee => {
            let mut carried = Carried::script(cx);
            // SAFETY: `body_call_helper` set both before the body left.
            let (registers, resume) = unsafe { ((*cx).registers, (*cx).callee_resume) };
            carried.acc = Slot::of(unsafe { (*cx).callee_acc });
            LaneBodyLeft::Resume(registers, resume, carried)
        }
        // SAFETY: the record that completes the activation is one of its function's.
        crate::compiled::LaneExit::Returned { pc, value } => LaneBodyLeft::Completed(unsafe { base.add(pc) }, value),
    })
}

/// Reloads the heap bases the running body's rows read through the payload (`Vm::refresh_body_bases`), as
/// every call, construct and step helper of the body ends: the callee's run or an allocation may have
/// moved them.
///
/// # Safety
/// `context` is the running lane's.
pub(crate) unsafe fn refresh_body_bases(context: u64) {
    let cx = context as usize as *mut LaneContext;
    // SAFETY: the caller's contract.
    unsafe { vm(cx).refresh_body_bases() };
}

/// Names `payload`, the dispatch of the native loop region that runs on the activation of the lane
/// `context`, to the helpers its rows call (`region_call_in_lane`), and returns the one it replaces:
/// a region's calls run lane code that may enter regions of its own.
///
/// # Safety
/// `context` is the running lane's.
pub(crate) unsafe fn region_payload_in_lane(context: usize, payload: usize) -> usize {
    // SAFETY: the caller's contract.
    std::mem::replace(unsafe { &mut (*(context as *mut LaneContext)).region_payload }, payload)
}

/// The helper a loop region's call row runs (`LaneHelper::RegionCall`): the call at `call_pc` of the
/// activation the lane context names, whose registers the region runs on, as `body_call_helper`
/// makes it, then the heap bases the region's payload holds reloaded (`LaneContext::region_payload`)
/// besides the body frame's, which the callee's run or an allocation may have moved.
///
/// # Safety
/// `context` is the running lane's, which the region entered (`region_payload_in_lane`), and `call_pc` a call record of its function.
pub(crate) unsafe extern "C" fn region_call_in_lane(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract.
    let (status, in_place) = unsafe { body_calls::body_call_status(context, call_pc) };
    let cx = context as usize as *mut LaneContext;
    let payload = unsafe { (*cx).region_payload };
    if in_place {
        // An in-place native pushed nothing: the stacks stand, only the heap bases may have moved.
        // SAFETY: a region's payload lives for its run.
        if let Some(payload) = unsafe { (payload as *mut quench_runtime::vm::engine_types::JsStepDispatch).as_mut() } { payload.refresh_object_bases(unsafe { super::vm(cx) }); }
    } else {
        // SAFETY: as above.
        unsafe { refresh_region_bases_of(context) };
    }
    status
}

/// Reloads the heap bases of the native loop region the lane context `context` names (`region_payload_in_lane`), where
/// one runs: the one refresh every lane helper a region's rows call ends with (`region_call_in_lane`, `body_general_step`,
/// `vm_operation_step`), so a row after any of them reads current bases (`Vm::refresh_region_bases`). Zero payload: no-op.
///
/// # Safety
/// `context` is the running lane's.
pub(crate) unsafe fn refresh_region_bases_of(context: u64) {
    let cx = context as usize as *mut LaneContext;
    // SAFETY: the caller's contract.
    let payload = unsafe { (*cx).region_payload };
    if payload != 0 {
        unsafe { super::vm(cx).refresh_region_bases(payload, direct_calls_in_lane(cx)) };
    }
}

/// Leaves the lane at `record`, where the running activation's body raised the exception in
/// `Vm::lane_raised`: the general loop unwinds from there, from the frames frameless
/// activations become first (`in_place_in_lane`).
pub(super) fn raised_exit_in_lane(cx: *mut LaneContext, record: Ip) -> Ip {
    let vm = unsafe { vm(cx) };
    let (function, throw_pc) = crate::compiled::raise_site_parts(vm.lane_raise_site);
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(error) = vm.lane_completion.raised_mut() { error.capture_source_location(code, function, throw_pc); }
    if let Some(settled) = raised_windows::settle_raised_window(cx, record) {
        return settled;
    }
    if vm.lane_stack.has_frames() {
        // SAFETY: `record` is one of the running function's records.
        let call_pc = unsafe { record_pc(cx, record) } as usize;
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
        vm.materialize_deferred_lane();
    }
    exit(record)
}

/// Where the lane continues after a body left at `pc` of `own`'s function on registers `r`: in
/// the loop's published BaseJIT code where it has a resume stub for `pc` (`loop_resume`),
/// else at the record, with the accumulator that record reads.
pub(super) fn body_resume_at_in_lane(cx: *mut LaneContext, r: Registers, own: &LaneView, pc: usize) -> LaneBodyLeft {
    let accumulator = own.accumulator_registers.get(pc).copied().filter(|&register| register != NO_ACCUMULATOR);
    if accumulator.is_none() && let Some(left) = loop_resume::resume_loop(cx, r, own, pc) {
        return left;
    }
    let mut carried = Carried::script(cx);
    if let Some(register) = accumulator {
        carried.acc = Slot::of(r.get(register));
    }
    // SAFETY: the body leaves at a PC of the function, whose records are contiguous.
    LaneBodyLeft::Resume(r, unsafe { (*cx).base.add(pc) }, carried)
}

/// Whether activating `target` leaves the running program. A target from
/// `LaneStack::script_targets` never does: `Vm::script_call_holds` admitted it for the VM's
/// active program, which is the lane's (`enter_program_in_lane` sets both), so the fast call
/// forms take that fact as proven (`MAY_CROSS` false) and only the general activation asks.
#[inline(always)]
pub(super) fn crosses_in_lane<const MAY_CROSS: bool>(cx: *mut LaneContext, target: &ScriptCallTarget) -> bool {
    MAY_CROSS && target.program() != unsafe { (*cx).program }
}

/// `activate_in_lane` past its checks: the call passes at most `target`'s parameters, the lane
/// stack has room (`activation_room_in_lane`), and the call may complete its caller. Pushes
/// the activation and returns its registers and the callee's first record; it makes no
/// call, so a handler activating through it keeps its registers. Without `GENERAL` its
/// callers (the fast call forms) have sent another program's target to the general form
/// (`crosses_in_lane`).
#[inline(always)]
pub(super) fn push_activation_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // SAFETY: `ip` is a record of the running view, whose window of `r` holds its values.
    let (passed, argc) = unsafe { (r.0.add(usize::from(arguments)).cast_const(), ImmediateLayout::argument_count((*ip).imm())) };
    push_passed_in_lane::<GENERAL, SETTLES>(cx, r, ip, target, passed, argc, this, constructs)
}

/// `push_activation_in_lane` of the `argc` values at `passed`, at most `target`'s parameters
/// and `FIXED_ARGUMENT_COPIES`, for the call record at `ip`; `SETTLES` for a settling
/// target's call (`Completion::SettlesPromise`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) fn push_passed_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // Only `script_call_settles_miss` activates a settling target, and it says so statically,
    // so the other call forms decide the completion without reading the target's entry.
    debug_assert_eq!(SETTLES, target.settles(), "a settling target activates through its own call form");
    let vm = unsafe { vm(cx) };
    let callee_registers = fill_window_in_lane(vm, target, passed, argc);
    enter_window_of_in_lane::<GENERAL, SETTLES, true>(cx, vm, r, ip, target, callee_registers, argc, this, constructs, None, None)
}

/// A plain call of `target`, of the running program, passed the `argc` values at `passed` (at
/// most its parameters) and not completing its caller, whose entry cell holds a published body
/// (`LaneView::cell_method`): the body runs on the activation's window before the activation
/// exists. The call is a leaf the context holds (`CONTEXT_LEAF`: the caller's receiver and
/// environment are the leaf's to restore), so a body that returns completes the call with the
/// caller's registers and the next record, nothing pushed and nothing to pop; a leave, and any
/// helper that observes activations (`materialize_running_leaf_in_lane`), make the activation the
/// push would have made, in the room the run reserved for it (`CALLEE_ACTIVATION_PENDING`),
/// and resume as the body's prologue would (`body_concluded_in_lane`). `None`, which changes
/// nothing, where the body does not run: the lane cannot make the activations a leave inside
/// its inlined calls would (`body_admits_in_lane`).
#[inline(never)]
pub(super) fn enter_cell_body_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the program store keeps a derived view alive with its program.
    let own = unsafe { &*target.view };
    let method = own.cell_method()?;
    let window = fill_window_in_lane(vm, target, passed, argc);
    if !body_admits_in_lane(cx, method, CALLEE_ACTIVATION_PENDING) {
        // SAFETY: the window starts at the stack's top, which the fill moved.
        unsafe { vm.lane_stack.values.set_top(window.0) };
        return None;
    }
    begin_cell_leaf_in_lane(cx, ip, r, target, argc, this, window);
    let run = crate::compiled::BodyRun {
        method, direct_calls: direct_calls_in_lane(cx), program: unsafe { (*cx).program }, residual: unsafe { &*(*cx).code },
        registers: window.0, receiver: entry_receiver_in_lane(cx, own.binds_this()), captures: capture_reads(target.capture), lane_context: cx.cast(),
        pending: CALLEE_ACTIVATION_PENDING, bases_stand: false };
    Some(cell_body_ended_in_lane(cx, window, own, target.function, method, vm.run_lane_body(&run)))
}

/// Makes the call of `target` the context's running leaf (`CONTEXT_LEAF`), which holds what its
/// activation would (`materialize_leaf_in_lane`): the callee's receiver, environment, captures,
/// registers and records are the context's, the caller's are the leaf's; the entry is counted.
#[inline(always)]
pub(super) fn begin_cell_leaf_in_lane(cx: *mut LaneContext, ip: Ip, r: Registers, target: &ScriptCallTarget, argc: u16, this: Value, window: Registers) {
    unsafe { vm(cx) }.programs.record_js_jit_lane_entry(target.program(), target.function);
    unsafe { vm(cx) }.programs.tier_events().record(Event::CellCrossed);
    // SAFETY: the running lane's context, and the callee's view outlives its program.
    // The staged leaf row (`call_leaf::enter_leaf`) writes the record, the receiver and the
    // environment, all the callee's in `CONTEXT_LEAF`.
    unsafe {
        lane_enter_leaf(cx, leaf_calls::FilledWindow, window, ip, r, target, CONTEXT_LEAF, argc, this, None, false);
    }
}

/// Where the lane continues after the body `enter_cell_body_in_lane` ran on `window` ended at
/// `exit`: the caller, past the call, where the body returned and the leaf still runs; else
/// the leave, in the activation made now. A helper that pushed it resolved targets since the
/// call, so the callee is `own` and `function`, not the call's target.
#[inline(always)]
pub(super) fn cell_body_ended_in_lane(
    cx: *mut LaneContext,
    window: Registers,
    own: &LaneView,
    function: u32,
    method: &crate::compiled::LaneMethod,
    exit: crate::compiled::LaneExit,
) -> Resumed {
    if let crate::compiled::LaneExit::Returned { value, .. } = exit
        && !unsafe { (*cx).leaf.call }.is_null()
    {
        // SAFETY: the window held the callee's registers; the completed call leaves the stack as it found it.
        unsafe {
            (*cx).callee_warming = false;
            vm(cx).lane_stack.values.set_top(window.0);
        }
        record_end_in_lane(cx, own, method, exit);
        note_body_end_in_lane(cx, own, exit, |_| {});
        let (registers, next) = complete_leaf_in_lane(cx, value, CONTEXT_LEAF).expect("the leaf runs until it completes or materializes");
        return Resumed::At(registers, next, Carried::script(cx));
    }
    materialize_running_leaf_in_lane(cx);
    body_concluded_in_lane(cx, window, own.start(), own, method, exit, function)
}

/// The window of `target`'s plain activation at the lane stack's top, holding the `argc`
/// values at `passed` as its parameters, which the stack's values cover from here on.
#[inline(always)]
pub(super) fn fill_window_in_lane(vm: &mut Vm, target: &ScriptCallTarget, passed: *const Value, argc: u16) -> Registers {
    let count = usize::from(target.registers);
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window's admitted room (`ScriptCallTarget::room`); the
    // registers live at entry are written before the length covers them, and no activation's
    // window moves. A register past `fill` is written before any instruction reads it, and
    // a collection reads it only where liveness proves it live (`ResidualProgram::entry_fill`).
    // Parameter `k` lives in register `k` of a plain activation (`Vm::place_plain_parameters`).
    unsafe {
        let registers = stack.values.top();
        fill_admitted_window(registers, usize::from(target.fill), count);
        copy_arguments(registers, passed, argc);
        stack.values.set_top(registers.add(count + lane_values::CONTEXT));
        Registers(registers)
    }
}

/// `push_passed_in_lane` past its window, which the stack's values cover from `window` on and
/// which holds the callee's registers: the activation, and the context made the callee's.
/// A native frame's materialization enters its window here too (`native_frames::advance_into_frames`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) fn enter_window_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    argc: u16,
    this: Value, constructs: bool, skipped: Option<SkippedForwarder>,
    kept: Option<u16>,
) -> (Registers, Ip) {
    enter_window_of_in_lane::<GENERAL, SETTLES, true>(cx, unsafe { vm(cx) }, r, ip, target, callee_registers, argc, this, constructs, skipped, kept)
}

/// `enter_window_in_lane` with the VM its caller already holds, so the push reads the lane
/// stack through it rather than through the context again after the window's writes.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) fn enter_window_of_in_lane<const GENERAL: bool, const SETTLES: bool, const COUNTED: bool>(
    cx: *mut LaneContext,
    vm: &mut Vm,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    argc: u16,
    this: Value, constructs: bool, skipped: Option<SkippedForwarder>,
    // The register of the first value a keeping activation kept past its registers
    // (`ScriptActivation::kept`), which marks it forwarding; `None` for any other.
    kept: Option<u16>,
) -> (Registers, Ip) {
    // The receiver goes to its context word first, so it holds no register past the push.
    note_lane_this(vm, this, || unsafe { (*target.view).this_rows });
    callee_registers.set_this(this);
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // Only a plain call activates another program's callee (`Vm::resolve_script_call_target`).
    let cross = crosses_in_lane::<GENERAL>(cx, target);
    // The keeping row's record words (`staged::call_keeping::KeepingRecord`).
    let record = quench_runtime::staged::call_keeping::KeepingRecord::of_kept(argc, kept);
    let activation = Activation::of_parts(
        target.function, ip, unsafe { (*cx).base }, r, unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc: record.argc,
            completion: if skipped.is_some() { Completion::Forwarder } else { Completion::of(constructs, cross, SETTLES) },
            forwards: record.forwards,
            kept: record.kept,
            environment: false,
            record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation, callee_registers.0) };
    if let Some(skipped) = skipped { stack.attach_payload(SidePayload::Forwarder(skipped)); }
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    callee_registers.set_env(target.env);
    callee_registers.set_capture(target.capture);
    if cross {
        enter_program_in_lane(cx, target.program());
    }
    // A proved plain call (`PLAIN_LINK`) skips this per-call count (`COUNTED`); under the tier's
    // diagnostics `enter_plain_window` counts it instead.
    if COUNTED { vm.programs.record_js_jit_lane_entry(target.program(), target.function); }
    // A construction runs its body from the first record; the tier enters plain calls.
    let start = if constructs { base } else { unsafe { (*target.view).start() } };
    (callee_registers, start)
}

/// Counts a proved call's entry, which only the tier's diagnostics make (`enter_plain_window`).
#[cold]
#[inline(never)]
pub(super) fn count_proved_entry_in_lane(vm: &mut Vm, target: &ScriptCallTarget) {
    let (program, function) = (target.program(), target.function);
    vm.programs.record_js_jit_lane_entry(program, function);
}

/// `enter_window_of_in_lane` of a proved plain call (`plain_call_activation` past `PLAIN_LINK`):
/// the record it would build is always plain (no general form, so no crossing; no
/// construction, settling, keeping or forwarding; an empty native prefix; a lane stack
/// window), so its header goes below the window straight from the call's words
/// (`protocol::push_plain_at`), with no `Activation` value and no `LaneStack::header_flags`
/// test. `depth` is the plain depth `LaneStack::plain_room` admitted it at.
/// The tier's entry count (`record_js_jit_lane_entry`) is made here only under diagnostics.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) fn enter_plain_window(
    cx: *mut LaneContext,
    vm: &mut Vm,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    depth: usize,
    argc: u16,
    this: Value,
    checked: bool,
) -> (Registers, Ip) {
    use quench_runtime::staged::protocol::{push_plain_at, PlainHeader};
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: the running lane's context.
    let (caller_base, program) = unsafe { ((*cx).base, (*cx).program) };
    debug_assert!(depth == stack.plain.depth.get());
    debug_assert!(stack.plain_program_holds(program));
    debug_assert!(callee_registers.0.addr() & HEAP_WINDOW == 0, "a lane stack window");
    // The record `enter_window_of_in_lane` builds here keeps `argc` (`KeepingRecord::of_kept`
    // without kept values) and completes `Completion::of(false, false, false)`, `Plain`.
    let header = PlainHeader::new(
        ip.expose_provenance() as u64, r.0.expose_provenance() as u64, caller_base.expose_provenance() as u64,
        target.function, argc, 0, target.callee.to_bits(),
    );
    push_plain_at(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()),
        stack.plain_frames_word() as u64, callee_registers.0.expose_provenance() as u64, header, u64::from(program.raw()), depth as u64,
    );
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    // A call row's hand-off checked `this` where it bound it (`checked`, `call_hand_over`).
    if !checked { note_lane_this(vm, this, || unsafe { (*target.view).this_rows }); }
    quench_runtime::staged::call_keeping::enter_context(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()), callee_registers.0.expose_provenance() as u64,
        this.to_bits(), target.env.to_bits(), target.capture.expose_provenance() as u64,
    );
    // A proved call skips the tier entry count (`enter_window_of_in_lane`'s `COUNTED`) unless the
    // tier's diagnostics count entries: then this cold branch does, so they observe the same path.
    if crate::compiled::admission::diagnosing(vm) { count_proved_entry_in_lane(vm, target); }
    (callee_registers, unsafe { (*target.view).start() })
}

/// The frameless target of another program's function `callee` a native calls from the
/// lane (`Vm::native_callback_in_lane`), as a trap's (`AccessorCall::trap_target`).
pub(super) fn cross_callback_target_in_lane(cx: *mut LaneContext, callee: Value) -> Option<(ScriptCallTarget, bool)> {
    AccessorCall::of(cx).trap_target(callee).filter(|&(_, crosses)| crosses)
}

/// The activation of a native's call of `target` with `args` from the lane's running
/// function (`Vm::native_callback_in_lane`), as the lane's own call would make it from the call
/// record `call`, completing to the native (`Completion::ReturnsToNative`): by its shape
/// (`PreparedShape`), or, for a gathering target, whose rest array it makes, or a call
/// without room, which grows the stack for the general path it takes, as
/// `activate_keeping_in_lane` makes it. Its first record, or `None` where the lane would not
/// activate the call.
pub(super) fn push_callback_in_lane(
    cx: *mut LaneContext,
    call: Ip,
    target: &ScriptCallTarget,
    args: &[Value],
    this: Value,
    crosses: bool,
) -> Option<Ip> {
    debug_assert_eq!(crosses, crosses_in_lane::<true>(cx, target), "a native's lane callee crosses where its program is another");
    if !crosses {
        // SAFETY: the running lane's context, which nothing else accesses.
        let shape = PreparedShape::of(unsafe { &*cx }, call, target, args.len());
        if let Some(shape) = &shape
            && let Some(start) = push_prepared_in_lane(cx, shape, &shape.entry(target, this), args)
        {
            return Some(start);
        }
    }
    let (r, arguments) = (unsafe { (*cx).registers }, ActivationArguments::Kept(args.as_ptr(), args.len()));
    let (_, start) = if crosses {
        activate_keeping_in_lane::<true>(cx, r, call, target, arguments, this, false, None).ok()?
    } else {
        activate_keeping_in_lane::<false>(cx, r, call, target, arguments, this, false, None).ok()?
    };
    let vm = unsafe { vm(cx) };
    // Given its completion where it is, a header's side record or the record (no spill).
    vm.lane_stack.recomplete_top(Completion::ReturnsToNative);
    Some(start)
}

/// `activate_in_lane` of an environment target (`CallEntry::Environment`): its window holds
/// its registers, then its environment, made now as the frame's promotion would make it
/// (each captured slot its parameter's argument or `undefined`, every other slot owned
/// elsewhere), then its frame locals (each parameter its argument, a captured slot owned
/// by the environment, every other local `undefined`), then the values passed. A parameter
/// promoted in place (`EnvironmentWindow::parameter_registers`) starts in its register and
/// leaves its local `undefined`, as a frame's promoted parameter does. The
/// environment is reachable only from the window until the lane leaves, which makes the
/// activation a frame holding it.
#[inline(always)]
pub(super) fn activate_environment_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    (this, constructs): (Value, bool),
) -> Result<(Registers, Ip), Ip> {
    use quench_runtime::staged::call_environment as environment_row;
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    // Another program's callee enters its program and completes across (`Completion::Cross`).
    let cross = target.program() != unsafe { (*cx).program };
    let Ok(argc) = u16::try_from(passed.len()) else {
        return Err(exit(ip));
    };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns && !returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    // SAFETY: the program store keeps a derived view alive with its program, and an
    // environment target's view has a window (`ScriptCallTarget::entry`).
    let layout = unsafe { (*target.view).environment_window.unwrap_unchecked() };
    let (rest, self_binding) = (layout.rest, layout.self_binding);
    // A body reading its `arguments` keeps every value passed past its parameters too, as
    // a frame does where legacy reflection (`f.arguments`) can read them; elsewhere nothing
    // observes them, and the window keeps only those its parameters bind. Only a call
    // passing more values than the parameters reads the body's plan.
    let extra = argc > target.params && !rest && {
        let Some(code) = vm.programs.residual(target.program()) else {
            return Err(exit(ip));
        };
        let plan = code.function(target.function as usize).call_plan(code);
        if !plan.reads_passed_arguments && plan.legacy_reflection {
            return Err(exit(ip));
        }
        !plan.reads_passed_arguments
    };
    if vm.with_scope_entered {
        return Err(exit(ip));
    }
    let (passed, argc) = if extra { (&passed[..usize::from(target.params)], target.params) } else { (passed, argc) };
    // A rest body keeps every value passed, the room sized for its parameters and the rest.
    let extra = usize::from(argc.saturating_sub(target.params));
    let needed = usize::from(target.room) + extra;
    // Both stacks' room, one row (`call_environment::admit_environment`).
    if !environment_row::concrete_admit_environment(
        vm.lane_stack.activation_count() as u64,
        vm.lane_stack.activation_limit.get() as u64,
        vm.lane_stack.values.room() as u64,
        needed as u64,
    ) {
        return Err(script_call_frame_in_lane(ip, needed));
    }
    // The rest array, made before the window is: the values past the other parameters,
    // which the rest parameter's local holds (`activate_keeping_in_lane` gathers likewise).
    // A plain branch: `bool::then` with this closure stayed an out-of-line call on every
    // activation.
    let gathered = if rest {
        let fixed = usize::from(target.params) - 1;
        Some(vm.rest_parameter_array(passed.get(fixed..).unwrap_or_default()))
    } else {
        None
    };
    // The environment window, one row (`call_environment::enter_environment`): every register
    // live at entry (`ScriptCallTarget::fill`) and every local starts `undefined` within the
    // target's reserved room (`ScriptCallTarget::room`), the entries are laid out (the
    // environment is made when one is needed), parameter `k` local `k` holds its argument where
    // one was passed and every value is kept after the locals for legacy reflection, then the
    // rest array, the self binding and the frame context (`this`, `env`, `capture`).
    const _: () = assert!(
        environment_row::ENTRY_ENVIRONMENT == WINDOW_ENVIRONMENT
            && environment_row::ENTRY_SLOTS == WINDOW_SLOTS
            && environment_row::ENTRY_CAPTURED == WINDOW_CAPTURED
            && environment_row::ENTRY_STORAGE == WINDOW_STORAGE
            && environment_row::STORAGE_WORDS as usize == quench_runtime::tier_surface::IN_PLACE_SLOTS_WORDS
            && environment_row::ENTRY_WORDS == WINDOW_LOCALS
    );
    let shape = environment_row::EnvironmentShape {
        registers: target.registers,
        live: target.entry_registers() as u16,
        locals: layout.locals,
        captured: layout.captured,
    };
    let held = shape.held();
    debug_assert!(fill_room::<SCRIPT_FILL_BLOCK>(held) <= usize::from(target.room));
    let rest = if let Some(gathered) = gathered { Some((target.params - 1, gathered.to_bits())) } else { None };
    let self_binding = if let Some(slot) = self_binding { Some((slot, target.callee.to_bits())) } else { None };
    note_lane_this(vm, this, || unsafe { (*target.view).this_rows });
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window (`ScriptCallTarget::room`); each slot is written
    // before the length covers it, and no activation's window moves.
    let callee_registers = unsafe {
        let registers = stack.values.top();
        environment_row::enter_environment(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()),
            registers.expose_provenance() as u64,
            shape,
            environment_row::EnvironmentPassed {
                arguments: passed.as_ptr().expose_provenance() as u64,
                count: argc,
                bound: argc.min(target.params),
                in_registers: argc.min(layout.parameter_registers),
                rest,
                self_binding,
            },
            environment_row::WindowContext {
                this: this.to_bits(),
                env: target.env.to_bits(),
                capture: target.capture.expose_provenance() as u64,
            },
        );
        stack.values.set_top(registers.add(held + usize::from(argc) + lane_values::CONTEXT));
        Registers(registers)
    };
    let activation = Activation::of_parts(
        target.function, ip, unsafe { (*cx).base }, r, unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc,
            completion: Completion::of(constructs, cross, false),
            forwards: false,
            kept: 0,
            environment: true,
            record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation, callee_registers.0) };
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    // The frame context was written with the window (`call_environment::bind_context`).
    if cross {
        enter_program_in_lane(cx, target.program());
    }
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    Ok((callee_registers, base))
}
