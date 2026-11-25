//! Interrupt polls in a lane region: which ones no reentry reaches, and the owner specialized there.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// Specializes the interrupt owner at the polls no reentry reaches (`retain_closed_polls`); with
/// none, the owner's call (`interrupt_owner_call`) is removed.
pub(crate) fn specialize_closed_polls(native: &mut ir::Function, reentries: &[ir::Block],
    polls: &mut smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>, owner: ir::Inst) {
    retain_closed_polls(native, reentries, polls);
    if polls.is_empty() { native.layout.remove_inst(owner); }
    else { specialize_interrupt_owner(native, owner, polls); }
}

/// The interrupt owner's proof: a call of its lane helper (`LaneHelper::InterruptOwner`), made
/// once after helper binding and before guest execution, which the closed polls are gated on.
pub(crate) fn interrupt_owner_call(builder: &mut FunctionBuilder<'_>, helpers: LaneHelpers, context: ir::Value,
    isa: &dyn cranelift_codegen::isa::TargetIsa) -> ir::Inst {
    let target = helpers.target(builder, LaneHelper::InterruptOwner);
    let signature = crate::compiled::call_signature::typed_signature(isa.default_call_conv(), &[types::I64], &[types::I64]);
    let signature = builder.import_signature(signature);
    let exclusive = emit(builder).call_signature(signature, target, &[context])[0];
    match builder.func.dfg.value_def(exclusive) {
        ir::ValueDef::Result(call, _) => call,
        _ => unreachable!("a call's result"),
    }
}

/// A callback retires ownership only on paths reachable from its completion. A callback
/// after a closed loop cannot invalidate that loop's entry proof retroactively.
pub(crate) fn retain_closed_polls(function: &ir::Function, reentries: &[ir::Block], polls: &mut smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>) {
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

/// Gates each selected poll on the owner's proof (`interrupt_owner_call`). Each selected poll lies
/// before any reachable callback completion.
fn specialize_interrupt_owner(function: &mut ir::Function, call: ir::Inst, polls: &[ir::Inst]) {
    use cranelift_codegen::cursor::{Cursor, FuncCursor};
    let mut cursor = FuncCursor::new(function);
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

impl Lowering<'_, '_> {
    pub(crate) fn interrupt_check(&mut self, state: &mut State, pc: u32) {
        if self.fault_poll {
            let interrupt = self.interrupt;
            let proof = super::super::EmitProof::Safepoint { interrupt, miss: None, fault_poll: true };
            self.step(state, frame::Edges::Value, quench_runtime::staged::Operation::Safepoint { proof });
            return;
        }
        // The shared back-edge poll (`staged::poll`): one flag load and one guard. The exit is cold;
        // whatever an interrupted loop does afterwards runs there, not on the edge.
        let interrupt = self.interrupt;
        let exit = self.guard_exit(state, pc);
        quench_runtime::staged::poll::back_edge_poll(&mut super::super::object_slots::emit(&mut self.builder), interrupt, exit).expect("a guard always emits");
    }

    pub(crate) fn emit_fault_poll(&mut self) {
        frame::calls::safepoint_parts(&mut self.builder, &mut self.polls,
            super::super::EmitProof::Safepoint { interrupt: self.interrupt, miss: None, fault_poll: true });
    }

    /// Joining edges must not move a guard, conversion or recovery across the
    /// interrupt check. Only already-conforming SSA views may share this latch;
    /// flag-based checks retain their exact-PC recovery on the original edge.
    pub(crate) fn shared_backedge(&mut self, state: &State, target: usize) -> bool {
        if !self.fault_poll || !self.shared_headers.contains(target) || !self.entered.contains(target) { return false; }
        let Some(block) = self.blocks.get(target).copied().flatten() else { return false };
        let Some(types) = self.facts.at(target) else { return false };
        if !types.iter().copied().map(Type::machine).enumerate().all(|(index, ty)| {
            if !self.is_live(target, index as Register) { return true; }
            let fact = state.values[index];
            fact.value.ty.is_subtype(ty) && if ty == Type::INT32 {
                fact.value.representation == Representation::Int32
            } else if ty == Type::NUMBER {
                fact.value.representation == Representation::Float64
            } else { fact.tagged.current() }
        }) { return false; }
        let latch = match self.poll_latches[block] {
            Some(latch) => latch,
            None => {
                let latch = self.builder.create_block();
                self.poll_latches[block] = Some(latch);
                latch
            }
        };
        self.finishes = true;
        self.goto(latch, &[]);
        true
    }
}
