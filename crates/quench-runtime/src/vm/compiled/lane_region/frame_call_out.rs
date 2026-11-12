//! A staged row's slow case on the lane frame: the VM's own operation, called in place through
//! the lane's helper (`Frame::vm_step`).

use super::{Edges, EmitFrame};
use super::super::{emit_js_payload_word, emit_js_property_values, emit_lane_helper_call, register_offset,
    Emit, JsStepDispatch, LaneHelper, LANE_CALL_COMPLETED, LANE_CALL_MISSED};
use super::{RegisterFact, Representation, Type};
use crate::bytecode::Register;
use cranelift_codegen::ir;
use cranelift_codegen::ir::{types, InstBuilder, MemFlagsData, condcodes::IntCC};

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
        stage.builder.ins().store(MemFlagsData::trusted(), bits, site.registers, register_offset(*register));
    }
    let context = emit_js_payload_word(stage.builder, frame.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
    let record = stage.builder.ins().iconst(types::I64, i64::from(site.record));
    let helper = frame.helpers.address(&mut stage.builder, LaneHelper::VmStep);
    let status = emit_lane_helper_call(stage.builder, frame.payload, helper, &[context, record]);
    let (completed, other) = (stage.builder.create_block(), stage.builder.create_block());
    let done = stage.builder.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_COMPLETED as i64);
    stage.builder.ins().brif(done, completed, &[], other, &[]);
    stage.builder.switch_to_block(other);
    let missed = stage.builder.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_MISSED as i64);
    stage.builder.ins().brif(missed, miss, &[], site.stop, &[ir::BlockArg::Value(status)]);
    stage.builder.switch_to_block(completed);
    let (arena, _) = emit_js_property_values(stage.builder, frame.payload);
    stage.builder.def_var(site.properties, arena);
    Some(match result {
        Some(register) => (stage.builder.ins().load(types::I64, MemFlagsData::trusted(), site.registers, register_offset(register)),
            RegisterFact::definition(Type::ANY, Representation::Tagged)),
        None => (stage.builder.ins().iconst(types::I64, 0), RegisterFact::canonical()),
    })
}
