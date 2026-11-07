//! Interrupt polls in a lane region: which ones no reentry reaches, and the owner specialized there.

use super::*;

/// Specializes the interrupt owner at the polls no reentry reaches (`retain_closed_polls`).
pub(super) fn specialize_closed_polls(native: &mut ir::Function, reentries: &[ir::Block],
    polls: &mut smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>, helpers: LaneHelpers, context: ir::Value,
    isa: &dyn cranelift_codegen::isa::TargetIsa) {
    if polls.is_empty() { return; }
    retain_closed_polls(native, reentries, polls);
    if !polls.is_empty() { specialize_interrupt_owner(native, helpers, context, polls, isa); }
}

/// A callback retires ownership only on paths reachable from its completion. A callback
/// after a closed loop cannot invalidate that loop's entry proof retroactively.
pub(super) fn retain_closed_polls(function: &ir::Function, reentries: &[ir::Block], polls: &mut smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>) {
    if reentries.is_empty() { return; }
    let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(function);
    let mut possible: CompoundBitSet = CompoundBitSet::with_capacity(function.dfg.num_blocks());
    let mut pending = smallvec::SmallVec::<[ir::Block; INLINE_EFFECT_EDGES]>::new();
    for &block in reentries {
        if possible.insert(block.index()) { pending.push(block); }
    }
    while let Some(block) = pending.pop() {
        for successor in cfg.succ_iter(block) {
            if possible.insert(successor.index()) { pending.push(successor); }
        }
    }
    polls.retain(|poll| {
        let block = function.layout.inst_block(*poll).expect("published poll block");
        let effect = if possible.contains(block.index()) { Reentry::Possible } else { Reentry::Closed };
        effect == Reentry::Closed
    });
}

/// Bind the current owner's proof once, after helper binding and before guest execution.
/// Each selected poll lies before any reachable callback completion.
fn specialize_interrupt_owner(function: &mut ir::Function, helpers: LaneHelpers, context: ir::Value,
    polls: &[ir::Inst], isa: &dyn cranelift_codegen::isa::TargetIsa)
{
    use cranelift_codegen::cursor::{Cursor, FuncCursor};
    let entry = function.layout.entry_block().expect("native entry");
    let after = match helpers {
        #[cfg(feature = "aot")]
        LaneHelpers::Bound(table, _) => match function.dfg.value_def(table) {
            ir::ValueDef::Result(inst, _) => Some(inst), _ => unreachable!("bound helper table"),
        },
        LaneHelpers::Numeric | LaneHelpers::Process(_) => None,
        #[cfg(feature = "aot")]
        LaneHelpers::Linked(..) => unreachable!("helpers bind before lowering"),
    };
    let mut cursor = FuncCursor::new(function);
    if let Some(inst) = after { cursor.goto_inst(inst); cursor.next_inst(); }
    else { cursor.goto_inst(cursor.func.layout.first_inst(entry).expect("entry instruction")); }
    let address = helpers.address(cursor.ins(), LaneHelper::InterruptOwner);
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(ir::AbiParam::new(types::I64));
    signature.returns.push(ir::AbiParam::new(types::I64));
    let signature = cursor.func.import_signature(signature);
    let call = cursor.ins().call_indirect(signature, address, &[context]);
    let exclusive = cursor.func.dfg.inst_results(call)[0];
    let mut gates = Vec::with_capacity(polls.len());
    for &poll in polls {
        let before = cursor.func.layout.inst_block(poll).expect("poll block");
        let polled = cursor.func.dfg.make_block();
        cursor.func.layout.split_block(polled, poll);
        let next = cursor.func.layout.next_inst(poll).expect("poll continuation");
        let resume = cursor.func.dfg.make_block();
        cursor.func.layout.split_block(resume, next);
        cursor.goto_bottom(before); let gate = cursor.ins().brif(exclusive, resume, &[], polled, &[]);
        gates.push((gate, false));
        cursor.goto_bottom(polled); cursor.ins().jump(resume, &[]);
    }
    cursor.func.effect_specializations.push(ir::function::EffectSpecialization { proof: call, gates });
}
