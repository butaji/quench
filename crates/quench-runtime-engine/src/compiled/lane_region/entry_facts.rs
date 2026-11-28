use super::*;

impl Lowering<'_, '_> {
    /// The receiver bits a run starts with: a body entry's word, which its driver passes (a lane
    /// body's caller, the OSR region's `entry_protocol` descriptor, the AOT trampoline); a scalar
    /// entry, which takes none, reads its window's header.
    fn entry_receiver_bits(&mut self) -> ir::Value {
        if self.completion == Completion::Body { return self.entry_words.0; }
        let window = self.loop_window();
        emit_frame_word(&mut self.builder, window, quench_runtime::staged::frame::CONTEXT_THIS)
    }

    /// The running activation's dense capture slots (address, count): a lane body's caller
    /// packs them in the entry's word (`entry_cell::capture_word`), a loop region reads its window's header.
    fn entry_capture_words(&mut self) -> (ir::Value, ir::Value, ir::Value) {
        let word = if self.body_entry() { self.entry_words.1 } else {
            let window = self.loop_window();
            emit_frame_word(&mut self.builder, window, quench_runtime::staged::frame::CONTEXT_BODY)
        };
        let (values, len) = self.capture_slots_of(word);
        (values, len, self.raw_binary_imm(Binary::And, word, quench_bjit::shared_words::CAPTURE_STORES_BIT as i64))
    }

    /// The window a loop region runs in: its payload's register file, whose header holds its receiver and captures word.
    fn loop_window(&mut self) -> ir::Value {
        emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, native_registers))
    }

    /// Whether this run is a lane body, whose callers pass its receiver and captures as the
    /// entry's words; a loop region reads the payload's.
    pub(super) fn body_entry(&self) -> bool { matches!(self.coverage, Coverage::Prefix | Coverage::PrefixFrom(_)) }

    /// The facts every run proves once at entry.
    pub(super) fn entry_facts(&mut self, instructions: &[WideInstruction], _loops: &[(usize, usize)]) -> Entry {
        let this_bits = match self.helpers {
            LaneHelpers::Numeric => unreachable!("Number signatures have no receiver"),
            LaneHelpers::Bound(..) => self.fixed_this(),
            LaneHelpers::Linked(..) => unreachable!("helper table bound before entry facts"),
            _ => self.entry_receiver_bits(),
        };
        let accesses_fields = self.accesses_fields(instructions);
        // The receiver fact makes the receiver a direct object (`dispatch_fast::receiver_trusted`).
        let trusted = accesses_fields && crate::dispatch_fast::receiver_trusted(self.vm.facts, self.function_id) && self.vm.online.is_some() && self.completion == Completion::Body;
        // A body whose named reads are all class-keyed reads its receiver's cell, never its record: no record is loaded here.
        let receiver = if accesses_fields && !self.class_keyed_reads(instructions) { emit_lane_receiver(&mut self.builder, self.payload, this_bits, trusted) } else { self.absent_receiver() };
        let this = self.builder.declare_var(types::I64);
        self.builder.def_var(this, this_bits);
        let receiver_shapes = self.entry_receiver_shapes(&receiver);
        let store_atoms = self.entry_store_atoms(instructions);
        // The running activation's dense capture slots (`EnvironmentSlots::native_dense_view`),
        // fixed for the run; each read loads its slot where it runs, after any call that may
        // have stored to it.
        let capture_values = self.builder.declare_var(types::I64);
        let capture_len = self.builder.declare_var(types::I64);
        let capture_stores = self.builder.declare_var(types::I64);
        let (values, len, stores) = self.entry_capture_words();
        self.builder.def_var(capture_values, values);
        self.builder.def_var(capture_len, len);
        self.builder.def_var(capture_stores, stores);
        let environment = self.builder.declare_var(types::I64);
        let storage = self.environment_word();
        self.builder.def_var(environment, storage);
        let mut miss = None;
        let mut calls = BitSet::with_capacity(self.unit.frames.len());
        let mut call_receivers = BitSet::with_capacity(self.unit.frames.len());
        if !self.completion.numeric() {
            // Roots name canonical caller homes never written by this unit.
            // Local Move aliases are valid only within their basic block.
            let roots: Vec<_> = (0..self.unit.registers).map(|register|
                (register < self.unit.caller_registers && !self.written.contains(usize::from(register)))
                    .then_some(register)).collect();
            let mut aliases = roots.clone();
            let mut admitted = smallvec::SmallVec::<[(Register, super::lane_unit::CallTarget); 2]>::new();
            let mut receivers = smallvec::SmallVec::<[(Register, ReceiverRecovery); 2]>::new();
            for (pc, instruction) in instructions.iter().copied().enumerate() {
                if self.blocks[pc].is_some() { aliases.copy_from_slice(&roots); }
                if !self.admits_at_entry(pc) { continue; }
                if let Some(frame) = self.unit.markers.iter().position(|marker| *marker == pc) {
                    let call = self.unit.frames[frame];
                    let body = self.program.function(call.target.function as usize);
                    let numeric = restartable_numeric_function(self.program, body, body.instructions());
                    if !(call.recovery == super::lane_unit::CallRecovery::RestartCall && numeric)
                        && let Some(root) = aliases[usize::from(call.receiver)] {
                        let recovery = ReceiverRecovery::for_body(body);
                        // A receiver the solve proves a direct object (its solver type) needs no recovery guard at entry.
                        if !self.solver_object(pc, call.receiver) {
                            if let Some((_, required)) = receivers.iter_mut().find(|(owner, _)| *owner == root) {
                                if recovery == ReceiverRecovery::Object { *required = recovery; }
                            } else { receivers.push((root, recovery)); }
                        }
                        call_receivers.insert(frame);
                    }
                    if call.alternative.is_none()
                        && numeric
                        && self.planned_target(frame).is_none()
                        && let Some(root) = aliases[usize::from(call.callee_register)] {
                        let binding = (root, call.target);
                        if !admitted.contains(&binding) {
                            let callee = self.raw_load(Lane::I64, regions::frame(), self.registers_base, register_offset(root));
                            let matched = match call.target.callee {
                                CalleeBinding::Function => self.function_binding(callee, call.target.function),
                                CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => self.is_identity(callee, identity),
                            };
                            let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                            emit_js_guard(&mut self.builder, matched, exit);
                            admitted.push(binding);
                        }
                        calls.insert(frame);
                    }
                }
                let root = (matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })))
                    .then(|| aliases[usize::from(instruction.register_b())]).flatten();
                for register in writes(instruction) { aliases[usize::from(register)] = root; }
            }
            for (root, recovery) in receivers {
                let receiver = self.raw_load(Lane::I64, regions::frame(), self.registers_base, register_offset(root));
                let matched = self.receiver_recovery(receiver, recovery);
                let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                emit_js_guard(&mut self.builder, matched, exit);
            }
        }
        let fields = {
            let mut fields = smallvec::SmallVec::new();
            if matches!(self.helpers, LaneHelpers::Bound(..)) {
                let mut uses = smallvec::SmallVec::<[(u16, bool, Type); INLINE_FIELD_BINDINGS]>::new();
                for (pc, instruction) in instructions.iter().enumerate() {
                    // An inlined call's later alternatives are entered by its switch, an edge
                    // the bytecode facts do not see: their sites bind too, or they would read
                    // through the VM's caches for the whole run.
                    if !self.facts.reached(pc) && !matches!(self.unit.origins[pc], super::lane_unit::Origin::Inlined { .. }) { continue; }
                    // A field chain names sequential sites, not alternatives for
                    // one polymorphic access. Bind each distinct column once.
                    let mut sites = smallvec::SmallVec::<[u16; 3]>::new();
                    match () {
                        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) => match instruction.field_lookup() {
                            Some(FieldLookup::Atom { cache_site, .. }) => sites.push(cache_site),
                            Some(FieldLookup::Site(site)) => {
                                let site = self.program.field_sites[site];
                                sites.push(site.first.1);
                                if let Some((_, cache)) = site.second { sites.push(cache); }
                                if instruction.writes_current_this()
                                    && let Some((_, cache)) = site.sink { sites.push(cache); }
                            }
                            _ => {},
                        },
                        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => sites.push(instruction.cache_site_index()),
                        _ => {},
                    }
                    let repeated = _loops.iter().any(|&(header, back)| (header..=back).contains(&pc));
                    for site in sites {
                        let writes = if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) {
                            self.cell_before(pc, instruction.register_a()).ty.machine()
                        } else if instruction.writes_current_this()
                            && matches!(instruction.field_lookup(), Some(FieldLookup::Site(index))
                                if self.program.field_sites[index].sink.is_some_and(|(_, sink)| sink == site)) {
                            Type::ANY
                        } else { Type::NEVER };
                        if let Some((_, reused, domain)) = uses.iter_mut().find(|(known, _, _)| *known == site) {
                            *reused |= repeated;
                            *domain = domain.join(writes);
                        } else { uses.push((site, repeated, writes)); }
                    }
                }
                for (site, repeated, writes) in uses {
                    let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                    let mut binding = emit_js_field_binding(&mut self.builder, self.payload, site, exit);
                    let slot = binding.slot.read(&mut self.builder);
                    binding.slot = FieldSlot::Bound(slot);
                    // A slot whose writes the run does not admit leaves only at its stores
                    // (`EmitFrame::bound_store`): the rest of the run, which reads it by shape, holds.
                    if writes != Type::NEVER { binding.admitted = Some(self.field_writes_admitted(&mut binding, writes)); }
                    // The extra entry arithmetic pays for repeated receiver loads.
                    // A one-off access keeps its cheaper storage-plus-slot form.
                    if repeated {
                        binding.arena = binding.arena.bind(&mut self.builder, slot);
                    }
                    fields.push((site, binding));
                }
            }
            fields
        };
        self.admit_store_domains(instructions, &mut miss);
        let records = self.entry_field_records(instructions, &receiver, this, &mut miss);
        if let Some(miss) = miss {
            let admitted = self.builder.create_block();
            self.goto(admitted, &[]);
            self.builder.switch_to_block(miss);
            self.builder.set_cold_block(miss);
            // No guest operation has run and canonical registers are untouched.
            // Decline the whole run once, at its start, even for a speculative unused binding.
            let origin = |pc: usize| match self.unit.origins[pc] {
                super::lane_unit::Origin::Caller(pc) => u64::from(pc),
                super::lane_unit::Origin::Inlined { .. } => unreachable!("a run starts in the caller"),
            };
            self.return_at(origin(self.coverage.entry()));
            self.builder.switch_to_block(admitted);
        }
        Entry { receiver, records, receiver_shapes, store_atoms, capture_values, capture_len, capture_stores, environment, this, fields, calls, call_receivers }
    }

    /// Per receiver shape a field site folds, whether the receiver has it.
    fn entry_receiver_shapes(&mut self, receiver: &LaneReceiver) -> BTreeMap<u32, Variable> {
        let mut receiver_shapes = BTreeMap::new();
        for field in self.fields.iter().flatten().flatten().filter(|field| !field.guessed()) {
            if receiver_shapes.contains_key(&field.shape) {
                continue;
            }
            let shape = self.builder.use_var(receiver.shape);
            let same = self.raw_compare_imm(Relation::Equal, shape, i64::from(field.shape));
            let variable = self.builder.declare_var(types::I8);
            self.builder.def_var(variable, same);
            receiver_shapes.insert(field.shape, variable);
        }
        receiver_shapes
    }

    /// Per atom the unit stores, whether the run admits direct stores of it. A direct store of an
    /// atom needs a specialized run and no method cache naming it; a run that makes no call
    /// changes neither (`emit_js_own_field_set`, `Lowering::site_store`).
    fn entry_store_atoms(&mut self, instructions: &[WideInstruction]) -> BTreeMap<Atom, Variable> {
        let mut store_atoms = BTreeMap::new();
        let stored = instructions.iter().filter_map(|instruction| match () {
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => Some(instruction.atom_index()),
            _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) && instruction.writes_current_this() => match instruction.field_lookup() {
                Some(FieldLookup::Site(index)) => self.program.field_sites.get(index).and_then(|site| site.sink).map(|(atom, _)| atom),
                _ => None,
            },
            _ => None,
        }).collect::<Vec<_>>();
        for atom in stored {
            if let std::collections::btree_map::Entry::Vacant(slot) = store_atoms.entry(atom) {
                let admitted = self.emit_store_atom_admitted(atom);
                let variable = self.builder.declare_var(types::I8);
                self.builder.def_var(variable, admitted);
                slot.insert(variable);
            }
        }
        store_atoms
    }

    /// Whether the entry edge admits the inlined call at `pc`, a call a run may skip
    /// (`packet != null`) and so not one every run may be refused for: a body that left at
    /// its entry once admits them at their markers instead (`Lowering::inline_call`).
    fn admits_at_entry(&self, pc: usize) -> bool {
        self.facts.reached(pc) && !self.generic.contains(&entry_leave(self.unit))
    }

    /// `invariant_field_records`; a body that left at its entry binds only its own receiver's
    /// record there (an object on every run of a method): the reads of a register it hoisted ran
    /// under a branch (`packet != null`) and the run declined at its start.
    fn entry_field_records(&mut self, instructions: &[WideInstruction], receiver: &LaneReceiver, this: Variable,
        miss: &mut Option<ir::Block>) -> Vec<Option<ObjectRecord>> {
        let receiver_only = self.generic.contains(&entry_leave(self.unit));
        self.invariant_field_records(instructions, receiver, this, miss, receiver_only)
    }

    /// The register (or `unit.registers` for `this`) whose object the field access at `pc` reads or
    /// writes, where `instruction` is one and the solver names the closed set of classes that object
    /// is of (`solved_classes`): an access the solver names no set for owns no hoisted record.
    pub(super) fn field_owner(&self, pc: usize, instruction: WideInstruction) -> Option<usize> {
        self.solved_classes(pc)?;
        self.accessed_register(pc, instruction)
    }

    /// The (class, atom) key of the named field access at unit PC `pc`, where the solver proves its
    /// receiver one class (`solved_classes`): the key its property values' region is declared under.
    pub(super) fn field_key(&self, pc: usize) -> Option<regions::FieldKey> {
        let instruction = *self.unit.instructions.get(pc)?;
        let Some(FieldLookup::Atom { atom, .. }) = instruction.field_lookup() else { return None };
        match self.solved_classes(pc)? {
            quench_runtime::typed_ir::Classes::Set { ids, len: 1, proven: true } => Some((ids[0], atom)),
            _ => None,
        }
    }

    /// The keys of the unit's named property accesses, where every property access of the unit is
    /// keyed (the soundness rule of `regions::retag_field_regions`): `None` where one is not, or where
    /// the unit makes an access the keys do not cover (a store helper's, an allocation's).
    fn field_keys(&self) -> Option<BTreeSet<regions::FieldKey>> {
        let mut keys = BTreeSet::new();
        for (pc, instruction) in self.unit.instructions.iter().enumerate() {
            for operation in [crate::compiled::performed(instruction), crate::compiled::decoded(instruction)].into_iter().flatten() {
                match operation {
                    quench_runtime::staged::Operation::GetField { .. } | quench_runtime::staged::Operation::SetField { .. } => {
                        keys.insert(self.field_key(pc)?);
                    }
                    quench_runtime::staged::Operation::StoreField { .. } | quench_runtime::staged::Operation::FieldAdd { .. }
                    | quench_runtime::staged::Operation::Delete { .. } | quench_runtime::staged::Operation::FieldGetHost { .. }
                    | quench_runtime::staged::Operation::Allocate { .. } => return None,
                    _ => {}
                }
            }
        }
        Some(keys)
    }

    /// Declares the unit's field regions after `Region::ALL`, one per key `field_keys` names; none
    /// where it names none.
    pub(super) fn declare_fields(&mut self) -> BTreeMap<regions::FieldKey, cranelift_codegen::ir::AliasRegion> {
        let Some(keys) = self.field_keys() else { return BTreeMap::new() };
        regions::declare_field_regions(&mut self.builder.func, keys)
    }

    /// Conform the entry's state to the facts at `pc`. A lane body's failed input proof
    /// leaves through one shared block with `LaneExit::Entry`, its specialization's feedback
    /// that the inputs need no contract; other regions leave at `pc`.
    pub(super) fn entry_conform(&mut self, state: &mut State, pc: usize, coverage: Coverage) {
        self.proving_entry = matches!(coverage, Coverage::Prefix) && self.completion.status_and_pc();
        self.conform(state, pc);
        self.proving_entry = false;
    }

    /// The guards' leaves to fill, once the entry's shared leave (`entry_conform`) is filled:
    /// it leaves with the plan's entry leave, with no register written yet to write back.
    pub(super) fn take_leaves(&mut self) -> Vec<GuardExit> {
        if let Some(block) = self.entry_exit.take() {
            self.switch_to(block, true);
            self.return_entry();
        }
        std::mem::take(&mut self.pending)
    }

    /// The leave of a failed entry proof, before any instruction ran: a lane plan names it
    /// (`LaneExit::Entry`), so the activation's specialization drops the contract that
    /// failed; a Number entry misses, restarting the original call.
    fn return_entry(&mut self) {
        match self.completion {
            Completion::Body => self.return_at(super::LaneExit::Entry.word()),
            Completion::Number | Completion::Scalar(_) => self.return_at(0),
        }
    }
}
