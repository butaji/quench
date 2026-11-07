//! The running function's own captures in general dispatch code (depth 0): a read or write
//! of an initialized slot of its dense capture slots, which the payload views
//! (`JsStepDispatch::capture_values`); a store also needs their environment to admit every
//! write directly (`capture_stores`), as the lane's store does (`lane_store_own_capture`).
//! An entry whose view is missing asks `general_capture_slots` once before the exact step.
use super::{emit_js_payload_word, emit_lane_helper_call, Host, JsExactFallback, JsStepDispatch, ResidualProgram, Vm};
use crate::bytecode::{Op, WideInstruction};
use crate::value::ValueLayout;
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

/// Lowers the depth-0 `LoadCapture` or `StoreCapture` at the exact fallback's PC; false
/// where only the exact step applies.
pub(super) fn lower_capture(
    builder: &mut FunctionBuilder<'_>,
    (instruction, next): (WideInstruction, ir::Block),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    let store = instruction.op() == Op::StoreCapture;
    if !store && instruction.op() != Op::LoadCapture { return false; }
    if instruction.capture_depth() != 0 || instruction.result_flags() != 0 || instruction.returns_from_frame()
        || instruction.writes_current_this() || instruction.numeric_local_target().is_some() {
        return false;
    }
    let register = if store { instruction.register_a() } else { instruction.result_register() };
    let Some(register) = register_offset(register) else { return false };
    let slot = usize::from(instruction.capture_slot());
    let fallback = builder.create_block();
    let values = emit_viewed_slots(builder, payload, slot, store, fallback);
    let offset = i32::try_from(slot * size_of::<Value>()).expect("capture slot offset fits i32");
    let old = builder.ins().load(types::I64, MemFlagsData::trusted(), values, offset);
    let tag = builder.ins().band_imm_u(old, ValueLayout::TAG_MASK as i64);
    let initialized = builder.ins().icmp_imm_u(IntCC::NotEqual, tag, ValueLayout::INTERNAL_TAG as i64);
    let done = builder.create_block();
    builder.ins().brif(initialized, done, &[], fallback, &[]);
    builder.switch_to_block(done);
    if store {
        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, register);
        builder.ins().store(MemFlagsData::trusted(), value, values, offset);
    } else {
        builder.ins().store(MemFlagsData::trusted(), old, arguments, register);
    }
    builder.ins().jump(next, &[]);
    exact.emit(builder, fallback);
    true
}

/// The payload's capture slots once they hold `slot` (`emit_view`), else `fallback`.
fn emit_viewed_slots(builder: &mut FunctionBuilder<'_>, payload: ir::Value, slot: usize, store: bool, fallback: ir::Block) -> ir::Value {
    let (viewed, retry) = (builder.create_block(), builder.create_block());
    let values = builder.append_block_param(viewed, types::I64);
    let admitted = emit_view(builder, payload, slot, store);
    let current = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_values));
    builder.ins().brif(admitted, viewed, &[ir::BlockArg::Value(current)], retry, &[]);
    // A missing view (a fresh activation's) asks for the slots once, then tests again.
    builder.switch_to_block(retry);
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, captures));
    emit_lane_helper_call(builder, payload, helper, &[payload]);
    let admitted = emit_view(builder, payload, slot, store);
    let current = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_values));
    builder.ins().brif(admitted, viewed, &[ir::BlockArg::Value(current)], fallback, &[]);
    builder.switch_to_block(viewed);
    values
}

/// Whether the payload's view holds `slot`, and admits a direct write for a `store`.
fn emit_view(builder: &mut FunctionBuilder<'_>, payload: ir::Value, slot: usize, store: bool) -> ir::Value {
    let len = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_len));
    let within = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, len, slot as i64);
    if !store { return within; }
    let stores = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_stores));
    let stores = builder.ins().icmp_imm_u(IntCC::NotEqual, stores, 0);
    builder.ins().band(within, stores)
}

/// Views the payload frame's own capture slots (`Vm::cached_direct_capture` takes them into
/// the frame's cache) and whether they admit every write directly, whose barrier this takes
/// once, as `lane_frame_capture` does.
///
/// # Safety
/// As `general_user_call`.
pub(super) unsafe extern "C" fn general_capture_slots<H: Host>(vm: u64, residual: u64, payload: u64) -> u64 {
    // SAFETY: as the caller promises; this reads the frame's environment and allocates nothing.
    let (vm, program, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
        &mut *(payload as *mut JsStepDispatch)) };
    let frame = dispatch.frame;
    vm.cached_direct_capture(frame, 0, 0);
    let Some((slots, None)) = vm.current_capture_slots(frame) else { return 0 };
    // SAFETY: the frame's cache names live slots this collection.
    let Some((values, len)) = unsafe { &*slots }.native_dense_view() else { return 0 };
    (dispatch.capture_values, dispatch.capture_len) = (values, len);
    if let Some(env) = vm.capture_env(frame, 0)
        && vm.admits_every_direct_store(program, dispatch.program_id, env) {
        vm.heap.remember_cell(env);
        dispatch.capture_stores = 1;
    }
    1
}
