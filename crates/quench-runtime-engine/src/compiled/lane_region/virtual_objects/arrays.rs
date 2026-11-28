//! Arrays kept as their element values (`VirtualArray`): array literals and `new Array(k)` of a constant
//! `k`, grown by `push`. Admission, element reads and writes, `.length`, and materialization. Split from
//! `virtual_objects` along the array seam.

#[allow(unused_imports)]
use quench_runtime::staged::AllocationAllocationExt;
use super::*;
use super::super::builtin_calls::Builtin;
use super::super::native_calls::{Inline, definition_before};
use quench_runtime::tier_surface::Native;
use quench_runtime::vm::global_facts::{GlobalRead, ReceiverLink};

/// Where a kept array's access lands: the one element the solver proves the index names, or any of
/// them by the index's word (`kept_index`).
#[derive(Clone, Copy)]
pub(super) enum KeptIndex {
    At(usize),
    Select(ir::Value),
}

/// The written mask of the first `count` elements, all of them (`VirtualArray::written`). A kept array holds at
/// most `ARRAY_LITERAL_WINDOW` elements, which fit its `u16` mask.
fn all_elements(count: usize) -> u16 {
    (1u16 << count) - 1
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
        state.virtual_arrays.retain(|other| other.register != result);
        let written = all_elements(elements.len());
        state.virtual_arrays.push(VirtualArray { register: result, elements: kept, written });
        self.consume_definitions(pc, elements.len());
    }

    /// `new Array(k)` at `pc` with a constant `k` that the kept window holds, kept as `k` holes: each element
    /// holds `undefined` until a write defines it. The callee is the realm's `Array`, guarded by identity as a
    /// call of it is (`intrinsic_call`), and the guard leaves before the array exists. False, with no effect,
    /// where the construction is another.
    pub(crate) fn kept_array_new(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        if self.completion.numeric() || instruction.result_flags() != 0 || instruction.returns_from_frame() || !self.is_construction(instruction) { return false; }
        let quench_runtime::bytecode::ConstructArguments::Registers(window) = instruction.construct_arguments() else { return false };
        let result = instruction.result_register();
        if window.count != 1 || result >= self.unit.registers || !self.holds_holders() { return false; }
        // The block the construction ends would carry the array into a join, which keeps none (`join_states`).
        if self.blocks.get(pc + 1).is_some_and(Option::is_some) { return false; }
        let Some(defined) = definition_before(self.unit, pc, instruction.register_b()) else { return false };
        let Some(read) = self.global_reads.get(&defined) else { return false };
        let (value, holder) = match *read {
            GlobalRead::NativeSlot { value, native: Native::Array, holder, .. } | GlobalRead::NativeMember { value, native: Native::Array, holder, .. } => (value, holder),
            _ => return false,
        };
        let Some(range) = self.cell_before(pc, window.base).ty.int_range() else { return false };
        if range.min != range.max || !(0..=ARRAY_LITERAL_WINDOW as i64).contains(&range.min) { return false; }
        let count = range.min as usize;
        let exit = self.guard_exit_for(state, pc as u32, ExitFact::Callee);
        self.hold_holder_any(state, exit, holder);
        // The construction's callee the solve proves is the builtin it names (`named_builtin`) needs no identity compare.
        if !self.named_builtin(defined).is_some_and(|expected| self.proves_builtin(pc, instruction.register_b(), expected)) {
            let held = self.tagged(state, instruction.register_b());
            let same = self.is_identity(held, value);
            emit_js_guard(&mut self.builder, same, exit);
        }
        let hole = Value::UNDEFINED;
        let mut elements = smallvec::SmallVec::<[(Variable, Type); ARRAY_LITERAL_WINDOW]>::new();
        for _ in 0..count {
            let variable = self.builder.declare_var(types::I64);
            let bits = self.raw_const(Lane::I64, hole.to_bits() as i64);
            self.builder.def_var(variable, bits);
            elements.push((variable, Type::immediate(hole)));
        }
        let placeholder = self.raw_const(Lane::I64, 0);
        self.define(state, result, placeholder, RegisterFact::definition(Type::ANY, Representation::Tagged));
        state.virtual_arrays.retain(|other| other.register != result);
        state.virtual_arrays.push(VirtualArray { register: result, elements, written: 0 });
        true
    }

    /// The access at `pc` to a kept array, lowered over its element variables: an element read, an element
    /// write whose value is no kept receiver or array, `.length`, a method read by name (`kept_method`), or a
    /// `push` (`kept_push`). `None` for any other instruction, which then materializes the arrays it reads
    /// (`keeps_virtuals`).
    pub(crate) fn virtual_array_access(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        if state.virtual_arrays.is_empty() { return None; }
        if crate::compiled::meaning_compiled::is_get_field(instruction) {
            return match self.kept_length(state, pc, instruction) {
                Some(falls) => Some(falls),
                None => self.kept_method(state, pc, instruction),
            };
        }
        if let Some(falls) = self.kept_push(state, pc, instruction) { return Some(falls); }
        match crate::compiled::performed(instruction)? {
            quench_runtime::staged::Operation::ElementRead { base, key, .. } => self.kept_read(state, pc, instruction, (base, key)),
            quench_runtime::staged::Operation::ElementWrite { value, base, key, .. } => self.kept_write(state, pc, (value, base, key)),
            _ => None,
        }
    }

    /// Whether `register` holds a kept receiver or a kept array.
    pub(crate) fn is_kept(&self, state: &State, register: Register) -> bool {
        state.virtuals.iter().any(|kept| kept.aliases.contains(&register)) || state.virtual_arrays.iter().any(|kept| kept.register == register)
    }

    /// Allocates each kept array live at `pc` from its element values, into its register, and drops the rest:
    /// what the instruction at `pc`, or the interpreter resuming there, reads as the array. Nothing allocates
    /// between a construction and here (`keeps_virtuals`). Each array is made by `MaterializeArray`, which never
    /// refuses for a due collection: an allocation sets a collection due and never runs one (DESIGN 4.7), and
    /// the next safepoint collects (the lane's leave, or an allocating helper's refusal or poll). So any number
    /// of kept arrays and receivers can be made at one point, and no poll is needed before them.
    pub(crate) fn materialize_arrays(&mut self, state: &mut State, pc: usize) {
        for kept in std::mem::take(&mut state.virtual_arrays) {
            if !self.is_live(pc, kept.register) { continue; }
            let count = kept.elements.len();
            let payload = self.payload;
            let length = self.raw_const(Lane::I64, count as i64);
            let this = if kept.written == all_elements(count) {
                let elements = self.kept_slot(&kept.elements);
                self.new_kept_array(payload, length, elements)
            } else {
                // A hole array first, then each written element defined as its literal's definition is.
                let none = self.raw_const(Lane::I64, 0);
                let array = self.new_kept_array(payload, length, none);
                for (index, (variable, _)) in kept.elements.iter().enumerate() {
                    if kept.written & (1u16 << index) == 0 { continue; }
                    let bits = self.builder.use_var(*variable);
                    let index = self.raw_const(Lane::I64, index as i64);
                    let defined = self.helper_call(LaneHelper::DefineKeptElement, &[payload, array, index, bits]);
                    let ok = self.raw_compare_imm(Relation::NotEqual, defined, 0);
                    self.builder.ins().trapz(ok, ir::TrapCode::unwrap_user(1));
                }
                array
            };
            self.bind_allocated(state, &[kept.register], this);
        }
    }

    /// The array `MaterializeArray` makes of `length` from `elements` (a slot's address, or none for holes). It
    /// never answers `DELETED`, so the result is the array with no check (`materialize_arrays`).
    fn new_kept_array(&mut self, payload: ir::Value, length: ir::Value, elements: ir::Value) -> ir::Value {
        self.helper_call(LaneHelper::MaterializeArray, &[payload, length, elements])
    }

    /// A stack slot holding the bits of `elements` in index order, its address.
    fn kept_slot(&mut self, elements: &[(Variable, Type)]) -> ir::Value {
        let slot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,
            (elements.len() * size_of::<Value>()) as u32, align_of::<Value>().trailing_zeros() as u8));
        for (index, (variable, _)) in elements.iter().enumerate() {
            let bits = self.builder.use_var(*variable);
            self.slot_store(slot, Lane::I64, bits, (index * size_of::<Value>()) as i32);
        }
        self.slot_address(slot, 0)
    }

    /// Where the access `key` of a kept array of `length` elements lands at `pc`: the solver's int range of the
    /// key must lie in `0..length`. A key of one value is that element; any other is selected by its word, read
    /// as an int (a guard leaves at `pc` where the register holds no int). `select_ok` admits the select: an access
    /// through it needs every element written, so no hole is read or left unrecorded.
    fn kept_index(&mut self, state: &mut State, pc: usize, register: Register, length: usize, select_ok: bool) -> Option<KeptIndex> {
        let range = self.cell_before(pc, register).ty.int_range()?;
        if range.min < 0 || range.max >= length as i64 { return None; }
        if range.min == range.max { return Some(KeptIndex::At(range.min as usize)); }
        if !select_ok { return None; }
        let exit = self.guard_exit(state, pc as u32);
        let index = self.int(state, register, exit);
        let index = if self.raw_lane(index) == Lane::I64 { index } else { self.raw_unary(Unary::ZeroExtend(Lane::I64), index) };
        Some(KeptIndex::Select(index))
    }

    /// The element `index` names among `elements`: its variable, or the select over all of them by the word.
    fn select_element(&mut self, elements: &[(Variable, Type)], index: KeptIndex) -> ir::Value {
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
    /// access may name. A destination that is the array itself ends the array there. A constant index of an unwritten
    /// element (a hole) is read only where `holes_read_undefined`; elsewhere the array materializes at the read.
    fn kept_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction,
        (base, key): (Register, quench_runtime::bytecode::Operand)) -> Option<bool> {
        let position = state.virtual_arrays.iter().position(|kept| kept.register == base)?;
        let array = &state.virtual_arrays[position];
        let (length, written) = (array.elements.len(), array.written);
        let every_written = written == all_elements(length);
        let index = self.kept_index(state, pc, key.register_index()?, length, every_written)?;
        if let KeptIndex::At(element) = index && written & (1u16 << element) == 0 && !self.holes_read_undefined() { return None; }
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
    /// materializes instead. Only a constant index proves its element written, so a select writes only an array
    /// with every element written (`kept_read`'s rule): its unwritten elements would otherwise lose the value.
    fn kept_write(&mut self, state: &mut State, pc: usize, (value, base, key): (Register, Register, Register)) -> Option<bool> {
        if value == base || self.is_kept(state, value) { return None; }
        let position = state.virtual_arrays.iter().position(|kept| kept.register == base)?;
        let array = &state.virtual_arrays[position];
        let (length, every_written) = (array.elements.len(), array.written == all_elements(array.elements.len()));
        let index = self.kept_index(state, pc, key, length, every_written)?;
        let ty = state.values[usize::from(value)].value.ty;
        let bits = self.tagged(state, value);
        if let KeptIndex::At(element) = index { state.virtual_arrays[position].written |= 1u16 << element; }
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

    /// `arr.push(value)` of a kept array, where the call is the native `push` the solver proved (`intrinsic_call`):
    /// the value becomes the element at the array's length, which grows by one. The callee is guarded by identity
    /// and the guard leaves before anything changes. A value that is kept, or a push past the kept window, is
    /// admitted by no form here and the call materializes the array.
    fn kept_push(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let Some(&Some((callee, Inline::Builtin(Builtin::Push), holder))) = self.intrinsics.get(pc) else { return None };
        let (receiver, argument) = (instruction.register_c(), quench_runtime::bytecode::ImmediateLayout::call_window_base(instruction.imm()));
        let position = state.virtual_arrays.iter().position(|kept| kept.register == receiver)?;
        if argument == receiver || self.is_kept(state, argument) || state.virtual_arrays[position].elements.len() >= ARRAY_LITERAL_WINDOW { return None; }
        if !self.holds_holders() { return None; }
        let exit = self.guard_exit_for(state, pc as u32, ExitFact::Callee);
        self.hold_holder_any(state, exit, holder);
        let held = self.tagged(state, instruction.register_b());
        let same = self.is_identity(held, callee);
        emit_js_guard(&mut self.builder, same, exit);
        let ty = state.values[usize::from(argument)].value.ty;
        let bits = self.tagged(state, argument);
        let variable = self.builder.declare_var(types::I64);
        self.builder.def_var(variable, bits);
        let array = &mut state.virtual_arrays[position];
        let slot = array.elements.len();
        array.elements.push((variable, ty));
        array.written |= 1u16 << slot;
        let length = self.raw_const(Lane::I32, (slot + 1) as i64);
        let destination = instruction.result_register();
        state.virtual_arrays.retain(|kept| kept.register != destination);
        self.set_int(state, destination, length);
        Some(true)
    }

    /// A method read by name of a kept array (`arr.push`): the member the read resolved to (`GlobalRead::NativeMember`).
    /// A kept array is a direct array of no named property, linked to `Array.prototype` (`ReceiverLink::Array`), so the
    /// read is that member and needs no record guard; its holder is held as a dependency, and a call of the member
    /// guards its identity (`intrinsic_call`). Any other read materializes the array.
    fn kept_method(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let Some(&GlobalRead::NativeMember { value, holder, link: ReceiverLink::Array(_), .. }) = self.global_reads.get(&pc) else { return None };
        let base = self.field_base(pc, instruction)?.register_index()?;
        state.virtual_arrays.iter().position(|kept| kept.register == base)?;
        let (parts, sink) = self.field_chain(instruction)?;
        let [_] = parts[..] else { return None };
        if sink.is_some() || instruction.result_flags() != 0 || instruction.writes_current_this() || instruction.returns_from_frame() || !self.holds_holders() { return None; }
        let exit = self.guard_exit_for(state, pc as u32, ExitFact::Shape);
        self.hold_holder_any(state, exit, holder);
        let member = self.identity_constant(value);
        let destination = instruction.result_register();
        self.define(state, destination, member, RegisterFact::definition(Type::immediate(value), Representation::Tagged));
        state.virtual_arrays.retain(|kept| kept.register != destination);
        Some(true)
    }

    /// `arr.length` of a kept array: its element count, an int32 constant.
    fn kept_length(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
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
