//! A staged row's slow case on the lane frame: the VM's own operation, called in place through
//! the lane's helper (`Frame::vm_step`).

use super::{Edges, EmitFrame};
use super::super::{emit_js_payload_word, emit_js_property_values, emit_lane_helper_call, register_offset,
    Emit, JsStepDispatch, LaneHelper, LANE_CALL_COMPLETED, LANE_CALL_HOLDS, LANE_CALL_MISSED};
use super::{RegisterFact, Representation, Type};
use crate::bytecode::Register;
use crate::staged::CallOut;
use crate::Value;
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
/// the body. A decision (`CallOut::Decide`) defines no register: its word is the boolean the helper's
/// status names. Where the lowering made no record for this step, it leaves at the miss edge.
pub(super) fn vm_step(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, operation: CallOut, result: Option<Register>)
    -> Option<(ir::Value, RegisterFact)> {
    let (Edges::ValueGuarded { miss } | Edges::Branch { miss, .. }) = frame.edges else { return None };
    let Some(site) = frame.call_out else { return Some(leave(stage, miss)) };
    let status = call_helper(frame, stage, site);
    let decides = matches!(operation, CallOut::Decide { .. });
    let holds = decides.then(|| stage.builder.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_HOLDS as i64));
    let (completed, other) = (stage.builder.create_block(), stage.builder.create_block());
    let plain = stage.builder.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_COMPLETED as i64);
    let done = match holds { Some(holds) => stage.builder.ins().bor(plain, holds), None => plain };
    stage.builder.ins().brif(done, completed, &[], other, &[]);
    stage.builder.switch_to_block(other);
    let missed = stage.builder.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_MISSED as i64);
    stage.builder.ins().brif(missed, miss, &[], site.stop, &[ir::BlockArg::Value(status)]);
    stage.builder.switch_to_block(completed);
    let (arena, _) = emit_js_property_values(stage.builder, frame.payload);
    stage.builder.def_var(site.properties, arena);
    Some(completion(stage, site, result, holds))
}

/// The helper's call: the registers the body wrote go to the lane's file first, each in its
/// tagged form, made on a copy of the facts so only this slow path pays for it.
fn call_helper(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, site: &CallOutSite) -> ir::Value {
    let mut scratch = frame.state.clone();
    for register in &site.spill {
        let bits = super::register_tagged(stage.builder, frame.registers, &mut scratch, *register);
        stage.builder.ins().store(MemFlagsData::trusted(), bits, site.registers, register_offset(*register));
    }
    let context = emit_js_payload_word(stage.builder, frame.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
    let record = stage.builder.ins().iconst(types::I64, i64::from(site.record));
    let helper = frame.helpers.address(&mut stage.builder, LaneHelper::VmStep);
    emit_lane_helper_call(stage.builder, frame.payload, helper, &[context, record])
}

/// The word a completed call-out answers: the canonical boolean of a decision, the register it
/// defined, or a placeholder where it defines none.
fn completion(stage: &mut Emit<'_, '_>, site: &CallOutSite, result: Option<Register>, holds: Option<ir::Value>)
    -> (ir::Value, RegisterFact) {
    match (holds, result) {
        (Some(holds), _) => {
            let (yes, no) = (stage.builder.ins().iconst(types::I64, Value::TRUE.to_bits() as i64),
                stage.builder.ins().iconst(types::I64, Value::FALSE.to_bits() as i64));
            (stage.builder.ins().select(holds, yes, no), RegisterFact::definition(Type::language(crate::ValueType::Boolean), Representation::Tagged))
        }
        (None, Some(register)) => (stage.builder.ins().load(types::I64, MemFlagsData::trusted(), site.registers, register_offset(register)),
            RegisterFact::definition(Type::ANY, Representation::Tagged)),
        (None, None) => (stage.builder.ins().iconst(types::I64, 0), RegisterFact::canonical()),
    }
}

/// A step no record exists for (a region with no VM helper, an operand that is no register): it
/// leaves at `miss`, exactly, and what the lowering emits past it is unreachable.
fn leave(stage: &mut Emit<'_, '_>, miss: ir::Block) -> (ir::Value, RegisterFact) {
    stage.builder.ins().jump(miss, &[]);
    let unreachable = stage.builder.create_block();
    stage.builder.switch_to_block(unreachable);
    (stage.builder.ins().iconst(types::I64, 0), RegisterFact::canonical())
}
