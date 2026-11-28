use super::*;

impl Lowering<'_, '_> {
    /// Nested sites retain callee coordinates; atom operands are already renamed by lane_unit.
    pub(super) fn field_base(&self, pc: usize, instruction: WideInstruction) -> Option<quench_runtime::bytecode::FieldBase> {
        use quench_runtime::bytecode::FieldBase;
        match instruction.field_lookup()? {
            FieldLookup::Atom { base, .. } => Some(base),
            FieldLookup::Site(index) => {
                let base = self.program.field_sites.get(index)?.base;
                match self.unit.origins.get(pc)? {
                    super::lane_unit::Origin::Inlined { frame, .. } => {
                        let frame = self.unit.frames[*frame];
                        Some(FieldBase::register(if base == FieldBase::THIS { frame.receiver }
                            else { frame.base.checked_add(base.register_index()?)? }))
                    }
                    _ => Some(base),
                }
            }
        }
    }

    /// The reads of `instruction`'s lookup, one per site, and the sink a nested read of the
    /// current `this` also stores to.
    pub(super) fn field_chain(&self, instruction: WideInstruction) -> Option<(Vec<(Atom, u16)>, Option<(Atom, u16)>)> {
        Some(match instruction.field_lookup()? {
            FieldLookup::Atom { atom, cache_site, .. } => (vec![(atom, cache_site)], None),
            FieldLookup::Site(index) => {
                let site = *self.program.field_sites.get(index)?;
                let reads = [Some(site.first), site.second].into_iter().flatten().collect();
                (reads, instruction.writes_current_this().then_some(site.sink).flatten())
            }
        })
    }

    /// The owner and name of a plain read of `instruction`'s lookup (no sink, no completion):
    /// `None` where the lookup names a site the program does not have.
    fn plain_field(&self, instruction: WideInstruction, owner: Option<Register>, plain: bool) -> Option<Option<(Option<Register>, Atom)>> {
        Some(match instruction.field_lookup() {
            Some(FieldLookup::Atom { atom, .. }) if plain => Some((owner, atom)),
            Some(FieldLookup::Site(index)) if plain => Some((owner, self.program.field_sites.get(index)?.first.0)),
            _ => None,
        })
    }

    /// A named read: what the instruction does around its value (completing the activation,
    /// storing a sink, a direct array's `length`, a whole-program prototype value, the proofs
    /// the value carries), with the value itself one `GetField` row (`field_value`).
    pub(super) fn get_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        // A returning read completes the activation with its value, at a caller record; an
        // inlined callee's is the unit's read, move and jump (`lane_unit`).
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) if self.completion.status_and_pc() => Some(record),
            (true, _) => return None,
        };
        // A nested lookup is a chain of reads from one base, each through its own site; a
        // guard that fails anywhere in it leaves at the instruction, whose reads have no effect.
        // A nested lookup that writes the current `this` stores the chain's value to `this`'s
        // sink field too, after its reads (`FieldSite::sink`).
        // An inlined frame's nested site names its base, and its sink's `this`, by the callee's
        // registers: they are the frame's (`lane_unit::InlinedCall`).
        let frame = match self.unit.origins.get(pc) {
            Some(super::lane_unit::Origin::Inlined { frame, .. }) => Some(self.unit.frames[*frame]),
            _ => None,
        };
        let base = self.field_base(pc, instruction)?;
        let (parts, sink) = self.field_chain(instruction)?;
        let sink_owner = frame.map(|frame| frame.receiver);
        if instruction.writes_current_this() && sink.is_none() {
            return None;
        }
        let register = base.register_index();
        if register.is_some_and(|register| register >= self.unit.registers)
            || (base != quench_runtime::bytecode::FieldBase::THIS && register.is_none())
        {
            return None;
        }
        let destination = instruction.result_register();
        let lookup = instruction.field_lookup()?;
        let exit = self.guard_exit(state, pc as u32);
        let (first, rest) = parts.split_first()?;
        // A direct array's `length` is its record's, an int32.
        if let (Some(register), [], None, None) = (register, rest, sink, completes)
            && super::names_length(self.program, first.0)
        {
            let length = self.length_read(state, pc, register, *first, exit, None);
            self.set_int(state, destination, length);
            return Some(true);
        }
        let field = self.plain_field(instruction, register, sink.is_none() && completes.is_none())?;
        // A whole-program prototype value: read the field once here as data, require the
        // value the closed program gives it, then fold it. A changed prototype fails the
        // guard and leaves to the shared body.
        if rest.is_empty() && sink.is_none() && completes.is_none()
            && !instruction.writes_current_this()
            && self.fold_frozen_field(state, (base, lookup), *first, destination, exit)
        {
            return Some(true);
        }
        // A fixed class's receiver reads its union slot in its class cell, with no record (`fixed_cells`).
        if rest.is_empty() && sink.is_none() && completes.is_none()
            && self.fixed_field_read(state, pc, instruction, (register, first.0), exit)
        {
            return Some(true);
        }
        let known = match field {
            Some((owner, atom)) => self.owner_word(state, owner)
                .and_then(|word| state.proofs.fields.iter().copied().find(|field| field.owner == word && field.atom == atom)),
            None => None,
        };
        #[cfg(test)] if known.is_some() { record_field_forward(first.1); }
        let held = HeldRead { known: known.map(|field| field.value), ..HeldRead::default() };
        let value = self.field_value(state, (base, lookup), &parts, held, exit)?;
        if let Some((atom, site)) = sink {
            self.sink_store(state, sink_owner, (atom, site), value, exit)?;
        }
        if let Some(record) = completes {
            self.return_value(record, value);
            return Some(false);
        }
        self.define_field_read(state, pc, instruction, value, (field, rest.is_empty(), known), exit);
        Some(true)
    }

    /// The value `base`'s lookup reads through `links`, the sites of its chain: one `GetField`
    /// row under the plans their caches proved, or the value `held` already has. A guard of it
    /// that fails leaves through `exit`.
    pub(super) fn field_value(&mut self, state: &mut State, (base, lookup): (quench_runtime::bytecode::FieldBase, FieldLookup), links: &[(Atom, u16)],
        held: HeldRead, exit: ir::Block) -> Option<ir::Value> {
        let mut inputs: Vec<LinkInput> = links.iter().map(|link| self.link_input(*link)).collect();
        if let Some(first) = inputs.first_mut() {
            first.probed |= self.inherited_off_record(state, base, &first.loads);
        }
        let folds = self.division().folds(quench_runtime::staged::division::INHERITED_FIELD);
        if folds { field_plans::embed_inherited(&inputs, &mut self.stable); }
        with_read_plan(&inputs, (folds, held.known.is_some()), |plans| {
            let edges = frame::Edges::ValueGuarded { miss: exit };
            match self.field_step(state, edges, quench_runtime::staged::Operation::GetField { base, lookup, destination: 0 }, (plans, held)) {
                quench_runtime::staged::Step::Value(value) => Some(value),
                _ => None,
            }
        })
    }

    /// A chain link's site, with the loads its cache proved and whether its misses probe.
    fn link_input(&self, (atom, site): (Atom, u16)) -> LinkInput {
        let loads = site_loads(self.fields, site).cloned().unwrap_or_default();
        let probed = self.probes_field_misses(&loads);
        LinkInput { atom, site, loads, probed }
    }

    /// The `length` read through `read` of the value `register` holds, as an int32: a direct
    /// array's from its record (`emit_array_length_read`), a flat string's from its own
    /// (`string_length`), any other's the VM's read, which leaves through `exit` where it
    /// misses or is not an int32.
    /// The `length` read at `pc` of `register`: a typed array a loop's entry proved
    /// (`hoist_arrays`) has the view's element count, which no store in the loop resizes, so the
    /// read is that word, as C's length is; any other base is `array_length`'s. On the entry
    /// edge of `entering`, only a view that edge already proved counts (captures are proved
    /// after the invariant reads).
    pub(super) fn length_read(&mut self, state: &mut State, pc: usize, register: Register, read: (Atom, u16), exit: ir::Block,
        entering: Option<usize>) -> ir::Value {
        if let Some(string) = self.loop_string(pc, Some(register)).filter(|string| entering != Some(string.header)) {
            return self.held_length(string);
        }
        let proved = |array: &LoopArray| matches!(array.views, ElementViews::Typed(_));
        if let Some(array) = self.loop_array(pc, Some(register)).filter(proved) {
            let (_, count) = self.hoisted_view(array);
            let fits = self.raw_compare_imm(Relation::UnsignedLessEqual, count, i64::from(i32::MAX));
            emit_js_guard(&mut self.builder, fits, exit);
            return self.raw_unary(Unary::Narrow(Lane::I32), count);
        }
        self.array_length(state, register, read, exit)
    }

    fn array_length(&mut self, state: &mut State, register: Register, read: (Atom, u16), exit: ir::Block) -> ir::Value {
        let base = self.tagged(state, register);
        let generic = cold_block(&mut self.builder);
        let join = self.builder.create_block();
        let length = self.builder.append_block_param(join, types::I32);
        let access = self.array_access();
        let native = super::emit_array_length_read(&mut self.builder, self.payload, base, access, generic);
        self.goto(join, &[native]);
        self.builder.switch_to_block(generic);
        let probe = cold_block(&mut self.builder);
        let string = self.string_length(base, probe);
        self.goto(join, &[string]);
        self.builder.switch_to_block(probe);
        let (atom, site) = read;
        let probe = quench_runtime::staged::Operation::FieldGetHost { object: base, atom, site };
        let quench_runtime::staged::Step::Value(value) = self.field_step(state, frame::Edges::ValueGuarded { miss: exit }, probe,
            (&LanePlans::NONE, HeldRead::default())) else { unreachable!("the lane's field read is a host call") };
        let integer = is_int_tagged(&mut self.builder, value);
        emit_js_guard(&mut self.builder, integer, exit);
        let int = self.raw_unary(Unary::Narrow(Lane::I32), value);
        self.goto(join, &[int]);
        self.builder.switch_to_block(join);
        length
    }

    /// Whether the read `instruction` makes has an own-field entry to fold: a read of prototype
    /// holders only (a method of an array, a `Map`) proves no record at entry, which such a
    /// receiver would fail at every run, and asks the VM's entries on a guard miss instead.
    pub(super) fn reads_own_field(&self, instruction: WideInstruction) -> bool {
        let Some(FieldLookup::Atom { cache_site, .. }) = instruction.field_lookup() else { return true };
        site_loads(self.fields, cache_site).is_none_or(|loads| loads.iter().any(|load| load.holder.is_none()))
    }

    /// Whether `register`, whose type no object proof covers, may hold a receiver the object
    /// table has no record of (a `Map`'s, a `Set`'s) at a read of inherited constants: its miss
    /// would leave at every entry, so it asks the VM's proved inherited entry instead.
    pub(super) fn inherited_off_record(&self, state: &State, base: quench_runtime::bytecode::FieldBase, shapes: &[NativeFieldLoad]) -> bool {
        shapes.iter().any(|load| load.inherited.is_some())
            && base.register_index().is_some_and(|register| !state.proofs.receivers.contains(&register)
                && !state.values[usize::from(register)].value.ty.is_direct_object_family())
    }

    /// Whether a read of these shapes asks the VM's caches on a guard miss rather than leaves.
    pub(super) fn probes_field_misses(&self, shapes: &[NativeFieldLoad]) -> bool {
        self.left_here || self.left_a_field_read() || NativeFieldLoad::probes_misses(shapes)
    }

    /// Whether a previous body left at some field read of this unit: its shapes were not all
    /// seen when it was planned, so every read now asks the VM's caches on a guard miss rather
    /// than leave, as this unit's other reads would leave one recompile at a time.
    fn left_a_field_read(&self) -> bool {
        self.generic.iter().any(|pc| self.unit.instructions.get(*pc).is_some_and(|instruction| matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. }))))
    }

    /// A dominating proof of `owner`'s object (`headers::object_header`).
    pub(super) fn object_header(&mut self, state: &mut State, owner: Option<Register>, read: bool, exit: ir::Block) -> Option<ObjectHeader> {
        let entry = self.entry.as_ref().expect("entry facts");
        headers::object_header(&mut self.builder, &mut self.registers, state, (entry, self.payload), owner, read, exit)
    }

    /// The resolved word a register holds: the value its proofs are keyed by (`headers::register_word`).
    pub(super) fn register_word(&mut self, state: &mut State, register: Register) -> ir::Value {
        headers::register_word(&mut self.builder, &mut self.registers, state, register)
    }

    /// The resolved word of `owner`, a register's or fixed `this`'s; `None` for `this` where the entry has none.
    pub(super) fn owner_word(&mut self, state: &mut State, owner: Option<Register>) -> Option<ir::Value> {
        headers::owner_word(&mut self.builder, &mut self.registers, state, self.entry.as_ref().map(|entry| entry.this), owner)
    }
}
