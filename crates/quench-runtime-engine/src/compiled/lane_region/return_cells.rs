use super::*;

impl Lowering<'_, '_> {
    /// `state` past a join with `incoming`: each register's facts as both paths prove them.
    /// A join no verified fact describes invalidates the region.
    pub(super) fn join_states(&mut self, state: &mut State, incoming: &State, pc: usize) {
        // A kept receiver never reaches a join: the block that would carry it allocates it.
        if !state.virtuals.is_empty() || !incoming.virtuals.is_empty() { self.valid = false; declined("a kept receiver reaches a join", pc); }
        if !state.virtual_arrays.is_empty() || !incoming.virtual_arrays.is_empty() { self.valid = false; declined("a kept array reaches a join", pc); }
        // Only the same pre-branch SSA definition can survive both numeric arms.
        state.proofs.fields.retain(|field| incoming.proofs.fields.contains(field));
        state.proofs.headers.retain(|header| incoming.proofs.headers.contains(header));
        state.join_kept_shapes(incoming);
        state.proofs.int32_projections.retain(|projection| incoming.proofs.int32_projections.contains(projection));
        state.proofs.numbers.retain(|projection| incoming.proofs.numbers.contains(projection));
        state.proofs.admitted_captures.retain(|view| incoming.proofs.admitted_captures.contains(view));
        // A view both arrivals prove is the join's, kept where either arrival kept it (`keep_typed_views`).
        state.proofs.typed.retain_mut(|view| incoming.proofs.typed.iter().find(|other| TypedProof { kept: view.kept, ..**other } == *view)
            .map(|other| view.kept |= other.kept).is_some());
        state.proofs.meet_homed(&incoming.proofs.homed);
        state.proofs.fresh.retain(|word| incoming.proofs.fresh.contains(word));
        // A record both arrivals name as the same value is the join's (`LocalProofs::records`).
        state.proofs.records.retain(|record| incoming.proofs.records.contains(record));
        for (index, incoming) in incoming.values.iter().copied().enumerate() {
            let current = state.values[index];
            if current != incoming {
                // Arms that hold one view keep it, as both define it; arms holding different views meet in the
                // tagged view (`repr(joined)` may be Float64, which the int32 arm never defines).
                let representation = if current.value.representation == incoming.value.representation {
                    current.value.representation
                } else {
                    self.valid &= current.tagged.current() && incoming.tagged.current();
                    Representation::Tagged
                };
                let Some(mut fact) = RegisterFact::admit(Fact {
                    ty: current.value.ty.join(incoming.value.ty), representation,
                    binding: BindingTime::Dynamic, proof: Proof::Definition,
                    projection: Projection::Exact,
                }) else { self.valid = false; declined("a join no fact describes", pc); return; };
                // A tagged view no later use reads is dropped (`demand`): a leave rebuilds it from the unboxed view.
                fact.tagged = if current.tagged == incoming.tagged { current.tagged }
                    else if current.tagged.current() && incoming.tagged.current()
                        && (representation == Representation::Tagged || self.demand_from(pc + 1, index).tagged()) { TaggedView::Variable }
                    else { TaggedView::Unavailable };
                state.values[index] = fact;
            }
        }
    }

    /// The ToInt32 of `number` (`emit_number_to_int32`).
    pub(super) fn number_to_int32(&mut self, number: ir::Value, exit: ir::Block) -> ir::Value {
        super::emit_number_to_int32(&mut self.builder, number, exit)
    }

    /// `register`'s int32 view (`Registers::int`).
    pub(super) fn int_var(&mut self, register: usize) -> Variable {
        frame::register_int(&mut self.builder, &mut self.registers, register)
    }

    /// `register`'s float64 view (`Registers::float`).
    pub(super) fn float_var(&mut self, register: usize) -> Variable {
        frame::register_float(&mut self.builder, &mut self.registers, register)
    }

    /// Leaves with `word`: a lane body stops with it as its result word (`stop`); a static
    /// typed region returns it as the source PC its canonical code continues at.
    pub(super) fn return_at(&mut self, word: u64) {
        match self.completion {
            Completion::Body => {
                let word = self.raw_const(Lane::I64, word as i64);
                let stop = self.stop;
                self.goto(stop, &[word]);
            }
            Completion::Number | Completion::Scalar(_) => {
                let miss = self.raw_const(Lane::I32, NUMBER_ENTRY_MISS);
                let no_word = self.raw_const(Lane::I64, 0);
                self.leave(&[miss, no_word]);
            }
        }
    }

    /// A Number return of `register`: the staged return, after the interrupt check and the
    /// type proof the lane keeps (a register not proved Number invalidates the region).
    pub(super) fn return_number(&mut self, state: &mut State, register: Register, pc: u32) {
        self.finishes = true;
        if !state.values[usize::from(register)].value.ty.is_subtype(Type::NUMBER) {
            self.valid = false;
            declined("a number return of a non-number", pc as usize);
            self.return_at(0);
            return;
        }
        self.interrupt_check(state, pc);
        let exit = self.guard_exit(state, pc);
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, quench_runtime::staged::Operation::Return { source: register });
    }

    /// The cell unit register `register` holds just before the unit instruction at `unit_pc`, read from the whole-program
    /// solve of the function the instruction came from (`FunctionFacts::cell_at`): the caller's, or an inlined callee's at
    /// its own register. ANY where there is no solve, the instruction is no origin's, or the register is not the callee's.
    pub(crate) fn cell_before(&self, unit_pc: usize, register: Register) -> quench_runtime::typed_ir::Cell {
        self.solved_cell(unit_pc, register, false)
    }

    /// The cell unit register `register` holds just after the unit instruction at `unit_pc` (`FunctionFacts::cell_after`),
    /// with the origins `cell_before` reads. ANY where `cell_before` is.
    pub(crate) fn cell_after(&self, unit_pc: usize, register: Register) -> quench_runtime::typed_ir::Cell {
        self.solved_cell(unit_pc, register, true)
    }

    /// The type the solve proves the result of the unit instruction at `unit_pc` has in `register`, once it is written:
    /// None where the solve proves nothing (ANY).
    pub(crate) fn proven_result(&self, unit_pc: usize, register: Register) -> Option<Type> {
        Some(self.cell_after(unit_pc, register).ty).filter(|ty| *ty != Type::ANY)
    }

    /// The type the solve proves the element read at unit `pc` yields, once it defines its destination (`proven_result`).
    pub(crate) fn proven_element(&self, pc: usize) -> Option<Type> {
        self.proven_result(pc, self.unit.instructions[pc].result_register())
    }

    /// The origin function, its PC and the local register of unit register `register` at `unit_pc`, as the solve
    /// names them: the caller's own register, or an inlined callee's at its own. None where the instruction is no
    /// origin's, or the register is not the callee's.
    pub(super) fn origin_local(&self, unit_pc: usize, register: Register) -> Option<(u32, u32, Register)> {
        match self.unit.origins.get(unit_pc)? {
            super::lane_unit::Origin::Caller(pc) => Some((self.function_id, *pc, register)),
            super::lane_unit::Origin::Inlined { frame, pc } => {
                let frame = &self.unit.frames[*frame];
                Some((frame.target.function, *pc, register.checked_sub(frame.base)?))
            }
        }
    }

    /// Whether index register `index` is non-negative and below the length of `array` before the unit instruction at
    /// `unit_pc`, as the solve proves it (`FunctionFacts::below_length` with the index's solved range): an access
    /// `array[index]` there needs no bounds test. A string receiver is its own length's array.
    pub(crate) fn proven_below_length(&self, unit_pc: usize, index: Register, array: Register) -> bool {
        let Some(facts) = self.vm.facts else { return false };
        let (Some((function, pc, index_local)), Some((_, _, array_local))) =
            (self.origin_local(unit_pc, index), self.origin_local(unit_pc, array)) else { return false };
        let non_negative = self.cell_before(unit_pc, index).ty.int_range().is_some_and(|range| range.min >= 0);
        non_negative && facts.function_facts(function).below_length(pc, index_local, array_local)
    }

    /// The register the solve names as the one unit register `index` is below before the unit instruction at `unit_pc`
    /// (`FunctionFacts::bound_below`), in the unit's registers. A loop's counter is below its bound at the sum that indexes an
    /// array, so its entry compares the bound's reach with the length once (`loop_versions::plan`).
    pub(crate) fn bound_below(&self, unit_pc: usize, index: Register) -> Option<Register> {
        let facts = self.vm.facts?;
        let (function, pc, index_local) = self.origin_local(unit_pc, index)?;
        let bound = facts.function_facts(function).bound_below(pc, index_local)?;
        match self.unit.origins.get(unit_pc)? {
            super::lane_unit::Origin::Caller(_) => Some(bound),
            super::lane_unit::Origin::Inlined { frame, .. } => self.unit.frames[*frame].base.checked_add(bound),
        }
    }

    /// Whether the unit instruction at `unit_pc` may resize an array of any class, as the solve reads its step
    /// (`FunctionFacts::may_resize_any`). An instruction the solve has no origin for may resize.
    pub(crate) fn may_resize(&self, unit_pc: usize) -> bool {
        let Some(facts) = self.vm.facts else { return true };
        let Some((function, pc)) = self.origin_pc(unit_pc) else { return true };
        facts.function_facts(function).may_resize_any(pc)
    }

    pub(super) fn solved_cell(&self, unit_pc: usize, register: Register, after: bool) -> quench_runtime::typed_ir::Cell {
        let Some(facts) = self.vm.facts else { return quench_runtime::typed_ir::Cell::ANY };
        let Some((function, pc, local)) = self.origin_local(unit_pc, register) else { return quench_runtime::typed_ir::Cell::ANY };
        let function = facts.function_facts(function);
        let cell = if after { function.cell_after(pc, local) } else { function.cell_at(pc, local) };
        cell.unwrap_or(quench_runtime::typed_ir::Cell::ANY)
    }

    /// The semantic join selects one ABI format for every successful return.
    /// Guards and termination leave without publishing a result.
    pub(super) fn finish_number_returns(&mut self) -> NativeNumberResult {
        let ty = self.number_returns.iter().fold(Type::NEVER, |ty, returned| ty.join(returned.fact.ty));
        let format = if repr(ty) == Representation::Int32 {
            NativeNumberResult::Int32
        } else { NativeNumberResult::Float64 };
        for returned in std::mem::take(&mut self.number_returns) {
            self.builder.switch_to_block(returned.block);
            let value = self.builder.block_params(returned.block)[0];
            let value = match (format, returned.fact.representation) {
                (NativeNumberResult::Int32, representation) => {
                    let integer = match representation {
                        Representation::Int32 => value,
                        Representation::Float64 => self.raw_unary(Unary::ToSignedSaturating(Lane::I32), value),
                        Representation::Int64 | Representation::Tagged => unreachable!("typed numeric return"),
                    };
                    self.raw_unary(Unary::ZeroExtend(Lane::I64), integer)
                }
                (NativeNumberResult::Float64, Representation::Int32) => {
                    let number = self.raw_unary(Unary::FromSigned(Lane::F64), value);
                    self.raw_unary(Unary::Bitcast(Lane::I64), number)
                }
                (NativeNumberResult::Float64, Representation::Float64) => self.raw_unary(Unary::Bitcast(Lane::I64), value),
                (_, Representation::Int64 | Representation::Tagged) => unreachable!("typed numeric return"),
            };
            let success = self.raw_const(Lane::I32, 0);
            self.leave(&[success, value]);
        }
        format
    }
}
