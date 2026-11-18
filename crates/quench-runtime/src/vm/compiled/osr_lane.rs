//! Binding one lane OSR activation: the dispatch words a loop region reads, its capture view,
//! the activation's own registers and locals, and the entry of the region's body
//! (`CompiledOsrRegion::run_lane`).
use super::*;

/// The loop's own words: its receiver, the entry's counter and the scratch a leave inside an
/// inlined callee writes the callee's live registers to.
pub(super) fn bind_loop<H: Host>(
    native: &mut JsNativeActivation, vm: &mut Vm<H>, region: &LoopRegion, entry: &mut LoopEntry,
    (_frame, lane_context): (usize, usize),
) {
    native.dispatch.bind_native_storage(vm);
    native.dispatch.lane_context = lane_context;
    // A region's `THIS`-based reads take the running activation's receiver, as a lane
    // body's do (`lane_loop_body`); `with` leaves it `DELETED`. A frameless activation's
    // receiver is the lane context's: `frame` is then the VM frame beneath it, its caller's.
    // SAFETY: `lane_context` is the running lane's context for `H`.
    native.dispatch.receiver = unsafe { super::dispatch_fast::lane_context_this::<H>(lane_context) }.to_bits();
    // The counter is the `words` field's first word, not the struct's offset 0:
    // `LoopEntry` is not `repr(C)`.
    native.dispatch.loop_entry = entry.words_mut() as usize;
    // A leave inside an inlined callee writes the callee's live registers here.
    if region.inlining.is_some() { native.dispatch.inlined = entry.scratch_address(); }
}

/// The loop entry's captures as the dense slot view the lane capture reads use, a stored
/// slot holding `DELETED` so its reads take the lane's own lookup.
fn dense_captures(region: &LoopRegion, entry: &LoopEntry) -> Vec<crate::Value> {
    let len = region.captures.iter().chain(&region.stored_captures)
        .map(|slot| usize::from(*slot) + 1).max().unwrap_or(0);
    let mut captures = vec![crate::Value::DELETED; len];
    for (slot, value) in region.captures.iter().zip(entry.captures(region.captures.len())) {
        captures[usize::from(*slot)] = value;
    }
    captures
}

/// Binds the capture view: a region that stores a capture or calls reads the activation's own slots,
/// as a lane body does; otherwise the returned dense view, which the dispatch points at and
/// the caller keeps alive for the run.
///
/// # Safety
/// `lane_context` is the running lane's context for `H`.
pub(super) unsafe fn bind_captures<H: Host>(
    native: &mut JsNativeActivation, region: &LoopRegion, entry: &LoopEntry, lane_context: usize,
) -> Vec<crate::Value> {
    // A region that reads and stores no capture binds no view.
    if region.captures.is_empty() && region.stored_captures.is_empty() { return Vec::new(); }
    // A region that calls reads its captures live too where the lane has them: a callee may
    // store one, which a copy would not see. Where it has none, the copy as before.
    let calls = region.instructions().iter().any(|instruction| instruction.op().control_flow_layout() == crate::bytecode::ControlFlowLayout::Call);
    let live = (calls || !region.stored_captures.is_empty())
        // SAFETY: the caller's contract.
        .then(|| unsafe { super::dispatch_fast::lane_context_captures::<H>(lane_context) }).flatten();
    let none = region.captures.is_empty() && region.stored_captures.is_empty();
    let captures = if live.is_some() || none { Vec::new() } else { dense_captures(region, entry) };
    if let Some(((values, len), stores)) = live {
        (native.dispatch.capture_values, native.dispatch.capture_len, native.dispatch.capture_stores) = (values, len, usize::from(stores));
    } else if !captures.is_empty() {
        native.dispatch.capture_values = captures.as_ptr();
        native.dispatch.capture_len = captures.len();
    }
    captures
}

/// The lane activation's own registers, locals, body and field cache entries. Its locals are where a lane body's
/// are (`dispatch_fast::lane_body_locals`): the VM frame may name the caller.
///
/// # Safety
/// `registers` is the activation's live register file; `lane_context` the running lane's
/// context for `H`.
pub(super) unsafe fn bind_frame<H: Host>(
    native: &mut JsNativeActivation, vm: &mut Vm<H>, (program, function, lane_context): (ProgramId, u32, usize),
    registers: *mut crate::value::Value,
) {
    if let Some(program) = vm.programs.get(program)
        && let Some(body) = program.function_get(function as usize) {
        native.dispatch.native_registers = registers;
        native.dispatch.register_count = usize::from(body.registers);
        native.dispatch.native_locals = super::dispatch_fast::lane_body_locals::<H>(lane_context as *mut (), registers, false);
        native.dispatch.local_count = usize::from(body.locals);
        native.dispatch.body = body as *const _;
        native.dispatch.residual = Rc::as_ptr(&program) as usize;
    }
    native.dispatch.refresh_object_bases(vm);
    // The program's field cache entries, which its site rows read and a body it calls by its entry
    // finds bound where the region lends it the lane frame's place (`region_lend`).
    native.dispatch.field_entries = vm.program_field_entries(program).unwrap_or(0);
}

/// The payload a region runs with: the lane's own, or one lent to it where `lend` names whether it
/// keeps direct calls (`region_lend::lend`).
///
/// # Safety
/// As `region_lend::lend`.
unsafe fn payload_for<H: Host>(vm: &mut Vm<H>, dispatch: &mut JsStepDispatch, lend: Option<bool>) -> *mut JsStepDispatch {
    match lend {
        Some(direct_calls) => unsafe { region_lend::lend(vm, dispatch, direct_calls) },
        None => &raw mut *dispatch,
    }
}

/// Enters the region's body over the activation's dispatch and maps its exit. A region that calls
/// callees by their entries (`lend`: whether the lane may make their activations) runs on the lane
/// frame's payload, which is named to the lane's helpers for the run (`dispatch_fast::lane_region_payload`):
/// a leave inside a callee stays on the lane stack as its native frames (`RegionRun::nested`).
///
/// # Safety
/// As `CompiledOsrRegion::run`: `registers` is the live register file, and `vm` and
/// `interrupt` belong to this lane activation.
pub(super) unsafe fn enter<H: Host>(
    native: &mut JsNativeActivation, vm: &mut Vm<H>, program: ProgramId,
    (code, resume): (BodyEntry, u64), registers: *mut crate::value::Value,
    interrupt: *const crate::execution_interrupt::ExecutionInterrupt, (lane_context, lend): (usize, Option<bool>),
) -> RegionRun {
    let mut context = CallContext::interrupt_only(unsafe { &*interrupt });
    context.vm = std::ptr::from_mut(vm).cast();
    context.program = program;
    // SAFETY: the region runs on the lane's activation at the lane stack's top, and `reclaim` follows the run.
    let payload = unsafe { payload_for(vm, &mut native.dispatch, lend) };
    context.adapter_payload = payload.cast();
    // The region's call rows reload its payload's heap bases after the callee ran (`lane_region_call`).
    // SAFETY: `lane_context` is the running lane's.
    let outer = unsafe { super::dispatch_fast::lane_region_payload::<H>(lane_context, payload.addr()) };
    // SAFETY: as the caller promises; the code reads the register file and returns
    // its status and result word.
    let BodyReturn { status, value: word } = unsafe {
        enter_body(&mut context, 0, resume, registers.cast::<u64>().cast_const(), code)
    };
    // SAFETY: as above.
    unsafe { super::dispatch_fast::lane_region_payload::<H>(lane_context, outer) };
    if lend.is_some() {
        // SAFETY: `payload` is the one `lend` was given, and the run it lent for is over.
        unsafe { region_lend::reclaim(&mut native.dispatch) };
    }
    if let Some(Raised::Panic(panic)) = context.raised.take() {
        std::panic::resume_unwind(panic);
    }
    let exit = LaneExit::read(status, word, || native.dispatch.returned_pc);
    // A leave with native frames on the lane stack is read in the innermost of them.
    RegionRun { exit, nested: vm.lane_stack.activations.native_depth() != 0 }
}
