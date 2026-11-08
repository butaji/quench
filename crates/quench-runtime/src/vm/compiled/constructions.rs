//! Constructions native code makes in place: a lane body's planned or initialized
//! receiver, and a frameless construction's receiver and result.
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

    dispatch.refresh_object_bases(vm);
    built.unwrap_or(Value::DELETED).to_bits()
}

/// The receiver an in-place construction of `callee` allocates (`Vm::inline_construct_receiver`),
/// in the callee's realm with no collection due; else `Value::DELETED` before any effect.
///
/// # Safety
/// As `general_user_call`.
pub(super) unsafe extern "C" fn general_construct_receiver<H: Host>(vm: u64, _residual: u64, callee: u64) -> u64 {
    // SAFETY: as the caller promises.
    let vm = unsafe { &mut *(vm as *mut Vm<H>) };
    let callee = Value::from_bits(callee);
    // A due collection runs at the construction's exact step, never under frameless windows.
    if vm.heap.should_collect() || vm.construct_target.is_some()
        || vm.heap.function(callee).is_none_or(|record| record.realm != vm.realm.globals) {
        return Value::DELETED.to_bits();
    }
    vm.inline_construct_receiver(callee).unwrap_or(Value::DELETED).to_bits()
}

/// [[Construct]]'s result: the body's returned object, otherwise the receiver.
///
/// # Safety
/// As `general_user_call`.
pub(super) unsafe extern "C" fn general_construct_result<H: Host>(vm: u64, _residual: u64, value: u64, receiver: u64) -> u64 {
    // SAFETY: as the caller promises.
    let vm = unsafe { &*(vm as *const Vm<H>) };
    if vm.is_object_like(Value::from_bits(value)) { value } else { receiver }
}

/// The construction helpers' addresses, as `JsStepDispatch` holds them.
pub(super) fn construct_helpers<H: Host>() -> (usize, usize) {
    (general_construct_receiver::<H> as *const () as usize, general_construct_result::<H> as *const () as usize)
}
