//! A staged row's slow case on the lane frame: the VM's own operation, called in place through
//! the lane's helper (`Frame::vm_step`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{Edges, EmitFrame, FRAME};
use super::tag_proofs::{decides_flat_strings, state_flat};
use super::super::{emit_js_guard, emit_js_payload_word, emit_js_property_values, emit_lane_helper_call,
    register_offset, CheckedObject, Emit, JsStepDispatch, LaneHelper, ObjectRecord, LANE_CALL_COMPLETED, LANE_CALL_HOLDS, LANE_CALL_MISSED};
use crate::compiled::object_slots::inline_storage;
use quench_runtime::vm::global_facts::GlobalSlot;
use super::{RegisterFact, Representation, Type};
use quench_runtime::bytecode::Register;
use quench_runtime::staged::CallOut;
use quench_runtime::staged::store::bound_full_store;
use quench_runtime::Value;
use cranelift_codegen::ir;
use quench_runtime::staged::{flow::FlowStage, Stage};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// Where a call-out's record is, and what its slow case needs of the lowering: the registers the
/// helper reads, and where a status other than completion or a miss ends the body.
pub(crate) struct CallOutSite {
    /// The record the helper decodes (`dispatch_fast::vm_operation_step`).
    pub(crate) record: u32,
    /// The block a status of a leave in a callee, or a termination, reaches with the status.
    pub(crate) stop: ir::Block,
    /// The base of the lane's register file.
    pub(crate) registers: ir::Value,
    /// The property arena's base, which the step may have moved.
    pub(crate) properties: cranelift_frontend::Variable,
    /// The body's written, live registers: stored to the file before the helper reads them.
    pub(crate) spill: Vec<Register>,
    /// The global slot a `GlobalWrite` or `GlobalTypeof` row here holds directly, under the
    /// dependencies the lowering took (`Lowering::global_cell`): the global object's layout fact is theirs.
    pub(crate) cell: Option<(GlobalSlot, quench_runtime::staged::ShapeFact)>,
}

/// A row's slow case: the registers the body wrote go to the lane's file, the helper makes the
/// VM's operation, and its completion continues with the register it defined, the property arena
/// reloaded; a raised error leaves at the miss edge, and a leave in a callee, or a termination, stops
/// the body. A decision (`CallOut::Decide`) defines no register: its word is the boolean the helper's
/// status names. Where the lowering made no record for this step, a decision still carries its
/// operands (`decide_words`) and any other step leaves at the miss edge.
pub(crate) fn vm_step(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, operation: CallOut, result: Option<Register>)
    -> Option<(ir::Value, RegisterFact)> {
    let (Edges::ValueGuarded { miss } | Edges::Branch { miss, .. }) = frame.edges else { return None };
    if let Some((operands, operator)) = operation.word_decision() {
        return Some(decide_words(frame, stage, operands, operator, miss));
    }
    let site = match (frame.call_out, operation) {
        // A decision the words may settle (a string relation, an ordinary `instanceof`) asks the
        // pure helper first, and makes the step in place only where it answers `DELETED`.
        (Some(site), CallOut::Decide { left, right, operator }) => {
            let answer = decide_answer(frame, stage, [left, right], operator);
            let decided = stage.ne_imm(answer, Value::DELETED.to_bits());
            let value = stage.choose_leaving(decided, |_| Some(answer),
                |stage| site_step(frame, stage, site, true, result).map(|(word, _)| word))?;
            return Some((value, RegisterFact::definition(Type::language(quench_runtime::ValueType::Boolean), Representation::Tagged)));
        }
        (Some(site), _) => site,
        (None, CallOut::Decide { left, right, operator }) => return Some(decide_words(frame, stage, [left, right], operator, miss)),
        (None, _) => return Some(leave(stage, miss)),
    };
    site_step(frame, stage, site, matches!(operation, CallOut::Decide { .. }), result)
}

/// The helper's call at `site`, continued by its status (`vm_step`); `decides` where the row is a
/// decision, whose `LANE_CALL_HOLDS` is completion too.
pub(crate) fn site_step(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, site: &CallOutSite, decides: bool,
    result: Option<Register>) -> Option<(ir::Value, RegisterFact)> {
    let (Edges::ValueGuarded { miss } | Edges::Branch { miss, .. }) = frame.edges else { return None };
    let status = call_helper(frame, stage, site);
    let holds = decides.then(|| stage.eq_imm(status, LANE_CALL_HOLDS as u64));
    let completed = stage.builder.create_block();
    let plain = stage.eq_imm(status, LANE_CALL_COMPLETED as u64);
    let done = match holds { Some(holds) => stage.or(plain, holds), None => plain };
    let incomplete = stage.not(done);
    stage.guard(incomplete, completed)?;
    let unmissed = stage.ne_imm(status, LANE_CALL_MISSED as u64);
    stage.guard(unmissed, miss)?;
    stage.jump(site.stop, &[status]);
    stage.builder.switch_to_block(completed);
    let (arena, _) = emit_js_property_values(stage.builder, frame.payload);
    stage.builder.def_var(site.properties, arena);
    Some(completion(stage, site, result, holds))
}

/// A decision of two registers no record names (`CallOut::Decide` carries its operands): the
/// lane's `Decide` helper takes their tagged words, made on a copy of the facts so only this slow
/// path pays for boxing, and the operator, and answers by the interpreter's own rule
/// (`comparison_decision`) the boolean it decides or `DELETED`, which leaves at the miss edge. It runs
/// no guest code and allocates nothing, so no register is spilled and the arena stays.
fn decide_words(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, operands: [Register; 2], operator: u32,
    miss: ir::Block) -> (ir::Value, RegisterFact) {
    let answer = decide_answer(frame, stage, operands, operator);
    // A strict equality decides every pair (`comparison_decision`: its `strict_equal` is never absent), so its answer is never
    // `DELETED` and no leave is made. Two flat strings under an operator the helper answers for strings (`decides_flat_strings`) are a Boolean too.
    let strict = matches!(quench_runtime::vm::operations::equality_operator(operator), Some((true, _)));
    let flat_pair = decides_flat_strings(operator) && operands.iter().all(|&register| state_flat(&*frame.state, register));
    if !strict && !flat_pair {
        let decided = stage.ne_imm(answer, Value::DELETED.to_bits());
        emit_js_guard(stage.builder, decided, miss);
    }
    (answer, RegisterFact::definition(Type::language(quench_runtime::ValueType::Boolean), Representation::Tagged))
}

/// The `Decide` helper's answer for two registers: the canonical boolean, or `DELETED` where only
/// the full step decides.
fn decide_answer(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, operands: [Register; 2], operator: u32) -> ir::Value {
    let mut scratch = frame.state.clone();
    let [left, right] = operands.map(|register| super::register_tagged(stage.builder, frame.registers, &mut scratch, register));
    let helper = frame.helpers.target(&mut stage.builder, LaneHelper::Decide);
    let operator = stage.constant(u64::from(operator));
    emit_lane_helper_call(stage.builder, frame.payload, helper, &[left, right, operator])
}

/// The helper's call: the registers the body wrote go to the lane's file first, each in its
/// tagged form, made on a copy of the facts so only this slow path pays for it.
fn call_helper(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, site: &CallOutSite) -> ir::Value {
    let mut scratch = frame.state.clone();
    for register in &site.spill {
        let bits = super::register_tagged(stage.builder, frame.registers, &mut scratch, *register);
        stage.store_u64(FRAME, site.registers, register_offset(*register), bits);
    }
    let context = emit_js_payload_word(stage.builder, frame.payload, std::mem::offset_of!(JsStepDispatch, head.lane_context));
    let record = stage.constant(u64::from(site.record));
    let helper = frame.helpers.target(&mut stage.builder, LaneHelper::VmStep);
    emit_lane_helper_call(stage.builder, frame.payload, helper, &[context, record])
}

/// The word a completed call-out answers: the canonical boolean of a decision, the register it
/// defined, or a placeholder where it defines none.
fn completion(stage: &mut Emit<'_, '_>, site: &CallOutSite, result: Option<Register>, holds: Option<ir::Value>)
    -> (ir::Value, RegisterFact) {
    match (holds, result) {
        (Some(holds), _) => {
            (super::boolean_word(stage.builder, holds), RegisterFact::definition(Type::language(quench_runtime::ValueType::Boolean), Representation::Tagged))
        }
        (None, Some(register)) => (stage.load_u64(FRAME, site.registers, register_offset(register)),
            RegisterFact::definition(Type::ANY, Representation::Tagged)),
        (None, None) => (stage.constant(0), RegisterFact::canonical()),
    }
}

/// A step no record exists for (a region with no VM helper, an operand that is no register): it
/// leaves at `miss`, exactly, and what the lowering emits past it is unreachable.
fn leave(stage: &mut Emit<'_, '_>, miss: ir::Block) -> (ir::Value, RegisterFact) {
    stage.jump(miss, &[]);
    let unreachable = stage.builder.create_block();
    stage.builder.switch_to_block(unreachable);
    (stage.constant(0), RegisterFact::canonical())
}

/// The projection of the global object's arena run behind its extras, and the object's tagged
/// word: its record by id, the realm and the layout proved by dependencies the lowering took.
/// The block's proof of it, where an earlier access made one (`LocalProofs::global`), is reused.
fn global_layout(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, (cell, fact): (GlobalSlot, quench_runtime::staged::ShapeFact), miss: ir::Block)
    -> Option<(CheckedObject, ir::Value)> {
    let bits = stage.constant(cell.global.to_bits());
    if let Some((kept, len)) = frame.state.proofs.global && kept.fact() == fact && len == cell.storage_len { return Some((kept, bits)); }
    let class_base = emit_js_payload_word(stage.builder, frame.payload, std::mem::offset_of!(JsStepDispatch, head.class_base));
    let address = ObjectRecord::prove(stage.builder, bits, class_base, None, miss).address(stage.builder);
    let inline = inline_storage(stage.builder, address);
    let (arena, _) = emit_js_property_values(stage.builder, frame.payload);
    let layout = CheckedObject::prove_arena_extras(stage.builder, (address, fact), inline, arena, cell.storage_len, miss)?;
    frame.state.proofs.global = Some((layout, cell.storage_len));
    Some((layout, bits))
}

/// The tagged word of the global's slot `held` names, read live under the layout the lowering holds.
pub(crate) fn global_slot_read(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, held: (GlobalSlot, quench_runtime::staged::ShapeFact), miss: ir::Block)
    -> Option<ir::Value> {
    let (layout, _) = global_layout(frame, stage, held, miss)?;
    let address = layout.arena_address(stage.builder, held.0.slot, miss)?;
    Some(stage.load_u64(quench_runtime::staged::Region::ObjectSlots.mutable(), address, 0))
}

/// A global-name store of the tagged `value` in `source` straight to the global's slot, which
/// the slot's observed type admits without a test (`GlobalSlot::dependency` retires the code
/// where a wider value is written); a heap value a marked owner would log, or a value this
/// type does not fit, calls the VM (`vm_step`).
pub(crate) fn global_write(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, source: Register, value: ir::Value)
    -> Option<()> {
    let held = frame.call_out?.cell?;
    let cell = held.0;
    let ty = frame.state.values[usize::from(source)].value.ty;
    let (Edges::ValueGuarded { miss } | Edges::Branch { miss, .. }) = frame.edges else { return None };
    if !ty.is_subtype(cell.ty) { return None; }
    let (layout, bits) = global_layout(frame, stage, held, miss)?;
    let address = layout.arena_address(stage.builder, cell.slot, miss)?;
    #[cfg(test)]
    quench_runtime::vm::call_out::counts::bump(&quench_runtime::vm::call_out::counts::DIRECT_GLOBAL_STORES);
    let record = stage.and_imm(bits, u64::from(u32::MAX));
    // A Number is immediate, so the row stores it with no gate and cannot leave.
    if ty.is_subtype(Type::NUMBER) {
        return bound_full_store(stage, record, address, (value, ty), miss);
    }
    let general = super::cold_block(stage.builder);
    let done = stage.builder.create_block();
    // The store gate of the staged row; a gate that does not admit the write leaves to the VM's step.
    bound_full_store(stage, record, address, (value, ty), general)?;
    stage.jump(done, &[]);
    stage.builder.switch_to_block(general);
    site_step(frame, stage, frame.call_out?, false, None)?;
    stage.jump(done, &[]);
    stage.builder.switch_to_block(done);
    // The VM's store may have moved the property arena.
    frame.state.proofs.global = None;
    Some(())
}
