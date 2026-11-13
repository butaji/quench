//! A staged row's slow case on the lane frame: the VM's own operation, called in place through
//! the lane's helper (`Frame::vm_step`).

use super::{Edges, EmitFrame};
use super::super::{emit_js_payload_word, emit_js_property_values, emit_lane_helper_call, emit_unlogged_property_store_guard,
    register_offset, CheckedObject, Emit, JsStepDispatch, LaneHelper, ObjectRecord, LANE_CALL_COMPLETED, LANE_CALL_MISSED};
use super::super::super::object_slots::inline_storage;
use crate::vm::global_facts::GlobalSlot;
use super::{RegisterFact, Representation, Type};
use crate::bytecode::Register;
use cranelift_codegen::ir;
use crate::staged::{region::Region, Access, Stage};
use cranelift_codegen::ir::InstBuilder;

/// The lane's register file.
const FRAME: Access = Region::Frame.mutable();
/// The property values a global store writes.
const OBJECT_SLOTS: Access = Region::ObjectSlots.mutable();

/// Where a call-out's record is, and what its slow case needs of the lowering: the registers the
/// helper reads, and where a status other than completion or a miss ends the body.
pub(in super::super) struct CallOutSite {
    /// The record the helper decodes (`dispatch_fast::lane_vm_step`).
    pub(in super::super) record: u32,
    /// The block a status of a leave in a callee, or a termination, reaches with the status.
    pub(in super::super) stop: ir::Block,
    /// The base of the lane's register file.
    pub(in super::super) registers: ir::Value,
    /// The property arena's base, which the step may have moved.
    pub(in super::super) properties: cranelift_frontend::Variable,
    /// The body's written, live registers: stored to the file before the helper reads them.
    pub(in super::super) spill: Vec<Register>,
    /// The global slot a `GlobalWrite` or `GlobalTypeof` row here holds directly, under the
    /// dependencies the lowering took (`Lowering::global_cell`).
    pub(in super::super) cell: Option<GlobalSlot>,
}

/// A row's slow case: the registers the body wrote go to the lane's file, the helper makes the
/// VM's operation, and its completion continues with the register it defined, the property arena
/// reloaded; a raised error leaves at the miss edge, and a leave in a callee, or a termination, stops
/// the body. `None` where the lowering made no record for this step.
pub(super) fn vm_step(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, result: Option<Register>)
    -> Option<(ir::Value, RegisterFact)> {
    let site = frame.call_out?;
    let (Edges::ValueGuarded { miss } | Edges::Branch { miss, .. }) = frame.edges else { return None };
    for register in &site.spill {
        let bits = stage.builder.use_var(frame.registers.tagged[usize::from(*register)]);
        stage.store_u64(FRAME, site.registers, register_offset(*register), bits);
    }
    let context = emit_js_payload_word(stage.builder, frame.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
    let record = stage.constant(u64::from(site.record));
    let helper = frame.helpers.address(&mut stage.builder, LaneHelper::VmStep);
    let status = emit_lane_helper_call(stage.builder, frame.payload, helper, &[context, record]);
    let completed = stage.builder.create_block();
    let incomplete = stage.ne_imm(status, LANE_CALL_COMPLETED as u64);
    stage.guard(incomplete, completed)?;
    let unmissed = stage.ne_imm(status, LANE_CALL_MISSED as u64);
    stage.guard(unmissed, miss)?;
    stage.builder.ins().jump(site.stop, &[ir::BlockArg::Value(status)]);
    stage.builder.switch_to_block(completed);
    let (arena, _) = emit_js_property_values(stage.builder, frame.payload);
    stage.builder.def_var(site.properties, arena);
    Some(match result {
        Some(register) => (stage.load_u64(FRAME, site.registers, register_offset(register)),
            RegisterFact::definition(Type::ANY, Representation::Tagged)),
        None => (stage.constant(0), RegisterFact::canonical()),
    })
}

/// The projection of the global object's arena run behind its extras, and the object's tagged
/// word: its record by id, the realm and the layout proved by dependencies the lowering took.
fn global_layout(frame: &EmitFrame<'_>, stage: &mut Emit<'_, '_>, cell: GlobalSlot, miss: ir::Block)
    -> Option<(CheckedObject, ir::Value)> {
    let records = emit_js_payload_word(stage.builder, frame.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
    let bits = stage.constant(cell.global.to_bits());
    let address = ObjectRecord::prove(stage.builder, bits, records, None, miss).address(stage.builder);
    let inline = inline_storage(stage.builder, address);
    let (arena, _) = emit_js_property_values(stage.builder, frame.payload);
    Some((CheckedObject::prove_arena_extras(stage.builder, address, inline, arena, cell.storage_len, miss)?, bits))
}

/// A global-name store of the tagged `value` in `source` straight to the global's slot, which
/// the slot's observed type admits without a test (`GlobalSlot::dependency` retires the code
/// where a wider value is written); a heap value a marked owner would log, or a value this
/// type does not fit, calls the VM (`vm_step`).
pub(super) fn global_write(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, source: Register, value: ir::Value)
    -> Option<()> {
    let cell = frame.call_out?.cell?;
    let ty = frame.state.values[usize::from(source)].value.ty;
    let (Edges::ValueGuarded { miss } | Edges::Branch { miss, .. }) = frame.edges else { return None };
    if !ty.is_subtype(cell.ty) { return None; }
    let (layout, bits) = global_layout(frame, stage, cell, miss)?;
    let address = layout.arena_address(stage.builder, cell.slot, miss)?;
    #[cfg(test)]
    crate::vm::call_out::counts::bump(&crate::vm::call_out::counts::DIRECT_GLOBAL_STORES);
    if ty.is_subtype(Type::NUMBER) {
        stage.store_u64(OBJECT_SLOTS, address, 0, value);
        return Some(());
    }
    let (general, done) = (stage.builder.create_block(), stage.builder.create_block());
    stage.builder.set_cold_block(general);
    let record = stage.and_imm(bits, u64::from(u32::MAX));
    emit_unlogged_property_store_guard(stage.builder, frame.payload, record, value, ty, general);
    stage.store_u64(OBJECT_SLOTS, address, 0, value);
    stage.builder.ins().jump(done, &[]);
    stage.builder.switch_to_block(general);
    vm_step(frame, stage, None)?;
    stage.builder.ins().jump(done, &[]);
    stage.builder.switch_to_block(done);
    Some(())
}

/// A global-name `typeof` of the slot the row holds, whose observed type spells one name: that
/// name, a constant of the string kind.
pub(super) fn global_typeof(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register) -> Option<()> {
    let name = frame.call_out?.cell?.typeof_name?;
    let bits = stage.constant(name.to_bits());
    let fact = RegisterFact::definition(Type::language(crate::ValueType::String), Representation::Tagged);
    <EmitFrame<'_> as crate::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, bits, fact);
    Some(())
}
