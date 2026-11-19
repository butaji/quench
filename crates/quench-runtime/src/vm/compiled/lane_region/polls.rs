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

/// The address of the interrupt owner helper: a symbol for a frameless Number region, whose
/// loader resolves it (`relocs::numeric_symbol`), else the helper table's.
fn interrupt_owner_address(cursor: &mut cranelift_codegen::cursor::FuncCursor<'_>, helpers: LaneHelpers) -> ir::Value {
    if helpers.names_symbols() {
        return crate::vm::jit_backend::symbol_in(cursor, RelocKind::Helper as u32, LaneHelper::InterruptOwner as u32);
    }
    let build = cursor.ins();
    match helpers.table_address(LaneHelper::InterruptOwner) {
        HelperAddress::Embedded(address) => build.iconst(types::I64, address),
        HelperAddress::Loaded { table, offset } => build.load(types::I64, super::super::invariant(), table, offset),
    }
}

/// Bind the current owner's proof once, after helper binding and before guest execution.
/// Each selected poll lies before any reachable callback completion.
fn specialize_interrupt_owner(function: &mut ir::Function, helpers: LaneHelpers, context: ir::Value,
    polls: &[ir::Inst], isa: &dyn cranelift_codegen::isa::TargetIsa)
{
    use cranelift_codegen::cursor::{Cursor, FuncCursor};
    let entry = function.layout.entry_block().expect("native entry");
    let after = match helpers {
        LaneHelpers::Bound(table, _) => match function.dfg.value_def(table) {
            ir::ValueDef::Result(inst, _) => Some(inst), _ => unreachable!("bound helper table"),
        },
        LaneHelpers::Linked(..) => unreachable!("helpers bind before lowering"),
        _ => None,
    };
    let mut cursor = FuncCursor::new(function);
    if let Some(inst) = after { cursor.goto_inst(inst); cursor.next_inst(); }
    else { cursor.goto_inst(cursor.func.layout.first_inst(entry).expect("entry instruction")); }
    let address = interrupt_owner_address(&mut cursor, helpers);
    let signature = crate::vm::compiled::call_signature::typed_signature(isa.default_call_conv(), &[types::I64], &[types::I64]);
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

impl Lowering<'_, '_> {
    pub(super) fn interrupt_check(&mut self, state: &mut State, pc: u32) {
        if self.fault_poll {
            let interrupt = self.interrupt;
            let proof = super::super::EmitProof::Safepoint { interrupt, miss: None, fault_poll: true };
            self.step(state, frame::Edges::Value, crate::staged::Operation::Safepoint { proof });
            return;
        }
        // The shared back-edge poll (`staged::poll`): one flag load and one guard. The exit is cold;
        // whatever an interrupted loop does afterwards runs there, not on the edge.
        let interrupt = self.interrupt;
        let exit = self.guard_exit(state, pc);
        crate::staged::poll::back_edge_poll(&mut super::super::object_slots::emit(&mut self.builder), interrupt, exit).expect("a guard always emits");
    }

    pub(super) fn emit_fault_poll(&mut self) {
        frame::calls::safepoint_parts(&mut self.builder, &mut self.polls,
            super::super::EmitProof::Safepoint { interrupt: self.interrupt, miss: None, fault_poll: true });
    }

    /// Joining edges must not move a guard, conversion or recovery across the
    /// interrupt check. Only already-conforming SSA views may share this latch;
    /// flag-based checks retain their exact-PC recovery on the original edge.
    pub(super) fn shared_backedge(&mut self, state: &State, target: usize) -> bool {
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
