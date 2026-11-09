//! Stage-generic callouts for a lane frame: the interrupt/collection safepoint, the
//! element store helper and a literal allocation's completion.

use super::EmitFrame;
use super::super::{emit_js_guard, emit_lane_helper_call, Emit, EmitProof, LaneHelper};
use super::{RegisterFact, Representation, Type};
use crate::bytecode::Register;
use cranelift_codegen::ir;
use crate::Value;
use cranelift_codegen::ir::{types, InstBuilder, MemFlagsData, condcodes::IntCC};

/// A fault-polling body records its poll instruction; any other leaves at the miss edge when
/// the flag is set.
pub(super) fn safepoint(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, proof: EmitProof) -> Option<()> {
    let EmitProof::Safepoint { interrupt, miss, fault_poll } = proof else { return None };
    let requested = stage.builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), interrupt);
    if fault_poll {
        let cranelift_codegen::ir::ValueDef::Result(inst, _) = stage.builder.func.dfg.value_def(requested) else { return None };
        frame.polls.push(inst);
    } else {
        let miss = miss?;
        let requested = stage.builder.ins().uextend(types::I32, requested);
        let clear = stage.builder.ins().icmp_imm_u(IntCC::Equal, requested, 0);
        emit_js_guard(stage.builder, clear, miss);
    }
    Some(())
}

/// The element store's host helper: its nonzero answer means the store landed.
pub(super) fn index_set_helper(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>,
    base: cranelift_codegen::ir::Value, key: cranelift_codegen::ir::Value,
    value: cranelift_codegen::ir::Value, proof: EmitProof) -> Option<()> {
    let EmitProof::IndexSetHelper { miss } = proof else { return None };
    let payload = frame.payload;
    let address = frame.helpers.address(stage.builder.ins(), LaneHelper::IndexSet);
    let stored = emit_lane_helper_call(stage.builder, payload, address, &[base, key, value]);
    let hit = stage.builder.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
    emit_js_guard(stage.builder, hit, miss);
    Some(())
}

/// A literal allocation's DELETED answer (a due collection) leaves at `miss`; otherwise the
/// literal is the destination's tagged value.
#[inline(always)]
pub(super) fn finish_allocation(stage: &mut Emit<'_, '_>, frame: &mut EmitFrame<'_>, destination: Register,
    value: ir::Value, miss: ir::Block) {
    let made = stage.builder.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
    super::super::emit_js_guard(stage.builder, made, miss);
    <EmitFrame<'_> as crate::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, value,
        RegisterFact::definition(Type::ANY, Representation::Tagged));
}

/// The lane's own global-name read (`LaneHelper::GlobalGet`): a DELETED answer leaves at the
/// miss edge, otherwise the value is the destination's tagged word.
pub(super) fn global_read(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register,
    atom: u32, site: u16, proof: EmitProof) -> Option<()> {
    let EmitProof::GlobalRead { miss } = proof else { return None };
    let payload = frame.payload;
    let atom = stage.builder.ins().iconst(types::I64, i64::from(atom));
    let site = stage.builder.ins().iconst(types::I64, i64::from(site));
    let get = frame.helpers.address(stage.builder.ins(), LaneHelper::GlobalGet);
    let value = emit_lane_helper_call(stage.builder, payload, get, &[atom, site]);
    let hit = stage.builder.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
    emit_js_guard(stage.builder, hit, miss);
    <EmitFrame<'_> as crate::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, value,
        RegisterFact::definition(Type::ANY, Representation::Tagged));
    Some(())
}

/// The VM's field-store hit (`LaneHelper::FieldSet`): its false answer leaves at the miss edge.
pub(super) fn field_store_miss(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, object: ir::Value,
    atom: u32, site: u16, value: ir::Value, proof: EmitProof) -> Option<()> {
    let EmitProof::FieldStoreMiss { miss } = proof else { return None };
    let payload = frame.payload;
    let atom = stage.builder.ins().iconst(types::I64, i64::from(atom));
    let site = stage.builder.ins().iconst(types::I64, i64::from(site));
    let set = frame.helpers.address(stage.builder.ins(), LaneHelper::FieldSet);
    let stored = emit_lane_helper_call(stage.builder, payload, set, &[object, atom, site, value]);
    let ok = stage.builder.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
    emit_js_guard(stage.builder, ok, miss);
    Some(())
}
