//! Constructions native code makes in place: a lane body's planned or initialized
//! receiver, a frameless construction's receiver and result, and an intrinsic array.
use super::{Host, JsStepDispatch, ResidualProgram, Vm};
use crate::Value;

/// A lane body's `new callee(...)` of the `argc` registers at `arguments`
/// (`dispatch_fast::lane_body_construction`): the receiver, or `Value::DELETED`, which leaves
/// the body at the construction. The allocation may have grown the heap's arenas, whose bases
/// the dispatch then holds again for the body to reload (`JsStepDispatch::refresh_object_bases`).
pub(super) unsafe extern "C" fn lane_construct<H: Host>(vm: u64, residual: u64, payload: u64, callee: u64, arguments: u64, argc: u64) -> u64 {
    // SAFETY: the body passes its own VM, running residual, dispatch and argument registers;
    // the construction runs no guest code and collects nothing.
    let (vm, residual, dispatch) = unsafe {
        (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch))
    };
    // A lane run's frame decides as its context says; a linked region's, as its dispatch does
    // for the general loop's in-place constructions.
    let constructs = if dispatch.lane_context == 0 {
        dispatch.allow_inline_calls
    } else {
        unsafe { super::dispatch_fast::lane_context_constructs::<H>(dispatch.lane_context as *mut ()) }
    };
    let built = constructs.then(|| unsafe {
        super::dispatch_fast::lane_body_construction(vm, residual, Value::from_bits(callee), arguments as *const Value, argc as u16)
    }).flatten();
    if built.is_none() {
        use super::super::dispatch_fast::lane_body_stats::{miss, Miss};
        miss(if !constructs { Miss::HelperConstructContext } else if vm.construct_target.is_some() || vm.heap.should_collect() { Miss::HelperConstructBusy } else { Miss::HelperConstructNoPlan });
    }

    dispatch.refresh_object_bases(vm);
    built.unwrap_or(Value::DELETED).to_bits()
}

/// The receiver a body's construction through forwarding constructor `callee` passes its
/// forwarded initializer (`dispatch_fast::lane_body_receiver`), or `Value::DELETED`, which
/// takes the construction's helper path before any effect.
pub(super) unsafe extern "C" fn lane_construct_receiver<H: Host>(vm: u64, _residual: u64, payload: u64, callee: u64) -> u64 {
    // SAFETY: the body passes its own VM and dispatch; allocating runs no guest code.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &mut *(payload as *mut JsStepDispatch)) };
    let built = super::dispatch_fast::lane_body_receiver(vm, Value::from_bits(callee));
    dispatch.refresh_object_bases(vm);
    built.unwrap_or(Value::DELETED).to_bits()
}
