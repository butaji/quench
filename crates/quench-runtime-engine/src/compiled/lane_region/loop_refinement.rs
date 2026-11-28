use super::*;

impl Lowering<'_, '_> {
    /// A family edge refines the value and existing SSA copies, not heap contents.
    pub(super) fn refine_object_family(&mut self, state: &mut State, bits: ir::Value) {
        let bits = self.builder.func.dfg.resolve_aliases(bits);
        for fact in &mut state.values {
            let TaggedView::Value(value) = fact.tagged else { continue };
            if self.builder.func.dfg.resolve_aliases(value) == bits && !fact.value.ty.is_direct_object_family() {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::narrowest(fact.value.ty.direct_object_part().unwrap_or(Type::DIRECT_OBJECT_FAMILY)) };
            }
        }
    }

    /// Move-only latches can carry the nullish test's direct-object proof to
    /// the next field receiver. These are guarded input contracts, including
    /// the first iteration, never assumptions about values stored in a field.
    pub(super) fn require_object_latches(&mut self, instructions: &[WideInstruction], leaders: &BitSet,
        loops: &[(usize, usize)]) {
        for &(header, back) in loops {
            if !matches!(crate::compiled::decoded(instructions[back]), Some(quench_runtime::staged::Operation::Jump))
                || self.unit.markers.iter().any(|pc| (header..=back).contains(pc)) { continue; }
            let first = instructions[header];
            if !matches!(crate::compiled::decoded(first), Some(quench_runtime::staged::Operation::GetField { .. })) || first.result_flags() != 0 { continue; }
            let Some(FieldLookup::Atom { base, .. }) = first.field_lookup() else { continue };
            let Some(receiver) = base.register_index() else { continue };
            let mut required = receiver;
            let mut inputs = smallvec::SmallVec::<[(usize, Register); INLINE_LOOP_INPUTS]>::new();
            inputs.push((header, receiver));
            let mut predicate = None;
            for pc in (header + 1..back).rev() {
                let instruction = instructions[pc];
                match () {
                    _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && instruction.result_flags() == 0 => {
                        if instruction.register_a() == required { required = instruction.register_b(); }
                    }
                    _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Nop)) => {},
                    _ => { predicate = Some(pc); break; }
                }
                if leaders.contains(pc) { inputs.push((pc, required)); }
            }
            let Some(pc) = predicate else { continue };
            let Some(comparison) = nullish_comparison(&self.program.constants, instructions[pc], |_| Type::ANY)
                .filter(|comparison| !comparison.strict && comparison.negated
                    && instructions[pc].jump_target() as usize > back) else { continue };
            let root = |register| move_root(instructions, leaders, (header, pc), register);
            if root(required) != root(comparison.source) { continue; }
            if !inputs.iter().any(|&(leader, _)| leader == pc + 1) { inputs.push((pc + 1, required)); }
            if !inputs.iter().all(|&(pc, register)| self.facts.at(pc)
                .and_then(|types| types.get(usize::from(register)))
                .is_some_and(|ty| ty.is_subtype(Type::DIRECT_OBJECT_FAMILY)
                    || Type::DIRECT_OBJECT_FAMILY.is_subtype(*ty))) { continue; }
            for (pc, register) in inputs { self.facts.require(pc, register, Type::DIRECT_OBJECT_FAMILY); }
        }
    }

    /// An unchanged arm preserves a checked entry hypothesis. Every definition
    /// and backedge must preserve it too; failed hypotheses are removed before
    /// reanalysis so a dependent candidate cannot keep using their assumptions.
    pub(super) fn refine_inductive_inputs(&mut self, initial: &mut [Type], instructions: &[WideInstruction],
        boundaries: &BitSet, loops: &[(usize, usize)],
        own_shape: &dyn Fn(usize) -> Option<(Register, u32)>) {
        let header = self.coverage.entry();
        let Some((_, back)) = loops.iter().copied().filter(|(start, _)| *start == header)
            .max_by_key(|(_, back)| *back) else { return };
        // Alternative markers add edges beyond the bytecode Nop transfer.
        // Keep their canonical lowering until this analysis models those edges.
        if self.unit.markers.iter().zip(&self.unit.frames).any(|(pc, frame)|
            (header..=back).contains(pc) && frame.alternative.is_some()) { return; }
        let mut candidates: BitSet = BitSet::with_capacity(initial.len());
        self.facts.for_each_definition(instructions, boundaries, &self.program.constants, &writes, self.native_calls, header..=back,
            |pc, register, ty| {
                let index = usize::from(register);
                if (header..=back).contains(&pc) && register < self.unit.caller_registers
                    && self.is_live(header, register) && self.written.contains(index)
                    && initial[index].is_any_or_number() && ty.is_int32() {
                    candidates.insert(index);
                }
            });
        if let Some(facts) = super::facts::inductive_int32(initial, candidates, header..=back,
            instructions, boundaries, &self.program.constants, &writes, own_shape, self.native_calls, &self.raises) {
            self.facts = facts;
        }
    }

    /// The registers a loop never writes whose every use proves them Numbers, each a Number header
    /// contract: proved once on the entering edge, so the body holds it as a double and tests it no more.
    fn number_inputs(&self, header: usize, back: usize, defined: &BitSet, candidates: &mut Vec<(super::facts::Contract, std::ops::RangeInclusive<usize>)>) {
        let Some(types) = self.facts.at(header) else { return };
        for instruction in &self.unit.instructions[header..=back] {
            for register in super::facts::proves_numbers(*instruction, types.len(), &self.program.constants, &writes, self.native_calls) {
                let index = usize::from(register);
                let contract = (header, register, Type::NUMBER);
                if !defined.contains(index) && !types[index].is_subtype(Type::NUMBER) && self.is_live(header, register)
                    && !candidates.iter().any(|(known, _)| *known == contract) {
                    candidates.push((contract, header..=back));
                }
            }
        }
    }

    /// An inner loop's counters and indices carried as int32s: a register its header joins as
    /// a Number (an index computed before the loop, say) while every definition in the loop
    /// yields an int32 takes an int32 header contract. The edge entering the loop proves it
    /// once (`conform`), instead of each use re-proving an exact int32 of a double.
    pub(super) fn refine_loop_inputs(&mut self, entry: usize, initial: &[Type], instructions: &[WideInstruction],
        boundaries: &BitSet, loops: &[(usize, usize)],
        own_shape: &dyn Fn(usize) -> Option<(Register, u32)>) {
        let mut candidates = Vec::new();
        for &(header, back) in loops.iter().filter(|(header, _)| *header != self.coverage.entry()) {
            let Some(types) = self.facts.at(header) else { continue };
            let mut defined: BitSet = BitSet::with_capacity(types.len());
            for instruction in &instructions[header..=back] {
                for register in writes(*instruction) { defined.insert(usize::from(register)); }
            }
            for (index, ty) in types.iter().enumerate() {
                if number_domains::is_double(*ty) && defined.contains(index) && self.is_live(header, index as Register) {
                    candidates.push(((header, index as Register, Type::INT32), header..=back));
                }
            }
            self.number_inputs(header, back, &defined, &mut candidates);
        }
        if let Some(facts) = super::facts::loop_int32(entry, initial, candidates, instructions, boundaries,
            &self.program.constants, &writes, own_shape, self.native_calls, &self.raises) {
            self.facts = facts;
        }
    }

    /// The type already proved for an operand at a use. Feedback is not enough:
    /// an admitted field read carries its actual loop-entry contract, even when
    /// the general analysis has no field feedback (as in linked AOT).
    fn int32_operand_at(&self, pc: usize, operand: Operand, boundaries: &BitSet) -> bool {
        if super::facts::operand_type(&self.program.constants, operand, |_| Type::ANY).is_int32() { return true; }
        let Some(mut register) = operand.register_index() else { return false };
        if self.cell_before(pc, register).ty.is_int32() { return true; }
        for at in (0..pc).rev() {
            if boundaries.contains(at + 1) || self.unit.markers.contains(&at) { return false; }
            let instruction = self.unit.instructions[at];
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return false; }
            if writes(instruction).contains(&register) {
                if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && instruction.result_register() == register {
                    register = instruction.register_b();
                } else {
                    return instruction.defined_register() == Some(register)
                        && self.hoisted.get(&at).is_some_and(|read| read.ty == Type::INT32);
                }
            }
        }
        false
    }

    /// A capture may choose the same checked integer view as a bitwise use, or
    /// as the already-proved integer on the other side of a Number relation.
    /// This selects an entry guard; it does not prove the capture's type.
    /// Whether the capture the unit instruction at `pc` reads, of the unit's own function's environment, holds an int32 at
    /// every read the solve settles (`FunctionFacts::capture_cells`): its read takes the integer view with no guard.
    pub(super) fn capture_solved_int(&self, pc: usize) -> bool {
        let instruction = self.unit.instructions[pc];
        let Some(super::lane_unit::Origin::Caller(_)) = self.unit.origins.get(pc) else { return false };
        instruction.capture_depth() == 0 && self.vm.facts.is_some_and(|facts| {
            let slot = quench_runtime::bytecode::ImmediateLayout::capture_slot(instruction.imm());
            facts.function_facts(self.function_id).capture_cells().iter().any(|(captured, cell)| *captured == slot && cell.ty.is_int32())
        })
    }

    pub(super) fn capture_int32_use(&self, pc: usize, boundaries: &BitSet) -> bool {
        let mut aliases: BitSet = BitSet::with_capacity(usize::from(self.unit.registers));
        aliases.insert(usize::from(self.unit.instructions[pc].result_register()));
        for at in pc + 1..self.unit.instructions.len() {
            if boundaries.contains(at) || self.unit.markers.contains(&at) { break; }
            let instruction = self.unit.instructions[at];
            let operator = match () {
                _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. })) => Some(instruction.binary_operator_field()),
                _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) => Some(instruction.binary_operator()),
                _ => None,
            };
            if let Some(operator) = operator {
                let operands = [instruction.operand_b(), instruction.operand_c()];
                let captured = |operand: Operand| operand.register_index()
                    .is_some_and(|register| aliases.contains(usize::from(register)));
                if operands.iter().copied().any(captured) {
                    if super::operations::NumberBitwise::from_operator(operator).is_some() { return true; }
                    if super::operations::NumberRelation::from_operator(operator).is_some()
                        && operands.iter().copied().enumerate().any(|(index, operand)| captured(operand)
                            && self.int32_operand_at(at, operands[1 - index], boundaries)) { return true; }
                }
            }
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { break; }
            let copied = matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && aliases.contains(usize::from(instruction.register_b()));
            for register in writes(instruction) { aliases.remove(usize::from(register)); }
            if copied { aliases.insert(usize::from(instruction.result_register())); }
            if aliases.is_empty() { break; }
        }
        false
    }

    /// Whether an enclosing loop's entry hoisted the capture read at `pc` already (its body holds
    /// this loop's, so the read is invariant there too): reading it again at each entry here would
    /// be a helper call inside the outer loop, spilling every value live across it.
    pub(super) fn hoisted_by_enclosing_loop(&self, pc: usize, owner: Option<Register>) -> bool {
        owner.is_none() && matches!(crate::compiled::performed(&self.unit.instructions[pc]), Some(quench_runtime::staged::Operation::LoadCapture { .. })) && self.hoisted.contains_key(&pc)
    }

    /// The hoisted read of the callee an inlined capture read belongs to, where it is the callee
    /// the frame's call binds: `Ok(None)` for a caller's own read, `Err` where the read is not
    /// admitted (its loop then reads it where it runs).
    pub(super) fn admitted_callee_read(&mut self, state: &mut State, pc: usize, reads: &[InvariantRead], boundaries: &BitSet,
        exit: ir::Block, held: Option<Register>) -> Result<Option<ir::Value>, ()> {
        let super::lane_unit::Origin::Inlined { frame, .. } = self.unit.origins[pc] else { return Ok(None) };
        if let Some(register) = held {
            // The closure the entry holds, admitted by its function on this edge: the capture view
            // every iteration reads through is the one it resolves here.
            let callee = self.tagged(state, register);
            // A call plan that names exactly this frame's target proves the callee's body, which the held closure is
            // (`invariant_callee_read`: the loop neither writes the register nor reads the callee again): no compare.
            if !self.plan_call(state, exit, frame) {
                let function = self.unit.frames[frame].target.function;
                let matched = self.function_binding(callee, function);
                emit_js_guard(&mut self.builder, matched, exit);
            }
            return Ok(Some(callee));
        }
        let read = invariant_callee_read(self.unit, reads.iter().map(|read| read.pc), frame, boundaries)
            .and_then(|read| self.hoisted.get(&read).copied()).ok_or(())?;
        let target = self.unit.frames[frame].target;
        let admitted = match target.callee {
            CalleeBinding::Identity(identity) => read.ty.function_identity() == Some(identity),
            CalleeBinding::Function => read.ty.body_index() == Some(target.function),
        };
        if admitted { Ok(Some(self.builder.use_var(read.value))) } else { Err(()) }
    }
}
