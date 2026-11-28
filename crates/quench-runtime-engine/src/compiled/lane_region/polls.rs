//! Interrupt polls in a lane region: which ones no reentry reaches, and the owner specialized there.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

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
    let mut possible: quench_runtime::dataflow::BitSet = quench_runtime::dataflow::BitSet::with_capacity(function.dfg.num_blocks());
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

/// The work, in instructions, a loop may do between two polls while its back edge polls nothing: its trips
/// bounded by the solver times its body (`Lowering::trip_bound`).
const POLL_FREE_WORK: u64 = 65_536;

/// The machine stack a checked entry keeps free beyond its budget (`check_native_stack`): the frames of any bounded
/// callee subtree whose entry is not checked, which `stack_check_free` admits only when they fit within it.
pub(crate) const STACK_CHECK_MARGIN: usize = 64 * 1024;

/// An upper bound on the native frame bytes one bytecode instruction lowers to. Not measured; it turns
/// `STACK_CHECK_MARGIN` into an instruction budget for a subtree.
const STACK_FRAME_BYTES_PER_INSTRUCTION: u64 = 64;

/// Whether the entry of `function` needs no stack check of its own: the functions it calls form a finite acyclic set of
/// named bodies (`CallGraph::stack_subtree_instructions`) whose frames fit in `STACK_CHECK_MARGIN`, which the caller's
/// checked entry keeps free. Recursion, an unnamed call, or a larger subtree keeps the check. No graph keeps it.
pub(crate) fn stack_check_free(graph: Option<&quench_runtime::compile::call_graph::CallGraph>, function: u32) -> bool {
    graph.and_then(|graph| graph.stack_subtree_instructions(function))
        .is_some_and(|instructions| instructions.saturating_mul(STACK_FRAME_BYTES_PER_INSTRUCTION) <= STACK_CHECK_MARGIN as u64)
}

/// Whether `instruction` makes no call and no allocation: a jump, a move, a local slot, or a Number operation.
fn call_and_allocation_free(instruction: WideInstruction) -> bool {
    use quench_runtime::staged::meaning::{means, Meaning};
    [Meaning::Move, Meaning::Nop, Meaning::Jump, Meaning::ConditionalJump, Meaning::TruthJump, Meaning::CompareJump,
        Meaning::NumericBinary, Meaning::NumericArithmetic, Meaning::NumericUnary, Meaning::IncDec,
        Meaning::LoadLocal, Meaning::StoreLocal, Meaning::BranchTable, Meaning::LoadConst]
        .into_iter().any(|meaning| means(instruction, meaning))
}

impl Lowering<'_, '_> {
    pub(crate) fn interrupt_check(&mut self, state: &mut State, pc: u32) {
        if self.fault_poll {
            let interrupt = self.interrupt;
            let proof = super::super::EmitProof::Safepoint { interrupt };
            self.step(state, frame::Edges::Value, quench_runtime::staged::Operation::Safepoint { proof });
            return;
        }
        // The shared back-edge poll (`staged::poll`): one flag load and one guard. The exit is cold;
        // whatever an interrupted loop does afterwards runs there, not on the edge.
        let interrupt = self.interrupt;
        let exit = self.guard_exit(state, pc);
        quench_runtime::staged::poll::back_edge_poll(&mut super::super::object_slots::emit(&mut self.builder), interrupt, exit).expect("a guard always emits");
    }

    /// The interrupt check of the back edge at `pc` to `target`, skipped where the loop polls nothing
    /// (`poll_free_loop`).
    pub(crate) fn back_edge_check(&mut self, state: &mut State, pc: usize, target: usize) {
        if !self.poll_free_loop(pc, target) { self.interrupt_check(state, pc as u32); }
    }

    /// Whether the loop of the back edge `pc` to `target` polls nothing: it is innermost, so its work is its
    /// body times its trips, makes no call and no allocation, and its trips are bounded so that the body times
    /// the trips stays within `POLL_FREE_WORK` instructions. Any other loop polls.
    fn poll_free_loop(&self, pc: usize, target: usize) -> bool {
        let instructions = &self.unit.instructions;
        if pc <= target { return false; }
        let body = &instructions[target..=pc];
        let innermost = (target..pc).all(|at| {
            let mut back = false;
            quench_runtime::bytecode::for_each_instruction_successor(at, instructions[at], |to| back |= to <= at);
            !back
        });
        if !innermost || !body.iter().all(|instruction| call_and_allocation_free(*instruction)) { return false; }
        self.trip_bound(target, pc).is_some_and(|trips| trips.saturating_mul(body.len() as u64) <= POLL_FREE_WORK)
    }

    /// The trips the loop from `target` to its back edge `pc` takes at most, from the solver's range of its
    /// counter at the header (`cell_before`, over every entry). The counter is a register the last instruction
    /// before the back edge increments by one and no other instruction of the loop writes, and no jump of the
    /// loop reaches the back edge past that increment, so every trip increments it once: its range's width bounds the trips.
    fn trip_bound(&self, target: usize, pc: usize) -> Option<u64> {
        let instructions = &self.unit.instructions;
        let increment = pc - 1;
        if increment < target { return None; }
        let skips = (target..increment).any(|at| {
            let mut reaches = false;
            quench_runtime::bytecode::for_each_instruction_successor(at, instructions[at], |to| reaches |= to == pc);
            reaches
        });
        if skips { return None; }
        let range = instructions[increment].written_registers().find_map(|counter| {
            let sole = self.steps_by_one(instructions[increment], counter)
                && !(target..increment).any(|at| instructions[at].written_registers().any(|written| written == counter));
            sole.then(|| self.cell_before(target, counter).ty.int_range()).flatten()
        })?;
        u64::try_from((range.max - range.min).checked_add(1)?).ok()
    }

    pub(crate) fn emit_fault_poll(&mut self) {
        frame::calls::safepoint_parts(&mut self.builder, &mut self.polls,
            super::super::EmitProof::Safepoint { interrupt: self.interrupt });
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
            } else if super::number_domains::is_double(ty) {
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
