//! Call-site targets: the script targets a call record links and the units a loop inlines.

use super::*;

/// The script targets the call record at `pc` of `view` linked (`call_link`), newest first:
/// its latest and its history (`LaneStack::call_sites`), each where a plain call of it binds
/// an ordinary `this` and activates in place: what a lane body inlines (`lane_unit::unit`).
pub(super) fn lane_site_targets<H: Host>(vm: &Vm<H>, view: &LaneView, pc: u32) -> Vec<super::compiled::CallTarget> {
    // SAFETY: the view holds a record per PC; reading its link changes nothing.
    let Some(record) = view.records.get(pc as usize).map(|record| unsafe { &*record.get() }) else { return Vec::new() };
    let history = vm.lane_stack.call_sites.borrow();
    // The callees the site resolved or memoized, in the order first seen (`LaneStack::note_site_callee`).
    let recorded = history.get(&std::ptr::from_ref(&record.call_link).addr());
    // A collection since the site linked retires its entry; the callee's own entry of this
    // generation, where a call resolved it again, is the same target. A linked entry another
    // site's callee took since names no callee of this site.
    let latest = vm.lane_stack.script_targets.entries.get(usize::from(record.call_link.get())).and_then(|linked| {
        if linked.generation() == vm.lane_stack.generation {
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
            let latest = super::compiled::CallTarget {
                callee: super::compiled::CalleeBinding::Identity(latest.callee), function: latest.function };
            std::iter::once(latest)
                .chain(recorded.into_iter().flatten().rev().copied().filter(|target| *target != latest))
                .take(super::compiled::tier_policy::POLYMORPHISM)
                .collect()
        }
        // A site that runs only in its function's lane body, or that a read memo answers,
        // links nothing current: the callees it recorded, else the body's own inlined calls
        // (`lane_unit::Frame`), are its targets.
        None => match recorded {
            Some(recorded) => recorded.iter().rev().copied().collect(),
            None => body_call_targets(view, pc),
        },
    }
}

/// The targets of the calls the lane body of `view` inlines at `pc`, its outermost frames' only.
fn body_call_targets(view: &LaneView, pc: u32) -> Vec<super::compiled::CallTarget> {
    view.lane_body().map_or_else(Vec::new, |method| {
        method.inlined.iter()
            .filter(|frame| frame.parent.is_none() && frame.call == pc)
            .map(|frame| frame.target)
            .collect()
    })
}

/// `lane_site_targets` of any function of the running program, by its view.
pub(super) fn lane_unit_targets<H: Host>(cx: *mut LaneContext<H>, function: u32, pc: u32) -> Vec<super::compiled::CallTarget> {
    // SAFETY: the program's views outlive the lane run.
    let Some(view) = unsafe { &*(*cx).views }.get(function as usize).and_then(super::program_store::derived_lane_view) else {
        return Vec::new();
    };
    lane_site_targets(unsafe { vm(cx) }, view, pc)
}

/// The mean trips of the loop at `header` of any function of the running program per native
/// run of it (`LoopTier::mean_trip`): what its call sites are weighed by (`lane_unit::with_trips`).
pub(super) fn lane_unit_trips<H: Host>(cx: *mut LaneContext<H>, function: u32, header: u32) -> Option<u64> {
    // SAFETY: the program's views outlive the lane run.
    let view = unsafe { &*(*cx).views }.get(function as usize).and_then(super::program_store::derived_lane_view)?;
    for site in &view.loops {
        if site.header == header {
            return site.tier.borrow().mean_trip();
        }
    }
    None
}

/// The dense capture slots of `callee`'s environment (`Vm::lane_callee_captures`).
pub(super) fn lane_callee_captures<H: Host>(cx: *mut LaneContext<H>, callee: Value) -> (usize, usize) {
    unsafe { vm(cx) }.lane_callee_captures(callee)
}

impl<H: Host> Vm<H> {
    /// The dense capture slots of `callee`'s environment (`EnvironmentSlots::native_dense_view`),
    /// as values and count, or none: what an inlined frame of it reads its captures from.
    pub(in crate::vm) fn lane_callee_captures(&self, callee: Value) -> (usize, usize) {
        self.script_call_entry(callee)
            // SAFETY: as `Vm::script_call_entry`; a live environment's slots never move.
            .and_then(|target| unsafe { crate::heap::EnvironmentSlots::native_storage_view(capture_reads((*target).capture)) })
            .map_or((0, 0), |(values, len)| (values.addr(), len))
    }
}

/// Per PC of `body`, the callees its call site reached where the callee's own lane body
/// specialized and the lane would activate it in place, with an ordinary `this`, from a
/// plain call of at most its parameters: what a lane body calls by its compiled entry
/// (`compiled::DirectCallee`), the latest first.
pub(super) fn lane_direct_callees<H: Host>(vm: &Vm<H>, view: &LaneView, function: u32, body: &crate::bytecode::Function) -> Vec<super::compiled::DirectCandidates> {
    let tail_calls = vm.host.proper_tail_calls() && body.strict;
    body.code.iter().enumerate().map(|(pc, packed)| {
        let instruction = decoded_instruction(body, *packed);
        let direct = instruction.op().is_call() && !instruction.direct_eval() && !instruction.parameter_eval()
            && !(tail_calls && instruction.returns_from_frame());
        let mut callees = super::compiled::DirectCandidates::new();
        if instruction.op().class().eq(Class::Construct) {
            callees.extend(lane_construct_callee(vm, view, function, pc, ImmediateLayout::argument_count(instruction.imm())));
            return callees;
        }
        callees.extend(lane_body_tier::site_targets(vm, view, pc).filter(|_| direct)
            .filter_map(|target| lane_direct_callee(vm, view, function, pc, ImmediateLayout::argument_count(instruction.imm()), target)));
        callees
    }).collect()
}

/// Records the forwarded initializer the construction at `i` ran, which a body may call by
/// its entry (`lane_construct_callee`).
pub(super) fn note_construct_site<H: Host>(vm: &Vm<H>, i: &LaneInstruction, constructor: Value, forwarded: &ScriptCallTarget) {
    vm.lane_stack.note_site_callee(&i.call_link, super::compiled::CallTarget {
        callee: super::compiled::CalleeBinding::Forwarded { through: constructor, callee: forwarded.callee },
        function: forwarded.function,
    });
}

/// The `compiled::DirectCallee` of the construction at `pc` of `view` through a forwarding
/// constructor: the forwarded initializer the site last ran (`lane_construct_keeping_activation`)
/// as the callee, the constructor as the identity it guards and the activation it skips.
pub(super) fn lane_construct_callee<H: Host>(vm: &Vm<H>, view: &LaneView, function: u32, pc: usize, argc: u16) -> Option<super::compiled::DirectCallee> {
    for site in lane_site_targets(vm, view, pc as u32) {
        let super::compiled::CalleeBinding::Forwarded { through, callee } = site.callee else { continue };
        // SAFETY: as `Vm::script_call_entry`; both targets are copied before anything resolves one.
        let (constructor, initializer) = unsafe { (*vm.script_call_entry(through)?, *vm.script_call_entry(callee)?) };
        let method = *vm.lane_stack.forwarded_methods.get(constructor.forwarded_method as usize)?;
        if !constructor.forwards() || !constructor.constructs() || holding_construct_plan(vm, through).is_none_or(|plan| plan.forwarded.is_none()) {
            return None;
        }
        let mut direct = lane_direct_callee(vm, view, function, pc, argc, initializer)?;
        direct.callee = through;
        direct.forwarder = Some(SkippedForwarder { function: constructor.function, callee: through, env: constructor.env, method });
        return Some(direct);
    }
    None
}

/// The `compiled::DirectCallee` of `target`, the callee the call at `pc` of `view` passes
/// `argc` arguments, where the lane would activate it in place.
pub(in crate::vm) fn lane_direct_callee<H: Host>(vm: &Vm<H>, view: &LaneView, function: u32, pc: usize, argc: u16, target: ScriptCallTarget) -> Option<super::compiled::DirectCallee> {
    let sloppy = match target.this_binding {
        ThisBinding::Sloppy => Some(vm.realm.globals),
        ThisBinding::Strict => None,
        _ => return None,
    };
    (target.generation() == vm.lane_stack.generation && target.program() == vm.active_program
        && target.entry.activates() && !target.view.is_null() && argc <= target.params)
        .then_some(())?;
    // SAFETY: a resolved target's view outlives its program.
    let view_of = unsafe { &*target.view };
    let (method, body) = view_of.pinned_callee(target.function == function);
    // SAFETY: a live environment's slots never move (`ScriptCallTarget::capture`).
    let (capture_values, capture_len) = unsafe { crate::heap::EnvironmentSlots::native_storage_view(capture_reads(target.capture)) }
        .unwrap_or((std::ptr::null(), 0));
    Some(super::compiled::DirectCallee {
        target, method, body, call: view.records().wrapping_add(pc).addr(), cell: view_of.entry_cell(),
        callee: target.callee, registers: target.registers, sloppy, capture_values, capture_len, capture_stores: capture_stores(target.capture).is_some(), forwarder: None,
    })
}

/// Whether a lane body's run may call bodies by their compiled entries: the lane may make
/// their activations should it leave inside one (`lane_body_call`'s own conditions).
pub(super) fn lane_direct_calls<H: Host>(cx: *mut LaneContext<H>) -> bool {
    let vm = unsafe { vm(cx) };
    let script_calls = unsafe { (*cx).script_calls };
    script_calls && !vm.with_scope_entered && vm.construct_target.is_none()
}

/// Whether a run of `own`'s specialized body may start: the one thing per run it cannot
/// prove at specialization, that the lane can make the activation of a call it inlines
/// should it leave inside one (`lane_body_inlined_activation`). `pending` activations are
/// the running body's own, which its call has not pushed yet (`lane_enter_cell_body`).
#[inline(always)]
pub(super) fn lane_body_admits<H: Host>(cx: *mut LaneContext<H>, method: &super::compiled::LaneMethod, pending: usize) -> bool {
    lane_room_admits(cx, method.recovery_space(), pending)
}

/// Whether the lane has the room `room` names: the activations and values a leave inside an inlined
/// callee of native code about to run makes (`lane_body_inlined_activation`), past the `pending`
/// activations the running body's own call has not pushed.
#[inline(always)]
pub(super) fn lane_room_admits<H: Host>(cx: *mut LaneContext<H>, room: LaneRecoverySpace, pending: usize) -> bool {
    let vm = unsafe { vm(cx) };
    room.activations == 0
        || (vm.lane_stack.activation_limit.get().saturating_sub(vm.lane_stack.activations.len() + pending) >= room.activations
            && vm.lane_stack.values.room() >= room.values
            && unsafe { (*cx).script_calls }
            && !vm.with_scope_entered)
}

/// A compiled region's recovery demand may exceed the calls the lane has observed.
/// Materialize before growing either Vec: live lane windows borrow their storage.
/// The caller must exit immediately and resume from its original semantic record.
#[cold]
#[inline(never)]
pub(super) fn lane_body_prepare_recovery<H: Host>(cx: *mut LaneContext<H>, method: &super::compiled::LaneMethod) -> bool {
    lane_room_prepare(cx, method.recovery_space())
}

/// Grows the lane stacks to the room `room` names (`lane_room_admits`), materializing the lane's
/// activations first; whether it grew them. The caller must exit at once and resume from its
/// original semantic record.
#[cold]
#[inline(never)]
pub(super) fn lane_room_prepare<H: Host>(cx: *mut LaneContext<H>, room: LaneRecoverySpace) -> bool {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } || vm.with_scope_entered { return false; }
    let values = vm.lane_stack.values.room() < room.values;
    let activations = vm.lane_stack.activations.capacity() - vm.lane_stack.activations.len() < room.activations;
    if !values && !activations { return false; }
    vm.materialize_lane_activations(cx);
    if values { vm.lane_stack.reserve_values(room.values); }
    if activations { vm.lane_stack.activations.reserve(room.activations); }
    vm.lane_stack.refresh_activation_limit();
    true
}

/// The calls native code runs as their callees' rows: a lane body's (`LaneMethod`) or a loop region's
/// (`CallCode`), their frames, whether the frames' registers are in the activation's register file past
/// the caller's (a stitched body's or region's) and not in the scratch window the code fills, and the
/// registers of the function that makes them.
#[derive(Clone, Copy)]
pub(super) struct InlinedRun<'a> {
    frames: &'a [super::compiled::InlinedCall],
    framed: bool,
    caller_registers: u16,
}

impl<'a> InlinedRun<'a> {
    pub(super) fn of_body(method: &'a super::compiled::LaneMethod) -> Self {
        Self { frames: &method.inlined, framed: method.framed(), caller_registers: method.caller_registers() }
    }

    /// A BaseJIT region's: its frames' registers are in the register file.
    pub(super) fn of_region(calls: &'a super::compiled::CallCode) -> Self {
        Self { frames: &calls.inlined, framed: true, caller_registers: calls.caller_registers() }
    }
}
