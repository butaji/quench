//! Accessor reads in general dispatch code: a field read whose site's cached accessor
//! answers it (`Vm::cached_field_getter`) with a user getter calls that getter frameless
//! (`frameless::emit_activation`), its receiver the read's object and no arguments, as the
//! read's exact step pushes it (`Vm::try_inline_getter`). Any other read takes the step.
use super::{emit_js_payload_word, emit_lane_helper_call, frameless, Host, JsExactFallback, JsStepDispatch, Vm};
use crate::bytecode::Atom;
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

/// The miss of a field read's primary row: for a single read with a home (`getter`) first its
/// site's cached user getter, its value going to that destination and on to its block (a read
/// the accessor entry answers would fail the data walk); then the VM's cached read
/// (`general_field_get`) on to `read`; the exact step at `fallback` where neither answers.
pub(super) fn emit_read_miss(
    builder: &mut FunctionBuilder<'_>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
    object: ir::Value,
    (atom, site): (Atom, u16),
    getter: Option<(i32, ir::Block)>,
    (read, fallback): (ir::Block, ir::Block),
) {
    let named = builder.ins().iconst(types::I64, (i64::from(atom) << u16::BITS) | i64::from(site));
    let emit_data_read = |builder: &mut FunctionBuilder<'_>| {
        let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, field_get));
        let probed = emit_lane_helper_call(builder, payload, helper, &[object, named]);
        let hit = builder.ins().icmp_imm_u(IntCC::NotEqual, probed, Value::DELETED.to_bits() as i64);
        builder.ins().brif(hit, read, &[ir::BlockArg::Value(probed)], fallback, &[]);
    };
    let Some((destination, next)) = getter else {
        emit_data_read(builder);
        return;
    };
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, field_getter));
    let callee = emit_lane_helper_call(builder, payload, helper, &[object, named]);
    let found = builder.ins().icmp_imm_u(IntCC::NotEqual, callee, Value::DELETED.to_bits() as i64);
    let (call, data) = (builder.create_block(), builder.create_block());
    builder.ins().brif(found, call, &[], data, &[]);
    builder.switch_to_block(data);
    emit_data_read(builder);
    builder.switch_to_block(call);
    frameless::emit_activation(builder, (exact.pc, next), (arguments, payload), exact, (callee, Some(object), &[]),
        (destination, fallback));
}

/// The user getter the read `read` (its atom above its cache site) calls on `object`, as its
/// site's accessor cache records it; `Value::DELETED` for any other read.
///
/// # Safety
/// As `general_user_call`.
pub(super) unsafe extern "C" fn general_field_getter<H: Host>(vm: u64, _residual: u64, object: u64, read: u64) -> u64 {
    let (atom, site) = ((read >> u16::BITS) as Atom, read as u16);
    // SAFETY: as the caller promises; the probe reads caches and never allocates.
    let vm = unsafe { &*(vm as *const Vm<H>) };
    vm.cached_field_getter(Value::from_bits(object), atom, vm.field_cache_index(site))
        .filter(|getter| vm.heap.function(*getter).is_some_and(|record| matches!(record.kind, crate::heap::FunctionKind::User(..))))
        .unwrap_or(Value::DELETED).to_bits()
}
