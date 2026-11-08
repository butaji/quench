//! The lane helpers that allocate (`LaneHelper::NewObject`, `NewArray`, `Binary`): each
//! leaves for a due collection first, and the dispatch then reads the moved arena bases again.

use super::{Host, JsStepDispatch, ResidualProgram, Value, Vm};

/// A lane body's empty object literal, as the lane's own (`dispatch_fast::lane_make_object`);
/// `DELETED` where a collection is due, which the body leaves for. The allocation may move
/// the property arena, whose base and length the dispatch reads again.
pub(super) unsafe extern "C" fn lane_new_object<H: Host>(vm: u64, _residual: u64, payload: u64) -> u64 {
    // SAFETY: the body passes its own dispatch; nothing else borrows the VM while it runs.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &mut *(payload as *mut JsStepDispatch)) };
    if vm.heap.should_collect() { return Value::DELETED.to_bits(); }
    let object = vm.object();
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    object.to_bits()
}

/// A binary operator one operand of which is a string or BigInt, by the interpreter's own
/// definition (`Vm::binary`); `DELETED` where an operand is an object (its coercion may run
/// guest code), the operator throws, or a collection is due: the interpreter runs it then.
pub(super) unsafe extern "C" fn lane_binary<H: Host>(vm: u64, residual: u64, payload: u64, operator: u64, left: u64, right: u64) -> u64 {
    // SAFETY: as `lane_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    let (left, right) = (Value::from_bits(left), Value::from_bits(right));
    if vm.heap.should_collect() || vm.is_object_like(left) || vm.is_object_like(right) { return Value::DELETED.to_bits(); }
    let Ok(value) = vm.binary(residual, operator as u32, left, right) else { return Value::DELETED.to_bits() };
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}

/// A lane body's array literal of `length` holes, or of the `elements` its definitions
/// write, as the lane's own (`dispatch_fast::lane_make_array`, `lane_make_array_elements`);
/// `DELETED` where a collection is due, as `lane_new_object`.
pub(super) unsafe extern "C" fn lane_new_array<H: Host>(vm: u64, _residual: u64, payload: u64, length: u64, elements: u64) -> u64 {
    // SAFETY: as `lane_new_object`; nonzero `elements` addresses `length` values the code wrote.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &mut *(payload as *mut JsStepDispatch)) };
    if vm.heap.should_collect() { return Value::DELETED.to_bits(); }
    let array = if elements == 0 { vm.hole_array(length as usize) } else { vm.literal_array(length as usize, |index| Value::from_bits(unsafe { *(elements as *const u64).add(index) })) };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    array.to_bits()
}
