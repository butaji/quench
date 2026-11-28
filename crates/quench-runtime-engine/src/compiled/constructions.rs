//! Constructions native code makes in place: a lane body's planned or initialized
//! receiver, a frameless construction's receiver and result, and an intrinsic array.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{JsStepDispatch, ResidualProgram, Vm};
use quench_runtime::Value;

/// A lane body's `new callee(...)` of the `argc` registers at `arguments`
/// (`dispatch_fast::body_construction_in_lane`): the receiver, or `Value::DELETED`, which leaves
/// the body at the construction. The allocation may have grown the heap's arenas, whose bases
/// the dispatch then holds again for the body to reload (`JsStepDispatch::refresh_object_bases`).
pub(crate) unsafe extern "C" fn construct_in_lane(vm: u64, residual: u64, payload: u64, callee: Value, arguments: u64, argc: u64) -> u64 {
    // SAFETY: as `construct_with`'s.
    unsafe { construct_with(vm, residual, payload, callee, arguments, argc, super::dispatch_fast::body_construction_in_lane) }
}

/// `construct_in_lane` for a materialization of a kept receiver (`materialize_virtuals`), which
/// makes its receiver whatever is due: the allocation sets a collection due and the next safepoint
/// collects (DESIGN 4.7). The plan the construction tested still decides, so the receiver is the
/// one the construction would have made.
pub(crate) unsafe extern "C" fn materialize_construct_in_lane(vm: u64, residual: u64, payload: u64, callee: Value, arguments: u64, argc: u64) -> u64 {
    // SAFETY: as `construct_with`'s.
    unsafe { construct_with(vm, residual, payload, callee, arguments, argc, super::dispatch_fast::materialized_construction_in_lane) }
}

/// The construction behind `construct_in_lane` and `materialize_construct_in_lane`: `build` makes the
/// receiver of the `argc` registers at `arguments`, or `None` leaves the body at the construction.
///
/// # Safety
/// As `construct_in_lane`: the body passes its own VM, running residual, dispatch and argument registers.
unsafe fn construct_with(vm: u64, residual: u64, payload: u64, callee: Value, arguments: u64, argc: u64,
    build: unsafe fn(&mut Vm, &ResidualProgram, Value, *const Value, u16) -> Option<Value>) -> u64 {
    // SAFETY: the body passes its own VM, running residual and dispatch; the construction runs no
    // guest code and collects nothing.
    let (vm, residual, dispatch) = unsafe {
        (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch))
    };
    // A lane run's frame decides as its context says; a linked region's, as its dispatch does
    // for the general loop's in-place constructions.
    let constructs = if dispatch.lane_context == 0 {
        dispatch.allow_inline_calls
    } else {
        unsafe { super::dispatch_fast::context_constructs_in_lane(dispatch.lane_context as *mut ()) }
    };
    let built = constructs.then(|| unsafe {
        build(vm, residual, callee, arguments as *const Value, argc as u16)
    }).flatten();
    if built.is_none() {
        use crate::dispatch_fast::lane_body_stats::{miss, Miss};
        miss(vm, if !constructs { Miss::HelperConstructContext } else if vm.construct_target.is_some() || vm.collection_due() { Miss::Busy } else { Miss::NoPlan });
    }

    dispatch.refresh_object_bases(vm);
    built.unwrap_or(Value::DELETED).to_bits()
}

/// Whether a construction of `callee` here would build its receiver by its plan
/// (`dispatch_fast::construct_plan_holds`), the frame constructing in place as `construct_in_lane`
/// asks: 1, else 0. A body keeping the receiver as its field values tests it where the
/// construction would run (`lane_region::virtual_objects`).
pub(crate) unsafe extern "C" fn construct_plan_holds_in_lane(vm: u64, _residual: u64, payload: u64, callee: Value) -> u64 {
    // SAFETY: the body passes its own VM and dispatch; the test changes nothing.
    let (vm, dispatch) = unsafe { (&*(vm as *const Vm), &*(payload as *const JsStepDispatch)) };
    let constructs = if dispatch.lane_context == 0 {
        dispatch.allow_inline_calls
    } else {
        unsafe { super::dispatch_fast::context_constructs_in_lane(dispatch.lane_context as *mut ()) }
    };
    u64::from(constructs && super::dispatch_fast::construct_plan_holds(vm, callee))
}

/// The receiver a body's construction through forwarding constructor `callee` passes its
/// forwarded initializer (`dispatch_fast::body_receiver_in_lane`), or `Value::DELETED`, which
/// takes the construction's helper path before any effect.
pub(crate) unsafe extern "C" fn construct_receiver_in_lane(vm: u64, _residual: u64, payload: u64, callee: Value) -> u64 {
    // SAFETY: the body passes its own VM and dispatch; allocating runs no guest code.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm), &mut *(payload as *mut JsStepDispatch)) };
    let built = super::dispatch_fast::body_receiver_in_lane(vm, callee);
    dispatch.refresh_object_bases(vm);
    built.unwrap_or(Value::DELETED).to_bits()
}
