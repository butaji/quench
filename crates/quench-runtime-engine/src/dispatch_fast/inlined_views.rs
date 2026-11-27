//! The inlined views of the lane (N7): a function's second record array, derived from its inlining
//! unit (`compiled::lane_unit::unit`), whose records run the callees its call sites link in place.
//! The function's own view stays: it owns every per-PC table, and a run in the inlined view leaves
//! to it at any caller origin (`record_pc`). This registry maps an inlined record to its origin by
//! the record's address, so a slow path's PC is the original's whichever view the run is in.

#[allow(unused_imports)]
use crate::ext_traits::*;
#[allow(unused_imports)]
use super::*;
use crate::compiled::lane_unit::{InlinedCall, Origin};
use quench_runtime::vm::engine_types::{Ip, LaneView};
use std::cell::{Cell, RefCell};

/// One inlined view: its records' address range, the origin of each record, the frames it inlines,
/// and the function's own view.
pub(crate) struct InlinedView {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) origins: Box<[Origin]>,
    pub(crate) frames: Box<[InlinedCall]>,
    pub(crate) caller_registers: u16,
    pub(crate) view: *const LaneView,
    pub(crate) original: *const LaneView,
}

thread_local! {
    /// The lane's inlined views, by the address of their first record.
    static INLINED: RefCell<Vec<InlinedView>> = const { RefCell::new(Vec::new()) };
    /// The span every inlined view's records lie in (`(usize::MAX, 0)` while there are none): one
    /// test tells a record of no inlined view.
    static SPAN: Cell<(usize, usize)> = const { Cell::new((usize::MAX, 0)) };
}

/// Registers `inlined`, whose records the program store keeps alive with the original's.
pub(crate) fn register(inlined: InlinedView) {
    SPAN.with(|span| {
        let (low, high) = span.get();
        span.set((low.min(inlined.start), high.max(inlined.end)));
    });
    INLINED.with(|views| {
        let mut views = views.borrow_mut();
        let at = views.partition_point(|view| view.start < inlined.start);
        views.insert(at, inlined);
    });
}

/// Whether `ip` may be a record of an inlined view (one test against the span).
#[inline(always)]
pub(crate) fn may_be_inlined(ip: Ip) -> bool {
    let address = ip.addr();
    SPAN.with(|span| {
        let (low, high) = span.get();
        address >= low && address < high
    })
}

/// `f` of the inlined view `ip` is a record of, and the record's index there; `None` for a record of
/// no inlined view.
#[cold]
pub(crate) fn with_inlined<R>(ip: Ip, f: impl FnOnce(&InlinedView, usize) -> R) -> Option<R> {
    let address = ip.addr();
    INLINED.with(|views| {
        let views = views.borrow();
        let at = views.partition_point(|view| view.start <= address).checked_sub(1)?;
        let view = &views[at];
        (address < view.end).then(|| f(view, (address - view.start) / size_of::<quench_runtime::vm::engine_types::LaneInstruction>()))
    })
}

/// The original PC of the record at `ip` of an inlined view: a caller origin's own PC, an inlined
/// origin's outermost call (the activation the original view would be at). `None` for a record of no
/// inlined view.
#[cold]
pub(crate) fn original_pc(ip: Ip) -> Option<usize> {
    with_inlined(ip, |view, index| match view.origins[index] {
        Origin::Caller(pc) => pc as usize,
        Origin::Inlined { frame, .. } => {
            let mut frame = frame;
            while let Some(parent) = view.frames[frame].parent { frame = parent; }
            view.frames[frame].call as usize
        }
    })
}

/// The inlined view a function's own view `original` runs new activations in, if one is made.
pub(crate) fn inlined_of(original: *const LaneView) -> Option<*const LaneView> {
    INLINED.with(|views| views.borrow().iter().find(|view| std::ptr::eq(view.original, original)).map(|view| view.view))
}

/// Whether `function` of `code` is a callee N7a inlines: a leaf (no call), straight-line (no
/// backward jump), short, of no handler, generator or async body.
fn leaf_callee(code: &quench_runtime::ResidualProgram, function: u32) -> bool {
    const MAX_LEAF_INSTRUCTIONS: usize = 16;
    let Some(body) = code.function_get(function as usize) else { return false };
    body.handlers.is_empty() && !body.is_generator && !body.is_async && body.code.len() <= MAX_LEAF_INSTRUCTIONS
        && body.instructions().enumerate().all(|(pc, instruction)| !quench_runtime::staged::calls(instruction.op())
            && (instruction.op().immediate_role() != quench_runtime::bytecode::ImmediateRole::JumpTarget || instruction.imm() as usize > pc))
}

/// Makes and registers the inlined view of `function` of the running program, whose own view is
/// `original` (N7a): its unit (`lane_unit::unit`) splicing in the leaf callees its call sites link,
/// derived as the function's second record array, its call markers made `inlined_call_marker`
/// records. The view and its unit's register count; `None`, registering nothing, where it inlines
/// no call or the function is one N7a leaves (handlers, generators, async, dynamic scopes).
pub(crate) fn make_inlined_view(cx: *mut super::LaneContext, function: u32, original: *const LaneView) -> Option<(*const LaneView, u16)> {
    // SAFETY: the running lane's context; its program outlives the run.
    let (vm, code, program) = unsafe { (super::vm(cx), &*(*cx).code, (*cx).program) };
    let body = code.function_get(function as usize)?;
    if !body.handlers.is_empty() || body.is_generator || body.is_async || body.has_dynamic_scope_access() { return None; }
    let targets = |caller: u32, pc: u32| -> Vec<crate::compiled::CallTarget> {
        if caller != function { return Vec::new(); }
        super::unit_targets_in_lane(cx, caller, pc).into_iter()
            .filter(|target| matches!(target.callee, crate::compiled::CalleeBinding::Identity(_)) && leaf_callee(code, target.function))
            .take(1)
            .collect()
    };
    let graph = vm.programs.call_graph(vm.active_program);
    let unit = crate::compiled::lane_unit::unit(code, graph.as_deref(), function, body, &targets, &|callee| code.function_get(callee as usize))?;
    if unit.frames.is_empty() || unit.registers > u16::from(u8::MAX) { return None; }
    // An inlined callee's stores to `this` store to its receiver register, which is the frame's
    // `this` the unit's other records read (`lane_unit` renames field bases, not these).
    let mut unit = unit;
    for (pc, origin) in unit.origins.iter().enumerate() {
        let Origin::Inlined { frame, .. } = *origin else { continue };
        let instruction = unit.instructions[pc];
        if instruction.stores_to_this() {
            let op = if instruction.op().field_store().is_some_and(|store| store.strict) {
                quench_runtime::bytecode::Op::SetFieldStrict
            } else {
                quench_runtime::bytecode::Op::SetField
            };
            unit.instructions[pc] = quench_runtime::bytecode::WideInstruction::new(op, instruction.a(), unit.frames[frame].receiver, instruction.c(), instruction.imm());
        } else if instruction.op() == quench_runtime::bytecode::Op::GetField && instruction.b() == quench_runtime::bytecode::FieldBase::NESTED {
            // A site's base is the callee's: no inlined view reads through one.
            return None;
        }
    }
    let masks = unit.register_masks(code, function).map(|masks| std::rc::Rc::new(masks.into_owned()));
    let synthetic = body.with_unit_code(&unit.instructions, unit.registers)?;
    let (heap, tiers) = (vm.heap_reads(), vm.programs.tiers());
    let view = vm.programs.store_extra_lane_view(program, function, |_, constants, kind, sites| {
        // The unit's pcs are not the function's, so the solve's cells at a pc do not describe these records.
        Some(super::derive_lane_view(function, &synthetic, constants, heap, kind, &|| masks.as_ref(), sites,
            crate::compiled::admission::record_plan(false), false, tiers, None, Vec::new()))
    })?;
    // SAFETY: the program store keeps the view alive with its program.
    let derived = unsafe { &*view };
    for (frame, &marker) in unit.frames.iter().zip(&unit.markers) {
        // SAFETY: the marker is a record of the view, which no run has entered yet.
        unsafe { mark_call(&mut *std::cell::UnsafeCell::raw_get(&derived.records[marker]), frame, code) };
    }
    // Its activations' facts: the function's, over the unit's registers, every one filled.
    // SAFETY: the original view outlives the run.
    let facts = quench_runtime::staged::call::CellFacts::of_word(unsafe { &*original }.entry.facts.get());
    derived.entry.set_facts(quench_runtime::staged::call::CellFacts::of_function(unit.registers, facts.params, u16::MAX,
        u64::from(facts.entry) == quench_runtime::staged::call::ENTRY_ACTIVATES));
    let start = derived.records().addr();
    let end = start + derived.records.len() * size_of::<quench_runtime::vm::engine_types::LaneInstruction>();
    register(InlinedView {
        start, end, origins: unit.origins.into_boxed_slice(), frames: unit.frames.into_boxed_slice(),
        caller_registers: unit.caller_registers, view, original,
    });
    Some((view, unit.registers))
}

/// Makes `record`, the marker of inlined `frame`, its `inlined_call_marker`: the callee register in
/// `a`, the call's window in `b`, the callee's first register in `c`; the values passed, the
/// callee's registers and the receiver register in the immediate; the callee's identity in `mode`.
/// A call whose values do not land in its callee's first registers in order, or that passes
/// through `Function.prototype.call`, always leaves at its marker.
fn mark_call(record: &mut quench_runtime::vm::engine_types::LaneInstruction, frame: &InlinedCall, code: &quench_runtime::ResidualProgram) {
    let leave = || leave_inlined as Handler as *const ();
    let Some(callee) = code.function_get(frame.target.function as usize) else { return record.handler = leave() };
    let passed = usize::from(frame.arguments.min(callee.params));
    let in_order = frame.passed_registers(callee).enumerate()
        .all(|(k, (home, value))| usize::from(home) == usize::from(frame.base) + k && usize::from(value) == usize::from(frame.window) + k)
        && frame.passed_registers(callee).count() == passed;
    let identity = frame.target.identity();
    match identity {
        // Registers past the callee's entry fill are written before any read (`entry_fill`).
        Some(identity) if in_order && frame.function_call.is_none() && frame.registers <= u16::from(u8::MAX) => {
            let fill = code.entry_fill(callee).clamp(passed as u16, frame.registers);
            record.handler = inlined_call_marker as Handler as *const ();
            record.a = frame.callee_register;
            record.b = frame.window;
            record.c = frame.base;
            record.operand.imm = passed as u32 | u32::from(fill) << 8 | u32::from(frame.receiver) << 16;
            record.mode = identity.to_bits();
        }
        _ => record.handler = leave(),
    }
}

/// Where the run continues once it leaves the inlined record at `ip` with registers `r`: at a caller
/// origin, the same PC of the function's own records; inside an inlined frame, the frames its
/// calls make (`body_inlined_activation_in_lane`), at the callee's own record.
pub(crate) fn leave(cx: *mut super::LaneContext, r: super::Registers, ip: Ip) -> (super::Registers, Ip, super::Carried) {
    let (origin, original, frames, caller_registers) = with_inlined(ip, |view, index| (view.origins[index], view.original, view.frames.to_vec(), view.caller_registers))
        .expect("a leave is of an inlined record");
    // SAFETY: the function's own view outlives the run.
    let records = unsafe { (*original).records() };
    // SAFETY: the running lane's context.
    unsafe { (*cx).base = records };
    match origin {
        // SAFETY: the original view holds a record for each of the function's PCs.
        Origin::Caller(pc) => (r, unsafe { records.add(pc as usize) }, super::carried_at_in_lane(cx, original, r, pc)),
        Origin::Inlined { frame, pc } => {
            let _ = caller_registers;
            materialize(cx, r, records, &frames, frame, pc,
                |vm, code, callee| vm.resolve_script_call_target(code, callee),
                |call, _, sink| {
                    for register in 0..call.registers {
                        sink(register, r.get(call.base + register));
                    }
                })
        }
    }
}

/// The frame `frame` and the frames around it, innermost first, along the parent links of `frames`
/// (`lane_unit::Unit::chain` is this walk, outermost first, over the unit's own frames).
fn frame_chain(frames: &[InlinedCall], frame: usize) -> Vec<usize> {
    let mut chain = vec![frame];
    while let Some(parent) = frames[*chain.last().expect("a chain has a frame")].parent {
        chain.push(parent);
    }
    chain
}

/// One frame meaning (C4): the interpreter activations that the inlined frames of a unit mean at an
/// exit, built from the unit's origins (the frame chain, each call's PC, its callee and its register
/// window) at the moment control leaves. Compiled code never writes the interpreter frame ahead: the
/// activations exist only once control leaves, and closed-world units have no exits, so this never
/// runs for them.
///
/// The activations are the calls of inlined `frame` and the frames around it, outermost first
/// (`push_passed_in_lane`), the innermost resuming at its record `pc`. `base` holds the records of the
/// function the unit leaves from, where the outermost frame's call sits. Each frame's callee is its
/// call's identity, or the function its callee register holds (a closed-world frame names no closure);
/// a cache miss resolves it through `resolve`. The tier supplies only what it keeps: `resolve` (how a
/// miss resolves a callee) and `leaving`, which hands a frame's registers to `sink` as the leaving tier
/// holds them, at the leave's PC `at` in the frame's function (the interpreter's window, or the compiled
/// body's spilled values).
pub(crate) fn materialize(
    cx: *mut super::LaneContext,
    r: super::Registers,
    base: Ip,
    frames: &[InlinedCall],
    frame: usize,
    pc: u32,
    resolve: impl Fn(&mut super::Vm, &quench_runtime::ResidualProgram, Value) -> Option<quench_runtime::vm::engine_types::ScriptCallTarget>,
    leaving: impl Fn(&InlinedCall, u32, &mut dyn FnMut(u16, Value)),
) -> (super::Registers, Ip, super::Carried) {
    let chain = frame_chain(frames, frame);
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let (mut registers, mut records, mut view) = (r, base, std::ptr::null::<LaneView>());
    for (depth, &index) in chain.iter().enumerate().rev() {
        let call = frames[index];
        let vm = unsafe { super::vm(cx) };
        let parent = call.parent_base(frames);
        let callee = call.target.identity().unwrap_or_else(|| registers.get(call.callee_register - parent));
        // SAFETY: copy the cache row before anything changes the cache.
        let target = vm.script_call_entry(callee).map(|target| unsafe { *target })
            .or_else(|| resolve(&mut *vm, code, callee))
            .unwrap_or_else(|| panic!("an inlined callee resolves: {:#x} (frame {index} of {}, callee register {}, parent {parent}, function {}, call {})",
                callee.to_bits(), frames.len(), call.callee_register, call.target.function, call.call));
        // SAFETY: the record is the call's in its parent's records.
        let record = unsafe { records.add(call.call as usize) };
        let this = super::call_this_in_lane(vm, &target, registers.get(call.receiver - parent)).expect("a direct-object receiver binds itself");
        // SAFETY: the call's window of `registers` holds its values.
        let passed = unsafe { registers.0.add(usize::from(call.window - parent)).cast_const() };
        let (callee_registers, _) = super::push_passed_in_lane::<false, false>(cx, registers, record, &target, passed, call.arguments, this, false);
        // The leave's PC in this frame's function: the inner frame's call, or the leave's own PC for the innermost.
        let at = match depth.checked_sub(1) { Some(inner) => frames[chain[inner]].call, None => pc };
        leaving(&call, at, &mut |register: u16, value: Value| callee_registers.set(register, value));
        // SAFETY: the running lane's context, whose base the push made the callee's records.
        (registers, records, view) = (callee_registers, unsafe { (*cx).base }, target.view);
    }
    // SAFETY: the innermost activation's view holds a record for each of its function's PCs.
    (registers, unsafe { records.add(pc as usize) }, super::carried_at_in_lane(cx, view, registers, pc))
}

/// The record the general path runs for a lane exit at `ip`, a record of an inlined view: the
/// function's own, once the run leaves the view (`leave`).
#[cold]
#[inline(never)]
pub(crate) fn leave_at_exit(cx: *mut super::LaneContext, ip: Ip) -> Ip {
    // The span holds other records too.
    if with_inlined(ip, |_, _| ()).is_none() { return ip; }
    // SAFETY: the running lane's context; its registers are the inlined activation's.
    let r = unsafe { (*cx).registers };
    leave(cx, r, ip).1
}

/// Makes and installs the inlined view of the function running the leaf call at `ip`, once the
/// call's link is proved (`script_call_leaf_prove_miss`), with every site linked so far, once per
/// function. The interpreter only (no compiled tier claims the view's PCs), and
/// only from the function's own records.
#[cold]
pub(crate) fn note_leaf_site(cx: *mut super::LaneContext, ip: Ip) {
    // SAFETY: the running lane's context.
    if unsafe { (*cx).tiers_run } || may_be_inlined(ip) || !unsafe { (*cx).leaf.call }.is_null() { return; }
    let function = super::running_function_in_lane(cx);
    // SAFETY: the program's views outlive the lane run.
    let Some(original) = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view) else { return };
    let start = original.records().addr();
    let end = start + original.records.len() * size_of::<quench_runtime::vm::engine_types::LaneInstruction>();
    if !(start..end).contains(&ip.addr()) || original.entry.facts.get() == 0 { return; }
    let original: *const LaneView = original;
    // A function keeps one inlined view: the program arena holds every view it stores for the
    // program's life, so a view made anew per proved site would hold each earlier one too.
    if inlined_of(original).is_some() { return; }
    let Some((view, registers)) = make_inlined_view(cx, function, original) else { return };
    install(cx, function, original, view, registers);
}

/// Points every cached call target of `function` at its inlined `view` of `registers` registers: the
/// calls made from now on activate in it; running activations keep their records.
fn install(cx: *mut super::LaneContext, function: u32, original: *const LaneView, view: *const LaneView, registers: u16) {
    let ours = |candidate: *const LaneView| std::ptr::eq(candidate, original)
        || INLINED.with(|views| views.borrow().iter().any(|inlined| std::ptr::eq(inlined.view, candidate) && std::ptr::eq(inlined.original, original)));
    let Ok(room) = u16::try_from(super::fill_room::<{ super::SCRIPT_FILL_BLOCK }>(usize::from(registers))) else { return };
    // SAFETY: the running lane's context.
    let (vm, program) = unsafe { (super::vm(cx), (*cx).program) };
    let stack = &mut vm.lane_stack;
    for target in stack.script_targets.entries.iter_mut().chain(stack.function_targets.iter_mut()) {
        if target.function == function && target.program() == program && !target.view.is_null() && ours(target.view) {
            target.view = view;
            target.registers = registers;
            target.room = room;
            target.fill = u8::try_from(registers).unwrap_or(u8::MAX);
        }
    }
}

script_handler! {
    /// The call marker of an inlined frame (`mark_call`): where the callee register holds the frame's
    /// callee and the receiver binds itself (a direct object), the values passed move to the callee's
    /// first registers, its other registers live at entry start `undefined`, and the callee's records
    /// run next.
    /// Any other call leaves at the call (`leave_inlined`), before any of its effects.
    pub(crate) fn inlined_call_marker<>(cx, r, ip, i, view) {
        let receiver = r.get((i.imm() >> 16) as u16);
        if r.get(i.a).to_bits() != i.mode
            || !quench_runtime::staged::value::is_direct_object(&mut quench_runtime::staged::Concrete::pure(), receiver.to_bits())
        {
            sibling!(leave_inlined as Handler, r, ip, view)
        }
        let (passed, fill) = ((i.imm() & 0xff) as u16, ((i.imm() >> 8) & 0xff) as u16);
        for k in 0..passed {
            r.set(i.c + k, r.get(i.b + k));
        }
        for k in passed..fill {
            r.set(i.c + k, Value::UNDEFINED);
        }
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// Leaves the inlined record at `ip` for the function's own records (`leave`): a marker whose call
    /// is another, and any miss of an inlined callee's record.
    #[cold]
    #[inline(never)]
    pub(crate) fn leave_inlined<>(cx, r, ip, i, view) {
        let (r, ip, carried) = leave(cx, r, ip);
        next!(cx, r, ip, carried)
    }
}
