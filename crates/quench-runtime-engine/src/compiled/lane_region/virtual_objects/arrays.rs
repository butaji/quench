//! Array literals kept as their element values (`VirtualArray`): admission, element reads and writes, `.length`,
//! and materialization. Split from `virtual_objects` along the array seam.

use super::*;

/// Where a kept array's access lands: the one element the solver proves the index names, or any of
/// them by the index's word (`kept_index`).
#[derive(Clone, Copy)]
pub(super) enum KeptIndex {
    At(usize),
    Select(ir::Value),
}

impl Lowering<'_, '_> {
    /// The literal at `pc` a kept array makes: an array of elements (`Allocation::ArrayOf`) whose definitions
    /// follow it, its element registers in order. A literal that returns its value is the allocation's own.
    pub(crate) fn virtual_array_plan(&self, pc: usize, instruction: WideInstruction) -> Option<smallvec::SmallVec<[Register; ARRAY_LITERAL_WINDOW]>> {
        if instruction.result_flags() != 0 || instruction.returns_from_frame() || !self.allocation_row(pc, instruction) { return None; }
        let Some((result, Allocation::ArrayOf { elements })) = Allocation::decode(instruction, self.straight_following(pc)) else { return None };
        if result >= self.unit.registers { return None; }
        Some(elements.registers().iter().copied().collect())
    }

    /// The literal at `pc` kept as its element values, read from the registers its definitions name, which
    /// those definitions consume. An element kept as a receiver or an array escapes into this one, so it is
    /// allocated first. The poll leaves before the array is kept where a collection is requested: nothing
    /// allocates between it and the array's allocation (`materialize_arrays`).
    pub(crate) fn virtual_array_construct(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, elements: &[Register]) {
        if elements.iter().any(|register| self.is_kept(state, *register)) { self.materialize_virtuals(state, pc); }
        self.interrupt_check(state, pc as u32);
        let mut kept = smallvec::SmallVec::new();
        for register in elements {
            let bits = self.tagged(state, *register);
            let variable = self.builder.declare_var(types::I64);
            self.builder.def_var(variable, bits);
            kept.push((variable, state.values[usize::from(*register)].value.ty));
        }
        let result = instruction.result_register();
        let placeholder = self.raw_const(Lane::I64, 0);
        self.define(state, result, placeholder, RegisterFact::definition(Type::ANY, Representation::Tagged));
        state.virtuals.retain(|other| other.register != result);
        state.virtual_arrays.retain(|other| other.register != result);
        state.virtual_arrays.push(VirtualArray { register: result, elements: kept });
        self.consume_definitions(pc, elements.len());
    }

    /// The access at `pc` to a kept array, lowered over its element variables: an element read, an element
    /// write whose value is no kept receiver or array, or `.length`. `None` for any other instruction, which
    /// then materializes the arrays it reads (`keeps_virtuals`).
    pub(crate) fn virtual_array_access(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        if state.virtual_arrays.is_empty() { return None; }
        if crate::compiled::meaning_compiled::is_get_field(instruction) { return self.kept_length(state, pc, instruction); }
        match crate::compiled::performed(instruction)? {
            quench_runtime::staged::Operation::ElementRead { base, key, .. } => self.kept_read(state, pc, instruction, (base, key)),
            quench_runtime::staged::Operation::ElementWrite { value, base, key, .. } => self.kept_write(state, pc, (value, base, key)),
            _ => None,
        }
    }

    /// Whether `register` holds a kept receiver or a kept array.
    pub(crate) fn is_kept(&self, state: &State, register: Register) -> bool {
        state.virtuals.iter().any(|kept| kept.register == register) || state.virtual_arrays.iter().any(|kept| kept.register == register)
    }

    /// Allocates each kept array live at `pc` from its element variables, into its register, and drops the rest:
    /// what the instruction at `pc`, or the interpreter resuming there, reads as the array. Nothing allocates
    /// between a construction and here (`keeps_virtuals`), so the allocation cannot find a collection due.
    pub(crate) fn materialize_arrays(&mut self, state: &mut State, pc: usize) {
        for kept in std::mem::take(&mut state.virtual_arrays) {
            if !self.is_live(pc, kept.register) { continue; }
            let count = kept.elements.len();
            let slot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,
                (count * size_of::<Value>()) as u32, align_of::<Value>().trailing_zeros() as u8));
            for (index, (variable, _)) in kept.elements.iter().enumerate() {
                let bits = self.builder.use_var(*variable);
                self.slot_store(slot, Lane::I64, bits, (index * size_of::<Value>()) as i32);
            }
            let elements = self.slot_address(slot, 0);
            let length = self.raw_const(Lane::I64, count as i64);
            let payload = self.payload;
            let this = self.helper_call(LaneHelper::NewArray, &[payload, length, elements]);
            let built = self.raw_compare_imm(Relation::NotEqual, this, Value::DELETED.to_bits() as i64);
            self.builder.ins().trapz(built, ir::TrapCode::unwrap_user(1));
            self.bind_allocated(state, kept.register, this);
        }
    }

    /// Where the access `key` of a kept array of `length` elements lands at `pc`: the solver's int range of the
    /// key must lie in `0..length`. A key of one value is that element; any other is selected by its word, read
    /// as an int (a guard leaves at `pc` where the register holds no int).
    pub(super) fn kept_index(&mut self, state: &mut State, pc: usize, register: Register, length: usize) -> Option<KeptIndex> {
        let range = self.cell_before(pc, register).ty.int_range()?;
        if range.min < 0 || range.max >= length as i64 { return None; }
        if range.min == range.max { return Some(KeptIndex::At(range.min as usize)); }
        let exit = self.guard_exit(state, pc as u32);
        let index = self.int(state, register, exit);
        let index = if self.raw_lane(index) == Lane::I64 { index } else { self.raw_unary(Unary::ZeroExtend(Lane::I64), index) };
        Some(KeptIndex::Select(index))
    }

    /// The element `index` names among `elements`: its variable, or the select over all of them by the word.
    pub(super) fn select_element(&mut self, elements: &[(Variable, Type)], index: KeptIndex) -> ir::Value {
        match index {
            KeptIndex::At(element) => self.builder.use_var(elements[element].0),
            KeptIndex::Select(index) => {
                let mut value = self.builder.use_var(elements[0].0);
                for (slot, (variable, _)) in elements.iter().enumerate().skip(1) {
                    let hit = self.raw_compare_imm(Relation::Equal, index, slot as i64);
                    let candidate = self.builder.use_var(*variable);
                    value = self.raw_select(hit, candidate, value);
                }
                value
            }
        }
    }

    /// `arr[key]` of a kept array into `destination`: the element's value, typed by the join of the elements the
    /// access may name. A destination that is the array itself ends the array there.
    pub(super) fn kept_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction,
        (base, key): (Register, quench_runtime::bytecode::Operand)) -> Option<bool> {
        let position = state.virtual_arrays.iter().position(|kept| kept.register == base)?;
        let index = self.kept_index(state, pc, key.register_index()?, state.virtual_arrays[position].elements.len())?;
        let elements = state.virtual_arrays[position].elements.clone();
        let value = self.select_element(&elements, index);
        let ty = elements.iter().map(|(_, ty)| *ty).reduce(|a, b| a.join(b))?;
        let destination = instruction.result_register();
        self.define(state, destination, value, RegisterFact::definition(ty, Representation::Tagged));
        state.virtual_arrays.retain(|kept| kept.register != destination);
        Some(true)
    }

    /// `arr[key] = value` of a kept array: the element the index names takes the value's bits, and each element the
    /// index may name takes them in a select. A value that is itself kept escapes into the array, so the write
    /// materializes instead.
    pub(super) fn kept_write(&mut self, state: &mut State, pc: usize, (value, base, key): (Register, Register, Register)) -> Option<bool> {
        if value == base || self.is_kept(state, value) { return None; }
        let position = state.virtual_arrays.iter().position(|kept| kept.register == base)?;
        let index = self.kept_index(state, pc, key, state.virtual_arrays[position].elements.len())?;
        let ty = state.values[usize::from(value)].value.ty;
        let bits = self.tagged(state, value);
        let elements = &mut state.virtual_arrays[position].elements;
        match index {
            KeptIndex::At(element) => {
                self.builder.def_var(elements[element].0, bits);
                elements[element].1 = ty;
            }
            KeptIndex::Select(index) => for slot in 0..elements.len() {
                let (variable, element_ty) = elements[slot];
                let hit = self.raw_compare_imm(Relation::Equal, index, slot as i64);
                let current = self.builder.use_var(variable);
                let next = self.raw_select(hit, bits, current);
                self.builder.def_var(variable, next);
                elements[slot].1 = element_ty.join(ty);
            },
        }
        Some(true)
    }

    /// `arr.length` of a kept array: its literal's length, an int32 constant.
    pub(super) fn kept_length(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let base = self.field_base(pc, instruction)?.register_index()?;
        let position = state.virtual_arrays.iter().position(|kept| kept.register == base)?;
        let (parts, sink) = self.field_chain(instruction)?;
        let [(atom, _)] = parts[..] else { return None };
        if sink.is_some() || instruction.result_flags() != 0 || instruction.writes_current_this() { return None; }
        if self.program.atoms.get(atom as usize) != Some("length") { return None; }
        let length = state.virtual_arrays[position].elements.len();
        let word = self.raw_const(Lane::I32, length as i64);
        let destination = instruction.result_register();
        self.set_int(state, destination, word);
        state.virtual_arrays.retain(|kept| kept.register != destination);
        Some(true)
    }
}
