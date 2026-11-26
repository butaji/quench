//! The stencil rows' call-outs that are not the general step: the constant, throw, element define,
//! call-out, allocation, closure and value steps of a running body (`base_body`).

#[allow(unused_imports)]
use crate::ext_traits::*;

/// The `LoadConst` record at `call_pc` of the running body (`LaneHelper::ConstantStep`): the running program's constant as
/// the VM materialized it (`ProgramStore::constant`), written to the record's register. `LANE_CALL_MISSED` for any other
/// record, which nothing ran for. A body running as a native callee runs it in its own frame (`in_innermost_frame`).
pub(crate) unsafe extern "C" fn constant_step(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    let cx = context as usize as *mut quench_runtime::vm::engine_types::LaneContext;
    crate::dispatch_fast::native_frames::in_innermost_frame(cx, || {
        // SAFETY: the context's program, records and registers are the running activation's (`in_innermost_frame`).
        let (code, program, registers) = unsafe { (&*(*cx).code, (*cx).program, (*cx).registers) };
        let body = code.function(crate::dispatch_fast::running_function_in_lane(cx) as usize);
        let instruction = body.widened(body.code[call_pc as usize]);
        let loaded = quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) && instruction.result_flags() == 0;
        // SAFETY: the VM is the running lane's.
        match loaded.then(|| unsafe { &*(*cx).vm }.programs.constant(program, instruction.constant_index())).flatten() {
            Some(value) => { registers.set(instruction.result_register(), value); super::LANE_CALL_COMPLETED }
            None => super::LANE_CALL_MISSED,
        }
    })
}

/// The `throw` at `call_pc` of the running body (`LaneHelper::ThrowStep`): the error of the value in its register
/// (`Vm::thrown_error`), raised where the record runs (`in_place_in_lane` stashes it for the general path, which the row
/// then leaves to at the record); `LANE_CALL_MISSED` always, as for any raise.
pub(crate) unsafe extern "C" fn throw_step(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    use quench_runtime::vm::engine_types::LaneContext;
    let cx = context as usize as *mut LaneContext;
    crate::dispatch_fast::native_frames::in_innermost_frame(cx, || {
        // SAFETY: the context's records are the running activation's (`in_innermost_frame`).
        let body = unsafe { &*(*cx).code }.function(crate::dispatch_fast::running_function_in_lane(cx) as usize);
        let instruction = body.widened(body.code[call_pc as usize]).without_return();
        if let Some(quench_runtime::staged::Operation::Throw { source }) = super::decoded(instruction) {
            // SAFETY: the running lane's context and a record of its body.
            let (value, ip) = unsafe { ((*cx).registers.get(source), (*cx).base.add(call_pc as usize)) };
            crate::dispatch_fast::in_place_in_lane(cx, ip, |vm, _| Err::<(), _>(vm.thrown_error(value)));
        }
        super::LANE_CALL_MISSED
    })
}

/// The element define of an array literal at `call_pc` of the running body (`LaneHelper::DefineElementStep`): the value
/// into the fresh dense array at its index (`Vm::define_literal_element`), which runs no guest code; where the array is
/// no such array, the general step.
pub(crate) unsafe extern "C" fn define_element_step(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    use quench_runtime::vm::engine_types::LaneContext;
    let cx = context as usize as *mut LaneContext;
    let defined = crate::dispatch_fast::native_frames::in_innermost_frame(cx, || {
        // SAFETY: the context's records are the running activation's (`in_innermost_frame`).
        let body = unsafe { &*(*cx).code }.function(crate::dispatch_fast::running_function_in_lane(cx) as usize);
        let Some(quench_runtime::staged::Operation::DefineElement { array, index, value }) = quench_runtime::staged::Operation::<(), ()>::decode_literal(body.widened(body.code[call_pc as usize]).without_return(), ())
            else { return super::LANE_CALL_MISSED };
        // SAFETY: the running lane's context.
        let (r, vm) = unsafe { ((*cx).registers, &mut *(*cx).vm) };
        if vm.define_literal_element(r.get(array), index as usize, r.get(value)) { super::LANE_CALL_COMPLETED } else { super::LANE_CALL_MISSED }
    });
    if defined == super::LANE_CALL_COMPLETED { return defined; }
    // SAFETY: the caller's contract, which the general step shares.
    unsafe { crate::dispatch_fast::body_general_step(vm, residual, context, call_pc) }
}

/// The staged call-out of the record at `call_pc` of the running body (`LaneHelper::CallOutStep`), made by the VM in place
/// over the running frame (`dispatch_fast::vm_operation_step`). Beside lane activations the call-out's owner would be the
/// caller's frame, so the general step makes the activations frames first.
pub(crate) unsafe extern "C" fn call_out_step(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    let cx = context as usize as *mut quench_runtime::vm::engine_types::LaneContext;
    // SAFETY: the running lane's context; the caller's contract, which both steps share.
    unsafe {
        if (*(*cx).vm).lane_stack.has_frames() { crate::dispatch_fast::body_general_step(vm, residual, context, call_pc) }
        else { crate::dispatch_fast::vm_operation_step(vm, residual, context, call_pc) }
    }
}

/// The value the allocating record at `call_pc` of the running body defines (`LaneHelper::AllocationStep`: an array,
/// constant array, template or string concatenation, `staged::Allocation`), made by the lane's allocator into its
/// destination (`dispatch_fast::lane_allocate_step`); where the allocator declines before any effect, the general step.
pub(crate) unsafe extern "C" fn allocation_step(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract, which both steps share.
    match unsafe { crate::dispatch_fast::lane_allocate_step(vm, residual, context, call_pc) } {
        super::LANE_CALL_MISSED => unsafe { crate::dispatch_fast::body_general_step(vm, residual, context, call_pc) },
        status => status,
    }
}

/// The closure the `MakeClosure` record at `call_pc` of the running body makes (`LaneHelper::ClosureStep`), over the binding
/// environment its frame captures (`Vm::owner_closure`), written to the record's destination; `LANE_CALL_MISSED` for any
/// other record, or where it raised (the error stashed for the general path). Making a closure runs no guest code, so the
/// body goes on in place. A lane activation's closure is its window's environment (`window_environment`), which the
/// general step's frames make for now.
pub(crate) unsafe extern "C" fn closure_step(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    use quench_runtime::vm::engine_types::LaneContext;
    let cx = context as usize as *mut LaneContext;
    // SAFETY: the running lane's context.
    if unsafe { &*(*cx).vm }.lane_stack.has_frames() {
        return unsafe { crate::dispatch_fast::vm_steps::body_general_step(vm, residual, context, call_pc) };
    }
    let status = crate::dispatch_fast::native_frames::in_innermost_frame(cx, || {
        // SAFETY: the context's records are the running activation's (`in_innermost_frame`).
        let body = unsafe { &*(*cx).code }.function(crate::dispatch_fast::running_function_in_lane(cx) as usize);
        let instruction = body.widened(body.code[call_pc as usize]).without_return();
        let Some((destination, quench_runtime::staged::Allocation::Closure { function })) = quench_runtime::staged::Allocation::decode(instruction, &[])
            else { return super::LANE_CALL_MISSED };
        // SAFETY: the running lane's context and a record of its body.
        let (ip, frame) = unsafe { ((*cx).base.add(call_pc as usize), (*cx).frame()) };
        let made = crate::dispatch_fast::in_place_in_lane(cx, ip, |vm, code| {
            vm.owner_closure(code, quench_runtime::vm::var_owner::VarOwner::Frame(frame), function, call_pc as usize + 1)
        });
        let Some(closure) = made else { return super::LANE_CALL_MISSED };
        // The owner's captured locals moved into its environment: the capture caches that named the old slots go.
        crate::dispatch_fast::handler_edges::drop_environment_caches(cx);
        // SAFETY: no guest code ran, so the lane's register file is the activation's still.
        unsafe { (*cx).registers }.set(destination, closure);
        super::LANE_CALL_COMPLETED
    });
    unsafe { crate::dispatch_fast::refresh_body_bases(context); crate::dispatch_fast::refresh_region_bases_of(context) };
    status
}

/// The value step at `call_pc` of the running body (`LaneHelper::ValueStep`: a `Binary`, `Unary` or comparison jump over
/// registers and constants), made by the VM's operator on their values over the running registers, a frame's or an
/// activation's (`value_steps::activation_value_step`), as that step answers; `LANE_CALL_MISSED` for any other record.
pub(crate) unsafe extern "C" fn value_step(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    let cx = context as usize as *mut quench_runtime::vm::engine_types::LaneContext;
    // A comparison jump a pure decision settles runs no guest code and moves nothing, as the general step's first test.
    // SAFETY: the running lane's context.
    if let Some(status) = unsafe { crate::dispatch_fast::vm_steps::decided_compare_jump(cx, call_pc) } { return status; }
    let status = crate::dispatch_fast::native_frames::in_innermost_frame(cx, || {
        // SAFETY: the context's records are the running activation's (`in_innermost_frame`).
        let body = unsafe { &*(*cx).code }.function(crate::dispatch_fast::running_function_in_lane(cx) as usize);
        let record = body.widened(body.code[call_pc as usize]);
        // SAFETY: the running lane's context, and `record` the record at `call_pc`.
        unsafe { crate::dispatch_fast::value_steps::activation_value_step(cx, call_pc, record) }.unwrap_or(super::LANE_CALL_MISSED)
    });
    // SAFETY: the running lane's; the operator may have run guest code, which allocates.
    unsafe { crate::dispatch_fast::refresh_body_bases(context); crate::dispatch_fast::refresh_region_bases_of(context) };
    status
}

