use super::*;

impl Lowering<'_, '_> {
    /// A fused `typeof value === name` (`bytecode::typeof_test`): an immediate value's tag
    /// decides it, as `Vm::typeof_test_value` would, and a heap value's cell the VM reads
    /// (`typeof_is_in_lane`). A returning test completes the activation with its Boolean.
    pub(super) fn typeof_test(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let (index, negated) = quench_runtime::bytecode::typeof_test(instruction.unary_operator())?;
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) if instruction.result_flags() == 0 => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) if self.completion.status_and_pc() => Some(record),
            _ => return None,
        };
        let value = self.tagged(state, instruction.register_b());
        // The immediate classes whose name is the tested one, by the VM's own rule.
        let named = |sample: Value| quench_runtime::vm::coercion::immediate_typeof_index(sample) == Some(index);
        let mut immediate = self.raw_const(Lane::I8, 0);
        if named(Value::number(0.0)) {
            let double = quench_runtime::staged::value::is_double(&mut emit(&mut self.builder), value);
            let int = is_int_tagged(&mut self.builder, value);
            let number = self.raw_binary(Binary::Or, double, int);
            immediate = self.raw_binary(Binary::Or, immediate, number);
        }
        for sample in [Value::UNDEFINED, Value::DELETED, Value::NULL] {
            if named(sample) {
                let same = self.raw_compare_imm(Relation::Equal, value, sample.to_bits() as i64);
                immediate = self.raw_binary(Binary::Or, immediate, same);
            }
        }
        if named(Value::TRUE) {
            let boolean = quench_runtime::staged::value::is_boolean(&mut emit(&mut self.builder), value);
            immediate = self.raw_binary(Binary::Or, immediate, boolean);
        }
        let heap = quench_runtime::staged::value::is_heap(&mut emit(&mut self.builder), value);
        let (cell, join) = (self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.branch_if(heap, (cell, &[]), (join, &[immediate]));
        self.builder.switch_to_block(cell);
        let tested = self.raw_const(Lane::I64, index as i64);
        let is = self.helper_call(LaneHelper::TypeofIs, &[value, tested]);
        let is = self.raw_unary(Unary::Narrow(Lane::I8), is);
        self.goto(join, &[is]);
        self.builder.switch_to_block(join);
        let holds = if negated { self.raw_binary_imm(Binary::Xor, holds, 1) } else { holds };
        let boolean = self.boolean(holds);
        if let Some(record) = completes {
            self.return_value(record, boolean);
            return Some(false);
        }
        self.set_boolean(state, instruction.result_register(), boolean);
        Some(true)
    }

    /// `operation` by the interpreter's semantics (`staged::step`) on the emit stage, with
    /// `edges` as its control edges. The lane specializer decides every truthiness itself, so
    /// no step is left unresolved.
    pub(super) fn step(&mut self, state: &mut State, edges: frame::Edges, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>)
        -> quench_runtime::staged::Step<ir::Value, ()> {
        self.step_with(state, edges, operation, StepWith::default())
    }

    /// `step` of a named field access: the frame holds the `plans` the sites proved and what
    /// the access already has (`HeldRead`), and the stage addresses the property arena.
    pub(super) fn field_step(&mut self, state: &mut State, edges: frame::Edges, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>,
        access: (&LanePlans<'_>, HeldRead)) -> quench_runtime::staged::Step<ir::Value, ()> {
        self.step_with(state, edges, operation, StepWith { access: Some(access), ..StepWith::default() })
    }

    /// `step` with what the operation's frame needs beyond its edges (`StepWith`).
    pub(super) fn step_with(&mut self, state: &mut State, edges: frame::Edges, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>,
        StepWith { access, sites, site }: StepWith<'_>) -> quench_runtime::staged::Step<ir::Value, ()> {
        let property = self.field_region(self.at);
        let locals = !self.completion.numeric();
        let makes_environment = operation.makes_environment();
        let property_values = access.and_then(|_| self.entry.as_ref()).map(|entry| self.builder.use_var(entry.receiver.properties));
        let gate = access.map(|_| self.payload);
        let fresh = self.store_into_fresh(state, &operation);
        let mut stage = Emit { builder: &mut self.builder, property_values, gate, payload: Some(self.payload), facts: Default::default(), fresh };
        let fields = access.map(|(plans, held)| FieldFacts { plans, entry: self.entry.as_ref(), native_calls: self.native_calls,
            held, property,
            stable: StableHold { holders: &mut self.stable.holders, shapes: &mut self.stable.shapes, views: &mut self.stable.views, loops: &self.stable.stable_loops, online: self.vm.online, at: self.at } });
        let mut frame = EmitFrame { registers: &mut self.registers, written: &mut self.written,
            state, edges, constants: &self.program.constants, this: self.entry.as_ref().map(|entry| entry.this),
            payload: self.payload, helpers: self.helpers, locals, polls: &mut self.polls,
            roots: &mut self.stable.roots, number_returns: &mut self.number_returns, fields,
            call_out: sites.call_out, capture: sites.capture, html_dda: self.vm.html_dda,
            prototype: sites.prototype, table_entry: self.entry.as_ref(),
            held: self.held_view.take(), hole_leaves: std::mem::take(&mut self.hole_leaves), arrays_fixed: self.arrays_fixed, site, proven_arguments: Default::default(), rebinds: Default::default() };
        super::record_boxed_numbers();
        let step = quench_runtime::staged::step_apart(&mut stage, &mut frame, operation);
        // A Number the step boxed (a raw-double slot's read) reads back as the word it boxed.
        for (bits, number) in super::take_boxed_numbers() {
            let bits = self.builder.func.dfg.resolve_aliases(bits);
            state.proofs.note_number(bits, number);
        }
        debug_assert!(!matches!(step, quench_runtime::staged::Step::Unresolved), "the lane specializer decides every truthiness");
        // A step that ends in a branch makes no environment: only a closure's allocation, a value step, does.
        if makes_environment && matches!(edges, frame::Edges::Value | frame::Edges::ValueGuarded { .. }) { self.reload_environment(); }
        step
    }

    /// Whether a literal's allocation lowers here: plain, or the one that returns its value
    /// from a caller record, which completes the activation (`literal_done`).
    pub(super) fn literal_completes(&self, instruction: WideInstruction, pc: usize) -> bool {
        self.completion.status_and_pc() && (instruction.result_flags() == 0
            || (instruction.returns_from_frame() && matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_)))))
    }

    /// A literal's allocation ran: a returning one completes its activation with the result.
    pub(super) fn literal_done(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(super::lane_unit::Origin::Caller(record)) = self.unit.origins.get(pc).copied().filter(|_| instruction.returns_from_frame()) else { return true };
        let value = self.tagged(state, instruction.result_register());
        self.return_value(record, value);
        false
    }

    pub(super) fn typeof_index(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let source = instruction.register_b();
        // The register's fact, or the type the whole-program solve proves it before this instruction: either one JS type folds it.
        let fact = state.values[usize::from(source)].value.ty;
        let solved = self.cell_before(pc, source).ty;
        let known = [quench_runtime::ValueType::Number, quench_runtime::ValueType::Boolean, quench_runtime::ValueType::Null,
            quench_runtime::ValueType::Undefined, quench_runtime::ValueType::String, quench_runtime::ValueType::BigInt, quench_runtime::ValueType::Symbol]
            .into_iter().find(|kind| [fact, solved].into_iter().any(|ty| ty != Type::NEVER && ty.is_subtype(Type::language(*kind))));
        let Some(kind) = known else {
            // A value of no one known kind: the VM names it, a heap value by its cell (`typeof_index_in_lane`).
            // The staged row's heap arm needs a frame that proves a cell's kind, which this one does not.
            let bits = self.tagged(state, source);
            let index = self.helper_call(LaneHelper::TypeofIndex, &[bits]);
            let int = self.raw_unary(Unary::Narrow(Lane::I32), index);
            self.define(state, instruction.result_register(), int, RegisterFact::narrowest(Type::INT32));
            return Some(true);
        };
        let index = quench_runtime::bytecode::typeof_name_index(quench_runtime::vm::coercion::typeof_text(kind)).expect("a language typeof name");
        let int = self.raw_const(Lane::I32, i64::try_from(index).expect("index fits i64"));
        self.define(state, instruction.result_register(), int, RegisterFact::narrowest(Type::INT32));
        Some(true)
    }

    /// `value instanceof constructor` by the VM's steady case (`Vm::steady_instanceof`), which
    /// calls nothing; a case only the full algorithm decides leaves at the test.
    pub(super) fn instance_of(&mut self, state: &mut State, pc: u32, value: Operand, constructor: Operand) -> Option<ir::Value> {
        let exit = self.guard_exit(state, pc);
        let (value, constructor) = (self.operand_tagged(state, value)?, self.operand_tagged(state, constructor)?);
        let holds = self.helper_call(LaneHelper::InstanceOf, &[value, constructor]);
        let decided = self.raw_compare_imm(Relation::NotEqual, holds, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, decided, exit);
        Some(self.raw_unary(Unary::Narrow(Lane::I8), holds))
    }
}
