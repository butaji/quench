//! Array element accesses in general dispatch code past a present dense element, answered
//! by the interpreter's own element steps without the exact step: a read the array and its
//! prototypes do not hold (`Vm::absent_array_element`) and a plain store
//! (`Vm::store_plain_element`). Anything else takes the step at the same PC.
use super::{emit_js_payload_word, emit_lane_helper_call, Host, JsExactFallback, JsStepDispatch, Vm};
use crate::bytecode::WideInstruction;
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

/// Stores the absent element `base[key]` reads at `destination` and continues at `next`;
/// leaves the builder in the block where the exact step answers instead.
pub(super) fn emit_absent(builder: &mut FunctionBuilder<'_>, payload: ir::Value, (base, key): (ir::Value, ir::Value),
    (arguments, destination, next): (ir::Value, i32, ir::Block)) {
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, element_get));
    let absent = emit_lane_helper_call(builder, payload, helper, &[base, key]);
    let found = builder.ins().icmp_imm_u(IntCC::NotEqual, absent, Value::DELETED.to_bits() as i64);
    let (read, exact) = (builder.create_block(), builder.create_block());
    builder.ins().brif(found, read, &[], exact, &[]);
    builder.switch_to_block(read);
    builder.ins().store(MemFlagsData::trusted(), absent, arguments, destination);
    builder.ins().jump(next, &[]);
    builder.switch_to_block(exact);
}

/// Lowers the `SetIndex` at the exact fallback's PC through the plain store; false where
/// an operand register has no home.
pub(super) fn lower_store(
    builder: &mut FunctionBuilder<'_>,
    (instruction, next): (WideInstruction, ir::Block),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    let (Some(value), Some(base), Some(key)) = (register_offset(instruction.register_a()),
        register_offset(instruction.register_b()), register_offset(instruction.register_c())) else { return false };
    let [value, base, key] = [value, base, key].map(|offset| builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, offset));
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, element_set));
    let stored = emit_lane_helper_call(builder, payload, helper, &[payload, base, key, value]);
    let stored = builder.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
    let fallback = builder.create_block();
    builder.ins().brif(stored, next, &[], fallback, &[]);
    exact.emit(builder, fallback);
    true
}

/// `Vm::absent_array_element` of `base` and `key`, else `Value::DELETED`.
///
/// # Safety
/// `vm` is the running VM, live and unborrowed for the call.
pub(super) unsafe extern "C" fn general_element_get<H: Host>(vm: u64, _residual: u64, base: u64, key: u64) -> u64 {
    // SAFETY: as the caller promises; the read allocates nothing.
    let vm = unsafe { &*(vm as *const Vm<H>) };
    vm.absent_array_element(Value::from_bits(base), Value::from_bits(key)).unwrap_or(Value::DELETED).to_bits()
}

/// `Vm::store_plain_element` of an integer key: 1 where it stored, 0 before any effect.
///
/// # Safety
/// `vm` and `payload` are the running VM and entry payload, live and unborrowed for the call.
pub(super) unsafe extern "C" fn general_element_set<H: Host>(vm: u64, _residual: u64, payload: u64, base: u64,
    key: u64, value: u64) -> u64 {
    // SAFETY: as the caller promises.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &mut *(payload as *mut JsStepDispatch)) };
    let Some(index) = Value::from_bits(key).as_int().and_then(|index| usize::try_from(index).ok()) else { return 0 };
    if !vm.store_plain_element(Value::from_bits(base), index, Value::from_bits(value)) { return 0; }
    // As `lane_store_miss`: a store may grow the backing; a due collection waits for the
    // next back edge's safepoint.
    dispatch.refresh_object_bases(vm);
    1
}
