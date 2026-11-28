//! Functions of `dispatch_fast` in a codegen unit of their own (B1: a staged edit recompiles only the units that inline it).
#![allow(unused_imports)]
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// Whether a leaf of `form` runs with its receiver bound (`THIS_LEAF`, `CONTEXT_LEAF`).
#[inline(always)]
pub(super) const fn leaf_binds_this(form: LeafForm) -> bool {
    quench_runtime::staged::call_leaf::binds_this(form)
}

/// The `LaneStack::noted` entry of a link address and callee function.
#[inline(always)]
pub(super) fn noted_slot(address: usize, function: u32) -> usize {
    ((address >> 1) ^ (function as usize).wrapping_mul(0x9E37)) & (NOTED_SITE_CALLEES - 1)
}

/// The script target index a call link names, with or without `PLAIN_LINK`.
#[inline(always)]
pub(super) fn link_index(link: u16) -> usize { usize::from(link & !PLAIN_LINK) }

/// Conservative lane-stack admission room for `count` registers: whole blocks, at
/// least one. Only the logical range is initialized and made live.
pub(super) const fn fill_room<const BLOCK: usize>(count: usize) -> usize {
    let room = count.next_multiple_of(BLOCK);
    if room < BLOCK { BLOCK } else { room }
}

/// The start of a plain activation of `count` registers, whose admission reserved whole
/// `SCRIPT_FILL_BLOCK`s past them (`ScriptCallTarget::room`), every one of the first `fill`
/// `undefined`: a `fill` within one `WINDOW_FILL_BLOCK` takes that block's fixed stores, the
/// values past it landing in registers no instruction reads before writing or in the reserved
/// room, so it makes no test of its count; a longer one fills all `count`.
///
/// # Safety
/// `registers` has writable storage for `fill_room::<SCRIPT_FILL_BLOCK>(count)` values, none
/// of them live, and `fill` covers every register the activation's function reads before
/// writing (`ResidualProgram::entry_fill`).
#[inline(always)]
pub(super) unsafe fn fill_admitted_window(registers: *mut Value, fill: usize, count: usize) {
    const _: () = assert!(WINDOW_FILL_BLOCK <= SCRIPT_FILL_BLOCK);
    if fill > WINDOW_FILL_BLOCK {
        // SAFETY: the function's contract.
        return unsafe { fill_window(registers, count, Value::UNDEFINED) };
    }
    for index in 0..WINDOW_FILL_BLOCK {
        // SAFETY: the admitted room holds a whole block.
        unsafe { registers.add(index).write_unaligned(Value::UNDEFINED) };
    }
}

/// The registers of an activation of `function` that start initialized: those below its entry
/// liveness bound. A body whose frames a collection scans whole (`RegisterMasks::too_large_to_keep`)
/// keeps every register initialized, as its masks are not kept.
pub(crate) fn entry_fill(code: &quench_runtime::bytecode::ResidualProgram, function: &Function) -> u16 {
    if quench_runtime::compile::liveness::RegisterMasks::too_large_to_keep(function) {
        function.registers
    } else {
        code.entry_fill(function)
    }
}

/// `entry_fill` where only the function is at hand: its bound where derived already, else every
/// register (a fill that covers all is always sound).
pub(super) fn entry_fill_known(function: &Function) -> u16 {
    match function.register_roots.entry_live_known() {
        Some(live) if !quench_runtime::compile::liveness::RegisterMasks::too_large_to_keep(function) => live.min(function.registers),
        _ => function.registers,
    }
}

/// Switches the record at `ip` to `handler` for the rest of its view's life, at an edge only: a handler is chosen once at
/// load from the facts (`derive_lane_view`) and execution never rewrites it. The edges that may are the realm's landing of
/// a probed name on the global object (`settle_name_record`) and the loop site's admission edge (`loop_retire_in_lane`).
/// Every handler a record may switch between computes the same result.
#[inline(always)]
pub(super) fn edge_rewrite(ip: Ip, handler: Handler) {
    // SAFETY: `ip` comes from `LaneView::records`, whose records live in `UnsafeCell`s; the
    // lane runs on the view's thread, and no reference to the handler field is live.
    unsafe {
        // A record whose result carries flags keeps its edge's general step (`derive_lane_view`).
        if (*ip).handler == general_step_in_lane as Handler as *const () && (*ip).a & !quench_runtime::bytecode::REGISTER_MASK != 0 {
            return;
        }
        // A numeric local target's row (`FLAG_LOCAL_RESULT`) is the only row that reads such an `a`: any other
        // handler the record would take is the general step.
        if is_local_result_row((*ip).handler) && !is_local_result_row(handler as *const ()) {
            (*ip.cast_mut()).handler = general_step_in_lane as Handler as *const ();
            return;
        }
        // A trusted row stays trusted: its view's `this` trust was fixed at derivation (`receiver_trusted`).
        let trusted = trusted_this_form((*ip).handler).is_some();
        let next = match trusted_this_form(handler as *const ()) { Some(form) if trusted => form, _ => handler as *const () };
        (*ip.cast_mut()).handler = next;
    }
}

/// The record index of `ip` in the running view: the PC its slow paths read the function's
/// instruction, sites and frames at. The one place a record's PC is computed (`record_pc`), so a
/// view whose records are not its function's one per PC (an inlining view, N7) maps them here.
///
/// # Safety
/// `cx` is the running lane's context and `ip` a record of its view.
#[inline(always)]
pub(crate) unsafe fn record_pc(cx: *const LaneContext, ip: Ip) -> usize {
    // SAFETY: the function's contract: the view's records start at the context's base.
    unsafe { pc_in_view((*cx).base, ip) }
}

/// The PC of the record at `ip` of the view whose records start at `base` (`record_pc` for a view
/// that is not the running one: an activation's caller, a region's).
///
/// # Safety
/// `ip` is a record of the view whose records start at `base`.
#[inline(always)]
pub(crate) unsafe fn pc_in_view(base: Ip, ip: Ip) -> usize {
    // A record of an inlined view (N7) is at its origin's PC, whichever records `base` starts.
    if inlined_views::may_be_inlined(ip) && let Some(pc) = inlined_views::original_pc(ip) {
        return pc;
    }
    // SAFETY: the function's contract.
    unsafe { ip.offset_from(base) as usize }
}

/// The header word `index` (a `staged::frame` `CONTEXT_*`) of the window at `registers`: the words count
/// down from register 0, so `sub(index)` words below it.
///
/// # Safety
/// `registers` is a live window, with the frame's context words below it.
#[inline(always)]
pub(crate) unsafe fn frame_word(registers: *const Value, index: usize) -> u64 {
    // SAFETY: as the caller promises.
    unsafe { *registers.cast::<u64>().sub(index) }
}

/// Writes the header word `index` of the window at `registers` (`frame_word`).
///
/// # Safety
/// As `frame_word`, and the caller owns the window's header.
#[inline(always)]
pub(crate) unsafe fn set_frame_word(registers: *mut Value, index: usize, word: u64) {
    // SAFETY: as the caller promises.
    unsafe { *registers.cast::<u64>().sub(index) = word }
}

/// The largest script call cache `policy` allows.
pub(super) fn script_call_targets(policy: quench_runtime::MemoryPolicy) -> usize {
    SCRIPT_CALL_TARGETS[policy as usize]
}

/// Drops the entries of `held` that activations from index `depth` on own. Activations push
/// their entries innermost last, so the owners ascend and those entries are a suffix: the
/// common case, none, is one comparison.
#[inline(always)]
pub(super) fn drop_owned_entries<T>(held: &mut Vec<(usize, T)>, depth: usize) {
    if held.last().is_some_and(|(owner, _)| *owner >= depth) {
        let first = held.partition_point(|(owner, _)| *owner < depth);
        held.truncate(first);
    }
}

/// Drops the storage pointers a lane context resolved before a collection, as
/// `after_nested_collection_in_lane` does after an in-place operation collected: the run's
/// capture storage (whose `CAPTURE_STORES` tag let stores skip the barrier until the next
/// collection), its last outer capture walk, a frame's captured-locals storage and a Wasm
/// instance's slots. Each is found again by the path that found it first. The context's
/// stack is retired by the collector itself (`LaneStack::retire_after_collection`).
pub(super) unsafe fn retire_lane_binding(context: *mut ()) {
    let cx = context.cast::<LaneContext>();
    // SAFETY: the prefix's callback was installed for this context's Host; the context is
    // live while it is the thread's running or a suspended outer one.
    unsafe {
        (*cx).registers.set_capture(std::ptr::null());
        (*cx).outer = NO_OUTER_WALK;
        (*cx).root_walk = NO_OUTER_WALK;
        (*cx).frame_storage = NO_FRAME_STORAGE;
        (*cx).instance_slots = (Value::DELETED, std::ptr::null());
    }
}

/// Retires the thread's running lane context after a collection ran under it: a native call
/// out of the lane (`gc()`, a host callback) or an allocation that made one due. The run
/// continues past the call with nothing it resolved under the epoch before.
pub(crate) fn retire_running_lane_context() {
    let cx = tls_lane_context();
    if cx.is_null() {
        return;
    }
    // SAFETY: every active context has the repr(C) `LaneBinding` prefix, whose callback
    // owns its actual Host type.
    unsafe { (cx.cast::<LaneBinding>().as_ref().unwrap_unchecked().retire)(cx) };
}

pub(super) unsafe fn resume_lane_binding(context: *mut ()) {
    let cx = context.cast::<LaneContext>();
    // SAFETY: the prefix's callback was installed for this context's Host.
    let base = unsafe { vm(cx) }.heap_reads().object_record_base().unwrap_or(std::ptr::null());
    unsafe { (*cx).binding.object_base = base };
}

/// The storage of the running frame's captured environment (`LaneContext::frame_storage`),
/// for a frame run of a function other than the root whose frame holds one; `None` leaves
/// the access to the heap.
#[inline(always)]
pub(super) fn frame_storage_in_lane(cx: *mut LaneContext) -> Option<&'static mut quench_runtime::tier_surface::EnvironmentSlots> {
    let vm = unsafe { vm(cx) };
    let index = unsafe { (*cx).frame() };
    let frame = &vm.frames[index];
    if !frame.captured || frame.function == quench_runtime::vm::ROOT_FUNCTION_ID {
        return None;
    }
    let known = unsafe { (*cx).frame_storage };
    let slots = if known.frame == index && known.env == frame.env {
        known.slots
    } else {
        find_frame_storage_in_lane(cx, index, frame.env)
    };
    // SAFETY: as `FrameStorage`; the lane holds no other reference to the storage.
    unsafe { slots.as_mut() }
}

/// `frame_storage_in_lane` past its cache: the environment's storage, recorded.
#[cold]
#[inline(never)]
pub(super) fn find_frame_storage_in_lane(
    cx: *mut LaneContext,
    frame: usize,
    env: Value,
) -> *mut quench_runtime::tier_surface::EnvironmentSlots {
    let vm = unsafe { vm(cx) };
    let slots = match vm.environment_mut(env) {
        Some(quench_runtime::tier_surface::EnvironmentRecord { slots, .. }) => std::ptr::from_mut(slots),
        _ => std::ptr::null_mut(),
    };
    unsafe { (*cx).frame_storage = FrameStorage { frame, env, slots } };
    slots
}

/// The slots a `LaneContext::capture` reads, its tag cleared.
#[inline(always)]
pub(crate) fn capture_reads(capture: *const usize) -> *const usize {
    capture.map_addr(|address| address & !CAPTURE_STORES)
}

/// The local slots (address and count) of the VM frame the lane context `cx` runs, as the lane's handlers read
/// them (`lane_frame::staged_load_local_plain`); none where there is no context (a test's entry).
///
/// # Safety
/// `cx` is null or a live `LaneContext`.
pub(super) unsafe fn frame_locals(cx: *mut ()) -> (*mut Value, usize) {
    std::ptr::NonNull::new(cx.cast::<LaneContext>()).map_or((std::ptr::null_mut(), 0), |cx| {
        // SAFETY: the caller's contract; the VM and its frame outlive the lane run.
        let (frames, root) = unsafe { (&mut vm(cx.as_ptr()).frames, (*cx.as_ptr()).root) };
        // A job's activations run in no frame (`LaneRoot::Job`).
        root.frame().map_or((std::ptr::null_mut(), 0), |frame| (frames[frame].locals.as_mut_ptr(), frames[frame].locals.len()))
    })
}

/// The frame slot `local` a numeric local target writes (`WideInstruction::numeric_local_target`), as the
/// VM's completion writes it (`Vm::run_row_completed`, the frame's own locals): in a frame run only (its
/// registers hold the deleted environment marker), not a root body's. `None` elsewhere: a frameless
/// activation's slot is the general step's.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(crate) unsafe fn numeric_local_slot(cx: *mut LaneContext, local: u16) -> Option<*mut Value> {
    if !unsafe { (*cx).registers }.env().is_deleted() {
        return None;
    }
    let vm = unsafe { vm(cx) };
    let frame = &mut vm.frames[unsafe { (*cx).frame() }];
    if frame.function == ROOT_FUNCTION_ID {
        return None;
    }
    frame.locals.get_mut(usize::from(local)).map(std::ptr::from_mut)
}

/// The slots a `LaneContext::capture` tagged `CAPTURE_STORES` lets a store write.
#[inline(always)]
pub(crate) fn capture_stores(capture: *const usize) -> Option<*const usize> {
    (capture.addr() & CAPTURE_STORES != 0).then(|| capture_reads(capture))
}

/// The dense slot view of the environment a `LaneContext::capture` names (`None` where it has
/// none), and whether that capture admits direct stores (`capture_stores`).
///
/// # Safety
/// `capture` is null or names a live environment's storage, which the caller keeps live.
#[inline(always)]
pub(crate) unsafe fn capture_view(capture: *const usize) -> (Option<(*const Value, usize)>, bool) {
    // SAFETY: as the caller promises.
    (unsafe { quench_runtime::tier_surface::EnvironmentSlots::native_storage_view(capture_reads(capture)) }, capture_stores(capture).is_some())
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[inline(always)]
pub(super) fn tls_lane_context() -> *mut () {
    let value: *mut ();
    // SAFETY: the executable's own TLS block; local-exec offset fixed at link time.
    unsafe { std::arch::asm!("mov {}, qword ptr fs:[{}@TPOFF]", out(reg) value, sym LANE_CONTEXT, options(pure, nostack, readonly, preserves_flags)) };
    value
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
#[inline(always)]
pub(super) fn tls_lane_context() -> *mut () {
    LANE_CONTEXT.0.get()
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[inline(always)]
pub(super) fn tls_lane_fuel() -> *mut u32 {
    let value: *mut u32;
    // SAFETY: as `tls_lane_context`.
    unsafe { std::arch::asm!("mov {}, qword ptr fs:[{}@TPOFF]", out(reg) value, sym LANE_FUEL, options(pure, nostack, readonly, preserves_flags)) };
    value
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
#[inline(always)]
pub(super) fn tls_lane_fuel() -> *mut u32 {
    LANE_FUEL.0.get()
}

/// Spend one unit of the running lane's fuel; true when it was out, so the backward jump
/// takes `safepoint_at`. One decrement of a context field and one branch: the safepoint
/// test of every backward jump, in place of the stack-floor compare.
#[inline(always)]
pub(super) fn script_fuel_out(cx: *mut LaneContext) -> bool {
    // SAFETY: the run's context, which only this thread touches.
    unsafe {
        let (left, out) = (*cx).fuel.overflowing_sub(1);
        (*cx).fuel = left;
        out
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[inline(always)]
pub(super) fn tls_lane_floor() -> *const std::sync::atomic::AtomicUsize {
    let value: *const std::sync::atomic::AtomicUsize;
    // SAFETY: as `tls_lane_context`.
    unsafe { std::arch::asm!("mov {}, qword ptr fs:[{}@TPOFF]", out(reg) value, sym LANE_FLOOR, options(pure, nostack, readonly, preserves_flags)) };
    value
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
#[inline(always)]
pub(super) fn tls_lane_floor() -> *const std::sync::atomic::AtomicUsize {
    LANE_FLOOR.0.get()
}

/// The running lane's floor.
#[inline(always)]
pub(super) fn floor_in_lane() -> &'static std::sync::atomic::AtomicUsize {
    // SAFETY: set for the whole of a lane run to its VM's interrupt state, which the VM
    // holds; only read while a lane runs.
    unsafe { &*tls_lane_floor() }
}

/// The current stack address.
#[cfg(quench_lane_tail_calls)]
#[inline(always)]
pub(super) fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reads the stack pointer register; no memory, stack or flags.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    // SAFETY: as above.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    sp
}

/// Without sibling calls every handler returns to the lane loop, so a run's
/// stack cannot grow; this address lies above every stack floor and below
/// the due-collection floor.
#[cfg(not(quench_lane_tail_calls))]
#[inline(always)]
pub(super) fn stack_pointer() -> usize {
    COLLECTION_DUE_FLOOR - 1
}

/// `below_floor` of a JavaScript record, whose context register carries the run's floor
/// (`LaneContext::floor`) without the thread-local load.
#[inline(always)]
pub(super) fn script_below_floor(cx: *mut LaneContext) -> bool {
    // SAFETY: the run's floor cell, which its VM holds for the whole run.
    stack_pointer() < unsafe { &*(*cx).floor }.load(std::sync::atomic::Ordering::Relaxed)
}

/// After a lane allocation, a due collection raises the floor of the running lane, where
/// one runs: a linked region's general frame polls the heap's own counters instead.
#[inline(always)]
pub(crate) fn note_allocation(vm: &Vm) {
    if vm.collection_due() {
        let floor = tls_lane_floor();
        // SAFETY: as `lane_floor`, while a lane runs; null otherwise.
        if let Some(floor) = unsafe { floor.as_ref() } {
            floor.store(COLLECTION_DUE_FLOOR, std::sync::atomic::Ordering::Relaxed);
        }
        // SAFETY: as `fuel_out`, while a lane runs; null otherwise.
        if let Some(fuel) = unsafe { tls_lane_fuel().as_mut() } {
            *fuel = 0;
        }
        vm.lane_stack.activation_limit.set(0);
        // The proved calls' bound on the plain depth (`PlainFrames::bound`) follows the limit.
        vm.lane_stack.plain.bound.set(0);
    }
}

/// The running activation's `this`; `None` where the general path decides it (a
/// derived constructor before `super()`).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(super) unsafe fn this_in_lane(cx: *mut LaneContext, r: Registers) -> Option<Value> {
    let this = r.this();
    if !this.is_deleted() {
        return Some(this);
    }
    unsafe { vm(cx).current_this((*cx).frame()) }
}

/// The running lane's context.
#[inline(always)]
pub(super) fn context_in_lane() -> *mut LaneContext {
    tls_lane_context().cast()
}

/// The general path executes the instruction at `ip`.
#[inline(always)]
pub(super) fn exit(ip: Ip) -> Ip {
    ip.map_addr(|address| address | EXIT_TAG)
}

/// The record a handler's result names, and whether the result leaves the lane there (`exit`).
#[inline(always)]
pub(super) fn exited(ip: Ip) -> (Ip, bool) {
    (ip.map_addr(|address| address & !EXIT_TAG), ip.addr() & EXIT_TAG != 0)
}

/// SAFETY: the context outlives the lane run and no other reference to the
/// VM is live while a handler runs.
#[inline(always)]
pub(crate) unsafe fn vm<'a>(cx: *mut LaneContext) -> &'a mut Vm {
    unsafe { &mut *(*cx).vm }
}

/// Sample the identity and body a script call site targets. Diagnostic builds use this to
/// determine whether a per-site target memo is worth implementing; production builds compile
/// every call to this helper away.
#[cfg(feature = "profile-lane-exits")]
#[inline(always)]
pub(super) fn profile_script_call_target(
    cx: *mut LaneContext,
    ip: Ip,
    callee: Value,
    target: &ScriptCallTarget,
) {
    let vm = unsafe { vm(cx) };
    let caller = vm
        .lane_stack
        .top_frame()
        .map_or_else(|| vm.frames[unsafe { (*cx).frame() }].function, |activation| {
            activation.function()
        });
    let pc = unsafe { record_pc(cx, ip) } as u32;
    quench_runtime::vm::lane_exits::call_target(
        unsafe { (*cx).program.raw() },
        caller,
        pc,
        callee,
        target.function,
    );
}

/// The handler of the record at `ip`.
#[inline(always)]
pub(super) fn handler(ip: Ip) -> Handler {
    #[cfg(feature = "profile-lane-exits")]
    quench_runtime::vm::lane_exits::count_record(ip.addr(), std::mem::size_of::<LaneInstruction>());
    // SAFETY: a view is only run by the VM type whose handlers built it, and
    // every reachable record is initialized.
    unsafe { std::mem::transmute::<*const (), Handler>((*ip).handler) }
}

/// The lane context record `OP`'s row runs in: the view's bytes, which `MemoryView::script` makes
/// that context, for a JavaScript record; the thread's for a Wasm one, whose view's bytes carry
/// no context (`Traits::WASM_OPCODE`). A JavaScript view's bytes are the assertion.
#[inline(always)]
pub(super) fn row_context<const OP: u16>(view: *mut LaneContext) -> *mut LaneContext {
    if const { matches!(Op::from_index(OP as usize), Some(op) if op.traits().has(quench_runtime::bytecode::Traits::WASM_OPCODE)) } {
        context_in_lane()
    } else {
        debug_assert_eq!(view, context_in_lane(), "a JavaScript view carries its context");
        view
    }
}

/// The shard (`sharded_handler!`) of a handler instance with const parameters `params`: 8 to 11 for a `numeric` row
/// (`numeric_row`), so an edit to the Number step recompiles only those units; any other keeps its shard of 0 to 7.
pub(crate) const fn handler_shard(params: &[u64], numeric: bool) -> usize {
    let mut hash = 0u64;
    let mut index = 0;
    while index < params.len() {
        hash = hash.wrapping_mul(31).wrapping_add(params[index]);
        index += 1;
    }
    if numeric { 8 + (hash % 4) as usize } else { (hash % 8) as usize }
}

/// Whether a row of opcodes `ops` runs the Number step (`staged::row_step`: Number, Arith, or Generic, whose
/// comparisons and keys convert numbers), which places its instance beside the other numeric rows
/// (`handler_shard`, `generated_row_shard`). A chain's absent link (`staged::NO_LINK`) names no opcode.
pub(crate) const fn numeric_row(ops: &[u16]) -> bool {
    use quench_runtime::staged::RowStep;
    let mut index = 0;
    while index < ops.len() {
        if let Some(op) = quench_runtime::bytecode::Op::from_index(ops[index] as usize) {
            if matches!(quench_runtime::staged::row_step(op), RowStep::Number | RowStep::Arith | RowStep::Generic) {
                return true;
            }
        }
        index += 1;
    }
    false
}

/// The value of register field `FIELD` of a record whose accumulator field
/// is `ACC`.
#[inline(always)]
pub(super) fn read<const ACC: u8, const FIELD: u8>(r: Registers, register: u16, view: Carried) -> Value {
    if ACC == FIELD {
        // SAFETY: the view selects an accumulator-reading record only after the producer
        // that set it.
        unsafe { view.acc.get() }
    } else {
        r.get(register)
    }
}

/// The record after `ip`.
#[inline(always)]
pub(super) fn following(ip: Ip) -> Ip {
    // SAFETY: validation ends every function in a terminator, so a lane
    // instruction that falls through has a successor.
    unsafe { ip.add(1) }
}

/// `ScopeReads::closure` of a window whose names do not resolve through its scope
/// (`FLAG_WINDOW_CLOSURE`): the closure over the running environment, as `window_closure_value_in_lane` makes it.
///
/// # Safety
/// As `lane_frame::ScopeReads`: `cx` is the running lane's context, `r` its register file.
pub(super) unsafe fn window_closure_into(cx: *mut (), r: Registers, _ip: Ip, function: u32, registers: u16) -> Option<Value> {
    let cx = cx.cast::<LaneContext>();
    if unsafe { (*cx).registers }.env().is_deleted() {
        return closure_value_of_in_lane(cx, function);
    }
    window_closure_value_in_lane(cx, r, function, registers)
}

/// `window_closure_into`'s closure of `function` over the running environment
/// activation's environment, whose window starts at register `registers`. Out of line, as
/// `compiled_new_object`: making the closure takes stack temporaries.
#[inline(never)]
pub(super) fn window_closure_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    function: u32,
    registers: u16,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // The innermost read where it is: a plain header's eval record is its side record's
    // (one-frame-one-call.md F4 (3)).
    let activation = vm.lane_stack.top_frame()?;
    let owner = activation.function();
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let parent = unsafe { (*cx).registers }.env();
    // A module's hoisted declaration is its instance's closure, which the interpreter returns.
    if owner == quench_runtime::vm::ROOT_FUNCTION_ID && vm.is_module_function(unsafe { (*cx).program }, function) {
        return None;
    }
    // A closure that reads nothing of its environment takes none, so the window is not made.
    if code.closure_takes_no_environment(function) {
        let value = closure_in_lane(vm, code, function, Value::NULL);
        note_allocation(vm);
        return value;
    }
    // SAFETY: the running environment activation's window, whose function is the
    // innermost activation's and whose environment's parent is the context's.
    let window = |vm: &mut Vm| unsafe {
        let bindings = activation.take_record_bindings();
        window_environment(vm, code, r.0.add(usize::from(registers)), owner, parent, (*cx).program, r.this(), Some(bindings))
    };
    let value = match vm.closure_template_in_lane(function) {
        // A recorded closure is made from its template once: over another environment where
        // it reads nothing of this activation's (`Vm::maker_free_environment`), so the
        // activation needs none of its own for it, else over the activation's. A template
        // bypasses `Vm::closure`'s recording, so a closure that must be findable is recorded here.
        Some(template) => {
            let env = match vm.maker_free_environment(&template, parent, owner) {
                Some(env) => env,
                None => window(vm),
            };
            let value = vm.closure_of_template(template, env);
            vm.note_closure_created(vm.active_program, function, value);
            // SAFETY: the context's program outlives the lane run.
            vm.record_lane_closure(code, unsafe { (*cx).program }, function, value);
            Some(value)
        }
        None => {
            let environment = window(vm);
            closure_in_lane(vm, code, function, environment)
        }
    };
    note_allocation(vm);
    value
}

/// The value of a register or constant operand; other operand kinds take
/// the general path.
#[inline(always)]
pub(super) fn operand_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    encoded: u16,
) -> Option<Value> {
    let operand = quench_runtime::bytecode::Operand(encoded);
    match operand.kind()? {
        quench_runtime::bytecode::OperandKind::Register => Some(r.get(operand.payload())),
        // SAFETY: the run's program keeps its constants (`LaneContext::constants`).
        quench_runtime::bytecode::OperandKind::Constant => unsafe { &*(*cx).constants }
            .get(usize::from(operand.payload()))
            .copied(),
        quench_runtime::bytecode::OperandKind::Local => plain_local_operand(cx, operand.payload()),
        _ => None,
    }
}

/// A call-out operand's value in the lane: a register's or a program constant's
/// (`operand_value_in_lane`); a call-out names only those two kinds.
#[inline(always)]
pub(super) fn call_operand_in_lane(cx: *mut LaneContext, r: Registers, operand: quench_runtime::bytecode::Operand) -> Value {
    operand_value_in_lane(cx, r, operand.0).expect("a call-out operand is a register or a constant")
}

/// A local operand of the running function that is a plain slot, which the general loop
/// reads as `LoadLocalPlain` does (`Vm::operand_value`): generators and other functions that
/// keep frame locals compare and compute with them in place, and an environment
/// activation reads its window's local (`row_frame`'s `FLAG_WINDOW`). Any other frameless
/// activation has no locals; any other local is the general path's.
#[inline(never)]
pub(super) fn plain_local_operand(cx: *mut LaneContext, slot: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(activation) = vm.lane_stack.top_frame() {
        activation.script().filter(|script| script.environment)?;
        let body = code.function_get(activation.function() as usize)?;
        if !body.plain_local(code, usize::from(slot)) {
            return None;
        }
        // SAFETY: the running function's registers, which its window's locals follow.
        let value = unsafe { (*cx).registers }.get(body.registers + WINDOW_LOCALS + slot);
        return (!value.is_deleted()).then_some(value);
    }
    // SAFETY: its frame is the running one.
    let frame = unsafe { (*cx).frame() };
    let frame = vm.frames.get(frame)?;
    let slot = usize::from(slot);
    code.functions
        .get(frame.function as usize)?
        .plain_local(code, slot)
        .then(|| frame.locals.get(slot).copied())
        .flatten()
}

#[cfg(test)]
/// The integer `IncDec` of `value`, the update `dispatch::number_increment` makes of an
/// integer that does not overflow; `None` for any other value.
#[inline(always)]
pub(crate) fn integer_increment<const DECREMENT: bool>(value: Value) -> Option<i32> {
    // `Value::int_key_offset` is crate-private to quench-runtime; `as_int` is the public
    // int32 view and gives the same result for every value.
    let integer = value.as_int()?;
    if DECREMENT { integer.checked_sub(1) } else { integer.checked_add(1) }
}

/// The environment clone of plan `plan` a `CloneEnv` row makes in the lane context `cx` over its
/// register file `r` (`Vm::clone_frame_environment`, `Vm::iteration_environment`). `window` is the
/// register count of the environment window the record names, 0 where it names none. In a frame's own
/// run, nothing is copied while the frame holds no captured environment, else the frame's environment
/// is replaced by its copy (a `for (let ...)` iteration's, which a closure in the body captures), after
/// which the run's capture caches name the old environment and are dropped. In an environment
/// activation, nothing is copied while its window has made no environment; else the window's
/// environment is replaced by its iteration copy. Closures made earlier keep the old environment, whose
/// function-level slots the copy shares. Where the general path must make the clone (an error, or a
/// shape the lane does not take), it returns `false` and writes nothing.
///
/// # Safety
/// `cx` is the running lane's context and `r` its register file.
pub(super) unsafe fn clone_environment_of(cx: *mut (), r: Registers, window: u16, plan: u32) -> bool {
    let cx = cx.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    let env_set = !unsafe { (*cx).registers }.env().is_deleted();
    if window == 0 || !env_set {
        // A frame's own clone; an activation's environment is its window's, so none is cloned here.
        if vm.lane_stack.has_frames() {
            return false;
        }
        let frame = unsafe { (*cx).frame() };
        if vm.frames[frame].captured {
            // SAFETY: the context's program outlives the lane run.
            let code = unsafe { &*(*cx).code };
            let body = code.function(vm.frames[frame].function as usize);
            let Some(clone) = body.environment_clones.get(plan as usize) else { return false };
            // It checks the environment before it allocates the copy, so an error leaves the frame
            // for the general path to raise it from.
            if vm.clone_frame_environment(frame, &clone.fresh, body.environment_clones.resets(clone)).is_err() {
                return false;
            }
            note_allocation(vm);
            handler_edges::drop_environment_caches(cx);
        }
        return true;
    }
    // An environment activation's window, which starts `window` registers into the file.
    // SAFETY: the running environment activation's window starts at `r + window`.
    let entries = unsafe { r.0.add(usize::from(window)) };
    let source = unsafe { *entries.add(usize::from(WINDOW_ENVIRONMENT)) };
    if source.is_deleted() {
        return true;
    }
    let Some(function) = vm.lane_stack.top_frame().map(|activation| activation.function()) else {
        return false;
    };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let body = code.function(function as usize);
    let Some(clone) = body.environment_clones.get(plan as usize) else { return false };
    // It checks the environment before it allocates the copy, so an error leaves the activation for
    // the general path to raise it from.
    let Ok(environment) = vm.iteration_environment(source, unsafe { (*cx).program }, function, &clone.fresh, body.environment_clones.resets(clone))
    else {
        return false;
    };
    note_allocation(vm);
    let Some(quench_runtime::tier_surface::EnvironmentRecord { slots, .. }) = vm.heap_reads().environment(environment) else {
        unreachable!("an iteration copy is an environment");
    };
    let storage = std::ptr::from_ref(slots);
    // SAFETY: as above; the copy's slot storage never moves while it lives.
    unsafe {
        entries.add(usize::from(WINDOW_ENVIRONMENT)).write(environment);
        entries.add(usize::from(WINDOW_SLOTS)).write(std::mem::transmute::<u64, Value>(storage as u64));
    }
    true
}

/// The memory view the record after a general step carries: a Wasm run's frame memory, read again
/// as a call's return reads it (`complete_call`: the step may have grown it), never the script
/// view `Carried::stepped` makes. A Wasm record addressing through the script view wrote and read
/// the lane context's own bytes as linear memory (JetStream argon2-wasm: `argon2_hash` stored its
/// costs after a `v128.store`, which a general step makes, past the context, so validation read
/// zeros). `incoming` is the view the step's record ran with; one that is not the run's resolved
/// memory is none, whose accesses take the general path.
#[inline(always)]
pub(super) fn memory_after_step(cx: *mut LaneContext, incoming: MemoryView) -> MemoryView {
    // SAFETY: the context's program outlives the lane run.
    if unsafe { (*(*cx).code).kind } != quench_runtime::bytecode::ProgramKind::Wasm {
        return MemoryView::script(cx);
    }
    let (binding, resolved) = unsafe { (*cx).resolved };
    let memory = if incoming.bytes == resolved.bytes {
        let fresh = memory_view(unsafe { vm(cx) }, binding);
        unsafe { (*cx).resolved = (binding, fresh) };
        fresh
    } else {
        MemoryView::NONE
    };
    unsafe { (*cx).memory = memory };
    memory
}

/// `general_step_in_lane` within a frameless activation: a value step (`Vm::call_out`) of
/// the activation's registers, made beside the activations as a native runs beside them
/// (`DeferredLaneCall`), so guest code it reaches makes them frames first; its result lands
/// in the running function's registers then. Any other step takes the general path.
#[cold]
#[inline(never)]
pub(super) fn step_beside_activations_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip) -> Resumed {
    let vm = unsafe { vm(cx) };
    let Some(function) = vm.lane_stack.top_frame().map(|activation| activation.function()) else {
        return Resumed::Exit(exit(ip));
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, record_pc(cx, ip) as usize) };
    let body = code.function(function as usize);
    let instruction = body.widened(body.code[pc]);
    // A record that returns its result completes its activation, which the general path does
    // (`general_step_in_lane`); no record follows it in the function.
    if !instruction.op().steps_with_result() || instruction.result_flags() & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        return Resumed::Exit(exit(ip));
    }
    let (operation, result) = quench_runtime::staged::Operation::<(), ()>::decode(instruction, quench_runtime::staged::DecodeContext::of(code))
        .and_then(|row| row.call_out()).expect("a result step is a call-out row");
    let operands = operation.operands().read(&code.method_arguments, |operand| call_operand_in_lane(cx, r, operand));
    let value = match unwound_in_place(cx, ip, |vm, code| {
        let site = quench_runtime::vm::call_out::CallSite { owner: vm.call_out_owner(operation), function, resume: pc + 1 };
        vm.call_out(code, site, operation, operands)
    }) {
        Ok(value) => value,
        Err(next) => return Resumed::Exit(next),
    };
    // Guest code the step ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    if let (Some(value), Some(result)) = (value, result) {
        r.set(result, value);
    }
    let memory = memory_after_step(cx, unsafe { (*cx).memory });
    Resumed::At(r, following(ip), Carried { memory, ..Carried::stepped(cx, Carried::script(cx), r, instruction) })
}

/// Make `operation` of the record at `ip` from the lane, as its general step would: in a
/// frame's own run with the frame's PC published (`general_step_in_lane`), or beside frameless
/// activations as a native beside them runs (`DeferredLaneCall`), so guest code it reaches
/// makes them frames first. A throw is stashed in `Vm::lane_raised`, raised from those
/// frames, and `None` is returned for the general path to raise it. The running function's
/// registers are `(*cx).registers` afterwards.
#[inline(always)]
pub(crate) fn in_place_in_lane<T>(
    cx: *mut LaneContext,
    ip: Ip,
    operation: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Option<T> {
    made_in_place(cx, ip, operation).map_err(|(error, made)| raised_in_place(cx, ip, error, made, false)).ok()
}

/// `in_place_in_lane` whose raise stays in the lane where the lane can unwind it
/// (frame-switches.md §5.3): `Err` carries the record the run goes on at, for the caller to
/// return as its handler's result (`Resumed::Exit` of it). The error was made as the
/// operation runs, with the record's pc published in a frame run, or deferred at the record
/// beside frameless activations (`DeferredLaneCall`), whose stack capture then read the frame
/// walk (`Vm::lane_stack_sites`); so it is the error the general path raises, and it is raised
/// and unwound at the record (`handler_edges::unwind_raised`): a handler of the running
/// function or of a plain caller takes it, else it leaves as a body's raise leaves
/// (`raised_exit_in_lane`). Where guest code or a declined walk made the activations frames
/// (`InPlaceMade::Materialized`), or the error is one the lane never unwinds
/// (`handler_edges::unwinds_in_lane`), it is raised from frames on the general path, exactly as
/// `in_place_in_lane` raises it.
#[inline(always)]
pub(super) fn unwound_in_place<T>(
    cx: *mut LaneContext,
    ip: Ip,
    operation: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Result<T, Ip> {
    made_in_place(cx, ip, operation).map_err(|(error, made)| raised_in_place(cx, ip, error, made, true))
}

/// The raise of an operation made in place at the record `ip`, out of line: the record the run
/// goes on at. Where `unwind` asks and the lane can (`unwound_in_place`), the raise is unwound
/// in the lane (`handler_edges::unwind_raised`); else frameless activations nothing observed
/// are made frames, deferred at the record as the operation was, and the general path raises
/// from them (`exit`).
#[cold]
#[inline(never)]
pub(super) fn raised_in_place(cx: *mut LaneContext, ip: Ip, error: JsError, made: InPlaceMade, unwind: bool) -> Ip {
    let vm = unsafe { vm(cx) };
    let unwinds = unwind && made != InPlaceMade::Materialized && handler_edges::unwinds_in_lane(&error);
    if made == InPlaceMade::Beside && !unwinds {
        // SAFETY: `ip` is a record of the running function's view.
        let pc = unsafe { record_pc(cx, ip) as usize };
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc: pc, frames: vm.frames.len() });
        vm.materialize_deferred_lane();
    }
    vm.lane_completion.raise(error);
    if unwinds { handler_edges::unwind_raised(cx, ip) } else { exit(ip) }
}

/// The operation of `in_place_in_lane`, made with its record published (frame) or deferred
/// (beside activations); its error with where it ran.
#[inline(always)]
pub(super) fn made_in_place<T>(
    cx: *mut LaneContext,
    ip: Ip,
    operation: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Result<T, (JsError, InPlaceMade)> {
    materialize_running_leaf_in_lane(cx);
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // running function's view.
    let (code, pc) = unsafe { (&*(*cx).code, record_pc(cx, ip) as usize) };
    // A Wasm activation the operation runs beside is at `pc` while it runs (`extend_wasm_roots`).
    vm.lane_stack.note_wasm_pc(pc);
    let beside = vm.lane_stack.has_frames();
    if beside {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc: pc, frames: vm.frames.len() });
    } else {
        let frame = unsafe { (*cx).frame() };
        vm.frames[frame].pc = pc;
        vm.frames[frame].pc_is_resume = false;
    }
    let generation_key = (vm.cache_epochs.gc, vm.realm.globals);
    let result = operation(vm, code);
    // The operation may allocate, which can make a collection due.
    note_allocation(vm);
    if (vm.cache_epochs.gc, vm.realm.globals) != generation_key {
        after_nested_collection_in_lane(cx);
    }
    let made = if !beside {
        InPlaceMade::Frame
    } else if vm.lane_deferred.take().is_some() {
        InPlaceMade::Beside
    } else {
        InPlaceMade::Materialized
    };
    result.map_err(|error| (error, made))
}

/// A lane run continues past guest code an operation made in place (`in_place_in_lane`), and
/// that code collected or switched realms: everything the run resolved under its generation (call targets, read
/// memos, capture and frame-storage pointers) assumed no collection runs in the lane, so it
/// names cells the collection may have swept. The run takes a new generation, which retires
/// every cached target, and drops its storage pointers, as a fresh run would start.
#[cold]
#[inline(never)]
pub(super) fn after_nested_collection_in_lane(cx: *mut LaneContext) {
    let vm = unsafe { vm(cx) };
    vm.lane_stack.enter_generation((vm.cache_epochs.gc, vm.realm.globals));
    // SAFETY: `cx` is the running context, of this Host.
    unsafe { retire_lane_binding(cx.cast()) };
}

#[cfg(test)]
/// A read's value may occupy either side of canonical commutative Number arithmetic.
/// Its register was already written, so reading the other register preserves aliases.
/// Ordered operations retain both original operands. A non-Number still declines through
/// the shared Number operation; its original record owns evaluation/coercion order.
#[inline(always)]
pub(super) fn read_number_operands<const OP: u32>(r: Registers, operator: &LaneInstruction, other: u16, element: Value) -> (Value, Value) {
    if quench_runtime::vm::operations::number_result_is_commutative(OP) {
        (element, r.get(other))
    } else {
        (r.get(operator.b), r.get(operator.c))
    }
}

/// The index a proved element record (`specialize_proved_arrays`) reads at: a non-negative
/// integer key, as `Vm::direct_dense_element` takes one.
#[cfg(all(test, feature = "bench-primitives"))]
#[inline(always)]
pub(crate) fn proof_index(key: Value) -> Option<usize> {
    usize::try_from(key.as_int()?).ok()
}

/// A `Binary` record's value where it runs no guest code: Number arithmetic and
/// relations, then identity equality, an ordinary `instanceof` and an `in` whose key
/// and receiver (`Vm::plain_in`) need no guest code.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(always)]
pub(super) unsafe fn binary_value(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let left = operand_value_in_lane(cx, r, i.b)?;
    let right = operand_value_in_lane(cx, r, i.c)?;
    unsafe { binary_values(cx, i.imm(), left, right) }
}

/// The value of `left operator right` where it runs no guest code (the pure part of a returning
/// `Binary`, which the lane frame answers a row's `Binary` or `Equality` call-out with).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(super) unsafe fn binary_values(
    cx: *mut LaneContext,
    operator: u32,
    left: Value,
    right: Value,
) -> Option<Value> {
    unsafe { binary_values_classified(cx, operator, left, right, Value::numeric_pair(left, right)) }
}

/// `binary_values` of operands whose classification `pair` is `Value::numeric_pair(left,
/// right)`, which a caller that has classified them already passes so the Number path
/// does not classify them again. The same value as `binary_values` for the same operands.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(super) unsafe fn binary_values_classified(
    cx: *mut LaneContext,
    operator: u32,
    left: Value,
    right: Value,
    pair: quench_runtime::value::NumericPair,
) -> Option<Value> {
    match unsafe { vm(cx) }.number_binary_classified(operator, left, right, pair) {
        Some(value) => Some(value),
        // Two strings ordered (`a < b` over string keys, a sort's comparator): their texts decide it
        // before the BigInt, immediate and relation probes, which would only decline them.
        None if left.is_heap() && right.is_heap()
            && (STRICT_INEQUALITY_OPERATOR + 1..=quench_runtime::vm::operations::LAST_COMPARISON_OPERATOR).contains(&operator)
            && let Some(holds) = unsafe { vm(cx) }.string_relation(operator, left, right) =>
            Some(if holds { Value::TRUE } else { Value::FALSE }),
        // Two BigInts ordered (`a < b` over BigInt keys): their cells decide it before the string and
        // immediate probes, which would only decline them.
        None if left.is_heap() && right.is_heap() && let Some(holds) = unsafe { vm(cx) }.bigint_relation(operator, left, right) =>
            Some(if holds { Value::TRUE } else { Value::FALSE }),
        // `undefined`, `null` or a Boolean among Numbers, as Sunspider's `x | 0` of a missing
        // element or an emulator's `flag & mask`; a string key's `"k" + i` first.
        None if let Some(value) = concat_or_immediate(cx, operator, left, right) => Some(value),
        None if operator == quench_runtime::vm::operations::ADDITION => concat_value(cx, left, right)
            .or_else(|| bigint_value(cx, operator, left, right)),
        // A comparison, `instanceof`, or an ordinary-object `in` (`relation_other`),
        // decides from the operands; any other operator past Numbers may be a BigInt's.
        None if operator <= quench_runtime::vm::operations::LAST_COMPARISON_OPERATOR
            || operator == INSTANCEOF_OPERATOR
            || operator == IN_OPERATOR =>
        {
            relation_outcome(operator, left, right)
                .map(|holds| if holds { Value::TRUE } else { Value::FALSE })
        }
        // `**` of two Numbers (raytrace's `x ** 2`), which `number_binary` leaves.
        None if operator == oxc_ast::ast::BinaryOperator::Exponential as u32
            && let (Some(base), Some(exponent)) = (left.as_number(), right.as_number()) =>
        {
            Some(Value::number(quench_runtime::vm::operations::exponentiate(base, exponent)))
        }
        None => bigint_value(cx, operator, left, right),
    }
}

/// `+` of a string operand (`concat_value`), as a key built as `"k" + i`, before the
/// immediate operands' tests, which a string fails; else `Vm::immediate_binary`.
#[inline(always)]
pub(super) fn concat_or_immediate(cx: *mut LaneContext, operator: u32, left: Value, right: Value) -> Option<Value> {
    let heap = unsafe { vm(cx) }.heap_reads();
    if operator == quench_runtime::vm::operations::ADDITION && (heap.is_string(left) || heap.is_string(right))
        && let Some(value) = concat_value(cx, left, right)
    {
        return Some(value);
    }
    unsafe { vm(cx) }.immediate_binary(operator, left, right)
}

/// The value of a returning `Binary`, which keeps its operands encoded: each operand is read
/// once (`operand_value_in_lane`, so a register, a folded constant or a plain local), after two
/// integer registers are tried as they stand. Two int32 operands decide it in place (`integer_binary`), as the operator-specialized records decide
/// theirs; the other operands are classified once (`Value::numeric_pair`) for the general value.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(always)]
pub(super) unsafe fn returning_binary_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    // Two integer registers decide it without decoding the operands further.
    let register = |encoded: u16| {
        let operand = quench_runtime::bytecode::Operand(encoded);
        (operand.kind() == Some(quench_runtime::bytecode::OperandKind::Register)).then(|| r.get(operand.payload()))
    };
    if let (Some(left), Some(right)) = (register(i.b), register(i.c))
        && let Some((a, b)) = Value::int_pair(left, right)
        && let Some(value) = quench_runtime::vm::operations::integer_binary(i.imm(), a, b)
    {
        return Some(value);
    }
    let left = operand_value_in_lane(cx, r, i.b)?;
    let right = operand_value_in_lane(cx, r, i.c)?;
    // A folded constant takes the integer path a register's value does; any other pair is
    // classified once, and the Number path reuses that classification.
    if let Some((a, b)) = Value::int_pair(left, right)
        && let Some(value) = quench_runtime::vm::operations::integer_binary(i.imm(), a, b)
    {
        return Some(value);
    }
    unsafe { binary_values_classified(cx, i.imm(), left, right, Value::numeric_pair(left, right)) }
}

/// A BigInt operator of two BigInts that raises nothing (`Vm::bigint_binary_value`), which
/// allocates the result; `None` leaves the operation, and any error, to the general path.
#[inline(never)]
pub(super) fn bigint_value(
    cx: *mut LaneContext,
    operator: u32,
    left: Value,
    right: Value,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let value = vm.bigint_binary_value(operator, left, right)?;
    note_allocation(vm);
    Some(value)
}

/// `+` of operands that become strings without guest code (`Vm::primitive_concatenation`),
/// which allocates the result; `None` leaves any other `+` to the general path. Out of line,
/// so the numeric handlers that fall back to it keep their code.
#[inline(never)]
pub(super) fn concat_value(cx: *mut LaneContext, left: Value, right: Value) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let value = vm.primitive_concatenation(unsafe { &*(*cx).code }, left, right)?;
    note_allocation(vm);
    Some(value)
}

/// A relation the lane decides without running guest code: Number comparison, identity
/// decided from the operand words, then `relation_other`. Matches rather than
/// `Option::or_else` closures, which capture the operands by reference and, left out of
/// line, would cost the calling handler its tail call to the next record.
#[inline(always)]
pub(super) fn relation_outcome(operator: u32, left: Value, right: Value) -> Option<bool> {
    // Two heap values are no Numbers: their identity decides without the Number decode.
    if left.is_heap() && right.is_heap() && let Some(holds) = Vm::word_identity_equality(operator, left, right) {
        return Some(holds);
    }
    if let Some((holds, _)) = quench_runtime::vm::operations::number_relation(operator, left, right) {
        return Some(holds);
    }
    if let Some(holds) = Vm::word_identity_equality(operator, left, right) {
        return Some(holds);
    }
    relation_other(operator, left, right)
}

/// `lane_return_value!` past the common return (`complete_plain_activation_in_lane`): any
/// other activation's completion, or the frame's return. Out of line, so the returning
/// handlers keep their registers.
#[inline(never)]
pub(super) fn return_value_general_in_lane(cx: *mut LaneContext, ip: Ip, value: Value) -> Resumed {
    // Another program's activation, which a program that calls across programs returns from
    // as often as the common return, completes without the frame the other completions need.
    if let Some(resumed) = complete_native_step_in_lane(cx, value) {
        return resumed;
    }
    if let Some(resumed) = lane_complete_wasm_activation(cx, value) {
        return resumed;
    }
    if let Some(resumed) = complete_cross_activation_in_lane(cx, value) {
        return resumed;
    }
    if let Some(resumed) = complete_returning_chain_in_lane(cx, value) {
        return resumed;
    }
    return_value_other_in_lane(cx, ip, value)
}

/// The return of a native's callback activation, one per element or comparison: an array
/// callback's (`Completion::ArrayStep`) takes the step of its iteration, any other native's
/// (`Completion::ReturnsToNative`, a sort's comparator, a prepared callback) ends the run with
/// the value. Tested first among the returns past the common one, since neither is a Wasm,
/// cross-program or chained activation, so a callback loop pays none of their tests. `None`
/// for any other activation.
#[inline(always)]
pub(super) fn complete_native_step_in_lane(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // Read where it is (a header's side record, a step header or a record), no spill.
    match vm.lane_stack.top_frame().and_then(|frame| frame.script()) {
        Some(script) if script.completion == Completion::ArrayStep => step_array_iteration_in_place_in_lane(cx, value),
        Some(script) if script.completion == Completion::ReturnsToNative => complete_native_callback_in_lane(cx, value),
        _ => None,
    }
}

/// `return_value_general_in_lane` of any other activation's completion, or the frame's return.
#[inline(never)]
pub(super) fn return_value_other_in_lane(cx: *mut LaneContext, ip: Ip, value: Value) -> Resumed {
    if let Some(resumed) = complete_returning_in_lane(cx, ip, value) {
        return resumed;
    }
    if unsafe { (*cx).returns_script_frame } {
        // This completion already owns its value; native SSA may never have
        // written the Return register. Publish it if this frame cannot retire.
        return Resumed::Exit(return_script_frame_in_lane(cx, ip, value, true));
    }
    Resumed::Exit(frame_return(unsafe { vm(cx) }, ip, value))
}

/// The function `LoadCallee` reads (`ActivationView::callee`): the innermost script activation's, or the
/// frame's when none runs, as its activation filled its self binding (`Vm::initialize_frame_locals`).
/// `None` where the innermost activation is not a script or is windowed, which the general step reads.
///
/// # Safety
/// `cx` is the running lane's context.
pub(super) unsafe fn running_callee(cx: *mut ()) -> Option<Value> {
    let cx = cx.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    match vm.lane_stack.top_frame().map(|frame| frame.kind()) {
        // A windowed activation names the owner of its frame, which is the record's.
        Some(ActivationKind::Script(script)) if !script.completion.is_windowed() => Some(script.callee),
        Some(_) => None,
        None => Some(vm.frames[unsafe { (*cx).frame() }].context.callee().unwrap_or(Value::UNDEFINED)),
    }
}

/// `plain_keyed_get` at lane site `ip`, recording the own slot a symbol or index key's proof
/// gives for the site's next read (`Vm::site_keyed_own_read`). Out of line: the site's hits
/// skip it.
#[inline(never)]
pub(super) fn site_keyed_get_in_lane(vm: &mut Vm, ip: Ip, base: Value, key: Value) -> Option<Value> {
    let value = vm.plain_keyed_get(base, key)?;
    vm.record_site_keyed_read(ip.addr(), base, key);
    Some(value)
}

/// An element whose lane `read` allocates: a one-unit string (`Vm::string_element`) or a BigInt (`Vm::bigint_typed_element`).
#[inline(never)]
pub(super) fn allocated_element_in_lane(vm: &mut Vm, base: Value, key: Value, read: fn(&mut Vm, Value, Value) -> Option<Value>) -> Option<Value> {
    let value = read(vm, base, key)?;
    note_allocation(vm);
    Some(value)
}

/// A `GetIndex` of `base[key]` past the lane's reads within a frameless activation: the
/// general read made in place (`in_place_in_lane`), as `row_get_field_beside_activations`
/// makes a field's, so a getter, proxy or throw makes the activations frames only when it
/// runs.
#[cold]
#[inline(never)]
pub(super) fn get_index_beside_activations_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    base: Value,
    key: Value,
) -> Resumed {
    let value = match unwound_in_place(cx, ip, |vm, code| vm.get_index(code, base, key)) {
        Ok(value) => value,
        Err(next) => return Resumed::Exit(next),
    };
    // Guest code the read ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    r.set(i.a, value);
    Resumed::At(r, following(ip), Carried::script(cx))
}

/// The element row's host read (`ElementFrame`'s `index_get_host`) at site `ip` of `base[key]`
/// past the row's dense arm: a symbol-keyed read this site made of the same shape and key reads
/// its own slot; else a dense or absent array element, a keyed read this site recorded, a typed
/// array's or a string's element. Out of line, as `row_index_write`.
#[inline(never)]
pub(super) fn row_index_read(context: *mut (), ip: Ip, base: Value, key: Value) -> Option<Value> {
    let vm = unsafe { vm(context.cast::<LaneContext>()) };
    if let Some(value) = vm.site_keyed_own_read(ip.addr(), base, key) {
        return Some(value);
    }
    let key = vm.canonical_index_key(key);
    // A typed array answers every index read itself, never its prototype chain (a BigInt kind's
    // element too, which allocates), and a string every index inside its length, so their reads
    // come before the ordinary array probes, which would only decline them.
    vm.dense_array_element(base, key)
        .or_else(|| named_key_element(vm, base, key))
        .or_else(|| allocated_element_in_lane(vm, base, key, Vm::bigint_typed_element))
        .or_else(|| row_typed_element(vm, base, key))
        .or_else(|| allocated_element_in_lane(vm, base, key, Vm::string_element))
        .or_else(|| vm.absent_array_element(base, key))
        .or_else(|| site_keyed_get_in_lane(vm, ip, base, key))
        .or_else(|| vm.keyed_array_length(base, key))
}

/// A typed array's element at an index key (`Vm::plain_typed_element`), which no dense array
/// read before it serves. The publish makes the view the typed row of the element's kind proves
/// (`LaneFrame::kept_view`) available to the next read of the record.
pub(super) fn row_typed_element(vm: &mut Vm, base: Value, key: Value) -> Option<Value> {
    let value = vm.plain_typed_element(base, key)?;
    vm.publish_typed_view(base);
    Some(value)
}

/// A lane `GetIndex` of a symbol key whose proof records an inherited getter
/// (`Vm::proved_symbol_getter`): the getter runs as a frameless activation of the read's
/// record, as `getter_activation_in_lane` runs a field's, with the base as receiver. `Err`
/// carries the record to continue at: the general path for any other read.
#[inline(never)]
pub(super) fn index_getter_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    base: Value,
    key: Value,
) -> Result<(Registers, Ip), Ip> {
    // The getter's activation asks the admission word (`lane_activate_getter`).
    let vm = unsafe { vm(cx) };
    let Some(getter) = vm.proved_symbol_getter(base, key) else {
        return Err(exit(ip));
    };
    activate_getter_in_lane(cx, r, ip, getter, base, None)
}

/// `row_set_index_refused` past the lane's stores within a frameless activation: the general
/// store made in place (`in_place_in_lane`), strict as its record or function is.
#[cold]
#[inline(never)]
pub(super) fn row_set_index_activation(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Resumed {
    let vm = unsafe { vm(cx) };
    let Some(function) = vm.lane_stack.top_frame().map(|activation| activation.function()) else {
        return Resumed::Exit(exit(ip));
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, record_pc(cx, ip) as usize) };
    let body = code.function(function as usize);
    let instruction = body.widened(body.code[pc]);
    if !quench_runtime::staged::writes_element(instruction.op()) {
        return Resumed::Exit(exit(ip));
    }
    let strict = body.strict || instruction.boolean_flag().unwrap_or(false);
    let (object, key, value) = (r.get(i.b), r.get(i.c), r.get(i.a));
    if let Err(next) = unwound_in_place(cx, ip, |vm, code| vm.set_index_mode(code, object, key, value, strict)) {
        return Resumed::Exit(next);
    }
    // Guest code the store ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    Resumed::At(r, following(ip), Carried::script(cx))
}

/// The element row's host store (`ElementFrame::index_set_host`) of `value` at `object[key]` for
/// the record at `ip`: a hole, a symbol, a dense replacement or append, a typed array's number,
/// a plain string key or an index property; `false` stores nothing, which the row's miss takes.
/// Out of line, as every store a lane handler makes through calls whose stack temporaries
/// escape: the handler's frame then holds none.
#[inline(never)]
pub(super) fn row_index_write(context: *mut (), ip: Ip, object: Value, key: Value, value: Value) -> bool {
    let vm = unsafe { vm(context.cast::<LaneContext>()) };
    if !vm.try_fill_record_hole(object, key, value)
        && !vm.site_symbol_set(ip.addr(), object, key, value)
        && !vm.try_replace_dense_element(object, key, value)
        && !vm.try_append_dense_element(object, key, value)
        && !vm.try_fill_dense_hole(object, key, value)
        && !vm.try_site_symbol_set(ip.addr(), object, key, value)
        && !vm.try_host_private_symbol_set(object, key, value)
        && !row_typed_store(vm, object, key, value)
        && !vm.array_index_key(key).is_some_and(|index| vm.typed_array_set_bigint(object, index as usize, value))
        && !vm.try_plain_string_key_set(object, key, value)
        && !key
            .as_int()
            .and_then(|index| usize::try_from(index).ok())
            .is_some_and(|index| vm.set_plain_index_property(object, index, value).is_some())
    {
        return false;
    }
    // Appending an element or adding a property can allocate.
    note_allocation(vm);
    true
}

/// A store of a number at an index of a typed array, which none of the array and symbol
/// paths before it serve.
pub(super) fn row_typed_store(vm: &mut Vm, object: Value, key: Value, value: Value) -> bool {
    let stored = vm
        .array_index_key(key)
        .is_some_and(|index| vm.typed_array_set_number(object, index as usize, value));
    if stored {
        // The typed row of the element's kind proves the view the VM publishes (`LaneFrame::kept_view`):
        // unpublished, every store would miss back here.
        vm.publish_typed_view(object);
    }
    stored
}

/// A capture of a root environment's global var or global lexical
/// (`Vm::plain_projected_root_binding`), which a script's functions read for its top-level
/// `var`s, and in an eval script's context its `let`s and `const`s: the capture's environment
/// as the general path finds it, from the frame or from the activation's environment.
#[inline(never)]
pub(super) fn projected_capture(cx: *mut LaneContext, depth: u16, slot: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let env = unsafe { (*cx).registers }.env();
    let env = if env.is_deleted() {
        vm.owner_capture_env(VarOwner::Frame(unsafe { (*cx).frame() }), depth)?
    } else {
        vm.environment_capture_env(env, false, depth)?
    };
    vm.plain_projected_root_binding(env, slot)
}

/// A frame run's innermost capture slots for `LaneContext::capture`, when the frame's
/// cache holds them dense: tagged `CAPTURE_STORES` when their environment admits every
/// store, whose barrier this takes once, as `Vm::target_capture` tags an activation's. The
/// first store of a frame run asks (`lane_store_capture`).
#[inline(never)]
pub(super) fn frame_capture_in_lane(cx: *mut LaneContext) -> Option<*const usize> {
    let vm = unsafe { vm(cx) };
    let frame = unsafe { (*cx).frame() };
    let Some((slots, None)) = vm.current_capture_slots(frame) else {
        return None;
    };
    // SAFETY: the frame's cache names live slots this collection; none runs in the lane.
    let dense = unsafe { &*slots }.dense_storage()?;
    // The cache's slots are those of the frame's depth-0 capture environment.
    let env = vm.owner_capture_env(VarOwner::Frame(frame), 0)?;
    // SAFETY: the context's program outlives the lane run.
    let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
    if vm.admits_every_direct_store(code, program, env) {
        vm.write_barrier(env);
        return Some(dense.map_addr(|address| address | CAPTURE_STORES));
    }
    Some(dense)
}

/// Resolve an owned or shared lane capture without guest code, refreshing the dense cache.
/// OSR entry and the LoadCapture slow lane use the same binding and TDZ guards.
#[inline(never)]
pub(super) fn capture_read_value(cx: *mut LaneContext, depth: u16, slot: u16) -> Option<Value> {
    read_capture_value::<false>(cx, depth, slot)
}

/// `capture_read_value`, with `PLAIN` also a root projection's plain read
/// (`Vm::plain_capture`), for a `LoadCapture` record, which takes the value however held.
#[inline(always)]
pub(super) fn read_capture_value<const PLAIN: bool>(cx: *mut LaneContext, depth: u16, slot: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let env = unsafe { (*cx).registers }.env();

    if env.is_deleted() {
        let frame = unsafe { (*cx).frame() };
        let value = if PLAIN {
            vm.cached_plain_capture(frame, depth, slot)
        } else {
            vm.cached_direct_capture(frame, depth, slot)
        };
        if depth == 0 {
            frame_capture_at_root(cx, frame);
        }
        value
    } else {
        // A frameless activation's function has no environment of its own.
        let (slots, root) = vm.walked_capture_slots(env, false, depth)?;
        // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
        let slots = unsafe { &*slots };
        if depth == 0
            && root.is_none()
            && let Some(dense) = slots.dense_storage()
        {
            unsafe { (*cx).registers.set_capture(dense) };
        }
        if PLAIN { vm.plain_capture(slots, root, slot) } else { vm.owned_capture(slots, root, slot) }
    }
}

/// The direct write of `value` to the running function's own capture `slot` through its dense
/// slots, where they admit every store (`CAPTURE_STORES`) and the slot holds an initialized
/// binding: whether it wrote.
#[inline(always)]
pub(super) fn store_own_capture(cx: *mut LaneContext, slot: u16, value: Value) -> bool {
    capture_stores(unsafe { (*cx).registers }.capture())
        // SAFETY: the running function's dense capture slots, which admit this write
        // (`CAPTURE_STORES`); no collection runs in the lane.
        .and_then(|slots| unsafe { quench_runtime::tier_surface::EnvironmentSlots::write_storage(slots, usize::from(slot), value) })
        .is_some()
}

/// The running lane activation's dense capture slots, the view a lane body's reads take
/// (`EnvironmentSlots::native_storage_view`), and whether they admit every direct store
/// (`capture_stores`): a frame run finds them as its first store does (`store_to_capture`).
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn context_captures(context: usize) -> Option<((*const Value, usize), bool)> {
    let cx = context as *mut LaneContext;
    unsafe {
        if capture_reads((*cx).registers.capture()).is_null() && (*cx).registers.env().is_deleted() {
            (*cx).registers.set_capture(frame_capture_in_lane(cx)?);
        }
        // SAFETY: the context's slots stay live while the lane runs, which collects nothing.
        let (view, stores) = capture_view((*cx).registers.capture());
        Some((view?, stores))
    }
}

/// The running lane activation's receiver (`this_in_lane`), which a loop region it runs reads as a
/// lane body does: a frameless activation's own, not the VM frame's beneath it; `DELETED` where
/// it has none.
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn context_this_in_lane(context: usize) -> Value {
    let cx = context as *mut LaneContext;
    unsafe { this_in_lane(cx, (*cx).registers) }.unwrap_or(Value::DELETED)
}

/// The object a callee-argument regexp literal record of literal `site`, whose call's callee
/// is `callee`, makes in a frame's own lane run: the general loop's
/// (`Vm::regexp_literal_argument_of`), keyed by the frame. `None` inside a lane activation,
/// which the general path makes it for, and where the site raises, which `Vm::lane_raised`
/// reports at the record.
#[inline(never)]
pub(super) fn lane_regexp_argument_value(cx: *mut LaneContext, callee: Value, site: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() || !unsafe { (*cx).has_frame() } {
        return None;
    }
    // SAFETY: the context's program outlives the lane run.
    let (code, program, frame) = unsafe { (&*(*cx).code, (*cx).program, (*cx).frame()) };
    let result = vm.regexp_literal_argument_of(code, program, frame, site as usize, callee);
    note_allocation(vm);
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            vm.lane_completion.raise(error);
            None
        }
    }
}

/// The closure a `MakeClosure` record makes as the general loop does, in a frame's own lane
/// run of a function with no binding sites outside a module's root: its environment is the
/// frame's, promoted once, and making it allocates and runs no guest code. Any other
/// closure takes the general path; a promotion made first is idempotent.
#[inline(never)]
pub(super) fn closure_value_of_in_lane(cx: *mut LaneContext, id: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() {
        return None;
    }
    // SAFETY: the context's program outlives the lane run.
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame()) };
    let function = vm.frames[frame].function;
    if function == quench_runtime::vm::ROOT_FUNCTION_ID
        || !code.function_get(function as usize)?.binding_sites.is_empty()
    {
        return None;
    }
    let env = if code.closure_takes_no_environment(id) {
        Value::NULL
    } else {
        let env = vm.promote_frame_environment(frame);
        // Promotion moves the frame's captured locals into the environment, so a capture
        // slot pointer found before it is not reused.
        unsafe { (*cx).registers.set_capture(std::ptr::null()) };
        env
    };
    let value = closure_in_lane(vm, code, id, env);
    note_allocation(vm);
    value
}

/// The closure of `function` over `env` that a lane `MakeClosure` makes: a call-only one
/// (`Function::prototype_unobservable`) as a `Value::known_closure`, with no function object,
/// while no `with` was ever entered (a `with` object could take the binding's store);
/// any other through `Vm::closure`.
#[inline(always)]
pub(super) fn closure_in_lane(vm: &mut Vm, code: &ResidualProgram, function: u32, env: Value) -> Option<Value> {
    if !vm.with_scope_entered
        && code.functions.get(function as usize).is_some_and(|slot| slot.header().prototype_unobservable)
        && let Some(known) = Value::known_closure(function, env)
    {
        vm.note_closure_created(vm.active_program, function, known);
        return Some(known);
    }
    vm.closure(code, function, env).ok()
}

/// The object a `CreateRegExpLiteral` record makes in the lane: its site belongs to the
/// running program, the frame's or an activation's (`LaneContext::program`). A pattern that
/// fails to compile raises at its instruction through `Vm::lane_raised` in a frame's own
/// run; inside a lane activation, `None` leaves it to the general path, which raises the
/// same error from the site's cached record.
#[inline(never)]
pub(super) fn regexp_literal_value_in_lane(cx: *mut LaneContext, site: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
    let result = vm.regexp_literal_of(code, program, site as usize);
    note_allocation(vm);
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            if !vm.lane_stack.has_frames() {
                vm.lane_completion.raise(error);
            }
            None
        }
    }
}

/// The window of property values a `MakeObjectLiteral` record reads.
#[inline(always)]
pub(super) fn literal_window_in_lane(i: &LaneInstruction) -> quench_runtime::bytecode::RegisterWindow {
    quench_runtime::bytecode::RegisterWindow { base: i.b, count: i.c }
}

/// The object a staged object literal row makes: its site's shape over the window of property
/// values (`Vm::object_site_shape`).
#[inline(always)]
pub(super) fn object_literal_value_in_lane(cx: *mut LaneContext, r: Registers, site: u32,
    window: quench_runtime::bytecode::RegisterWindow) -> Value {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let shape = vm.object_site_shape(unsafe { &*(*cx).code }, site as usize);
    // SAFETY: `fields_fit_in_lane` admitted the window inside the register file, and no
    // other reference into it is live while the object is allocated.
    let properties = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(window.base)), usize::from(window.count)) };
    let value = vm.allocate_object_with_properties(vm.object_proto, shape, properties);
    note_allocation(vm);
    value
}

/// The array of the values in `elements` (`Vm::literal_array`), out of line as `compiled_new_object`.
#[inline(never)]
pub(super) fn literal_array_value_in_lane(vm: &mut Vm, r: Registers, elements: RegisterList) -> Value {
    let registers = elements.registers();
    let value = vm.literal_array(registers.len(), |index| r.get(registers[index]));
    note_allocation(vm);
    value
}

/// A literal's empty object (`Allocation::Empty`). Out of line: an allocation whose record space is full falls
/// back to a heap slot, which takes the new object by its stack address, a temporary the
/// handler then does not hold.
#[inline(never)]
pub(super) fn new_object_in_lane(vm: &mut Vm) -> Value {
    let value = vm.object();
    note_allocation(vm);
    value
}

/// The `MakeArray` row's array of `length` holes (`Vm::make_array`, a double backing where the
/// literal's annotation declares Number elements), out of line as `compiled_new_object`.
#[inline(never)]
pub(super) fn new_hole_array_in_lane(vm: &mut Vm, length: usize, numbers: bool) -> Value {
    let value = vm.make_array(length, numbers);
    note_allocation(vm);
    value
}

/// The object of literal `site`'s shape holding registers `first` and `second` (`Vm::object_pair`).
#[inline(always)]
pub(super) fn object_pair_in_lane(cx: *mut LaneContext, r: Registers, site: u32, [first, second]: [u16; 2]) -> Value {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let value = vm.object_pair(unsafe { &*(*cx).code }, site as usize, r.get(first), r.get(second));
    note_allocation(vm);
    value
}

/// The four records of superinstruction `index` of the running lane's program, which a
/// `SuperConstArrayObject2` record names (`Operation::decode_literal_with`); `None` past the table.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(crate) unsafe fn super_parts_in_lane(cx: *mut LaneContext, index: usize) -> Option<[WideInstruction; 4]> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    code.superinstructions.get(index).map(|site| site.wide)
}

/// A `SuperConstArrayObject2` record's object: a copy of the constant array, the two
/// fused `Binary` results, and the literal holding them, with each part's result in its
/// register as the general path leaves it. `None` before any guest code could run: an
/// operation on an object, or one that throws. The hand body a returning pair's staged row
/// (`returning_row`) misses to (`returning_miss`); a non-returning pair's miss is the general step.
///
/// Out of line: its general binary result returns through a stack slot, which would cost a
/// handler inlining it the tail call to the next record.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(never)]
pub(super) unsafe fn literal_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let parts = &code.superinstructions.get(i.imm() as usize)?.wide;
    let [array, first, second, object] = [parts[0], parts[1], parts[2], parts[3]];
    let start = array.constant_index();
    let elements = vm.programs.const_array(
        unsafe { (*cx).program },
        start,
        usize::from(array.element_count()),
    )?;
    let array_value = vm.allocate_array(quench_runtime::tier_surface::ArrayObject::new(Vm::empty_object(vm.array_proto), elements));
    r.set(array.result_register(), array_value);
    for part in [first, second] {
        let left = operand_value_in_lane(cx, r, part.operand_b().0)?;
        let right = operand_value_in_lane(cx, r, part.operand_c().0)?;
        if vm.is_object_like(left) || vm.is_object_like(right) {
            return None;
        }
        let value = vm.binary(code, part.binary_operator(), left, right).ok()?;
        r.set(part.result_register(), value);
    }
    let value = vm.object_pair(
        code,
        object.object_site_index(),
        r.get(object.register_b()),
        r.get(object.register_c()),
    );
    note_allocation(vm);
    Some(value)
}

/// The object a non-nested field record's base names: a register, or `this` where the lane
/// knows it. `None` for a nested site, whose record names no single base.
///
/// # Safety
/// As for `field_value_in_lane`.
#[inline(always)]
pub(super) unsafe fn single_field_base_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    base: u16,
) -> Option<Value> {
    if quench_runtime::bytecode::FieldBase(base).local_index().is_some() {
        return None;
    }
    if base == quench_runtime::bytecode::FieldBase::NESTED {
        return None;
    }
    match quench_runtime::bytecode::FieldBase(base).register_index() {
        Some(register) => Some(r.get(register)),
        None => unsafe { fixed_this_in_lane(cx, r) },
    }
}

/// `this` where the lane holds it or the frame fixed it; `None` for a lexical `this`, which
/// the general lookup (`this_in_lane`) resolves with a call. Without that call, a handler's
/// fast path keeps its values in registers no callee preserves.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(super) unsafe fn fixed_this_in_lane(cx: *mut LaneContext, r: Registers) -> Option<Value> {
    let this = r.this();
    if !this.is_deleted() {
        return Some(this);
    }
    let frame = (unsafe { vm(cx) }).frames.get(unsafe { (*cx).frame() })?;
    frame.fixed_this.then_some(frame.this)
}

/// `this` as the context holds it, for a record whose cached hit answers only an object:
/// `DELETED` where the frame's binding decides it (`LaneContext::this`), which no cache entry
/// answers, so the record declines to its general form, which resolves the binding
/// (`fixed_this_in_lane`, `this_in_lane`). One load, with no test of its own.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(super) unsafe fn held_this_in_lane(_cx: *mut LaneContext, r: Registers) -> Value {
    r.this()
}

/// The base of a single-base record whose handler was chosen for it (`single_base_handler`):
/// `this` when `THIS`, else the register the field names.
///
/// # Safety
/// As for `single_field_base_in_lane`; the record's base is `this` exactly when `THIS`.
#[inline(always)]
pub(super) unsafe fn known_field_base_in_lane<const THIS: bool>(
    cx: *mut LaneContext,
    r: Registers,
    base: u16,
) -> Option<Value> {
    if THIS {
        debug_assert_eq!(base, quench_runtime::bytecode::FieldBase::THIS.0);
        // Every caller reads a cached object field of it, which declines `DELETED`.
        Some(unsafe { held_this_in_lane(cx, r) })
    } else {
        debug_assert!(base < quench_runtime::bytecode::FieldBase::NESTED);
        Some(r.get(base))
    }
}

/// The instantiation of a single-base read handler that a record of base `base` takes: the
/// one for `this` or the one for a register. A nested record, which names no single base,
/// takes `nested`.
pub(super) fn single_base_handler(base: u16, this: Handler, register: Handler, nested: Handler) -> Handler {
    match base {
        base if base == quench_runtime::bytecode::FieldBase::THIS.0 => this,
        base if quench_runtime::bytecode::FieldBase(base).register_index().is_some() => register,
        // A nested record names no single base, and a local base (`FieldBase::local`) is no
        // register the single-base handlers could read.
        _ => nested,
    }
}

