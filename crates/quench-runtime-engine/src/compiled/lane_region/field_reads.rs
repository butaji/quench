//! How a named field read defines its register (`Lowering::get_field`): a whole-program
//! prototype value folded under its guard, or the read value with the int32 view a slot
//! that has only held int32 values proves: by the code's dependency on the slot's observation
//! (an online body), else by a tag test.
use quench_runtime::vm::global_facts::ReceiverLink;
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The cache site of the last read of `instruction`'s lookup, whose slots the value comes from;
/// `None` for a lookup with a sink.
fn last_read_site(program: &ResidualProgram, instruction: WideInstruction) -> Option<u16> {
    match instruction.field_lookup()? {
        FieldLookup::Atom { cache_site, .. } => Some(cache_site),
        FieldLookup::Site(index) => {
            let site = program.field_sites.get(index)?;
            site.sink.is_none().then_some(site.second.unwrap_or(site.first).1)
        }
    }
}

/// The type every slot the last read of `instruction`'s lookup reads has held: int32 or
/// direct object (`NativeFieldLoad::ty`), for a lookup with no sink or completion.
fn held_slot_type(program: &ResidualProgram, fields: &FieldSites, instruction: WideInstruction) -> Option<Type> {
    agreed_slot_type(program, fields, instruction)
        .filter(|ty| *ty == Type::INT32 || *ty == Type::NUMBER || *ty == Type::DIRECT_OBJECT_FAMILY || *ty == Type::NULLABLE_DIRECT_OBJECT)
}

/// The slot type every load of `instruction`'s plain field read observed, where they agree.
fn agreed_slot_type(program: &ResidualProgram, fields: &FieldSites, instruction: WideInstruction) -> Option<Type> {
    if !matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) || instruction.returns_from_frame() || instruction.writes_current_this() {
        return None;
    }
    let loads = last_read_site(program, instruction).and_then(|last| site_loads(fields, last))?;
    let ty = loads.first()?.ty;
    loads.iter().all(|load| load.ty == ty).then_some(ty)
}

/// Whether `instruction` reads a field whose slots have held Numbers, not only int32s: the read
/// proves Number once and keeps the double view, so its arithmetic takes the double operation.
pub(crate) fn number_read(program: &ResidualProgram, fields: &FieldSites, instruction: WideInstruction) -> bool {
    agreed_slot_type(program, fields, instruction) == Some(Type::NUMBER)
}

/// Whether `instruction` reads a field as an int32 (`Lowering::get_field`): its last read's slots
/// have only held int32 values.
pub(crate) fn int32_read(program: &ResidualProgram, fields: &FieldSites, instruction: WideInstruction) -> bool {
    held_slot_type(program, fields, instruction) == Some(Type::INT32)
}

impl Lowering<'_, '_> {
    /// Whether the value of `instruction`'s read of held slots (`held_slot_type`) comes only from its
    /// loads' native reads, whose slot dependencies cover it: not a read the VM's caches stand
    /// in for (a probe of a miss, a link no native read admits), which no dependency covers.
    fn reads_covered_slots(&self, state: &State, instruction: WideInstruction) -> bool {
        let Some(base) = self.field_base(self.at, instruction) else { return false };
        last_read_site(self.program, instruction).and_then(|last| site_loads(self.fields, last))
            .is_some_and(|loads| field_plans::reads_natively(loads) && !self.probes_field_misses(loads)
                && !self.inherited_off_record(state, base, loads))
    }

    /// Whether a reached instruction reads or stores a named field: only then does the body
    /// look at the receiver's record and the property arena at entry.
    pub(crate) fn accesses_fields(&self, instructions: &[WideInstruction]) -> bool {
        instructions.iter().enumerate().any(|(pc, instruction)| self.facts.reached(pc) && quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NamedFieldAccess))
    }

    /// The entry facts of a body that never looks at the receiver's record: no object, no
    /// shape, no arena.
    pub(crate) fn absent_receiver(&mut self) -> LaneReceiver {
        let receiver = LaneReceiver { object: self.builder.declare_var(types::I64), shape: self.builder.declare_var(types::I32),
            inline: self.builder.declare_var(types::I8), properties: self.builder.declare_var(types::I64) };
        let (zero, none, outside) = (self.raw_const(Lane::I64, 0), self.raw_const(Lane::I32, i64::from(super::super::NO_FIELD_RECEIVER)), self.raw_const(Lane::I8, 0));
        for (variable, value) in [(receiver.object, zero), (receiver.properties, zero), (receiver.shape, none), (receiver.inline, outside)] {
            self.builder.def_var(variable, value);
        }
        receiver
    }

    /// Folds the read of `first` through `read` (its base and lookup) into `destination` when
    /// the closed program gives the field one value: the field is read once here as data and
    /// required to hold it, so a changed prototype fails the guard and leaves to the shared
    /// body. False, emitting nothing, otherwise.
    /// The read at `pc` of a native member of a Stable global (`GlobalRead::NativeMember`), folded
    /// to the function it held: the holder's values are a dependency (`hold_holder`), so a write
    /// to one retires this code rather than each run re-reading the member.
    /// `None` where the read is not one, or completes its frame or writes `this` besides.
    pub(crate) fn fold_native_member(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let Some(&quench_runtime::vm::global_facts::GlobalRead::NativeMember { value, native, holder, folds, link, cell }) = self.global_reads.get(&pc) else { return None };
        // A method read by name folds only where the receiver can hold no own member of that
        // name: a string primitive, whose members are its prototype's, or a direct array of no
        // named property whose prototype is the holder, or a Map or Set of no named property linked
        // to a prototype that holds the name (`ReceiverLink`). A string method's read proves its
        // receiver a string by one tag test, as the call it names would; an Array method's proves
        // its receiver such an array (`elements::plain_array_of`); a collection method's proves its
        // receiver of the shape and link its cache saw (`elements::collection_of`).
        let receiver = match instruction.field_lookup() {
            Some(FieldLookup::Atom { base, .. }) => base.register_index().filter(|register| usize::from(*register) < state.values.len()),
            _ => None,
        };
        let string = Type::language(quench_runtime::ValueType::String);
        let string_method = matches!(native, quench_runtime::tier_surface::Native::StringCharCodeAt | quench_runtime::tier_surface::Native::StringCharAt);
        // A member of a global read by name (`Math.imul`) stands for the object the global's cell held. Code that holds the
        // cell folds the read under it; code that holds none (an offline region) reads the global live, so the read's
        // receiver is compared with that object once: after `Math = other` it leaves.
        let held_cell = folds && self.division().folds(BindingTime::Stable);
        let live_cell = match (folds, cell, receiver) {
            (true, Some(cell), Some(register)) if !held_cell => Some((register, cell.value)),
            (true, Some(_), None) if !held_cell => return None,
            _ => None,
        };
        let receiver = match (folds, receiver) {
            (true, _) => None,
            (false, Some(register)) if string_method && (string.is_subtype(state.values[usize::from(register)].value.ty) || state.values[usize::from(register)].value.ty.is_subtype(string)) => Some(register),
            (false, Some(register)) if link != ReceiverLink::None => Some(register),
            _ => return None,
        };
        if !self.holds_holders() || instruction.returns_from_frame() || instruction.writes_current_this() { return None; }
        let exit = self.guard_exit_for(state, pc as u32, ExitFact::Shape);
        if let (Some(register), ReceiverLink::Array(link)) = (receiver, link) {
            let bits = self.tagged(state, register);
            let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.array_record_base));
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            super::builtin_calls::continues(quench_runtime::staged::elements::plain_array_of(&mut stage, records, bits, link, exit));
        } else if let (Some(register), ReceiverLink::Collection { shape, link }) = (receiver, link) {
            let bits = self.tagged(state, register);
            let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.collection_record_base));
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            super::builtin_calls::continues(quench_runtime::staged::elements::collection_of(&mut stage, records, bits,
                quench_runtime::tier_surface::NATIVE_COLLECTION_LAYOUT, (shape, link), exit));
        } else if let Some(register) = receiver.filter(|register| !state.values[usize::from(*register)].value.ty.is_subtype(string)) {
            let bits = self.tagged(state, register);
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            let is_string = quench_runtime::staged::value::is_direct_string(&mut stage, bits);
            emit_js_guard(&mut self.builder, is_string, exit);
            state.values[usize::from(register)] = RegisterFact { tagged: state.values[usize::from(register)].tagged,
                ..RegisterFact::definition(string, Representation::Tagged) };
        }
        match (cell, live_cell) {
            (_, Some((register, object))) => {
                let bits = self.tagged(state, register);
                let same = self.is_identity(bits, object);
                emit_js_guard(&mut self.builder, same, exit);
            }
            (Some(cell), None) if held_cell => self.hold_global(state, exit, cell.fact, (cell.global, cell.realm_offset)),
            _ => {}
        }
        self.hold_holder_any(state, exit, holder);
        let constant = self.identity_constant(value);
        self.define(state, instruction.result_register(), constant, RegisterFact::definition(Type::immediate(value), Representation::Tagged));
        Some(true)
    }

    pub(crate) fn fold_frozen_field(&mut self, state: &mut State, read: (quench_runtime::bytecode::FieldBase, FieldLookup),
        first: (Atom, u16), destination: Register, exit: ir::Block) -> bool {
        if !self.division().folds(quench_runtime::staged::division::FROZEN_FIELD) { return false; }
        let Some(expected) = self.vm.frozen_fields.and_then(|fields| fields.get(&first.0)).copied() else { return false };
        let Some(actual) = self.field_value(state, read, &[first], HeldRead::default(), exit) else { return false };
        let same = self.is_identity(actual, expected);
        emit_js_guard(&mut self.builder, same, exit);
        let value = self.identity_constant(expected);
        self.define(state, destination, value, RegisterFact::definition(Type::immediate(expected), Representation::Tagged));
        true
    }

    /// Defines `instruction`'s register as the `value` it read, typed as the slots it reads
    /// have held where that is proved (`prove_held_slot`): `field` names the slot a one-read
    /// lookup (`single`) reads, whose proof `known` held before.
    pub(crate) fn define_field_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, value: ir::Value,
        (field, single, known): (Option<(Option<Register>, Atom)>, bool, Option<FieldValue>), exit: ir::Block) {
        let held = known.filter(|field| single && field.ty != Type::ANY).map(|field| field.ty);
        let ty = match held {
            Some(ty) => ty,
            None => self.prove_held_slot(state, instruction, (value, known.is_none()), exit),
        };
        // A fused chain can consume an existing first-slot proof, but its guest
        // result names the final slot. Do not retain a new anonymous intermediate
        // solely for forwarding; the existing proof needs no duplicate update.
        if let Some((owner, atom)) = field.filter(|_| single)
            && let Some(owner) = self.owner_word(state, owner) {
            let read = FieldValue { owner, atom, value, ty };
            match state.proofs.fields.iter_mut().find(|field| field.owner == owner && field.atom == atom) {
                Some(field) => *field = read,
                None => state.proofs.fields.push(read),
            }
        }
        self.define_read_value(state, (pc, instruction), (value, ty), exit);
    }

    /// Defines `instruction`'s register as the `value` it read, of the slots' type `ty`: a raw
    /// double's Number, an int32 view, or a tagged value proved Number where only Number
    /// operations read it.
    fn define_read_value(&mut self, state: &mut State, (pc, instruction): (usize, WideInstruction), (value, ty): (ir::Value, Type),
        exit: ir::Block) {
        let destination = instruction.result_register();
        // A raw double's read boxed its binary64 word (`compiled::take_boxed_numbers`): the
        // register holds that word, and the box, which nothing else reads, folds away; an exit
        // boxes the register on its own path.
        let bits = self.builder.func.dfg.resolve_aliases(value);
        if let Some(number) = state.proofs.numbers.iter().find(|(source, _)| *source == bits).map(|(_, number)| number.word()) {
            self.set_number(state, destination, number);
            return;
        }
        let described = if ty == Type::INT32 { Type::ANY } else { ty };
        self.define(state, destination, value, RegisterFact::definition(described, Representation::Tagged));
        // A slot that has held doubles, read only by Number operations: prove Number here,
        // once, and keep the double view, so its arithmetic takes the double operation first.
        if ty != Type::INT32 && number_read(self.program, self.fields, instruction)
            && self.demand_from(pc + 1, usize::from(destination)).numeric_only() {
            self.number(state, destination, exit);
        }
        if ty == Type::INT32 {
            let int = self.raw_unary(Unary::Narrow(Lane::I32), value);
            self.define_int_view(state, destination, value, int);
        }
    }

    /// The type of `value`, read from slots that have held one type (`held_slot_type`), proved: an
    /// online body depends on the slots' observations, which every write joins before it lands
    /// (`ValueArena::note_slot_value`) and retires the body at a widening
    /// (`LaneDependencies`), so a read of the covered slots alone (`fresh`: no earlier proof stands
    /// in for them) reads the invalidation word (`hold_stable`) and tests no tag, whatever class the
    /// slots held (int32, Number, direct object). Otherwise an int32 or direct-object tag is tested
    /// once here, leaving through `exit` where it fails, so no later use tests it again; a Number
    /// is left untyped, as `define_read_value` proves it where only Number operations read it.
    fn prove_held_slot(&mut self, state: &mut State, instruction: WideInstruction, (value, fresh): (ir::Value, bool),
        exit: ir::Block) -> Type {
        let Some(ty) = held_slot_type(self.program, self.fields, instruction) else { return Type::ANY };
        if fresh && (self.division().folds(BindingTime::Site) || self.closed_typed_read(instruction)) && self.reads_covered_slots(state, instruction) {
            self.hold_stable(state, exit);
        } else if ty == Type::INT32 {
            let integer = is_int_tagged(&mut self.builder, value);
            emit_js_guard(&mut self.builder, integer, exit);
        } else if ty == Type::DIRECT_OBJECT_FAMILY {
            let object = quench_runtime::staged::value::is_direct_object(&mut emit(&mut self.builder), value);
            emit_js_guard(&mut self.builder, object, exit);
        } else {
            return Type::ANY;
        }
        ty
    }

    /// Whether `instruction` reads only an ahead image's guessed slots of one class
    /// (`NativeFieldLoad::closed_typed`): the closed world's field type, which the image's slot
    /// dependencies hold (`ahead_images::guard_dependencies`), retiring it where a store widens one.
    fn closed_typed_read(&self, instruction: WideInstruction) -> bool {
        self.vm.facts.is_some() && self.helpers.links()
            && last_read_site(self.program, instruction).and_then(|last| site_loads(self.fields, last))
                .is_some_and(|loads| !loads.is_empty() && loads.iter().all(|load| load.closed_typed()))
    }

    /// Gives `destination`, defined as the tagged `value`, the int32 view `int` of it.
    pub(crate) fn define_int_view(&mut self, state: &mut State, destination: Register, value: ir::Value, int: ir::Value) {
        let variable = self.int_var(usize::from(destination));
        self.builder.def_var(variable, int);
        state.values[usize::from(destination)] = RegisterFact { tagged: TaggedView::Value(value),
            ..RegisterFact::definition(Type::INT32, Representation::Int32) };
    }
}

impl Lowering<'_, '_> {
    /// Whether the loop field `field`, whose first read is `instruction`, is carried as a raw
    /// double: every read and store site names the one own slot of one shape, whose arena words
    /// are raw doubles, and every store writes a Number. Its stores then write the binary64 word
    /// through the slot the entry projected and its reads take the carried word, boxing nothing.
    pub(crate) fn carries_number(&self, field: &LoopField, instruction: WideInstruction, boundaries: &quench_runtime::dataflow::BitSet) -> bool {
        let Some(FieldLookup::Atom { cache_site, .. }) = instruction.field_lookup() else { return false };
        let raw_slot = |site: u16| match site_loads(self.fields, site).map(Vec::as_slice) {
            Some([load]) if load.holder.is_none() && load.valid() && !load.function_shape && load.raw_double => Some((load.shape, load.slot)),
            _ => None,
        };
        let Some(slot) = raw_slot(cache_site) else { return false };
        let reads = field.reads.iter().all(|pc| match self.unit.instructions[*pc].field_lookup() {
            Some(FieldLookup::Atom { cache_site, .. }) => raw_slot(cache_site) == Some(slot),
            _ => false,
        });
        reads && field.stores.iter().all(|store| {
            let instruction = self.unit.instructions[*store];
            raw_slot(instruction.cache_site_index()) == Some(slot)
                && self.facts.before(*store, instruction.register_a(), &self.unit.instructions, boundaries,
                    &self.program.constants, &writes, self.native_calls).is_subtype(Type::NUMBER)
        })
    }
}

