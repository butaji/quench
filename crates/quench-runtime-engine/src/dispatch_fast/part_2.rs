//! Functions of `dispatch_fast` in a codegen unit of their own (B1: a staged edit recompiles only the units that inline it).
#![allow(unused_imports)]
use super::*;

/// Where the lane goes after a region of the loop of `function` left inside the callee of a call it
/// made by its entry, with `exit`: in the innermost of the native frames the call pushed
/// (`frames_resumed_in_lane`).
pub(super) fn region_left_in_lane(cx: *mut LaneContext, r: Registers, function: u32, exit: crate::compiled::LaneExit) -> LoopContinuation {
    // SAFETY: the program's views outlive the lane run, and the loop's function has the view its site was made from.
    let own = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view).expect("the loop's function has its view");
    if exit == crate::compiled::LaneExit::Terminated {
        // As a body's: a fault can bypass the native exit's register snapshots.
        native_frames::discard_frames(cx);
        return LoopContinuation::Safepoint;
    }
    LoopContinuation::Recovered(frames_resumed_in_lane(cx, r, (own, None), exit))
}

/// Where the lane goes after native code left inside inlined `frame` of `region`'s callee, at its PC
/// `pc`. An inlining region of the Cranelift tier makes the callee's activation from the entry's
/// scratch window (`inlined_activation_in_lane`); a stitched region's registers are in the activation's
/// register file past its own, which the lane recovers as a lane body's (`body_inlined_activation_in_lane`).
pub(super) fn inlined_continuation_in_lane(cx: *mut LaneContext, r: Registers, site: &LoopSite, (region, code): (&crate::compiled::LoopRegion, &crate::compiled::CompiledOsrRegion), (frame, pc): (usize, u32)) -> LoopContinuation {
    if region.inlining.is_some() {
        return LoopContinuation::Inlined { frame, pc };
    }
    // The exit is feedback as a top-level one is: the callee's instruction widens in the next plan, and is counted.
    let call = code.calls.inlined[frame];
    site.tier.borrow_mut().left_at(site.tiers, crate::compiled::base_region::inlined_exit_key(call.call, pc), false);
    // SAFETY: the context's program outlives the lane run.
    if let Some(body) = unsafe { &*(*cx).code }.function_get(call.target.function as usize) {
        crate::compiled::base_region::leave(call.target.function, pc, body.widened(body.code[pc as usize]).op());
    }
    // SAFETY: the context's records are the activation's, which the region ran on.
    let base = unsafe { (*cx).base };
    let (registers, resume, carried) = body_inlined_activation_in_lane(cx, r, base, InlinedRun::of_region(&code.calls), frame, pc);
    LoopContinuation::Recovered(LaneBodyLeft::Resume(registers, resume, carried))
}

/// The entry record of `site`'s native code from the running activation, and the element
/// views of its arrays (`loop_tier::loop_arrays_in_lane`), when its arrays and captures pass the
/// region's guards: each capture an initialized binding resolved without guest code by the
/// same lookup as LoadCapture (`capture_read_value`).
/// The entry record of `region` (`loop_entry_checked_in_lane`), without the reason of a decline.
pub(super) fn loop_entry_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    region: &crate::compiled::LoopRegion,
    entry: &mut crate::compiled::LoopEntry,
) -> Option<crate::compiled::ElementViews> {
    loop_entry_checked_in_lane(cx, r, region, entry).ok()
}

/// `loop_entry_in_lane`, where each check that fails names its `RegionDecline`.
#[inline(never)]
pub(super) fn loop_entry_checked_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    region: &crate::compiled::LoopRegion,
    entry: &mut crate::compiled::LoopEntry,
) -> Result<crate::compiled::ElementViews, RegionDecline> {
    if !region.captures.is_empty() {
        let slots = capture_reads(unsafe { (*cx).registers }.capture());
        for slot in &region.captures {
            // The activation's `this`, for reads of `this` the region stores (`THIS_CAPTURE`).
            if *slot == crate::compiled::THIS_CAPTURE {
                entry.push_capture(unsafe { held_this_in_lane(cx, r) });
                continue;
            }
            // An inlined callee's capture reads its own environment (`Inlining::callee_captures`).
            if slot & crate::compiled::CALLEE_CAPTURE != 0 {
                let inlining = region.inlining.as_deref().ok_or(RegionDecline::CalleeCapture)?;
                let &(frame, callee_slot) = inlining.callee_captures.get(usize::from(slot & !crate::compiled::CALLEE_CAPTURE)).ok_or(RegionDecline::CalleeCapture)?;
                let callee = inlined_callee_of_guard(cx, r, inlining.frames.get(frame).ok_or(RegionDecline::CalleeCapture)?).ok_or(RegionDecline::CalleeCapture)?;
                let vm = unsafe { vm(cx) };
                // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
                let env = unsafe { &*vm.script_call_entry(callee).ok_or(RegionDecline::CalleeCapture)? }.env;
                // A top-level binding the global object projects is its property, which the
                // region keeps only while it stores no field (as the caller's own captures).
                let value = vm.environment_capture(env, callee_slot).or_else(|| {
                    (!region.writes_fields())
                        .then(|| vm.environment_capture_env(env, false, 0)
                            .and_then(|env| vm.plain_projected_root_binding(env, callee_slot)))
                        .flatten()
                }).ok_or(RegionDecline::CalleeCapture)?;
                entry.push_capture(value);
                continue;
            }
            // SAFETY: the context's dense slots remain live until the next collection.
            // A cache miss follows the same non-effectful lookup as LoadCapture.
            let value = unsafe { quench_runtime::tier_surface::EnvironmentSlots::read_storage(slots, *slot) }
                .or_else(|| capture_read_value(cx, 0, *slot))
                .or_else(|| {
                    // A projected global var aliases its global object's property. Native
                    // field stores could change it after entry, invalidating this snapshot.
                    (!region.writes_fields())
                        .then(|| projected_capture(cx, 0, *slot))
                        .flatten()
                }).ok_or(RegionDecline::Capture)?;
            entry.push_capture(value);
        }
    }
    let vm = unsafe { vm(cx) };
    // No guest call or collection occurs in a native run, and an array literal it allocates
    // is no entry value. Field writes replace guarded own slots, and element stores change
    // no shape slot. Immutable fields keep their entry values; admitted same-receiver Number
    // writes update SSA field cells. Other possible aliases are rejected by region
    // admission. Any miss, accessor, private name or non-Number declines before native
    // effects. Nested receivers follow the earlier field's value.
    for (index, field) in region.fields.iter().enumerate() {
        // A field native code reads at its instruction (`LoopRegion::is_dynamic_field`) takes
        // its entry value only to seed its representation: a receiver it does not answer for
        // now (a null child, another shape), or a value its Number representation does not
        // hold, seeds the representation's own placeholder.
        let blind = region.seeds_without_receiver(index);
        let read = (!blind).then(|| region_field_in_lane(cx, r, field, |index| entry.field_value(region, index))).flatten();
        let read_some = read.is_some();
        let value = match read {
            Some(value) if field.tagged || !region.is_dynamic_field(index) || value.as_number().is_some() => value,
            _ => (region.is_dynamic_field(index)
                    && (blind || field.tagged || vm.field_cache_entry(field.cache_site).receiver != quench_runtime::vm::NO_FIELD_RECEIVER))
                    .then(|| if field.tagged { Value::UNDEFINED } else { Value::integer(0) })
                    .ok_or(if read_some { RegionDecline::FieldNumber } else { RegionDecline::Field })?,
        };
        entry.push_field(value, field.tagged).ok_or(RegionDecline::EntryFull)?;
    }
    let views = loop_tier::loop_arrays_in_lane(vm, r, region, entry).ok_or(RegionDecline::Array)?;
    loop_field_stores(cx, r, region, entry).ok_or(RegionDecline::FieldStore)?;
    if region.reads_heap_natively() {
        let object_base = vm.heap_reads().object_record_base().unwrap_or(std::ptr::null());
        let (properties_base, properties_len) = vm.heap_reads().native_property_values();
        let array_base = vm.heap_reads().array_record_base().unwrap_or(std::ptr::null());
        entry.push_native_heap(object_base, properties_base, properties_len, array_base);
    }
    if let Some(inlining) = &region.inlining {
        // Leaving at an inlined instruction makes its callee's activation as the call would:
        // each call must still be one the lane activates in place (`inlined_activation_in_lane`).
        for frame in &inlining.frames {
            inlined_target_in_lane(cx, r, inlined_receiver_in_lane(r, region, entry, frame), frame).ok_or(RegionDecline::Inlined)?;
        }
    }
    Ok(views)
}

/// A cached data field's value as the region's entry reads it: a hit of the field's cache
/// on its receiver, or a typed array's geometry, for no private name. `earlier` gives the value of the earlier field a
/// receiver may be (`FieldReceiver::Field`).
pub(super) fn region_field_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    field: &crate::compiled::FieldUse,
    earlier: impl FnOnce(usize) -> Value,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if vm.is_private_name(field.atom) { return None; }
    let receiver = match field.receiver {
        crate::compiled::FieldReceiver::This => unsafe { held_this_in_lane(cx, r) },
        crate::compiled::FieldReceiver::Register(register) => r.get(register),
        crate::compiled::FieldReceiver::Capture(slot) => capture_read_value(cx, 0, slot)?,
        crate::compiled::FieldReceiver::Field(index) => earlier(index),
    };
    // The site's primary entry first, as the lane's own reads take it (`own_formed_handler`).
    if let Some(value) = vm.record_field_entry_hit(vm.field_cache_entry(field.cache_site), receiver) {
        return Some(value);
    }
    match vm.cached_field_hit(code, receiver, field.cache_site) {
        quench_runtime::vm::object::FieldCacheProbe::Hit(value) => Some(value),
        // A site of several shapes answers from its polymorphic entries, own or of the
        // immediate prototype, as the lane's polymorphic read does (`POLYMORPHIC_SITE`).
        // A typed array's geometry is its intrinsic getter's answer, which only a call (a
        // detach, a resize, a redefinition) changes, and a call leaves the region.
        _ => vm.polymorphic_field_hit(code, receiver, field.cache_site)
            .or_else(|| vm.typed_array_geometry_read(receiver, field.atom))
            .or_else(|| (field.atom == vm.length_atom).then(|| vm.own_array_length(receiver).map(Value::length)
                // A string's length is its own, immutable: the region reads it at the instruction too.
                .or_else(|| vm.string_length_value(receiver))).flatten()),
    }
}

/// The call target of an inlined `frame`'s callee, when `script_call_prove_miss` would activate it
/// in place now, binding the frame's receiver as its `this`: the target the call caches, an
/// activation of the running program with room on the lane stack.
pub(super) fn inlined_target_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    receiver: Value,
    frame: &crate::compiled::InlineFrame,
) -> Option<*const ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    let target_pointer = vm.script_call_entry(inlined_callee_of_guard(cx, r, frame)?)?;
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
    let target = unsafe { &*target_pointer };
    (target.function == frame.function
        && target.entry.activates()
        && target.program() == unsafe { (*cx).program }
        && unsafe { (*cx).script_calls }
        && !vm.with_scope_entered
        && activation_room_in_lane(vm, target)
        && call_this_in_lane(vm, target, receiver).is_some_and(|bound| {
            bound == receiver || matches!(frame.guard, crate::compiled::InlineGuard::Capture(_) | crate::compiled::InlineGuard::Register(_))
        }))
    .then_some(target_pointer)
}

/// The callee an inlined `frame`'s call binds now: the closure its register or capture holds,
/// which the region binds by its function (a fresh closure of it on each entry is one case,
/// `osr_compile::unit_frames`), else the identity its field read guards.
pub(super) fn inlined_callee_of_guard(cx: *mut LaneContext, r: Registers, frame: &crate::compiled::InlineFrame) -> Option<Value> {
    match frame.guard {
        crate::compiled::InlineGuard::Capture(slot) => capture_read_value(cx, 0, slot),
        crate::compiled::InlineGuard::Register(register) => Some(r.get(register)),
        _ => Some(frame.callee),
    }
}

/// The receiver of an inlined `frame`'s call: the field value a register the region writes
/// holds (`LoopRegion::held_field`), else the register's.
pub(super) fn inlined_receiver_in_lane(
    r: Registers,
    region: &crate::compiled::LoopRegion,
    entry: &crate::compiled::LoopEntry,
    frame: &crate::compiled::InlineFrame,
) -> Value {
    match region.held_field(frame.receiver) {
        Some(index) => entry.field_value(region, index),
        None => r.get(frame.receiver),
    }
}

/// The region of `site`'s loop with its calls inlined, each callee the function the cached
/// field it is read from holds now, whose call the lane activates in place.
#[inline(never)]
pub(super) fn inlining_region_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    site: &LoopSite,
) -> Option<crate::compiled::LoopRegion> {
    // Only Cranelift compiles inlining regions, so a mode with no Cranelift tier does not analyze them.
    if !crate::compiled::admission::inlining_regions_route_to_cranelift() { return None; }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let program = unsafe { (*cx).program };
    let (function_index, function, decoded, constants) = loop_tier::loop_function_in_lane(cx, code)?;
    let live = code.register_root_masks(function_index)?;
    // The callee is the last of the fields its read reads through, each read from the
    // registers and the fields before it as they are now.
    let mut resolve = |chain: &[crate::compiled::FieldUse], source: crate::compiled::CalleeSource| -> Option<crate::compiled::InlineCallee<'_>> {
        let mut values = Vec::with_capacity(chain.len());
        for field in chain {
            let value = region_field_in_lane(cx, r, field, |index| values[index])?;
            values.push(value);
        }
        let callee = match source {
            crate::compiled::CalleeSource::Capture(slot) => capture_read_value(cx, 0, slot)?,
            crate::compiled::CalleeSource::Register(register) => r.get(register),
            crate::compiled::CalleeSource::Path => *values.last()?,
        };
        let vm = unsafe { vm(cx) };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
        let target = unsafe { &*vm.script_call_entry(callee)? };
        (target.entry.activates() && target.program() == program).then_some(())?;
        let body = code.function_get(target.function as usize)?;
        Some(crate::compiled::InlineCallee {
            callee,
            function: target.function,
            params: body.params,
            registers: body.registers,
            instructions: body.instructions().collect(),
            live: code.register_root_masks(target.function)?,
        })
    };
    crate::compiled::analyze_inlining_osr_region(
        function,
        &decoded,
        &constants,
        &code.field_sites,
        live,
        site.header,
        site.backedge,
        &mut resolve,
    )
}

/// The record native code leaving at the inlined instruction `callee_pc` of `frame`
/// continues at: the callee's activation the call at `call` makes (`push_activation_in_lane`),
/// a parameter bound to a caller register that register's value, each register live at
/// `callee_pc` the value native code left in the scratch window (as a lane body's leave,
/// `body_inlined_activation_in_lane`), and any other `undefined`, which nothing reads before
/// writing. The entry checked the call activates in place (`inlined_target_in_lane`).
#[inline(never)]
pub(super) fn inlined_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    region: &crate::compiled::LoopRegion,
    entry: &crate::compiled::LoopEntry,
    frame: crate::compiled::InlineFrame,
    call: Ip,
    callee_pc: u32,
) -> Ip {
    let receiver = inlined_receiver_in_lane(r, region, entry, &frame);
    let target = inlined_target_in_lane(cx, r, receiver, &frame).expect("the entry checked the inlined call");
    // SAFETY: as `Vm::script_call_entry`; `call` is the call's record in the running view.
    let (target, arguments) =
        unsafe { (&*target, ImmediateLayout::call_window_base((*call).imm())) };
    let receiver = call_this_in_lane(unsafe { vm(cx) }, target, receiver)
        .expect("entry checked the receiver's effect-free binding");
    let (registers, _) =
        push_activation_in_lane::<false, false>(cx, r, call, target, arguments, receiver, false);
    let inlining = region.inlining.as_ref().expect("an inlining region");
    // The admitted target is the function the region compiled for this callee (`frame.function`,
    // fixed at analysis): the guard named the same callee, which holds one function.
    debug_assert_eq!(target.function, frame.function, "the entry's target is the inlined function");
    // SAFETY: the context's program outlives the lane run.
    let live = unsafe { &*(*cx).code }.register_root_masks(target.function)
        .expect("an inlined callee has its register roots (`inlining_region_in_lane`)");
    for register in 0..frame.registers {
        let bound = frame.bound.get(usize::from(register)).copied().flatten().map(|source| r.get(source));
        // SAFETY: native code wrote back the registers live where it left.
        let held = || live.is_live(callee_pc as usize, register).then(|| unsafe { entry.scratch_value(inlining, frame.base + register) });
        registers.set(register, bound.or_else(held).unwrap_or(Value::UNDEFINED));
    }
    // SAFETY: the activation's view holds a record for each of its function's PCs.
    unsafe { (*cx).base.add(callee_pc as usize) }
}

/// The bytes of memory binding `binding` code addresses in place (`MemoryStorage::direct_view`).
pub(crate) fn direct_memory(vm: &Vm, binding: Value) -> Option<(*mut u8, usize)> {
    direct_memory_and_guard(vm, binding).map(|(bytes, _)| bytes)
}

/// `direct_memory` with the memory's guard reservation, read together.
pub(crate) fn direct_memory_and_guard(
    vm: &Vm,
    binding: Value,
) -> Option<((*mut u8, usize), Option<(*mut u8, usize)>)> {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap_reads().get(binding) else {
        return None;
    };
    bytes.direct_view()
}

pub(super) fn memory_view(vm: &Vm, binding: Value) -> MemoryView {
    direct_memory(vm, binding).map_or(MemoryView::NONE, |(bytes, len)| MemoryView::new(bytes, len))
}

/// The view of memory binding `binding`, reusing the run's last resolution.
#[inline(always)]
pub(super) fn resolve_memory(cx: *mut LaneContext, binding: Value) -> MemoryView {
    // SAFETY: handlers run with exclusive access to their context.
    let (resolved, view) = unsafe { (*cx).resolved };
    if resolved == binding {
        return view;
    }
    let view = memory_view(unsafe { vm(cx) }, binding);
    unsafe { (*cx).resolved = (binding, view) };
    view
}

/// The view behind `view`'s memory register in `registers`.
pub(crate) fn frame_memory_view(
    cx: *mut LaneContext,
    view: &LaneView,
    registers: Registers,
) -> MemoryView {
    // A lane run is all JavaScript or all Wasm (`LaneContext::script_calls`, `inline_calls`).
    // SAFETY: the context's program outlives the lane run.
    if unsafe { (*(*cx).code).kind } != quench_runtime::bytecode::ProgramKind::Wasm {
        return MemoryView::script(cx);
    }
    view.memory_register.map_or(MemoryView::NONE, |register| {
        resolve_memory(cx, registers.get(register))
    })
}

/// A taken safepoint jump whose fuel ran out (`script_fuel_out`): leave the lane when a
/// collection is due or termination requested, otherwise refill the fuel, credit the work
/// clock and dispatch the target from the lane loop,
/// which unwinds the stack, with the accumulator `acc` the jump carries
/// (`LaneContext::resumed_acc`): a loop head whose every edge carries a
/// register reads it there. Out of line, so a
/// safepoint's fast path neither loads the context nor saves registers.
#[cold]
#[inline(never)]
pub(super) fn safepoint_at(ip: Ip, target: Ip, acc: Slot<Value>) -> Ip {
    let cx = context_in_lane();
    // SAFETY: called only while a lane runs, from one of its handlers.
    if unsafe { vm(cx) }.collection_due() || unsafe { vm(cx) }.execution_interrupt.is_termination_requested() {
        // SAFETY: as above.
        unsafe { (*cx).fuel = 0 };
        return exit(ip);
    }
    // A crediting run counts every backward jump here; any other run takes a batch of
    // jumps before its next visit, and credits the work clock with the batch it spent.
    // SAFETY: as above.
    unsafe {
        if (*cx).backedges_left.is_some() {
            (*cx).fuel = 0;
        } else {
            (*cx).fuel = LANE_FUEL_BATCH;
            quench_runtime::clock::note_backedges(LANE_FUEL_BATCH);
        }
    }
    count_backedge(cx);
    // SAFETY: as above; the lane loop takes it before dispatching `target`.
    unsafe { (*cx).resumed_acc = acc };
    target
}

/// `safepoint_at` for a jump that writes `register` when taken.
#[cold]
#[inline(never)]
pub(super) fn safepoint_writing(
    r: Registers,
    ip: Ip,
    target: Ip,
    acc: Slot<Value>,
    register: u16,
    value: Value,
) -> Ip {
    let next = safepoint_at(ip, target, acc);
    if next == target {
        r.set(register, value);
    }
    next
}

/// The head record of a move pair followed by a jump (`(Move, Move, Jump)`, a `FUSED_CHAINS`
/// entry). The pair's two moves and the jump run as one record, whichever way the jump goes.
pub(super) fn move_pair_jump_handler() -> Handler {
    lane_frame::chain_row::<{ quench_runtime::staged::MOVE_ROW }, { quench_runtime::staged::MOVE_ROW }, { quench_runtime::staged::JUMP_ROW }, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::FUSED_PLAIN }>()
}

/// Continue in `frame` at `pc`: the record the frame's function resumes at.
#[inline(always)]
pub(super) fn enter_frame_in_lane(cx: *mut LaneContext, frame: usize, pc: usize) -> Resumed {
    let (r, ip, view) = frame_state_in_lane(cx, frame, pc);
    // A frame call or return reached from an out-of-line path may nest rather than run as a
    // sibling, so a run of calls and returns with no backward jump between them grew the
    // stack until it overflowed (mandreel). Below the floor the lane loop dispatches the
    // record instead, from the frame state the context now holds.
    if script_below_floor(cx) {
        return Resumed::Exit(ip);
    }
    Resumed::At(r, ip, view)
}

/// Make `frame` the lane's running function, at the record of `pc`: its registers, that
/// record and the state its records start from.
#[inline(always)]
pub(super) fn frame_state_in_lane(
    cx: *mut LaneContext,
    frame: usize,
    pc: usize,
) -> (Registers, Ip, Carried) {
    let vm = unsafe { vm(cx) };
    let function = vm.frames[frame].function;
    let program = unsafe { (*cx).program };
    // The context's table holds the frame's program's views: a derived one is read there.
    // SAFETY: the table lives as long as its program entry.
    let derived = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view);
    let view_in_lane = match derived {
        Some(view) => view,
        None => {
            if !vm.runs_lane {
                unreachable!("a host that runs no lane has no lane frame");
            }
            // The same derivation as every entry's (`ahead_runner::view_in_lane`).
            let image = vm.programs.residual(program).is_some_and(|code| vm.programs.ahead_images(program, code).is_some());
            let Some(view_in_lane) = vm.programs.view_in_lane(program, function, |code, constants, kind, live, sites| {
                crate::ahead_runner::view_in_lane(vm, function, code, constants, kind, live, sites, image)
            }) else {
                unreachable!("an inline Wasm frame runs a function of its program");
            };
            // SAFETY: views live as long as their program entry.
            unsafe { &*view_in_lane }
        }
    };
    let base = view_in_lane.records();
    let r = Registers(vm.frames[frame].registers.as_mut_ptr());
    let view = frame_memory_view(cx, view_in_lane, r);
    // SAFETY: the view has one record per instruction of the function, and
    // `pc` is the frame's validated resume point.
    let ip = unsafe { base.add(pc) };
    // The general path's register access names the active frame's register file.
    vm.active_registers = r.0;
    unsafe {
        (*cx).root = LaneRoot::Frame(frame);
        (*cx).registers = r;
        (*cx).base = base;
        (*cx).memory = view;
        let this = vm.frame_lane_this(frame);
        note_lane_this(vm, this, || view_in_lane.this_rows);
        r.set_this(this);
        r.set_env(Value::DELETED);
        (*cx).registers.set_capture(std::ptr::null());
        (*cx).outer = NO_OUTER_WALK;
        (*cx).root_walk = NO_OUTER_WALK;
    }
    let view = Carried {
        memory: view,
        integers: quench_runtime::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    (r, ip, view)
}

/// A function whose run-time address the lane plan prints (`LANEPLAN-ANCHOR`): less its
/// address in the symbol table, it is the executable's load bias, PIE or not.
#[inline(never)]
pub(super) fn plan_anchor_in_lane() {
    std::hint::black_box(());
}

/// Sets whether the lane run calls script functions (`LaneContext::script_calls`), which the
/// lane's admission mirrors (`LaneStack::set_script_calls`).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(super) unsafe fn set_script_calls_in_lane(cx: *mut LaneContext, allowed: bool) {
    unsafe {
        (*cx).script_calls = allowed;
        (*cx).returns_script_frame = allowed && !(*cx).inline_calls;
        vm(cx).lane_stack.set_script_calls(allowed);
    }
}

/// The activation of `target`, a plain entry the leaf prove miss (`script_call_leaf_prove_miss`) found
/// for the call record `i` at `ip` of argument count `ARGC`, as an unproven call makes it: an entry
/// cell's body runs first where it pays its entry (`enter_cell_body_in_lane`), else the window is
/// filled and the activation pushed (`enter_window_of_in_lane`). `None`, before any write, where the
/// call takes the full form: a receiver whose binding reads the heap, or values past the window.
#[inline(always)]
pub(super) fn activate_unproven_in_lane<const ARGC: u16>(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction,
    target: &ScriptCallTarget) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let this = direct_call_this_in_lane(vm, target, r.get(i.c))?;
    let argc = call_argument_count::<ARGC>(i);
    if argc > FIXED_ARGUMENT_COPIES || !admits_window_in_lane(vm, target, argc) {
        return None;
    }
    // A `with` scope refuses every cached target (`LaneStack::refuse_cached_calls`), and only an admitted one reaches here.
    debug_assert!(!vm.with_scope_entered);
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns && !returning_call_admitted(cx) {
        return Some(Resumed::Exit(exit(ip)));
    }
    // SAFETY: the record's window of `r` holds its values.
    let passed = unsafe { r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm()))).cast_const() };
    // A callee whose entry cell holds a body that pays its entry runs it before any activation exists.
    if !returns && unsafe { (*target.view).enters_cell_body() } {
        if let Some(resumed) = enter_cell_body_in_lane(cx, r, ip, target, passed, argc, this) {
            return Some(resumed);
        }
        // A declined body may have made the plain activations records (entering emitted code spills them):
        // the push writes over the depth it left, and a first plain activation names the records' count
        // to the ones above (`PlainFrames::bound`).
        if vm.lane_stack.plain.depth.get() == 0 { vm.lane_stack.refresh_plain_bound(vm.lane_stack.values.top().addr()); }
    }
    let callee_registers = fill_window_in_lane(vm, target, passed, argc);
    let (registers, base) = enter_window_of_in_lane::<false, false, true>(cx, vm, r, ip, target, callee_registers, argc, this, false, None, None);
    Some(Resumed::At(registers, base, Carried::script(cx)))
}

/// Refuses the direct calls of a lane body or region about to run in `registers` where the running leaf owns them: a
/// leaf's activation is pending (`script_call_leaf_prove_miss`, `enter_cell_body_in_lane`), and a direct row's push would put
/// the callee's frame (and, at a leaf's unmoved top, its window over the leaf's registers) below the activation the
/// leaf makes on a leave, which then resumes the wrong frame. Zeroing the plain depth limit makes every direct row
/// refuse to its helper row, which makes the leaf's activation first (`in_innermost_frame`). Called after the run's
/// stack words are bound, which set the limit.
pub(crate) fn refuse_direct_calls_in_leaf(lane_context: *mut (), registers: *mut Value) {
    let cx = lane_context.cast::<LaneContext>();
    // SAFETY: a lane body's context is the running lane's, which nothing else accesses.
    if cx.is_null() || unsafe { (*cx).leaf.call }.is_null() || unsafe { (*cx).registers.0 } != registers {
        return;
    }
    // The body runs in the leaf only where no activation owns `registers` yet: a body's call made from inside the
    // leaf pushed the callee's activation on its own window, which the context names while the leaf stays set.
    let stack = &unsafe { vm(cx) }.lane_stack;
    if stack.top_frame().is_some_and(|frame| frame.window == registers.addr()) {
        return;
    }
    stack.frames_changed();
}

/// Make the running leaf (`LaneContext::leaf`) the activation its call would have pushed
/// (`enter_window_in_lane`), before anything observes activations: their materialization
/// (`Vm::materialize_lane_exit`) at the lane's exit, a getter's
/// activation. Its registers, records and PC stay where they are, so a handler may make it
/// anywhere in its record.
#[cold]
#[inline(never)]
pub(super) fn materialize_leaf_in_lane(cx: *mut LaneContext) {
    // SAFETY: the running lane's context.
    let leaf = unsafe { std::mem::replace(&mut (*cx).leaf, NO_LEAF_CALL) };
    let vm = unsafe { vm(cx) };
    // SAFETY: the leaf's target stays in its cache while it runs, and its call record
    // belongs to the caller's live view.
    let (target, call) = unsafe { (&*leaf.target, &*leaf.call) };
    let form = leaf.form;
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: the leaf's window starts at the stack's length, within its capacity
    // (`script_call_leaf_prove_miss`).
    let window = unsafe { (*cx).registers.0 };
    unsafe { stack.values.set_top(window.add(usize::from(target.registers) + lane_values::CONTEXT)) };
    let activation = Activation::of_parts(
        target.function, leaf.call, leaf.caller_base, leaf.caller_registers, leaf.caller_program.unwrap_or(unsafe { (*cx).program }),
        ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc: leaf.argc,
            completion: if leaf.caller_program.is_some() { Completion::Cross } else { Completion::Plain },
            forwards: false,
            kept: 0,
            environment: false,
            record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the call proved room for the activation (`activation_room_in_lane`).
    unsafe { stack.push_activation(activation, window) };
    // The receiver the call proved it binds directly (`script_call_leaf_prove_miss`), from the
    // caller's registers, which the leaf does not write, for a form that binds none yet.
    let this = (!leaf_binds_this(form)).then(|| {
        direct_call_this_in_lane(vm, target, leaf.caller_registers.get(call.c))
            .expect("the leaf's call proved its receiver's binding")
            .to_bits()
    });
    if let Some(this) = this { note_lane_this(vm, Value::from_bits(this), || unsafe { (*target.view).this_rows }); }
    // The context words the form left to its caller, by the staged row on machine words.
    quench_runtime::staged::call_leaf::materialize_leaf_context(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()), window.expose_provenance() as u64, form, this,
        target.env.to_bits(), target.capture.expose_provenance() as u64,
    );
}

/// The argument count of the call record `i`: `ARGC`, unless that is `ANY_ARGC`.
#[inline(always)]
pub(super) fn call_argument_count<const ARGC: u16>(i: &LaneInstruction) -> u16 {
    if ARGC == ANY_ARGC { ImmediateLayout::argument_count(i.imm()) } else { ARGC }
}

/// The protocol row of a `Call` record that runs the miss body `MISS` (`protocol_misses`).
pub(super) const fn call_row<const MISS: u16>() -> Handler {
    protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, MISS>()
}

/// The protocol row of a `Call` record whose argument count is `argc` (`ANY_ARGC` past four), `PLAIN`
/// where its receiver is `undefined` and `RETURNS` where it returns its callee's completion
/// (`protocol_misses::script_call`).
pub(super) const fn script_call_row(argc: u16, plain: bool, returns: bool) -> Handler {
    use protocol_misses::script_call as code;
    macro_rules! by_argc {
        ($parameters:expr) => {
            match argc {
                0 => call_row::<{ protocol_family::SCRIPT_CALL | $parameters | code::ARGC_0 as u16 }>(),
                1 => call_row::<{ protocol_family::SCRIPT_CALL | $parameters | code::ARGC_1 as u16 }>(),
                2 => call_row::<{ protocol_family::SCRIPT_CALL | $parameters | code::ARGC_2 as u16 }>(),
                3 => call_row::<{ protocol_family::SCRIPT_CALL | $parameters | code::ARGC_3 as u16 }>(),
                4 => call_row::<{ protocol_family::SCRIPT_CALL | $parameters | code::ARGC_4 as u16 }>(),
                _ => call_row::<{ protocol_family::SCRIPT_CALL | $parameters | code::ARGC_ANY as u16 }>(),
            }
        };
    }
    match (plain, returns) {
        (false, false) => by_argc!(0),
        (true, false) => by_argc!(code::PLAIN as u16),
        (false, true) => by_argc!(code::RETURNS as u16),
        (true, true) => by_argc!((code::PLAIN | code::RETURNS) as u16),
    }
}

/// The general protocol row of a call record whose immediate is `imm`: that of its argument count, not
/// `PLAIN` nor `RETURNS`, which the record's requickened form takes (`specialize_script_calls`).
pub(super) fn script_call_handler(imm: u32) -> Handler {
    script_call_row(ImmediateLayout::argument_count(imm), false, false)
}

/// Gives each call record the protocol row of its argument count, of a receiver that is the function's
/// `undefined` register (`undefined`), and, where `admits_returns` (`concrete_admits_returning_call` for
/// the host and the function), of a returning record (`RETURN_REGISTER`), whose completion the row admits.
pub(super) fn specialize_script_calls(records: &mut [LaneInstruction], undefined: Option<quench_runtime::bytecode::Register>, admits_returns: bool) {
    let general = script_call_row(ANY_ARGC, false, false) as *const ();
    for record in records.iter_mut().filter(|record| record.handler == general) {
        let plain = undefined.is_some_and(|register| u16::from(register) == record.c);
        let returns = admits_returns && record.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        let argc = ImmediateLayout::argument_count(record.imm());
        const _: () = assert!(FIXED_ARGUMENT_COPIES == 4);
        record.handler = script_call_row(argc, plain, returns) as *const ();
    }
}

/// The `this` a plain call of `target` binds for `receiver`: an arrow's lexical one
/// (`ScriptCallTarget::lexical_this`), else `call_this_value`'s without its boxing of
/// primitives, which allocates, so `None` for a sloppy callee's primitive.
#[inline(always)]
pub(super) fn call_this_in_lane(vm: &Vm, target: &ScriptCallTarget, receiver: Value) -> Option<Value> {
    match direct_call_this_in_lane(vm, target, receiver) {
        Some(this) => Some(this),
        None => vm.is_object_like(receiver).then_some(receiver),
    }
}

/// `call_this_in_lane` where the receiver's own bits decide it: a strict callee, an object in
/// the direct record table, or `null`/`undefined`. `None` for any other receiver, whose
/// object test reads the heap.
#[inline(always)]
pub(super) fn direct_call_this_in_lane(
    vm: &Vm,
    target: &ScriptCallTarget,
    receiver: Value,
) -> Option<Value> {
    match target.this_binding {
        ThisBinding::Lexical => Some(target.lexical_this),
        binding => receiver_this_in_lane(vm, binding, receiver),
    }
}

/// `direct_call_this_in_lane` of a binding other than an arrow's own `this`, which the
/// receiver alone decides; `None` for `ThisBinding::Lexical`.
#[inline(always)]
pub(super) fn receiver_this_in_lane(vm: &Vm, binding: ThisBinding, receiver: Value) -> Option<Value> {
    // Compares, not a table: a sloppy method's object receiver, the common call, takes the
    // first two.
    if binding != ThisBinding::Sloppy {
        // An arrow whose environment binds no `this` takes its caller's receiver.
        return (binding != ThisBinding::Lexical).then_some(receiver);
    }
    // The staged binding (`protocol::bind_this`): an object receiver binds itself, `undefined`
    // and `null` bind the global object, and any other receiver is the host's to box.
    quench_runtime::staged::protocol::concrete_bind_this(receiver.to_bits(), vm.realm.globals.to_bits()).map(Value::from_bits)
}

/// Whether `target`, a settling target of the running program (`CallEntry::Settles`), may
/// suspend: its first `await` leaves its caller the promise, which only a caller that keeps
/// the call's result can take (`Vm::try_settling_call`).
#[inline(never)]
pub(super) fn settling_body_suspends(cx: *mut LaneContext, target: &ScriptCallTarget) -> bool {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    code.function_get(target.function as usize).is_none_or(|body| body.suspends())
}

/// The realm's atoms an iteration step's `done` and `value` reads name
/// (`call_iteration::reads_step`).
#[inline(always)]
pub(super) fn step_atoms_of(vm: &Vm) -> call_iteration::StepAtoms {
    call_iteration::StepAtoms { done: u64::from(vm.runtime_atoms.done), value: u64::from(vm.runtime_atoms.value) }
}

/// `iterator_step_miss`'s step and result, out of line: the first result allocates.
#[inline(never)]
pub(super) fn array_iteration_result_in_lane(vm: &mut Vm, iterator: Value) -> Option<Value> {
    let step = match vm.step_present_array_value(iterator) {
        Some(step) => step,
        None => Some(vm.step_collection_value(iterator)?),
    };
    let result = vm.reused_iterator_result(step.unwrap_or(Value::UNDEFINED), step.is_none());
    note_allocation(vm);
    Some(result)
}

/// The `script_call_cross_plain_miss` instantiation of a window of `argc` values, at most
/// `FIXED_ARGUMENT_COPIES`.
pub(super) fn cross_plain_handler(argc: u16) -> Handler {
    const _: () = assert!(FIXED_ARGUMENT_COPIES == 4);
    match argc {
        0 => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_0 as u16 }>(),
        1 => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_1 as u16 }>(),
        2 => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_2 as u16 }>(),
        3 => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_3 as u16 }>(),
        _ => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_4 as u16 }>(),
    }
}

/// The resolution of the by-name load of `atom` in the function `cx` runs. A native an internal
/// name stands for is recorded for its reads (`Vm::intrinsic_natives`). An internal name is no
/// binding source text can declare, except the internal globals the realm installs on its global
/// object, which a static lookup reads as any other.
pub(super) fn name_resolution(cx: *mut LaneContext, atom: u32) -> NameResolution {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    let internal = vm.atom_class(atom).contains(AtomClass::INTERNAL);
    if internal && let Some(native) = quench_runtime::bytecode::intrinsic_name_native(&code.atoms[atom as usize]) {
        let slot = atom as usize;
        if vm.intrinsic_natives.len() <= slot {
            vm.intrinsic_natives.resize(slot + 1, None);
        }
        vm.intrinsic_natives[slot] = Some(native);
        return NameResolution::Native;
    }
    // The running function decides the name's scope: a running leaf's is its materialized
    // activation's.
    materialize_running_leaf_in_lane(cx);
    let function = running_function_in_lane(cx);
    let global = code.function_get(function as usize).is_some_and(|function| function.names_resolve_statically(code))
        && vm.programs.names_are_global(unsafe { (*cx).program })
        && (!internal || quench_runtime::bytecode::INTERNAL_GLOBAL_BINDINGS.contains(&vm.atom_name(atom)));
    match (global, vm.atom_name(atom)) {
        (true, UNDEFINED_GLOBAL) => NameResolution::Undefined,
        (true, _) => NameResolution::Global,
        (false, name) if quench_runtime::bytecode::is_intrinsic_name(name) => NameResolution::Intrinsic,
        (false, _) => NameResolution::General,
    }
}

/// The function the lane runs: its innermost activation's, or else its frame's.
#[inline(always)]
pub(crate) fn running_function_in_lane(cx: *mut LaneContext) -> u32 {
    let vm = unsafe { vm(cx) };
    vm.lane_stack.innermost_function().unwrap_or_else(|| vm.frames[unsafe { (*cx).frame() }].function)
}

/// Notes the function a forwarded call calls as the site's callee, through its native `through`, for a body that
/// inlines it (`lane_unit::Planner::through_call`).
pub(super) fn note_forwarded_in_lane(vm: &mut Vm, i: &LaneInstruction, through: Value, callee: Value) {
    if let Some((program, function, _, false)) = vm.same_realm_script_target(callee) && program == vm.active_program {
        vm.note_site_callee(&i.call_link, crate::compiled::CallTarget { callee: crate::compiled::CalleeBinding::Forwarded { through, callee }, function });
    }
}

/// Remembers `callee` for `function_call_miss`'s bound fast paths (`LaneStack::bound_call`)
/// when it is a bound function of a user function: its words, with an empty list of bound
/// arguments kept as `undefined`, so the window alone is what such a call passes.
#[inline(never)]
pub(super) fn remember_bound_call_in_lane(vm: &mut Vm, callee: Value) {
    let Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionBoundCall), env, .. }) = vm.function_record(callee) else { return };
    let Some((target, this, list)) = vm.bound_function_words(*env) else { return };
    let list = if vm.heap_reads().array_elements(list).is_some_and(|elements| elements.len() != 0) { list } else { Value::UNDEFINED };
    if matches!(vm.function_record(target), Some(FunctionRecord { kind: FunctionKind::User(..), .. })) {
        vm.lane_stack.bound_call = Some((callee, target, this, list));
    }
}

/// `function_call_miss` of a bound function with bound arguments, which lead the values the
/// call passes (ECMA-262 10.4.1.1): the forwarded call with the values copied out
/// (`Vm::forwarded_call`), a user function's activation or frame call
/// (`FrameCallArguments::List`), else the general path.
#[inline(never)]
pub(super) fn bound_arguments_call_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: as in `function_call_miss`; the values are copied before anything runs.
    let window = unsafe { call_window_in_lane(r, i) };
    // The bound function the lane last called (`LaneStack::bound_call`): its target, a user
    // function, and its receiver as remembered, and the passed values filled once, the bound
    // list's elements then the window, with no walk of its parts.
    let remembered = match vm.lane_stack.bound_call {
        Some((bound, target, this, list)) if bound == r.get(i.b) && !list.is_undefined() => {
            vm.heap_reads().array_elements(list).map(|elements| {
                let bound = elements.as_slice();
                let mut args = smallvec::SmallVec::<[Value; FRAME_CALL_INLINE_ARGUMENTS]>::with_capacity(bound.len() + window.len());
                args.extend_from_slice(&bound);
                args.extend_from_slice(window);
                (target, this, args)
            })
        }
        _ => None,
    };
    let (callee, receiver, args) = match remembered {
        Some(call) => call,
        None => {
            // The window is read in place by the walk and copied once, behind the bound
            // prefix that leads it (`WindowArgs`).
            let walked = vm.forwarded_call(r.get(i.b), r.get(i.c), quench_runtime::vm::function::WindowArgs::<FRAME_CALL_INLINE_ARGUMENTS>::Window(window));
            let Some(quench_runtime::vm::function::Forwarded { callee, receiver, args, kind, .. }) = walked else {
                return Resumed::Exit(exit(ip));
            };
            if !matches!(kind, Some(FunctionKind::User(..))) {
                return Resumed::Exit(exit(ip));
            }
            let args = args.into_owned();
            remember_bound_call_in_lane(vm, r.get(i.b));
            (callee, receiver, args)
        }
    };
    let Some(target) = passed_target_in_lane(cx, callee, args.len()) else {
        return passed_frame_call_in_lane(cx, r, ip, i, callee, receiver, FrameCallArguments::List(&args));
    };
    let Some(this) = call_this_in_lane(vm, &target, receiver) else {
        return Resumed::Exit(exit(ip));
    };
    match activate_passed_in_lane(cx, r, ip, &target, &args, this) {
        Ok((registers, base)) => Resumed::At(registers, base, Carried::script(cx)),
        Err(next) => Resumed::Exit(next),
    }
}

/// `function_call_miss`'s activation of the frameless `callee` with the forwarded `receiver`
/// and `args`: the callee's registers and first record, or the record to continue at. Out of line, as `construct_keeping_activation_in_lane`.
#[inline(never)]
pub(super) fn function_call_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Resumed {
    let vm = unsafe { vm(cx) };
    let mut resolved = None;
    let Some(target) = forwarded_target_in_lane(cx, ip, callee, args.len(), &mut resolved) else {
        return Resumed::Exit(forwarded_frame_call_in_lane(cx, r, ip, i, callee, receiver, args));
    };
    // `call_this_value` without its boxing of primitives, which allocates.
    let Some(this) = call_this_in_lane(vm, target, receiver) else {
        return Resumed::Exit(exit(ip));
    };
    if let Some(resumed) = forwarded_cell_entry_in_lane(cx, r, ip, i, target, args, this) {
        return resumed;
    }
    match activate_passed_in_lane(cx, r, ip, target, args, this) {
        Ok((registers, base)) => Resumed::At(registers, base, Carried::script(cx)),
        Err(next) => Resumed::Exit(next),
    }
}

/// `passed_target_in_lane` of a forwarded call's `callee` at the call record `ip`, whose read
/// memo slot links the callee's cache entry, as an array callback method's call links its
/// callback's (`callback_target_in_lane`): the link, tested first, skips the set's hash and the
/// cross-program and environment lookups for a callee the running program activates.
#[inline(always)]
pub(super) fn forwarded_target_in_lane<'a>(
    cx: *mut LaneContext,
    ip: Ip,
    callee: Value,
    passed: usize,
    resolved: &'a mut Option<ScriptCallTarget>,
) -> Option<&'a ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // SAFETY: a call record's operand is `call`.
    let link = unsafe { (*ip).operand.call.memo } as usize;
    if vm.script_call_holds(link, callee) && !constructing_in_lane(vm, callee) {
        // SAFETY: `script_call_holds` found the entry within the cache, which nothing
        // resolves into before the caller reads it; read in place, not copied.
        let target = unsafe { &*std::ptr::from_ref(vm.lane_stack.script_targets.entries.get_unchecked(link)) };
        if admits_activation_in_lane(target) {
            return Some(target);
        }
    }
    let target = resolved.insert(passed_target_in_lane(cx, callee, passed)?);
    if let Some(index) = vm.script_call_index(callee) {
        // SAFETY: as `requicken`: the record's operand is the call's, written in place.
        unsafe { (*ip.cast_mut()).operand.call.memo = index as u32 };
    }
    Some(target)
}

/// `enter_cell_body_in_lane` of a forwarded call of a plain target of the running program, passed
/// at most its parameters and not completing its caller, whose entry cell holds a body.
#[inline(always)]
pub(super) fn forwarded_cell_entry_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    target: &ScriptCallTarget,
    args: &[Value],
    this: Value,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let argc = u16::try_from(args.len()).ok().filter(|argc| *argc <= target.params)?;
    (target.entry == CallEntry::Activates
        && !crosses_in_lane::<true>(cx, target)
        && i.a & quench_runtime::bytecode::RETURN_REGISTER == 0
        && !vm.with_scope_entered
        && activation_room_in_lane(vm, target)
        && unsafe { (*target.view).enters_cell_body() })
    .then_some(())?;
    enter_cell_body_in_lane(cx, r, ip, target, args.as_ptr(), argc, this)
}

/// The frameless target a forwarded or applied call of `callee` with `passed` values
/// activates: one of the running program (`script_target_in_lane`), another program's plain,
/// keeping or environment activation (`cross_call_entry`), or the running program's
/// environment target, which `script_target_in_lane` leaves to plain calls; an environment
/// target only when it binds each value to a parameter (`activate_environment_in_lane`).
#[inline(never)]
pub(super) fn passed_target_in_lane(cx: *mut LaneContext, callee: Value, passed: usize) -> Option<ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // Matched in turn rather than chained: the combinators moved the target through copies.
    let target = match script_target_in_lane(cx, callee) {
        Some(target) => target,
        None => {
            // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
            let cross = (!constructing_in_lane(vm, callee))
                .then(|| vm.cross_call_entry(callee))
                .flatten()
                .map(|entry| unsafe { &*entry })
                .filter(|target| target.entry.activates_across() || target.entry == CallEntry::Environment);
            // SAFETY: as above.
            let own = || {
                vm.script_call_entry(callee)
                    .map(|entry| unsafe { &*entry })
                    .filter(|target| target.entry == CallEntry::Environment && target.program() == vm.active_program)
            };
            match cross {
                Some(target) => *target,
                None => *own()?,
            }
        }
    };
    if target.entry == CallEntry::Environment
        && passed > usize::from(target.params)
        && !vm.programs.residual(target.program())?.function(target.function as usize).rest
    {
        return None;
    }
    Some(target)
}

/// The activation of a `passed_target_in_lane` with receiver `this` and the `passed` values:
/// an environment activation, a plain call's push where the callee keeps nothing past its
/// parameters, or a keeping one, which keeps them; either may enter another program
/// (`Completion::Cross`).
#[inline(always)]
pub(super) fn activate_passed_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    if target.entry == CallEntry::Environment {
        return activate_environment_in_lane(cx, r, ip, target, passed, (this, false));
    }
    activate_kept_in_lane::<true>(cx, r, ip, target, passed, this)
}

/// The activation of `target`, of the running program unless `MAY_CROSS`, for the record at
/// `ip` with receiver `this` and the `passed` values, which lie outside the caller's call
/// window (a forwarded or applied call's, a getter's none): a callee that keeps no passed
/// values (`ScriptCallTarget::forwards`) and is passed at most its parameters, or drops the
/// rest, takes the plain call's push (`activate_in_lane`); any other, a keeping activation.
#[inline(always)]
pub(super) fn activate_kept_in_lane<const MAY_CROSS: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    let argc = u16::try_from(passed.len()).unwrap_or(u16::MAX);
    // SAFETY: `ip` is a record of the running view.
    let returns = unsafe { (*ip).a } & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if !target.forwards()
        && (argc <= target.params || target.drops_extra_arguments())
        && (!returns || returning_call_admitted(cx))
        && !vm.with_scope_entered
        && activation_room_in_lane(vm, target)
    {
        return Ok(push_passed_in_lane::<MAY_CROSS, false>(cx, r, ip, target, passed.as_ptr(), argc.min(target.params), this, false));
    }
    let arguments = ActivationArguments::Kept(passed.as_ptr(), passed.len());
    activate_keeping_in_lane::<MAY_CROSS>(cx, r, ip, target, arguments, this, false, None)
}

/// `function_call_miss` of a forwarded `callee` with no frameless target: a user function
/// the general loop pushes in place (`Vm::in_loop_call_target`) takes its frame as
/// `frame_call_in_lane` makes it, with the forwarded `receiver` and `args`, the call window's
/// tail. Any other takes the general path.
#[inline(never)]
pub(super) fn forwarded_frame_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Ip {
    let skip = usize::from(ImmediateLayout::argument_count(i.imm())) - args.len();
    passed_frame_call_in_lane(cx, r, ip, i, callee, receiver, FrameCallArguments::Window(skip)).into_loop()
}

/// The frame call of `callee` with `receiver` and `passed` that `forwarded_frame_call_in_lane`
/// and an applied call (`apply_activation_in_lane`) make.
pub(super) fn passed_frame_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    passed: FrameCallArguments<'_>,
) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if !unsafe { (*cx).script_calls } || returns && !returning_call_admitted(cx) {
        return Resumed::Exit(exit(ip));
    }
    let Some(target) = vm.in_loop_call_target(code, callee) else {
        #[cfg(feature = "profile-lane-exits")]
        quench_runtime::vm::lane_exits::record_detail(format!(
            "passed frame call declined kind={:?} realm_matches={} name={:?}",
            vm.function_record(callee).map(|record| match record.kind {
                FunctionKind::Native(Native::HostFunction) => vm.host_function_name(callee).unwrap_or_default().to_string(),
                FunctionKind::Native(native) => format!("{native:?}"),
                FunctionKind::User(..) => "user".to_string(),
            }),
            vm.function_record(callee).is_some_and(|record| vm.record_realm(record) == vm.realm.globals),
            vm.function_record(callee).and_then(|record| match record.kind {
                FunctionKind::User(program, function) => vm.programs.residual(program).and_then(|code| {
                    let body = code.function_get(function as usize)?;
                    Some(format!("{} @{:?} async={} generator={} in_loop={}",
                        body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
                        body.source_span.map(|span| span.start), body.is_async, body.is_generator, body.call_plan(code).in_loop))
                }),
                _ => None,
            }),
        ));
        return Resumed::Exit(exit(ip));
    };
    push_frame_call_in_lane(cx, r, ip, i, target, callee, receiver, passed)
}

/// The native call `native_call_miss` makes in place (`native_value_in_lane`), completed in its
/// result register or as its caller's return (`native_complete_in_lane`).
#[inline(always)]
pub(super) fn native_call_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
) -> Resumed {
    // A call that returns its result completes its caller: an activation in the lane, or
    // the frame in the general loop (`frame_return`).
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    // The running function is the same activation or, once the call made activations
    // frames (`lane_call_native`), the frame it became.
    if returns && !returning_call_admitted(cx) {
        return Resumed::Exit(exit(ip));
    }
    // A generator's `next` resumes it as an activation, which makes no frame of the caller's
    // (one-frame-one-call.md, frame switches).
    if let Some((registers, start)) = generator_windows::resume_called_generator(cx, r, ip, i, native) {
        return Resumed::At(registers, start, Carried::script(cx));
    }
    // SAFETY: the call's window is registers of the running function, which the native
    // call copies before anything can move them.
    let args = unsafe { call_window_in_lane(r, i) };
    let value = native_value_in_lane(cx, ip, i, native, r.get(i.b), r.get(i.c), args);
    native_complete_in_lane(cx, ip, i, value)
}

/// `call_spread_miss` of a callee the site links: the elements of the intact `list`, `length`
/// of them, passed as `passed_linked_call_in_lane` passes values, read in place from a wide
/// backing, decoded from a narrow one. `None` for any other call, which the general spread
/// path makes.
#[inline(always)]
pub(super) fn spread_linked_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    list: Value,
    length: usize,
) -> Option<(Registers, Ip)> {
    let argc = u16::try_from(length).ok()?;
    let elements = unsafe { vm(cx) }.heap_reads().array_elements(list)?;
    let mut decoded = [Value::UNDEFINED; FRAME_CALL_INLINE_ARGUMENTS];
    let (passed, valid) = match elements.wide_values() {
        Some(values) => (values.as_ptr(), argc),
        None => {
            let mut valid = 0;
            for (slot, value) in decoded.iter_mut().zip(elements.iter().take(length)) {
                *slot = value;
                valid += 1;
            }
            (decoded.as_ptr(), valid)
        }
    };
    passed_linked_call_in_lane(cx, r, ip, i, passed, argc, valid)
}

/// A spread call of a callee the site links (`Vm::linked_script_call_entry`, as a plain
/// call's), passed the `argc` values at `passed`, of which the first `valid` may be read: a
/// rest body that keeps nothing as `activate_gathering_in_lane` activates it, else a target
/// keeping nothing (`ScriptCallTarget::forwards`) activated as a plain call is
/// (`push_passed_in_lane`), taking the values as its parameters or dropping those past them.
/// `passed` stays valid until the activation is made: no collection runs inside it. `None`
/// for any other call.
#[inline(always)]
pub(super) fn passed_linked_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    passed: *const Value,
    argc: u16,
    valid: u16,
) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    if constructing_in_lane(vm, callee) {
        return None;
    }
    // SAFETY: as `Vm::script_call_entry`: read in place, not copied (64 bytes a call), since
    // nothing before the activation is made resolves a target or collects; the gathering
    // activation copies it before it allocates the rest array (`activate_gathering_in_lane`).
    let target = vm.linked_script_call_entry(callee, &i.call_link).map(|target| unsafe { &*target })?;
    if target.entry == CallEntry::Gathers && target.forms & TARGET_GATHERS_KEEPING == 0 {
        if !admits_activation_in_lane(&target) || valid < argc {
            return None;
        }
        // Refused here, not by the activation: without room it makes the lane's activations
        // frames (`script_call_frame_in_lane`), after which the caller may not go on in the lane.
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        let room = usize::from(target.room) + if target.forms & TARGET_REST_VIEW != 0 { usize::from(argc) } else { 0 };
        if (returns && !returning_call_admitted(cx))
            || vm.with_scope_entered
            || !quench_runtime::staged::call_keeping::concrete_admits_keeping(
                vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get(), vm.lane_stack.values.room(), room,
            )
        {
            return None;
        }
        let this = call_this_in_lane(vm, &target, r.get(i.c))?;
        return activate_gathering_in_lane(cx, r, ip, &target, passed, argc, this).ok();
    }
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if !admits_activation_in_lane(&target)
        || target.forwards()
        || (argc > target.params && !target.drops_extra_arguments())
        || (returns && !returning_call_admitted(cx))
        || vm.with_scope_entered
        || !activation_room_in_lane(vm, &target)
    {
        return None;
    }
    let this = call_this_in_lane(vm, &target, r.get(i.c))?;
    let argc = argc.min(target.params);
    if valid < argc {
        return None;
    }
    Some(push_passed_in_lane::<true, false>(cx, r, ip, &target, passed, argc, this, false))
}

/// The values a lane `Call` record passes: its window of the running function's registers.
///
/// # Safety
/// `r` is the registers of the activation `i` belongs to; the slice is valid until they move.
#[inline(always)]
pub(super) unsafe fn call_window_in_lane<'a>(r: Registers, i: &LaneInstruction) -> &'a [Value] {
    unsafe { window_in_lane(r, ImmediateLayout::call_window_base(i.imm()), i) }
}

/// The `argument_count` values of the record `i` at register `window` of the running function.
///
/// # Safety
/// As `call_window_in_lane`.
#[inline(always)]
pub(super) unsafe fn window_in_lane<'a>(r: Registers, window: u16, i: &LaneInstruction) -> &'a [Value] {
    unsafe { std::slice::from_raw_parts(r.0.add(usize::from(window)), usize::from(ImmediateLayout::argument_count(i.imm()))) }
}

/// The value of the lane `Call` record `i` of the native `callee`, of `kind`, with
/// `receiver` and `args`: the in-place cases every tier shares (`Vm::in_place_native_value`),
/// the lane's own, else `string_or_native_call_in_lane`. `None` leaves the call to the general path,
/// which raises an error the call stashed in `Vm::lane_raised`. Out of line: its calls take
/// stack temporaries, which a handler inlining them would hold on its common path.
#[inline(never)]
pub(super) fn native_value_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // The Native argument carries the caller's function-kind proof. Each operation uses
    // its semantic fallback; the remaining natives try `string_or_native_call_in_lane`.
    match native {
        // The DataView Float64 lane, in every mode: one read of the view and one of its
        // buffer; a miss takes the native, which coerces and throws.
        Native::DataViewGetFloat64 | Native::DataViewSetFloat64 => {
            let value = if native == Native::DataViewGetFloat64 {
                vm.data_view_access_of(Native::DataViewGetFloat64, receiver, args)
            } else {
                vm.data_view_access_of(Native::DataViewSetFloat64, receiver, args)
            };
            if value.is_some() {
                if vm.execution_mode == quench_runtime::vm::ExecutionMode::Compiled {
                    vm.programs.note_js_jit_dataview_float64(native == Native::DataViewSetFloat64);
                }
                return value;
            }
        }
        Native::RegExpExec | Native::RegExpTest | Native::StringReplace | Native::StringMatch => {
            return regexp_calls::regexp_call_in_lane(cx, ip, i, native, callee, receiver, args);
        }
        // A generator's resumption runs its body, and a Wasm export its function: guest code
        // that no in-place shortcut of `string_or_native_call_in_lane` makes.
        Native::GeneratorNext | Native::GeneratorReturn | Native::GeneratorThrow | Native::WasmExportedFunction => {
            return call_native_in_lane(cx, ip, native, callee, receiver, args);
        }
        // `with (object)` and its exit, in a frameless activation: its scope chain is the lane stack's.
        Native::WithEnter | Native::WithExit => {
            if let Some(value) = with_scopes::native_with_scope(cx, native, args) {
                return Some(value);
            }
        }
        // Neither a string method nor a pure native: the promise shortcut, else the native.
        Native::PromiseThen | Native::PromiseCatch | Native::PromiseResolve => {
            return promise_native_call_in_lane(cx, ip, native, callee, receiver, args);
        }
        _ => {}
    }
    string_or_native_call_in_lane(cx, ip, native, callee, receiver, args)
}

/// The value of a promise native call: the lane's shortcut where it makes one, else the native.
pub(super) fn promise_native_call_in_lane(cx: *mut LaneContext, ip: Ip, native: Native, callee: Value, receiver: Value, args: &[Value]) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(value) = promise_native_in_lane(vm, code, native, callee, receiver, args) {
        note_allocation(vm);
        return Some(value);
    }
    call_native_in_lane(cx, ip, native, callee, receiver, args)
}

/// Whether a compiled loop may call `native` in place and go on (`in_place_natives::in_place_in_loop`).
pub(crate) const fn in_place_in_loop(native: Native) -> bool { in_place_natives::in_place_in_loop(native) }

/// Whether liveness proves the result register of the running function's call record `i`
/// at `ip` dead once the call completes (`liveness::register_dead` before the next PC), so
/// its value is never read: what the call writes there is unobservable.
#[cold]
pub(super) fn call_result_dead_in_lane(cx: *mut LaneContext, ip: Ip, i: &LaneInstruction) -> bool {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // A leaf makes no call, so the innermost activation, or else the frame, runs `i`.
    let function = running_function_in_lane(cx);
    // SAFETY: `ip` is a record of the running view, which starts at the context's base.
    let pc = unsafe { record_pc(cx, ip) } as usize;
    code.register_root_masks(function)
        .and_then(|masks| masks.get(pc + 1))
        .is_some_and(|after| quench_runtime::compile::liveness::register_dead(after, usize::from(i.a & quench_runtime::bytecode::REGISTER_MASK)))
}

/// Whether `callee` is a function whose behavior is the native `native`'s.
#[inline(always)]
pub(super) fn callee_is_native(vm: &Vm, callee: Value, native: Native) -> bool {
    matches!(vm.function_record(callee), Some(FunctionRecord { kind: FunctionKind::Native(found), .. }) if *found == native)
}

/// The lane `Call` record `i` completed with `value`: in its result register, or returned
/// from the caller for a call that returns its result; the general path for `None`.
#[inline(always)]
pub(super) fn native_complete_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    value: Option<Value>,
) -> Resumed {
    let Some(value) = value else {
        return Resumed::Exit(exit(ip));
    };
    // The call may have made the lane's activations frames, so the running function's
    // registers and caller are read again.
    if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        let Some(resumed) = complete_activation_in_lane(cx, ip, value) else {
            // A frame of a script run retires in place, as its `Return` would.
            if unsafe { !(*cx).inline_calls && (*cx).script_calls } {
                return Resumed::Exit(return_script_frame_in_lane(cx, ip, value, true));
            }
            return Resumed::Exit(frame_return(unsafe { vm(cx) }, ip, value));
        };
        return resumed;
    }
    let r = unsafe { (*cx).registers };
    r.set(i.a, value);
    Resumed::At(r, following(ip), Carried::script(cx))
}

/// A lane native call that `native_call_miss` does not make inline: a pure string method or
/// a pure native in place, else the native through `call_native_in_lane`. Inline into its two callers: a native
/// call otherwise pays this layer's own frame.
#[inline(always)]
pub(super) fn string_or_native_call_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(value) = vm.in_place_native_value_inline(code, native, callee, receiver, args) {
        // An in-place case may allocate; a due collection runs at the lane's next safepoint.
        note_allocation(vm);
        return Some(value);
    }
    call_native_in_lane(cx, ip, native, callee, receiver, args)
}

/// A lane call of a promise native whose steps provably observe nothing, answered in place
/// so the lane's activations need not become frames (`call_native_in_lane`): `then` and
/// `catch` of a receiver with intrinsic species (`Vm::promise_then_in_lane`) and the intrinsic
/// `Promise.resolve` (`Vm::promise_resolve_in_lane`). A resolving function (one with an
/// environment), any other shape, or a lifecycle-observing host takes the general call.
#[inline(never)]
pub(super) fn promise_native_in_lane(
    vm: &mut Vm,
    code: &ResidualProgram,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    if !matches!(native, Native::PromiseThen | Native::PromiseCatch | Native::PromiseResolve)
        || !vm.function_record(callee).is_some_and(|function| function.env.is_null())
    {
        return None;
    }
    match native {
        Native::PromiseThen => vm.promise_then_in_lane(
            code,
            receiver,
            args.first().copied().unwrap_or(Value::UNDEFINED),
            args.get(1).copied().unwrap_or(Value::UNDEFINED),
        ),
        Native::PromiseCatch => {
            // `catch` reads `then` of its receiver and calls it: only the intrinsic `then`,
            // read as plain data, is the shortcut's to make.
            let then_atom = vm.runtime_atoms.then;
            let then = vm.plain_named_load(receiver, then_atom)?;
            if !matches!(
                vm.function_record(then),
                Some(FunctionRecord { kind: FunctionKind::Native(Native::PromiseThen), .. })
            ) {
                return None;
            }
            vm.promise_then_in_lane(
                code,
                receiver,
                Value::UNDEFINED,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            )
        }
        Native::PromiseResolve => vm.promise_resolve_in_lane(
            code,
            receiver,
            args.first().copied().unwrap_or(Value::UNDEFINED),
        ),
        _ => unreachable!("only promise shortcut natives reach here"),
    }
}

/// A lane `Call` of a native that `Native::runs_from_lane` admits, made as the general path
/// makes it from the running frame: with the frame's resume PC published and the arguments
/// read from its registers, which the frame keeps live at the call. Only a frame's own lane run
/// calls out, so the frame stack the native and its nested calls observe is the general
/// path's. `None` exits to the general path, which raises the error the call stashed in
/// `Vm::lane_raised` or makes a call this does not admit.
#[inline(never)]
pub(super) fn call_native_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    if !native.runs_from_lane() {
        return None;
    }
    call_native_out_in_lane(cx, ip, native, callee, receiver, args)
}

/// `call_native_in_lane` past its admission: the native's call out of the lane,
/// beside its activations where the native admits it, else from its frame
/// (`call_from_frame_in_lane`), as `call_array_callback_miss` calls an array's callback method,
/// which no shortcut answers.
#[inline(always)]
pub(super) fn call_native_out_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } {
        return None;
    }
    // Arguments in the call's window are held by the frame parked at the call, so the native
    // roots only its receiver; any others (an `apply`'s elements) are rooted with it.
    let in_window = arguments_in_window(cx, args);
    // SAFETY: `ip` is a record of the active view, which is the running function's.
    let call_pc = unsafe { record_pc(cx, ip) } as usize;
    // Without activations the window lies in the lane's frame's register file, which nothing
    // moves or writes while the frame is parked at the call: the native reads it in place.
    if in_window && !vm.lane_stack.has_frames() && registers_are_frames(vm, cx) {
        return call_from_frame_in_lane(cx, ip, |vm, code| {
            vm.call_native_from_lane_arguments_held(code, native, receiver, args, callee)
        });
    }
    // With activations the innermost one's frame holds the window once they become frames.
    let held = in_window && vm.lane_stack.has_frames();
    // The values are copied before activations become frames, which moves the registers
    // they may lie in; the frames then hold what the call passes.
    let arguments = quench_runtime::vm::call_arguments::CallArguments::from_slice(args);
    let call = |vm: &mut Vm, code: &ResidualProgram| {
        if held {
            vm.call_native_from_lane_arguments_held(code, native, receiver, arguments.as_slice(), callee)
        } else {
            vm.call_native_from_lane(code, native, receiver, arguments.as_slice(), callee)
        }
    };
    // A native that observes frames only through the points `materialize_deferred_lane`
    // guards runs beside the activations; the first such point makes them frames, and a
    // collection takes that point before it scans (`Vm::collect_inner`).
    if vm.lane_stack.has_frames()
        && (vm.runs_beside_activations(native, callee) || vm.steps_beside_activations(native, receiver))
    {
        return call_deferred_native_in_lane(cx, call_pc, call);
    }
    #[cfg(feature = "profile-lane-exits")]
    if vm.lane_stack.has_frames() {
        quench_runtime::vm::lane_exits::record_detail(match native {
            Native::HostFunction => format!("materializes for host {}", vm.host_function_name(callee).unwrap_or_default()),
            native => format!("materializes for native {native:?}"),
        });
    }
    call_from_frame_in_lane(cx, ip, call)
}

/// Runs a native call beside the lane's activations as a deferred call (`DeferredLaneCall`),
/// whose frames the first observer makes; an error is raised for the general path as the
/// call's own frames would raise it.
#[cold]
#[inline(never)]
pub(super) fn call_deferred_native_in_lane(
    cx: *mut LaneContext,
    call_pc: usize,
    call: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<Value, JsError>,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
    let result = call(vm, unsafe { &*(*cx).code });
    note_allocation(vm);
    vm.lane_stack.refresh_activation_limit();
    let unobserved = vm.lane_deferred.take().is_some();
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            // The general path raises it from the frames its activations become.
            if unobserved {
                vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
                vm.materialize_deferred_lane();
            }
            vm.lane_completion.raise(error);
            None
        }
    }
}

/// Whether `args` lie in the running function's registers, as its call record's window does:
/// the frame parked at the call (the lane's own, or the one the innermost activation becomes,
/// which a collection makes first) holds them live at the call's pc, where the collector scans
/// a parked frame (`Frame::source_instruction_pc`). Values a call made of others (an `apply`'s
/// array elements, a bound prefix) lie elsewhere. Only addresses are compared.
#[inline(always)]
pub(super) fn arguments_in_window(cx: *mut LaneContext, args: &[Value]) -> bool {
    if args.is_empty() {
        return true;
    }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let count = usize::from(code.function(running_function_in_lane(cx) as usize).registers);
    let registers = unsafe { (*cx).registers.0 }.cast_const();
    let first = args.as_ptr();
    registers <= first && first.wrapping_add(args.len()) <= registers.wrapping_add(count)
}

/// Whether the lane's registers are its frame's register file, which stays where it is while
/// the frame is parked at a call.
#[inline(always)]
pub(super) fn registers_are_frames(vm: &Vm, cx: *mut LaneContext) -> bool {
    // SAFETY: the lane runs, so its context lives.
    unsafe { (*cx).has_frame() && vm.frames[(*cx).frame()].registers.as_ptr() == (*cx).registers.0.cast_const() }
}

/// A native call at the lane `Call` record `ip` made from the running frame: the callee
/// observes the frame stack, so lane activations become frames first, as leaving the lane
/// would make them, and `call` runs from the innermost one. `None` leaves the error it raised
/// in `Vm::lane_raised`.
#[inline(always)]
pub(super) fn call_from_frame_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    call: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<Value, JsError>,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the active view, which is the running function's.
    let call_pc = unsafe { record_pc(cx, ip) } as usize;
    vm.materialize_lane_activations(cx);
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // active view, which is the running frame's.
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame()) };
    vm.frames[frame].pc = call_pc + 1;
    vm.frames[frame].pc_is_resume = true;
    // The native branch of `Vm::call_value_with_target_inner`, whose callee this is; its
    // calls of JavaScript run in this lane while it runs from this frame.
    let site = LaneNativeCall { cx: cx.cast(), call: ip, frames: vm.frames.len() };
    let outer = vm.lane_native.replace(site);
    let result = call(vm, code);
    vm.lane_native = outer;
    // The native's allocations may make a collection due, which the lane's safepoints take.
    note_allocation(vm);
    // A host edge can change the thread's guest-call allowance. Rebind the
    // capacity certificate here, before any lane/native entry consumes it.
    vm.lane_stack.refresh_activation_limit();
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            vm.lane_completion.raise(error);
            None
        }
    }
}

/// A lane `new` of a native that `Native::constructs_from_lane` admits, made as the general
/// path makes it (`Vm::construct_value_with_new_target`) from the running frame, after
/// activations become frames, as `call_native_in_lane` makes a call. `None` exits to the general
/// path, which raises the error the construction stashed in `Vm::lane_raised` or makes one
/// this does not admit.
#[inline(never)]
pub(super) fn construct_native_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    callee: Value,
    new_target: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } || vm.construct_target.is_some() {
        return None;
    }
    // The values are copied before activations become frames, which moves the registers
    // they may lie in; the frames then root what the construction passes.
    let arguments = quench_runtime::vm::call_arguments::CallArguments::from_slice(args);
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // running function's view.
    let call_pc = unsafe { record_pc(cx, ip) } as usize;
    // A native's construction runs beside the activations, as a native call beside them
    // runs (`call_native_in_lane`): the first point that observes frames makes them frames.
    let beside = vm.lane_stack.has_frames() && vm.constructs_beside_activations(callee);
    if beside {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
    } else {
        vm.materialize_lane_activations(cx);
        let frame = unsafe { (*cx).frame() };
        vm.frames[frame].pc = call_pc + 1;
        vm.frames[frame].pc_is_resume = true;
    }
    // SAFETY: as above; the construction runs from the running function's program.
    let code = unsafe { &*(*cx).code };
    let result = vm.construct_value_with_new_target(code, callee, new_target, arguments.as_slice());
    // The construction's allocations may make a collection due, which the lane's safepoints
    // take.
    note_allocation(vm);
    let unobserved = beside && vm.lane_deferred.take().is_some();
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            // The general path raises it from the frames the activations become.
            if unobserved {
                vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
                vm.materialize_deferred_lane();
            }
            vm.lane_completion.raise(error);
            None
        }
    }
}

/// The native parent a `new` of `callee` constructs in its place: `callee`'s own parent
/// (`[[GetPrototypeOf]]`) when its constructor only constructs it
/// (`Function::super_forwarding`) and that parent is a native the lane constructs other
/// than an error constructor, with what the constructor passes on.
#[inline(never)]
pub(super) fn super_forwarded_parent_in_lane(vm: &Vm, callee: Value) -> Option<(Value, quench_runtime::bytecode::SuperForwarded)> {
    // A plan with a recorded user parent (`super_construction`) is no native forwarding: its
    // construction goes straight to that plan, not decoding the constructor's body each time.
    if holding_construct_plan_index(vm, callee).is_some_and(|index| vm.lane_stack.construct_plans[index].super_parent != Value::UNDEFINED) {
        return None;
    }
    let Some(FunctionRecord { kind: FunctionKind::User(program, function), .. }) = vm.function_record(callee) else {
        return None;
    };
    let code = vm.programs.residual(*program)?;
    let body = code.function_get(*function as usize)?;
    if !body.derived_constructor {
        return None;
    }
    // The parent first: a class whose parent is no native the lane constructs, as every
    // user class hierarchy's, declines here without decoding its constructor's body
    // (`Function::super_forwarding`, ~900 Ir), which every `new` of it would otherwise pay.
    let parent = vm.heap_reads().object(callee)?.proto();
    match vm.function_record(parent) {
        // An error's stack starts past its constructors' frames, which this construction
        // would not make.
        Some(FunctionRecord { kind: FunctionKind::Native(native), .. })
            if (native.constructs_from_lane() && !native.is_error_constructor()) || *native == Native::Array =>
        {
            Some((parent, body.super_forwarding(&code.atoms, &code.constants)?))
        }
        _ => None,
    }
}

/// `new callee(...)` of a class whose constructor only constructs its native `parent`
/// (`super_forwarded_parent_in_lane`): the parent's construction with what the constructor
/// passes on (every value, or its parameters with missing ones `undefined`) and `callee`
/// as `new.target`, as the body's `super(...)` makes it; its receiver is the construction's
/// value.
#[inline(never)]
pub(super) fn construct_forwarded_parent_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    (parent, forwarded): (Value, quench_runtime::bytecode::SuperForwarded),
) -> Resumed {
    let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
    let count = match forwarded {
        quench_runtime::bytecode::SuperForwarded::Passed => argc,
        quench_runtime::bytecode::SuperForwarded::Parameters(count) => usize::from(count),
    };
    let passed = (0..count)
        .map(|index| if index < argc { r.get(i.c + index as u16) } else { Value::UNDEFINED })
        .collect::<smallvec::SmallVec<[Value; GATHERED_INLINE_VALUES]>>();
    let vm = unsafe { vm(cx) };
    // An array subclass's `super(...)`: the intrinsic's array with the class's `prototype`,
    // made in place and named as the general path names it (`capture_constructed_value`).
    if vm.construct_target.is_none()
        && matches!(vm.function_record(parent), Some(FunctionRecord { kind: FunctionKind::Native(Native::Array), .. }))
        && let Some(array) = vm.array_construction_by(callee, &passed)
    {
        vm.capture_constructed_value(array, parent, false);
        note_allocation(vm);
        return native_complete_in_lane(cx, ip, i, Some(array));
    }
    let value = construct_native_in_lane(cx, ip, parent, callee, &passed);
    native_complete_in_lane(cx, ip, i, value)
}

/// `script_call_keeping_miss`'s activation: the callee's registers and first record, the
/// record to continue at, or `None` for a callee whose target keeps nothing, which the full
/// form calls. Inline in its one handler, which is itself out of line.
#[inline(always)]
pub(super) fn call_keeping_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(Registers, Ip), Option<Ip>> {
    if !unsafe { (*cx).script_calls } {
        return Err(Some(exit(ip)));
    }
    let vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    // The site's linked entry, as `script_call_environment_miss` reads it, read in place: only
    // an activation that allocates (a rest array) copies it first; any other callee takes the
    // general lookup.
    let linked = if constructing_in_lane(vm, callee) {
        None
    } else {
        // SAFETY: as `Vm::script_call_entry`; nothing before the activation resolves a target.
        vm.linked_script_call_entry(callee, &i.call_link)
            .map(|target| unsafe { &*target })
            .filter(|target| admits_activation_in_lane(target))
    };
    let resolved;
    let target: &ScriptCallTarget = match linked {
        Some(target) => target,
        None => {
            let Some(target) = script_target_in_lane(cx, callee) else {
                return Err(None);
            };
            resolved = target;
            &resolved
        }
    };
    if !target.forwards() {
        return Err(None);
    }
    #[cfg(feature = "profile-lane-exits")]
    profile_script_call_target(cx, ip, callee, &target);
    let receiver = r.get(i.c);
    let Some(this) = call_this_in_lane(vm, &target, receiver) else {
        return Err(Some(exit(ip)));
    };
    let window = ImmediateLayout::call_window_base(i.imm());
    if target.entry == CallEntry::Gathers && target.forms & TARGET_GATHERS_KEEPING == 0 {
        // SAFETY: the call's window of `r` holds its `argc` values.
        let passed = unsafe { r.0.add(usize::from(window)).cast_const() };
        return activate_gathering_in_lane(cx, r, ip, &target, passed, ImmediateLayout::argument_count(i.imm()), this).map_err(Some);
    }
    if target.entry == CallEntry::Keeps {
        return activate_keeping_window_in_lane::<false>(cx, r, ip, &target, window, this).map_err(Some);
    }
    let arguments = ActivationArguments::Window(window);
    // This activation may allocate its rest array before it reads the target: a copy.
    activate_keeping_in_lane::<false>(cx, r, ip, &{ *target }, arguments, this, false, None).map_err(Some)
}

/// The keeping activation of a target of the running program that keeps the values it was
/// passed (`CallEntry::Keeps`), called with its window at `arguments` of `r`, made as a plain
/// call's push is (`fill_window_in_lane`, `enter_window_in_lane`) with the values kept after its
/// registers, as `activate_keeping_in_lane` keeps them, and the activation marked keeping.
#[inline(always)]
pub(super) fn activate_keeping_window_in_lane<const GENERAL: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if (returns && !returning_call_admitted(cx)) || vm.with_scope_entered {
        return Err(exit(ip));
    }
    let argc = ImmediateLayout::argument_count(i.imm());
    let count = usize::from(target.registers);
    let window_size = quench_runtime::staged::call_keeping::keeping_window_words(target.registers, usize::from(argc), true);
    let room = fill_room::<ACTIVATION_FILL_BLOCK>(window_size).max(usize::from(target.room));
    if !quench_runtime::staged::call_keeping::concrete_admits_keeping(
        vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get(), vm.lane_stack.values.room(), room,
    ) {
        return Err(script_call_frame_in_lane(ip, room));
    }
    // SAFETY: the call's window of `r` holds its `argc` values; capacity holds the window
    // and the kept values after it, each written before the length covers it. The registers
    // are the admitted fill's (`fill_window_in_lane`), the kept values the keeping row's.
    let callee_registers = unsafe {
        let passed = r.0.add(usize::from(arguments)).cast_const();
        let registers = fill_window_in_lane(vm, target, passed, argc.min(target.params));
        quench_runtime::staged::call_keeping::keep_passed(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()), registers.0.add(count).expose_provenance() as u64,
            passed.expose_provenance() as u64, argc,
        );
        vm.lane_stack.values.set_top(registers.0.add(window_size + lane_values::CONTEXT));
        registers
    };
    Ok(enter_window_in_lane::<GENERAL, false>(cx, r, ip, target, callee_registers, argc, this, false, None, Some(target.registers)))
}
