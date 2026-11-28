use super::*;

impl Lowering<'_, '_> {
    /// The alias region of the named field access at unit PC `pc`, where its key is declared.
    pub(super) fn field_region(&self, pc: usize) -> Option<cranelift_codegen::ir::AliasRegion> {
        self.field_regions.get(&self.field_key(pc)?).copied()
    }

    /// The closed set of classes the solver names for the object the access at unit PC `pc` reads or
    /// stores (`FunctionFacts::access_classes`, of the function the unit's origin of `pc` is from). The
    /// one source of which class an access reads: the AOT loads (`ahead_images::fields`) read it too.
    pub(super) fn solved_classes(&self, pc: usize) -> Option<quench_runtime::typed_ir::Classes> {
        let facts = self.vm.facts?;
        let (function, pc) = match *self.unit.origins.get(pc)? {
            super::lane_unit::Origin::Caller(pc) => (self.function_id, pc),
            super::lane_unit::Origin::Inlined { frame, pc } => (self.unit.frames.get(frame)?.target.function, pc),
        };
        let classes = facts.function_facts(function).access_classes(pc);
        matches!(classes, quench_runtime::typed_ir::Classes::Set { .. }).then_some(classes)
    }

    /// The register (or `unit.registers` for `this`) the field access at `pc` reads or writes, where
    /// `instruction` is one, by its operands alone (`field_owner` decides whether the solver names its class).
    pub(super) fn accessed_register(&self, pc: usize, instruction: WideInstruction) -> Option<usize> {
        let count = usize::from(self.unit.registers);
        match () {
            // A native method's read folds to its holder's function whatever its receiver (`fold_native_member`):
            // a string primitive's `charCodeAt` reads no object's field.
            _ if matches!(self.global_reads.get(&pc), Some(quench_runtime::vm::global_facts::GlobalRead::NativeMember { .. })) => None,
            _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) && !super::reads_length(self.program, instruction) => {
                let base = self.field_base(pc, instruction);
                base.and_then(|base| if base == quench_runtime::bytecode::FieldBase::THIS { Some(count) }
                    else { base.register_index().map(usize::from) }).filter(|_| self.reads_own_field(instruction))
            }
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => Some(match (instruction.field_store_register(), self.unit.origins[pc]) {
                (Some(register), _) => usize::from(register),
                (None, super::lane_unit::Origin::Inlined { frame, .. }) => usize::from(self.unit.frames[frame].receiver),
                (None, _) => count,
            }),
            _ => None,
        }
    }

    /// Admit once at the edge; later raw stores consume the compile-time domain proof.
    pub(super) fn admit_field_writes(&mut self, binding: &mut FieldSlotBinding, writes: Type, exit: ir::Block) {
        if binding.writes.is_some_and(|domain| writes.is_subtype(domain)) {
            if let Some(admitted) = binding.admitted { emit_js_guard(&mut self.builder, admitted, exit); }
            return;
        }
        let admitted = self.field_writes_admitted(binding, writes);
        emit_js_guard(&mut self.builder, admitted, exit);
    }

    /// Whether the slot `binding` names admits `writes` (`LaneHelper::FieldWriteAdmission`),
    /// the domain then recorded on the binding.
    pub(super) fn field_writes_admitted(&mut self, binding: &mut FieldSlotBinding, writes: Type) -> ir::Value {
        let shape = self.raw_unary(Unary::ZeroExtend(Lane::I64), binding.receiver);
        let slot = binding.slot.read(&mut self.builder);
        let (admitted, domain) = self.slot_writes_admitted(shape, slot, writes);
        binding.writes = Some(binding.writes.map_or(domain, |held| held.join(domain)));
        admitted
    }

    /// Admits at entry the writes `writes` to `slot` of `shape`, failing through `exit`
    /// (`Heap::admit_slot_writes`).
    pub(crate) fn admit_slot_writes(&mut self, (shape, slot): (ir::Value, ir::Value), writes: Type, exit: ir::Block) {
        let (admitted, _) = self.slot_writes_admitted(shape, slot, writes);
        emit_js_guard(&mut self.builder, admitted, exit);
    }

    /// Whether the store `instruction` at unit pc `unit_pc` needs no run-time admission of `writes`: its receiver's proven class
    /// seeds the slot (`tag_proofs::store_seeded`). Where it does not, the admission (`admit_slot_writes`) stays the slow path.
    pub(crate) fn store_proven(&self, unit_pc: usize, instruction: WideInstruction, writes: Type) -> bool {
        self.vm.facts.is_some_and(|facts| tag_proofs::store_seeded(facts, self.unit, self.function_id, unit_pc, instruction, writes))
    }

    fn slot_writes_admitted(&mut self, shape: ir::Value, slot: ir::Value, writes: Type) -> (ir::Value, Type) {
        let domain = quench_runtime::typed_ir::SlotType::from_type(writes);
        let encoded = self.raw_const(Lane::I64, domain as i64);
        let helper = self.helpers.target(&mut self.builder, LaneHelper::FieldWriteAdmission);
        let admitted = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[shape, slot, encoded]);
        (self.raw_compare_imm(Relation::NotEqual, admitted, 0), domain.ty())
    }

    pub(super) fn emit_store_atom_admitted(&mut self, atom: Atom) -> ir::Value {
        self.emit_store_atom_admission(atom).0
    }

    /// `emit_store_atom_admitted`, with its two parts where it tests both at the store
    /// (`store_atom_admission`).
    fn emit_store_atom_admission(&mut self, atom: Atom) -> (ir::Value, Option<(ir::Value, ir::Value)>) {
        if let Some(held) = self.loop_held_atom(atom) { return (held, None); }
        let specialized = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.stores_specialized));
        let specialized = self.raw_compare_imm(Relation::NotEqual, specialized, 0);
        if let Some(held) = self.store_held_unnamed(atom, specialized) { return (held, None); }
        let free = self.atom_unnamed(atom);
        (self.raw_binary(Binary::And, specialized, free), Some((specialized, free)))
    }

    pub(super) fn set_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let source = instruction.register_a();
        if source >= self.unit.registers || instruction.writes_current_this() || instruction.returns_from_frame() {
            return None;
        }
        let register = self.store_owner(pc, instruction)?;
        // A store no program code observes is dead: it emits nothing (`field_store_unread`).
        if register.is_some_and(|base| self.field_store_unread(pc, base, instruction.atom_index())) {
            return Some(true);
        }
        let carried = match self.loop_fields.iter().find(|field| field.stores.contains(&pc)) {
            Some(field) => Some(*self.hoisted.get(&field.reads[0])?),
            None => None,
        };
        let exit = self.guard_exit(state, pc as u32);
        // A fixed class's receiver stores its union slot in place, with no record (`fixed_cells`).
        if carried.is_none() && self.fixed_field_store(state, pc, instruction, register, exit) {
            return Some(true);
        }
        let carried_int = carried.filter(|read| read.representation == Representation::Int32)
            .map(|_| self.int(state, source, exit))
            .or_else(|| carried.filter(|read| read.representation == Representation::Float64)
                .map(|_| self.number(state, source, exit)));
        self.refine_to_solved(state, pc, source);
        let ty = state.values[usize::from(source)].value.ty;
        let operation = quench_runtime::staged::Operation::SetField { value: source, base: register,
            atom: instruction.atom_index(), site: instruction.cache_site_index(),
            strict: matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { strict: true, .. })) };
        // A store a previous body left at may add its property (`compiled::field_add_in_lane`),
        // which changes the object's shape and may move the property arena: every proof of
        // an object retires, as another register may name it, and `this`'s facts are read
        // again.
        // The site's own shapes still store in place first: a body that left here once (before
        // the site cached a shape, say) keeps its arms, and only their miss adds.
        if self.generic.contains(&pc) && self.completion.status_and_pc() {
            let (arms, _) = self.site_store_plan(instruction.cache_site_index(), ty);
            let admitted = (!arms.is_empty()).then(|| self.store_atom_admitted(state, instruction.atom_index(), exit));
            state.proofs.write_field(instruction.atom_index());
            self.store_step(state, operation, (StorePlan::of(&arms, StoreJoin::Add), HeldRead { admitted, ..HeldRead::default() }), exit)?;
            self.after_store(state, StoreJoin::Add);
            return Some(true);
        }
        let (atom, site) = (instruction.atom_index(), instruction.cache_site_index());
        let slot = self.projected_slot(state, pc, (register, ty));
        if slot.is_some() {
            state.proofs.write_field(atom);
            self.store_step(state, operation, (StorePlan::Entry, HeldRead { slot, ..HeldRead::default() }), exit)?;
        } else {
            self.site_store(state, operation, (atom, site, ty), exit)?;
        }
        if let Some(read) = carried {
            self.carry_store(state, source, (read, carried_int));
        }
        Some(true)
    }

    /// The loop-carried variable of the field a store writes takes the stored value: the int32
    /// view proved before the store where the carried read is unboxed, else the tagged word.
    fn carry_store(&mut self, state: &mut State, source: Register, (read, int): (LoopRead, Option<ir::Value>)) {
        let bits = int.unwrap_or_else(|| self.tagged(state, source));
        self.builder.def_var(read.value, bits);
        #[cfg(test)] if read.representation == Representation::Int32 {
            LOOP_INT32_PHIS.with(|count| count.set(count.get() + 1));
        }
    }

    /// The object `instruction` stores to: a register, or the fixed `this` for `None`; `None`
    /// for a register the unit does not have.
    fn store_owner(&self, pc: usize, instruction: WideInstruction) -> Option<Option<Register>> {
        let register = if let Some(register) = instruction.field_store_register() {
            Some(register)
        } else if let Some(super::lane_unit::Origin::Inlined { frame, .. }) = self.unit.origins.get(pc) {
            Some(self.unit.frames[*frame].receiver)
        } else { None };
        (!register.is_some_and(|register| register >= self.unit.registers)).then_some(register)
    }

    /// The slot a loop's entry projected for the int32 store at `pc` (`LoopStore`), where the
    /// store's object is the record the projection addresses.
    fn projected_slot(&mut self, state: &mut State, pc: usize, (register, ty): (Option<Register>, Type)) -> Option<LoopStore> {
        let slot = self.loop_stores.get(&pc).copied().filter(|slot| ty.is_subtype(slot.domain))?;
        let word = self.owner_word(state, register)?;
        state.proofs.headers.iter().any(|(known, header, _)| *known == word && header.record == slot.header.record).then_some(slot)
    }

    /// The named store of `operation` through its site's plan: `ty` is the stored value's type,
    /// and the site's own shapes, binding and completion decide how it writes (`site_store_plan`).
    pub(super) fn site_store(&mut self, state: &mut State, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>,
        (atom, site, ty): (Atom, u16, Type), exit: ir::Block) -> Option<()> {
        let (arms, join) = self.site_store_plan(site, ty);
        let plan = StorePlan::of(&arms, join);
        let (admitted, split) = if !arms.is_empty() || join == StoreJoin::Bound {
            let (admitted, split) = self.store_atom_admission(state, atom, exit);
            (Some(admitted), split.filter(|_| !arms.is_empty()))
        } else { (None, None) };
        // A store whose one arm is an ahead image's construction (`StoreArm::image`) stores a class cell's field: a miss
        // leaves, so the class's proof holds past the store.
        let image = matches!(&arms[..], [arm] if arm.image);
        let leaves = join == StoreJoin::Host && (self.store_leaves() || image);
        state.proofs.write_field(atom);
        self.store_step(state, operation, (plan, HeldRead { admitted, split, leaves, ..HeldRead::default() }), exit)?;
        if leaves { return Some(()); }
        self.after_store(state, join);
        Some(())
    }

    /// The store of `value`, a word no register names, to the sink field `atom` of `owner`'s
    /// object (`this` for `None`) that a nested lookup writes (`FieldSite::sink`), through its
    /// site's plan. It is the only store of a value with no register, so it runs the row's word
    /// form (`Operation::StoreField`).
    pub(super) fn sink_store(&mut self, state: &mut State, owner: Option<Register>, (atom, site): (Atom, u16), value: ir::Value,
        exit: ir::Block) -> Option<()> {
        let operation = quench_runtime::staged::Operation::StoreField { base: owner, value, fact: Type::ANY, atom, site };
        self.site_store(state, operation, (atom, site, Type::ANY), exit)
    }

    /// The in-place arms and the VM's completion of a store of a value of type `ty` through
    /// `site`, which the row runs (`StorePlan::of`): one arm for each of the site's own shapes
    /// whose layout the plan admits and whose slot the value's type fits or widens by a test
    /// (`StoreArm::of`); the VM's own-field hit where no arm takes the store, through the slot
    /// the entry bound for the site where there is one, with no arm whose test the value's type
    /// never passes; and in a body that completes through
    /// the VM's polymorphic entries (`status_and_pc`) a site of several shapes ends in its
    /// adding entry.
    pub(super) fn site_store_plan(&self, site: u16, ty: Type) -> (Vec<StoreArm>, StoreJoin) {
        let own: Vec<&NativeFieldLoad> = site_loads(self.fields, site).into_iter().flatten().filter(|load| load.holder.is_none()).collect();
        let bound = self.entry.as_ref().is_some_and(|entry| entry.fields.iter().any(|(known, _)| *known == site));
        // The class-keyed loads are the class arms, which lead the shape arms: a live cell takes its class's arm
        // (`StoreArm::of_class`, whose cell's gate admits the write, so the slot's type is no test here), and the other
        // receivers the shape arms; with no shape arm, a cell of another class leaves for the VM's store.
        let arms: Vec<StoreArm> = {
            let classes = own.iter().filter(|load| load.valid())
                .filter_map(|load| load.class.map(|class| StoreArm::of_class(quench_runtime::staged::field_cache_layout::CLASS_KEY | class, load.slot)));
            // An ahead image's construction is a class cell whose class the loader resolves at run time: its arm is
            // `StoreArm::of_image` where it is the site's only shape arm, else its stores take the VM's (the join).
            let loads: Vec<&NativeFieldLoad> = own.iter().filter(|load| load.class.is_none() && load.valid()).collect();
            let image_alone = matches!(loads.as_slice(), [load] if load.image_layout());
            let shapes = loads.iter().filter(|load| !load.image_layout() || image_alone)
                .filter_map(|load| if load.image_layout() {
                        Some(StoreArm::of_image(load.shape & !quench_runtime::staged::ic::SYMBOLIC_SHAPE, load.slot))
                    } else { StoreArm::of((load.shape, load.record_slot()), load.ty, ty) })
                // An arm whose test no value of `ty` passes would take its shape's store and miss: a
                // shape the image guessed once per stored class (`ahead_images::fields`) keeps the
                // class the value may be.
                .filter(|arm: &StoreArm| arm.holds.is_none_or(|holds| holds.is_subtype(ty)));
            classes.chain(shapes).collect()
        };
        let join = if own.len() > 1 && self.completion.status_and_pc() { StoreJoin::Add }
            else if bound { StoreJoin::Bound } else { StoreJoin::Host };
        (arms, join)
    }

    /// Whether a store of a loop region that its in-place arms miss leaves at its PC rather than
    /// calling the VM's store: the call would spill every value live across the loop and keep the
    /// loop's poll (a reentry). A store that added its property (`lane_compile::adding_stores`) takes the VM's store there.
    pub(super) fn store_leaves(&self) -> bool {
        matches!(self.coverage, Coverage::Loop { .. }) && !self.generic.contains(&self.at)
    }

    /// What a store the VM may have completed changes: an adding entry reshapes any object, so
    /// every proof of one retires and the receiver's facts load again; the VM's own-field hit
    /// writes a field, which a prototype's Stable facts may have named.
    fn after_store(&mut self, state: &mut State, join: StoreJoin) {
        self.forget_append_rooms(self.at);
        match join {
            StoreJoin::Add => {
                self.reshaped(state);
                self.reload_receiver();
            }
            StoreJoin::Host | StoreJoin::Bound => state.proofs.stable = false,
        }
    }

    /// The store `operation` as one row step under `plan` and the `held` facts of its access.
    fn store_step(&mut self, state: &mut State, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>,
        (plan, held): (StorePlan, HeldRead), exit: ir::Block) -> Option<()> {
        let plans = LanePlans { store: Some(plan), ..LanePlans::NONE };
        match self.field_step(state, frame::Edges::ValueGuarded { miss: exit }, operation, (&plans, held)) {
            quench_runtime::staged::Step::Value(_) => Some(()),
            _ => None,
        }
    }

    /// The receiver's shape and storage kind again, and the entry's tests of its shape, after
    /// a store that may have changed them (`emit_lane_receiver`, `Lowering::entry_facts`).
    fn reload_receiver(&mut self) {
        let entry = self.entry.as_ref().expect("entry facts");
        let (object_var, shape_var, inline_var) = (entry.receiver.object, entry.receiver.shape, entry.receiver.inline);
        let tests: Vec<(u32, Variable)> = entry.receiver_shapes.iter().map(|(shape, variable)| (*shape, *variable)).collect();
        let object = self.builder.use_var(object_var);
        let (found, done) = (self.builder.create_block(), self.builder.create_block());
        let present = self.raw_compare_imm(Relation::NotEqual, object, 0);
        self.branch_if(present, (found, &[]), (done, &[]));
        self.builder.switch_to_block(found);
        let (shape, _, inline, _) = super::object_slots::shape_and_inline(&mut self.builder, object);
        self.builder.def_var(shape_var, shape);
        self.builder.def_var(inline_var, inline);
        self.goto(done, &[]);
        self.builder.switch_to_block(done);
        let shape = self.builder.use_var(shape_var);
        for (expected, variable) in tests {
            let same = self.raw_compare_imm(Relation::Equal, shape, i64::from(expected));
            self.builder.def_var(variable, same);
        }
    }

    /// Whether the run admits a direct store of `atom`. A run that calls may meet a method
    /// cache that names the atom only after a call; one that does not tests it once, at
    /// entry (`Lowering::entry_facts`).
    pub(super) fn store_atom_admitted(&mut self, state: &mut State, atom: Atom, exit: ir::Block) -> ir::Value {
        self.store_atom_admission(state, atom, exit).0
    }

    /// `store_atom_admitted`, and where the run tests both at the store, its two parts: that its
    /// stores specialize, and that no method cache names the atom (`HeldRead::split`), which an
    /// in-place arm may do without for a quiet store (`store::admit_named`).
    fn store_atom_admission(&mut self, state: &mut State, atom: Atom, exit: ir::Block) -> (ir::Value, Option<(ir::Value, ir::Value)>) {
        if !self.native_calls {
            return (self.builder.use_var(self.entry.as_ref().expect("entry facts").store_atoms[&atom]), None);
        }
        if let Some(held) = self.called_held_atom(state, atom, exit) { return (held, None); }
        if let Some(held) = self.online_store_held(state, atom, exit) { return (held, None); }
        self.emit_store_atom_admission(atom)
    }
}
