use super::*;

impl Lowering<'_, '_> {
    /// Lower the instruction at `pc`; whether control falls through to the next.
    pub(super) fn instruction(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        if self.consumed.contains(pc) { return true; }
        // A construction readiness a loop's entry tested holds only across quiet instructions
        // (`virtual_objects::hoist_constructions`): any other forgets it here, before its code.
        if !self.construct_ready.is_empty() && !self.keeps_constructions_ready(state, pc, instruction) {
            self.unready_constructions();
        }
        self.left_here = self.generic.contains(&pc);
        self.advance_inductions(pc);
        if let Some(frame) = self.unit.markers.iter().position(|marker| *marker == pc) {
            state.proofs.arrays.clear();
            // A call marker runs the call's guards and binds its frame's registers (from `base` up), and no guest
            // code: the views of the registers below stay proved.
            let base = self.unit.frames[frame].base;
            state.proofs.typed.retain(|view| view.register < base);
        } else if !retains_array_views(&self.program.constants, instruction) {
            state.proofs.arrays.clear();
            state.proofs.keep_typed_views();
        }
        if let Some(read) = self.hoisted.get(&pc).copied() {
            self.hoisted_read(state, pc, instruction, read);
            return true;
        }
        // An array literal kept as its elements allocates nothing, so the kept receivers and arrays it
        // leaves are kept past it (`virtual_array_construct`).
        if let Some(elements) = self.virtual_array_plan(pc, instruction) {
            self.virtual_array_construct(state, pc, instruction, &elements);
            self.native += 1;
            return true;
        }
        // `new Array(k)` of a constant `k` is kept as its holes (`kept_array_new`).
        if self.kept_array_new(state, pc, instruction) {
            self.native += 1;
            return true;
        }
        // Kept receivers (`virtual_objects`): a dead one is dropped, a read of a field it keeps
        // is its value, and an instruction that needs it as an object allocates it first.
        if !state.virtuals.is_empty() || !state.virtual_arrays.is_empty() {
            self.prune_virtuals(state, pc);
            state.virtual_arrays.retain(|kept| self.is_live(pc, kept.register));
            if crate::compiled::meaning_compiled::is_get_field(instruction) && let Some(falls) = self.virtual_field(state, pc, instruction) {
                self.native += 1;
                return falls;
            }
            // A kept array's element access or `.length` is lowered over its variables (`virtual_array_access`).
            if let Some(falls) = self.virtual_array_access(state, pc, instruction) {
                self.native += 1;
                return falls;
            }
            // A copy of a kept receiver's alias adds the destination as an alias (`virtual_copy`).
            if self.virtual_copy(state, instruction) {
                self.native += 1;
                return true;
            }
            if !state.virtuals.is_empty() || !state.virtual_arrays.is_empty() {
                if self.keeps_virtuals(state, pc, instruction) { self.drop_overwritten_virtuals(state, instruction); } else { self.materialize_virtuals(state, pc); }
            }
        }
        if let Some(plan) = self.virtual_construct_plan(pc, instruction) && self.virtual_construct(state, pc, instruction, plan) {
            self.native += 1;
            return true;
        }
        let pc32 = pc as u32;
        let plain = instruction.result_flags() == 0;
        let ran = match () {
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Throw) => self.throw(state, pc, instruction),
            // A Math call of exact arguments, folded at the lane's one fold point (`fold_static`).
            _ if self.fold_static(state, pc, instruction, None) => Some(true),
            // A call, a construction in the caller's window and a return: the staged operations
            // the tier's facts admit (`call_rows`).
            _ if self.scalar_call_admitted(state, pc, instruction) => Some(self.scalar_call(state, pc, instruction)),
            _ if plain && self.is_scalar_callee_load(pc, instruction) => Some(self.scalar_callee_load(state, pc, instruction)),
            _ if self.is_call_row(instruction) => self.call_row_step(state, pc, instruction),
            _ if self.is_call_out(instruction) => Some(self.call_out_step(state, pc, instruction)),
            _ if self.is_register_row(instruction) => Some(self.register_step(state, pc, instruction)),
            _ if self.is_window_row(pc, instruction) => Some(self.window_step(state, pc, instruction)),
            _ if matches!(quench_runtime::staged::Operation::<(), ()>::decode_load_name(instruction, ()), Some(quench_runtime::staged::Operation::LoadName { .. }))
                && plain && self.global_reads.contains_key(&pc) => Some(self.global_read(state, pc, instruction)),
            _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Nop)) && plain => match self.unit.markers.iter().position(|marker| *marker == pc) {
                Some(frame) => Some(self.inline_call(state, pc, frame)),
                None => Some(true),
            },
            _ if plain && quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) => {
                let Some(constant) = self.program.constants.get(instruction.constant_index()) else { return false };
                self.load_constant(state, pc, instruction, constant)
            }
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Return { .. })) && plain && self.completion.numeric() => {
                self.return_number(state, instruction.register_a(), pc32);
                Some(false)
            }
            // A capture's read or store, by its staged row.
            // A read of a written-once root binding whose function is held (`root_constant_read`).
            _ if plain && self.root_constant_at(pc).is_some() => self.root_constant_read(state, pc, instruction),
            _ if self.is_capture_row(pc, instruction) => Some(self.capture_step(state, pc, instruction)),
            _ if crate::compiled::meaning_compiled::is_get_field(instruction) && instruction.result_flags() & !(quench_runtime::bytecode::SET_THIS_REGISTER | quench_runtime::bytecode::RETURN_REGISTER) == 0 => {
                let ran = self.fold_native_member(state, pc, instruction)
                    .or_else(|| self.fold_own_constant(state, pc, instruction)).or_else(|| self.get_field(state, pc, instruction));
                // A named read of a declared parameter's member enters its destination with the member's type.
                if ran == Some(true) && !instruction.writes_current_this() && !instruction.returns_from_frame() {
                    if let (Some(base), Some((parts, None))) = (self.field_base(pc, instruction).and_then(|base| base.register_index()), self.field_chain(instruction)) {
                        if let [(atom, _)] = parts.as_slice() {
                            self.conform_declared_member_read(state, pc, base, *atom, instruction.result_register());
                        }
                    }
                }
                ran
            }
            _ if self.is_element_row(instruction) => Some(self.element_step(state, pc, instruction)),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::SetFunctionName) && plain && self.completion.status_and_pc() => self.function_name(state, pc, instruction),
            _ if matches!((plain, instruction.op().field_store()), (true, Some(_))) => self.set_field(state, pc, instruction),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) && instruction.numeric_local_target().is_none() => {
                self.operator(state, pc, instruction).or_else(|| self.binary_call_out(state, pc, instruction))
            }
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Unary) && plain && instruction.unary_operator() == quench_runtime::bytecode::UNARY_TYPEOF_INDEX
                && instruction.register_b() < self.unit.registers => self.typeof_index(state, pc, instruction),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Unary) && quench_runtime::bytecode::typeof_test(instruction.unary_operator()).is_some()
                && !self.helpers.is_numeric() => self.typeof_test(state, pc, instruction),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericUnary)
                && instruction.register_b() < self.unit.registers => self.unary(state, pc, instruction),
            _ if self.allocation_row(pc, instruction) => Some(self.allocation(state, pc, instruction)),
            _ if self.is_construction(instruction) => self.construct(state, pc, instruction),
            // The entry function's own `arguments` binding, whose values the activation kept
            // or an unmaterialized binding holds; an inlined callee's reads its own, which no
            // window of this activation holds.
            _ if plain && matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::ArgumentsLength { .. })) && self.own_arguments_read(pc, instruction) =>
                self.arguments_length(state, pc32, instruction),
            _ if plain && quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::ArgumentGet) && self.own_arguments_read(pc, instruction) =>
                self.arguments_get(state, pc32, instruction),
            _ if plain && quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Jump) => return self.jump(state, pc, instruction),
            _ if plain && instruction.op().control_flow_layout() == ControlFlowLayout::BranchTable => return self.branch_table(state, pc, instruction),
            _ if plain && matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IntCase { .. }))) => self.int_case_index(state, pc, instruction),
            _ if plain && matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::JumpIf { .. })) => {
                let target = instruction.jump_target() as usize;
                let exit = self.guard_exit(state, pc32);
                let taken_block = self.builder.create_block();
                let fall_block = self.builder.create_block();
                self.step(state, frame::Edges::Branch { target: taken_block, following: fall_block, miss: exit },
                    quench_runtime::staged::Operation::JumpIf { register: instruction.register_a(), taken: matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::JumpIf { taken: true, .. })) });
                self.continue_branch(state, pc, target, taken_block, fall_block);
                return false;
            }
            _ if plain && self.is_compare_jump(instruction) => {
                if self.compare_jump(state, pc, instruction) { return false; }
                None
            }
            _ => None,
        };
        match ran {
            Some(falls) => {
                self.native += 1;
                if falls { self.conform_declared_definitions(state, pc, instruction); }
                falls
            }
            None => self.unlowered(state, pc, instruction, plain),
        }
    }

    /// The end of a body whose helper neither completed nor missed: a termination stops with its
    /// status, and any other `status` is a leave in a callee.
    pub(super) fn stop_by_status(&mut self, status: ir::Value) {
        let terminated = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_TERMINATED as i64);
        let stopped = self.builder.create_block();
        let callee = self.builder.create_block();
        self.branch_if(terminated, (stopped, &[]), (callee, &[]));
        self.builder.switch_to_block(stopped);
        let terminate = self.raw_const(Lane::I32, TERMINATED_STATUS);
        self.leave_with(terminate, None);
        self.builder.switch_to_block(callee);
        let word = self.raw_const(Lane::I64, LaneExit::Callee.word() as i64);
        let stop = self.stop;
        self.goto(stop, &[word]);
    }

    /// A helper's result, read from the lane's register `destination` as the register's value.
    pub(super) fn reload_result(&mut self, state: &mut State, destination: Register) {
        let base = self.registers_base;
        let bits = self.raw_load(Lane::I64, regions::frame(), base, register_offset(destination));
        self.set_tagged(state, destination, bits);
        state.proofs.homed.insert(usize::from(destination));
    }

    /// The registers the body wrote that are live at `pc`: what a helper reads from the lane's file.
    pub(super) fn spilled(&self, pc: usize) -> Vec<Register> {
        self.written.iter().map(|register| register as Register)
            .take_while(|register| *register < self.unit.caller_registers)
            .filter(|register| self.is_live(pc, *register)).collect()
    }

    /// The live registers the body wrote since their last write-back, stored to the lane's registers for a
    /// helper to read. A register whose slot is current (`LocalProofs::homed`) is neither boxed nor stored.
    pub(super) fn home_written(&mut self, state: &mut State, pc: usize) {
        for register in self.spilled(pc) {
            let index = usize::from(register);
            if state.proofs.homed.contains(index) { continue; }
            let bits = self.tagged(state, register);
            let base = self.registers_base;
            self.store_word(regions::frame(), bits, base, register_offset(register));
            state.proofs.homed.insert(index);
        }
    }

    /// A helper's call, by its address in the running image's table (`LaneHelpers::address`).
    pub(super) fn helper_call(&mut self, helper: LaneHelper, arguments: &[ir::Value]) -> ir::Value {
        let address = self.helpers.target(&mut self.builder, helper);
        emit_lane_helper_call(&mut self.builder, self.payload, address, arguments)
    }

    /// The property arena's base again, which an allocation may have moved.
    pub(super) fn reload_property_arena(&mut self) {
        let properties = self.entry.as_ref().expect("entry facts").receiver.properties;
        let (values, _) = emit_js_property_values(&mut self.builder, self.payload);
        self.builder.def_var(properties, values);
    }

    /// After a helper that allocated, which may move the property arena: the projections
    /// into it retire and its base reloads.
    pub(super) fn allocated(&mut self, state: &mut State) {
        // The allocation is a safepoint: a collection it runs may promote every holder made before it.
        state.proofs.fresh.clear();
        state.proofs.layouts.clear();
        state.proofs.global = None;
        self.reload_property_arena();
    }

    /// The function and PC the unit instruction at `unit_pc` came from: the caller's own, or an inlined callee's
    /// at its own PC (`Origin`). None where the instruction has no origin.
    pub(super) fn origin_pc(&self, unit_pc: usize) -> Option<(u32, u32)> {
        match self.unit.origins.get(unit_pc)? {
            super::lane_unit::Origin::Caller(pc) => Some((self.function_id, *pc)),
            super::lane_unit::Origin::Inlined { frame, pc } => Some((self.unit.frames[*frame].target.function, *pc)),
        }
    }

    /// Whether the step at unit PC `pc` may allocate or run guest code, a safepoint that retires the fresh holders
    /// (`LocalProofs::fresh`): the settled effect of its origin's step (`FunctionFacts::may_allocate`, the caller's
    /// or an inlined callee's), else the operation's kind row, which is ANY for a store.
    fn safepoint_at(&self, pc: usize) -> bool {
        let settled = self.vm.facts.zip(self.origin_pc(pc))
            .map(|(facts, (function, at))| facts.function_facts(function).may_allocate(at));
        settled.unwrap_or_else(|| crate::compiled::performed(self.unit.instructions[pc])
            .map_or(true, |operation| { let effects = operation.effects(); effects.may_alloc || effects.may_call_js }))
    }

    /// After the instruction at `pc` is lowered: a safepoint retires every fresh holder, and an object or array literal
    /// it allocated is fresh from here (`allocation`'s own safepoint retired the holders before it). The holder's word is
    /// the one `register_word` names it by, which a store's owner reads.
    pub(super) fn retire_fresh(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) {
        // A kept literal (`virtual_objects`) holds no object of its own, so it is no fresh holder.
        let holder = self.allocation_row(pc, instruction).then(|| quench_runtime::staged::Allocation::decode(instruction, self.straight_following(pc)))
            .flatten().and_then(|(destination, allocation)| (makes_holder(allocation) && !self.is_kept(state, destination)).then_some(destination));
        if holder.is_none() && self.safepoint_at(pc) { state.proofs.fresh.clear(); }
        if let Some(destination) = holder {
            let word = self.register_word(state, destination);
            state.proofs.fresh.push(word);
        }
    }

    /// Narrows register `register`'s fact to the type the solve proves it holds before `pc` (`cell_before`), where that is a
    /// narrower type of a tagged word (as `State::narrow`). A stored value the solve proves immediate then takes the
    /// immediate store path, which needs no barrier log (4.7 (a)).
    pub(super) fn refine_to_solved(&self, state: &mut State, pc: usize, register: Register) {
        let solved = self.cell_before(pc, register).ty;
        let fact = state.values[usize::from(register)];
        if fact.value.representation == Representation::Tagged && solved != fact.value.ty && solved.is_subtype(fact.value.ty) {
            state.values[usize::from(register)] = RegisterFact { tagged: fact.tagged, ..RegisterFact::definition(solved, Representation::Tagged) };
        }
    }

    /// Whether the store `operation` writes into a fresh holder (`Emit::fresh`): a named field or an element store
    /// whose holder is fresh (`holder_fresh`).
    pub(super) fn store_into_fresh(&mut self, state: &mut State, operation: &quench_runtime::staged::Operation<ir::Value, EmitProof>) -> bool {
        let owner = match operation {
            quench_runtime::staged::Operation::SetField { base, .. } | quench_runtime::staged::Operation::StoreField { base, .. } => *base,
            quench_runtime::staged::Operation::ElementWrite { base, .. } => Some(*base),
            _ => return false,
        };
        self.holder_fresh(state, owner)
    }

    /// Whether the holder `owner` names (a register's, or fixed `this`'s) is one this unit allocated with no safepoint
    /// since: its resolved word is in `LocalProofs::fresh`, so a store into it needs no barrier log.
    pub(super) fn holder_fresh(&mut self, state: &mut State, owner: Option<Register>) -> bool {
        self.owner_word(state, owner).is_some_and(|word| state.proofs.fresh.contains(&word))
    }

    /// After a helper that may also have changed any object's shape or storage (a
    /// construction, an adding store): every proof of an object retires, as another register
    /// may name it.
    pub(super) fn reshaped(&mut self, state: &mut State) {
        state.call_effect();
        self.reload_property_arena();
    }

    /// `source`'s value into `destination`, with every fact proved of it.
    pub(super) fn copy(&mut self, state: &mut State, destination: Register, source: Register) -> ir::Value {
        frame::register_copy(&mut self.builder, &mut self.registers, &mut self.written, state,
            destination, source)
    }
}
