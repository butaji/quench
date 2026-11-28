//! Binding one lane OSR activation: the dispatch words a loop region reads, its capture view,
//! the activation's own registers and locals, and the entry of the region's body
//! (`CompiledOsrRegion::run_lane`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The loop's own words: the entry's counter and the scratch a leave inside an inlined callee
/// writes the callee's live registers to.
///
/// # Safety
/// `lane_context` is the running lane's context for `H`.
pub(crate) unsafe fn bind_loop(dispatch: &mut JsStepDispatch, region: &LoopRegion, entry: &mut LoopEntry, lane_context: usize) {
    dispatch.lane_context = lane_context;
    // The counter is the `words` field's first word, not the struct's offset 0:
    // `LoopEntry` is not `repr(C)`.
    dispatch.loop_entry = entry.words_mut() as usize;
    // A leave inside an inlined callee writes the callee's live registers here.
    if let Some(inlining) = &region.inlining { dispatch.inlined = entry.scratch_address(inlining.window()); }
}

/// Fills `captures` with the loop entry's captures as the dense slot view the lane capture
/// reads use, a stored slot holding `DELETED` so its reads take the lane's own lookup. It is
/// refilled in place: an entry allocates nothing once the view has its length.
fn dense_captures(region: &LoopRegion, entry: &LoopEntry, captures: &mut Vec<quench_runtime::Value>) {
    let len = region.captures.iter().chain(&region.stored_captures)
        .map(|slot| usize::from(*slot) + 1).max().unwrap_or(0);
    captures.clear();
    captures.resize(len, quench_runtime::Value::DELETED);
    for (slot, value) in region.captures.iter().zip(entry.captures(region.captures.len())) {
        captures[usize::from(*slot)] = value;
    }
}

/// Whether `region`'s instructions call (`bind_captures`'s `calls`).
pub(crate) fn region_calls(region: &LoopRegion) -> bool {
    region.instructions().iter().any(|instruction| instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::Call)
}

/// Binds the capture view into the window's captures word: a region that stores a capture or
/// `calls` reads the activation's own slots live, as a lane body does (`context_captures`);
/// otherwise the dense copy, refilled in place in `captures`. The caller owns `captures` and keeps
/// it alive, unchanged, until the run returns: the word points at its buffer.
///
/// # Safety
/// `lane_context` is the running lane's context for `H`; `registers` is the live window the run
/// executes in, whose header is its own.
pub(crate) unsafe fn bind_captures(
    region: &LoopRegion, entry: &LoopEntry, lane_context: usize, registers: *mut quench_runtime::value::Value, calls: bool,
    captures: &mut Vec<quench_runtime::Value>,
) {
    // Every region binds a view, even one whose own instructions name no capture: its window's captures word is
    // otherwise whatever the activation's push left there (an interpreter push writes none), and a base region's
    // rows may read it all the same (richards under the b-jit tier read capture 34 through a deleted marker). One that
    // calls may also test a captured callee's identity at its entry (`CAPTURE_IDENTITY_ROW`).
    // A region that calls reads its captures live too where the lane has them: a callee may
    // store one, which a copy would not see. Where it has none, the copy as before.
    let live = (calls || !region.stored_captures.is_empty())
        // SAFETY: the caller's contract.
        .then(|| unsafe { super::dispatch_fast::context_captures(lane_context) }).flatten();
    if let Some(((values, len), stores)) = live {
        // SAFETY: as the caller promises.
        unsafe { super::dispatch_fast::set_frame_word(registers, quench_runtime::staged::frame::CONTEXT_BODY, super::entry_cell::capture_word(values, len, stores)) };
    } else {
        dense_captures(region, entry, captures);
        // SAFETY: as the caller promises; the caller keeps `captures` alive for the run.
        unsafe { super::dispatch_fast::set_frame_word(registers, quench_runtime::staged::frame::CONTEXT_BODY, super::entry_cell::capture_word(captures.as_ptr(), captures.len(), false)) };
    }
}

/// The lane activation's own registers, locals, body and field cache entries. Its locals are where a lane body's
/// are (`dispatch_fast::body_locals` with `LocalsOf::Osr`): the VM frame may name the caller.
///
/// # Safety
/// `registers` is the activation's live register file; `lane_context` the running lane's
/// context for `H`.
pub(crate) unsafe fn bind_frame(
    dispatch: &mut JsStepDispatch, residual: &Rc<ResidualProgram>, vm: &mut Vm, (program, function, lane_context): (ProgramId, u32, usize),
    registers: *mut quench_runtime::value::Value, rebound: bool,
) {
    // The activation holds the program and the function's body it was made for (`with_function`).
    if dispatch.program_id == program
        && let Some(body) = residual.function_get(function as usize) {
        dispatch.native_registers = registers;
        dispatch.register_count = usize::from(body.registers);
        // SAFETY: as the caller promises; the run's registers are the window's.
        dispatch.native_locals = unsafe { super::dispatch_fast::body_locals(lane_context as *mut (), registers, super::dispatch_fast::LocalsOf::Osr) }.locals;
        dispatch.local_count = usize::from(body.locals);
        dispatch.body = body as *const _;
        dispatch.residual = Rc::as_ptr(residual) as usize;
        // A region's inline-cache stores run in place only where the VM and the program run
        // specialized, the fact its rows were planned under (`start_osr_region`'s `stores`).
        // A rebound dispatch's base refresh set it from the same program.
        if !rebound { dispatch.stores_specialized = usize::from(vm.specialized && residual.specialized); }
    }
    // A dispatch with views read its bases as the activation was made, and nothing since moved them.
    if dispatch.layout_seen == u64::MAX { dispatch.refresh_object_bases(vm); }
    // The program's field cache entries, which its site rows read and a body it calls by its entry
    // finds bound where the region lends it the lane frame's place (`region_lend`). A rebound
    // dispatch's base refresh read them for this program already (`rebind_region`).
    if !rebound || dispatch.program_id != program { dispatch.field_entries = vm.program_field_entries(program).unwrap_or(0); }
}

/// The payload a region runs with: the lane's own, or one lent to it where `lend` names whether it
/// keeps direct calls (`region_lend::lend`).
///
/// # Safety
/// As `region_lend::lend`.
unsafe fn payload_for(vm: &mut Vm, dispatch: &mut JsStepDispatch, lend: Option<bool>) -> *mut JsStepDispatch {
    match lend {
        Some(direct_calls) => unsafe { region_lend::lend(vm, dispatch, direct_calls) },
        None => &raw mut *dispatch,
    }
}

/// Enters the region's body over the activation's dispatch and maps its exit. A region that calls
/// callees by their entries (`lend`: whether the lane may make their activations) runs on the lane
/// frame's payload, which is named to the lane's helpers for the run (`dispatch_fast::region_payload_in_lane`):
/// a leave inside a callee stays on the lane stack as its native frames (`RegionRun::nested`).
///
/// # Safety
/// As `CompiledOsrRegion::run`: `registers` is the live register file, and `vm` and
/// `interrupt` belong to this lane activation.
pub(crate) unsafe fn enter(
    dispatch: &mut JsStepDispatch, vm: &mut Vm, program: ProgramId,
    (code, resume): (BodyEntry, u64), registers: *mut quench_runtime::value::Value,
    interrupt: *const quench_runtime::execution_interrupt::ExecutionInterrupt, (lane_context, lend): (usize, Option<bool>),
) -> RegionRun {
    // A region's `THIS`-based reads take the running activation's receiver, as a lane body's do
    // (`run_loop_body`); `with` leaves it `DELETED`. A frameless activation's receiver is the lane
    // context's: `frame` is then the VM frame beneath it, its caller's. The entry takes it as its
    // receiver word; the window's header holds the same word for the helpers that read `this`
    // there (`this_in_lane`), so it is written once per run, here.
    // SAFETY: as the caller promises.
    let receiver = unsafe { super::dispatch_fast::context_this_in_lane(lane_context).to_bits() };
    unsafe { super::dispatch_fast::set_frame_word(registers, quench_runtime::staged::frame::CONTEXT_THIS, receiver) };
    let mut context = CallContext::interrupt_only(unsafe { &*interrupt });
    context.vm = std::ptr::from_mut(vm).cast();
    context.program = program;
    // SAFETY: the region runs on the lane's activation at the lane stack's top, and `reclaim` follows the run.
    let payload = unsafe { payload_for(vm, dispatch, lend) };
    // A region's direct calls push their callees' frames over the running activation, which a leaf has not made yet:
    // they refuse there, once the lent payload's words (which set the limit) are bound.
    if lend.is_some() {
        super::dispatch_fast::refuse_direct_calls_in_leaf(lane_context as *mut (), registers);
    }
    context.adapter_payload = payload.cast();
    // The region's call rows reload its payload's heap bases after the callee ran (`region_call_in_lane`).
    // SAFETY: `lane_context` is the running lane's.
    let outer = unsafe { super::dispatch_fast::region_payload_in_lane(lane_context, payload.addr()) };
    // The region's entry takes its receiver, its resume stub's address as the capture word, and the
    // register file.
    let descriptor = super::entry_protocol::EntryDescriptor {
        kind: super::entry_protocol::EntryKind::Region, receiver, captures: resume,
        registers: registers.cast::<u64>().cast_const(), entry: code,
    };
    // SAFETY: as the caller promises; the code reads the register file and returns
    // its status and result word.
    let returned = unsafe { super::entry_protocol::enter(&mut context, &descriptor) };
    // SAFETY: as above.
    unsafe { super::dispatch_fast::region_payload_in_lane(lane_context, outer) };
    if lend.is_some() {
        // SAFETY: `payload` is the one `lend` was given, and the run it lent for is over.
        unsafe { region_lend::reclaim(dispatch) };
    }
    if let Some(Raised::Panic(panic)) = context.raised.take() {
        std::panic::resume_unwind(panic);
    }
    let exit = super::entry_protocol::exit(&descriptor, returned, || dispatch.returned_pc);
    // A leave with native frames on the lane stack is read in the innermost of them.
    RegionRun { exit, nested: vm.lane_stack.native_depth() != 0 }
}
