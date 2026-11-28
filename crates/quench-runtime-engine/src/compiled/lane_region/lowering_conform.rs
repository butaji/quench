use super::*;

impl<'a, 'f> Lowering<'a, 'f> {
    /// Conform the machine view and discharge the target's semantic requirements.
    pub(super) fn conform(&mut self, state: &mut State, target: usize) {
        let Some(types) = self.facts.at(target).map(<[Type]>::to_vec) else {
            self.canonicalize(state, target);
            return;
        };
        for (index, ty) in types.iter().copied().enumerate() {
            let register = index as Register;
            if !self.is_live(target, register) { continue; }
            // A kept receiver's register holds no value of its own: the join carries its object (`carry_kept`).
            if self.is_kept(state, register) { continue; }
            // A parameter's entry fact varies per call: a mismatched value declines the
            // region (the lane runs the header) instead of exiting native code.
            let saved = self.proving_entry;
            if target == self.coverage.entry() && index < self.params { self.proving_entry = true; }
            let proved = self.conform_register(state, target, register, ty);
            self.proving_entry = saved;
            if !proved { return; }
        }
    }

    /// One live register conformed to `target`'s fact `ty`; whether the edge still reaches it.
    pub(super) fn conform_register(&mut self, state: &mut State, target: usize, register: Register, ty: Type) -> bool {
        let index = usize::from(register);
        let held = state.values[index].value.representation;
        match number_domains::target_representation(ty) {
            Representation::Int32 if held != Representation::Int32 => {
                let exit = self.guard_exit(state, target as u32);
                self.int(state, register, exit);
            }
            Representation::Int64 if held != Representation::Int64 => {
                let exit = self.guard_exit(state, target as u32);
                frame::register_long_value(&mut self.builder, &mut self.registers, state, register, ty, exit);
            }
            Representation::Float64 if held != Representation::Float64 => {
                let exit = self.guard_exit(state, target as u32);
                let number = self.number(state, register, exit);
                let variable = self.float_var(index);
                self.builder.def_var(variable, number);
            }
            Representation::Tagged => { self.tagged(state, register); }
            Representation::Int32 | Representation::Int64 | Representation::Float64 => {}
        }
        self.conform_object(state, target, register, ty);
        self.conform_language(state, target, register, ty);
        // A fact this edge does not prove and cannot guard leaves at the target, exactly:
        // the edge loses native code, never the body (AGENTS.md, check once).
        if self.proves(state, register, ty, target) { return true; }
        let exit = self.guard_exit(state, target as u32);
        self.goto(exit, &[]);
        let unreached = self.builder.create_block();
        self.builder.switch_to_block(unreached);
        false
    }

    /// An object fact of the target the edge has not proved: the direct-object family by one
    /// guard, an exact shape guarded on this edge with the owner's header read once, so the
    /// edge keeps native code.
    fn conform_object(&mut self, state: &mut State, target: usize, register: Register, ty: Type) {
        let index = usize::from(register);
        if ty == Type::DIRECT_OBJECT_FAMILY && !state.values[index].value.ty.is_direct_object_family() && !self.solver_object(target, register) {
            let exit = self.guard_exit(state, target as u32);
            let bits = self.tagged(state, register);
            let object = quench_runtime::staged::value::is_direct_object(&mut super::object_slots::emit(&mut self.builder), bits);
            emit_js_guard(&mut self.builder, object, exit);
            self.refine_object_family(state, bits);
        }
        // An entry-fixed receiver is a class cell with no record shape to guard (`fixed_cells`): the edge leaves, unproven.
        if let Some(shape) = ty.object_shape() && !state.values[index].value.ty.is_subtype(ty) && !self.entry_fixed(Some(register)) {
            let exit = self.guard_exit(state, target as u32);
            // An ordinary object names no record a shape guard could hold (`object_header`): the edge keeps no shape.
            let Some(header) = self.object_header(state, Some(register), false, exit) else { return };
            let word = self.register_word(state, register);
            headers::prove_header_shape(&mut self.builder, state, word, header, shape, exit);
            state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                ..RegisterFact::narrowest(Type::direct_object(shape)) };
            state.proofs.kept_shapes.insert(word, shape);
        }
    }

    /// A language fact of the target the edge has not proved, when `ty` is exactly a set
    /// `scalar::checkable` names: one guard that the register holds a member, then the set as its
    /// fact. A String member is proved by its direct tag, and any other string by the VM's `typeof`
    /// helper on a slow path only. An array or plain object member (`ARRAY`, `OBJECT_PLAIN`, bits no
    /// `ValueType` names) is proved by its family tag alone. A Number, BigInt or Symbol set is not tested here and leaves to `proves`.
    fn conform_language(&mut self, state: &mut State, target: usize, register: Register, ty: Type) {
        use quench_runtime::ValueType::{Boolean, Function, Null, Object, String as Text, Undefined};
        use quench_runtime::staged::Stage;
        use quench_runtime::typed_ir::Languages;
        use quench_runtime::staged::value::{is_boolean, is_direct_array, is_direct_function, is_direct_object, is_direct_string, is_null, is_undefined};
        use super::declared_checks::{ARRAY, OBJECT_PLAIN};
        let index = usize::from(register);
        let kinds = [Boolean, Undefined, Null, Object, Function, Text];
        let families = [ARRAY, OBJECT_PLAIN];
        let bits = kinds.into_iter().filter(|kind| Type::language(*kind).is_subtype(ty))
            .fold(Languages::EMPTY, |set, kind| set | Languages::of(kind)).bits()
            | families.into_iter().filter(|family| Type::languages(*family).is_subtype(ty)).fold(0u16, |set, family| set | family);
        if !scalar::checkable(bits) || Type::languages(bits) != ty || state.values[index].value.ty.is_subtype(ty) { return; }
        let value = self.tagged(state, register);
        let exit = self.guard_exit(state, target as u32);
        // The one-word tests of the set's members other than String, any of which proves the value.
        let others = {
            let stage = &mut super::object_slots::emit(&mut self.builder);
            let mut member: Option<ir::Value> = None;
            for kind in kinds.into_iter().filter(|kind| *kind != Text && Languages::from_bits(bits).contains(*kind)) {
                let test = match kind {
                    Boolean => is_boolean(stage, value),
                    Undefined => is_undefined(stage, value),
                    Null => is_null(stage, value),
                    Object => is_direct_object(stage, value),
                    Function => is_direct_function(stage, value),
                    // Not in `kinds`: no exact one-word test.
                    _ => continue,
                };
                member = Some(match member { Some(any) => stage.any(any, test), None => test });
            }
            // A plain object is the plain family's tag, as `Object` is above; an array is the array tag.
            for family in families.into_iter().filter(|family| bits & *family != 0) {
                let test = if family == ARRAY { is_direct_array(stage, value) } else { is_direct_object(stage, value) };
                member = Some(match member { Some(any) => stage.any(any, test), None => test });
            }
            member
        };
        if Languages::from_bits(bits).contains(Text) {
            // A direct string is accepted by its tag; any other value falls to the VM's `typeof`
            // test, which only that slow path pays for.
            let direct = is_direct_string(&mut super::object_slots::emit(&mut self.builder), value);
            let accepted = match others {
                Some(others) => super::object_slots::emit(&mut self.builder).any(others, direct),
                None => direct,
            };
            let slow = self.builder.create_block();
            let done = self.builder.create_block();
            emit(&mut self.builder).branch_if(accepted, (done, &[]), (slow, &[]));
            self.builder.switch_to_block(slow);
            let name = self.raw_const(Lane::I64, quench_runtime::bytecode::typeof_name_index("string").expect("a typeof name") as i64);
            let named = self.helper_call(LaneHelper::TypeofIs, &[value, name]);
            let holds = self.raw_compare_imm(Relation::NotEqual, named, 0);
            emit_js_guard(&mut self.builder, holds, exit);
            self.goto(done, &[]);
            self.builder.switch_to_block(done);
        } else {
            let member = others.expect("a checkable set names a member");
            emit_js_guard(&mut self.builder, member, exit);
        }
        state.values[index] = RegisterFact { tagged: state.values[index].tagged,
            ..RegisterFact::definition(ty, Representation::Tagged) };
    }

    pub(super) fn entry_state(&mut self, pc: usize, registers: usize) -> State {
        let mut state = State::canonical(registers);
        self.home_entry_registers(&mut state, pc);
        let Some(types) = self.facts.at(pc) else { return state };
        for (index, ty) in types.iter().copied().enumerate() {
            if !self.is_live(pc, index as Register) { continue; }
            let Some(fact) = self.entry_fact(pc, ty) else { self.valid = false; declined("no entry fact for a live register", pc); return state; };
            state.values[index] = fact;
        }
        state
    }

    /// The solver's type of each register at unit PC `pc` (`AotFacts::block_entry`, the cell every register
    /// of the block starts with), by unit register: the origin's function and PC (an inlined frame's registers
    /// sit at its `base`). Empty where no facts were given or where the origin's block starts no solved block. Where
    /// the cells are trusted, each covered function's live dependency keys are recorded (`StableUses::cells`): an
    /// edge that widens one of them retires the code.
    fn cell_types(&mut self, pc: usize) -> Vec<Option<Type>> {
        let Some(facts) = self.vm.facts else { return Vec::new() };
        let (function, at, base) = match self.unit.origins.get(pc) {
            Some(super::lane_unit::Origin::Caller(at)) => (self.function_id, *at, 0),
            Some(super::lane_unit::Origin::Inlined { frame, pc: at }) => {
                let call = &self.unit.frames[*frame];
                (call.target.function, *at, usize::from(call.base))
            }
            None => return Vec::new(),
        };
        let Some(cells) = facts.function_facts(function).block_entry(at) else { return Vec::new() };
        let covered = std::iter::once(self.function_id).chain(self.unit.frames.iter().map(|frame| frame.target.function));
        let live: Vec<_> = covered.flat_map(|function| facts.function_facts(function).dependencies().iter().copied()
            .filter(|key| facts.live(*key)).collect::<Vec<_>>()).collect();
        self.stable.cells.extend(live);
        (0..usize::from(self.unit.registers)).map(|register| register.checked_sub(base)
            .and_then(|own| cells.get(own)).map(|cell| cell.ty).filter(|ty| *ty != Type::NEVER)).collect()
    }

    /// Narrows each register's fact of `state` at unit PC `pc` to its solved type (`cell_types`), where the
    /// solved type is narrower and the held representation still holds it (a tagged fact, or an unboxed one
    /// of the same machine representation). A fact is never widened; a register the solve holds nothing for stays.
    pub(super) fn narrow_to_cells(&mut self, state: &mut State, pc: usize) {
        for (index, cell) in self.cell_types(pc).into_iter().enumerate() {
            let Some(ty) = cell else { continue };
            let Some(fact) = state.values.get(index).copied() else { continue };
            let held = fact.value.representation;
            let narrower = ty != fact.value.ty && ty.is_subtype(fact.value.ty);
            if narrower && (held == Representation::Tagged || number_domains::target_representation(ty) == held) {
                state.values[index] = RegisterFact { tagged: fact.tagged, ..RegisterFact::definition(ty, held) };
            }
        }
    }

    /// The live caller registers a once-entered region entry loaded from their frame slots.
    fn home_entry_registers(&self, state: &mut State, pc: usize) {
        if pc == self.coverage.entry() && !self.completion.numeric()
            && edges::entered_once(&self.unit.instructions, pc, |at| self.coverage.lowered(at)) {
            for register in (0..self.unit.caller_registers).filter(|register| self.is_live(pc, *register)) {
                state.proofs.homed.insert(usize::from(register));
            }
        }
    }

    /// The fact a register of type `ty` has at `pc`: guarded at the region entry, a definition after.
    fn entry_fact(&self, pc: usize, ty: Type) -> Option<RegisterFact> {
        let entry = pc == self.coverage.entry();
        RegisterFact::admit(Fact { ty, representation: number_domains::target_representation(ty),
            binding: if entry { BindingTime::Location } else { BindingTime::Dynamic },
            proof: if !entry { Proof::Definition }
                else if ty == Type::ANY { Proof::CanonicalValue } else { Proof::EntryGuard },
            projection: Projection::Exact,
        })
    }

    /// The unit's block leaders (`leaders`), made once for the unit.
    pub(super) fn unit_leaders(&self) -> &BitSet {
        self.unit_leaders.get_or_init(|| std::rc::Rc::new(leaders(&self.unit.instructions)))
    }

    /// The one-unit string constant `operand` names, for a strict comparison that decides on its unit
    /// (`Lowering::unit_compare`).
    pub(super) fn one_unit_constant(&self, operand: Operand) -> Option<u16> {
        if operand.kind()? != OperandKind::Constant { return None; }
        match self.program.constants.get(usize::from(operand.payload()))? {
            quench_runtime::bytecode::Constant::String(text) => {
                let mut units = text.encode_utf16();
                let unit = units.next()?;
                units.next().is_none().then_some(unit)
            }
            quench_runtime::bytecode::Constant::StringUnits(units) => match units[..] {
                [unit] => Some(unit),
                _ => None,
            },
            _ => None,
        }
    }
}
