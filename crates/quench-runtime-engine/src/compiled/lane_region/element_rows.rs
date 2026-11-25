//! The staged element rows (`staged::element_rows`) lowered: the lane decides by the row's data,
//! not by the opcode it decoded from. A loop's hoisted array (`LoopArray`), whose view its entering
//! edge proved once, is read and written through that view; any other access is the row itself,
//! over the frame's proofs of the array (`frame_elements`), with the exit, the join and the
//! views a store retires left to this lowering.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::typed_elements::{typed_read_integer, TypedView};
use quench_runtime::staged::{Operation, Stage, Step};
use quench_runtime::staged::NumericStage as _;

impl Lowering<'_, '_> {
    /// The row of `instruction` where it is an element access whose operands this unit holds.
    fn element_row(&self, instruction: WideInstruction) -> Option<Operation<ir::Value, EmitProof>> {
        let row = Operation::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY)?;
        let held = |register: Register| register < self.unit.registers;
        let supported = match row {
            Operation::ElementRead { base, key, .. } => held(base) && self.operand_supported(key),
            Operation::ElementWrite { value, base, key, .. } => [value, base, key].into_iter().all(held),
            _ => false,
        };
        supported.then_some(row)
    }

    /// Whether `instruction` is a row `element_step` lowers.
    pub(crate) fn is_element_row(&self, instruction: WideInstruction) -> bool {
        self.element_row(instruction).is_some()
    }

    /// The element row at `pc`, lowered by its definition; whether control falls through.
    pub(crate) fn element_step(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        match self.element_row(instruction).expect("an element row decodes") {
            Operation::ElementRead { base, key, .. } => {
                let destination = instruction.result_register();
                let falls = self.element_read(state, pc, (destination, base, key));
                if falls { self.conform_declared_rest_read(state, pc, base, destination); }
                falls
            }
            Operation::ElementWrite { value, base, key, .. } => self.element_write(state, (pc, instruction), (value, base, key)),
            _ => unreachable!("an element row is a read or a write"),
        }
    }

    /// An element read into `destination`: a hoisted array's through its view (a typed array's a
    /// Number), any other the row. The element an observation typed (`number_reads`) is proved after it.
    fn element_read(&mut self, state: &mut State, pc: usize, (destination, base, key): (Register, Register, quench_runtime::bytecode::Operand)) -> bool {
        let exit = self.guard_exit_for(state, pc as u32, ExitFact::Element);
        let array = self.loop_array(pc, Some(base));
        if let Some(pending) = self.unit_compare(state, pc, (destination, base, key), exit) {
            self.pending_unit = Some(pending);
            return true;
        }
        if let Some(array) = array.filter(|array| self.int32_read(*array, pc))
            && self.int32_element_read(state, (destination, key), array, exit) { return true; }
        if let Some(array) = array.filter(|array| self.reads_int32(*array))
            && self.integer_element_read(state, (destination, key), array, exit) { return true; }
        if let Some(array) = array.filter(|array| self.int_only.contains(&(array.header, array.source)))
            && self.observed.at(pc).is_some_and(|ty| ty.is_int32())
            && self.proved_int_element_read(state, pc, (destination, key), array, exit) { return true; }
        // An element read as a Number (`number_reads`) needs no hole test of its own: a hole is a NaN
        // pattern, which the Number proof's one NaN test sends to the tag check that leaves.
        let packed = self.observed.at(pc).is_some();
        let within = self.ranges.in_bounds(pc);
        self.held_view = array.map(|array| self.held_array(array, (packed, within)));
        // A hoisted array's hole leaves, until the read has left often enough to be widened
        // (`LoopTier::left_at`), whose recompile then takes the VM's read.
        self.hole_leaves = array.is_some() && !self.generic.contains(&pc);
        let value = match (array, key.register_index()) {
            // A base no loop array holds, read by a register key: a string's unit (`string_or_row`).
            (None, Some(index)) => {
                let string_within = self.ranges.string_in_bounds(pc);
                self.string_or_row(state, pc, (destination, base, index, key), string_within, exit)
            }
            _ => {
                let Step::Value(value) = self.step(state, frame::Edges::ValueGuarded { miss: exit }, Operation::ElementRead { base, key, destination })
                    else { unreachable!("an element read yields a value or leaves at its miss edge") };
                value
            }
        };
        if array.is_some_and(|array| matches!(array.views, ElementViews::Typed(_) | ElementViews::Double)) {
            self.set_number(state, destination, value);
        } else {
            self.set_tagged(state, destination, value);
        }
        match self.observed.at(pc) {
            Some(ty) if ty.is_int32() => { self.int(state, destination, exit); }
            Some(_) => { self.number(state, destination, exit); }
            None => {}
        }
        true
    }

    /// The unit compare of the string element read at `pc` into `destination` (`PendingUnit`): a read of
    /// a string's unit whose next instruction is a strict `===` or `!==` of the read's register with a
    /// one-unit string constant, and whose register no path reads after the comparison. The one-character
    /// string such a read makes is then never made: the comparison decides on the unit
    /// (`Lowering::unit_compare_jump`). The read leaves at `exit` for a base that is no string, and
    /// for an index outside it, as the row does.
    fn unit_compare(&mut self, state: &mut State, pc: usize, (destination, base, key): (Register, Register, quench_runtime::bytecode::Operand),
        exit: ir::Block) -> Option<PendingUnit> {
        let index = key.register_index()?;
        let next = pc + 1;
        let instruction = *self.unit.instructions.get(next)?;
        let Some(quench_runtime::staged::Operation::CompareJump { comparison: quench_runtime::staged::Comparison::Equality { strict: true, negated }, left, right, .. })
            = crate::compiled::performed(instruction) else { return None };
        let constant = match (left.register_index(), right.register_index()) {
            (Some(register), None) if register == destination => self.one_unit_constant(right)?,
            (None, Some(register)) if register == destination => self.one_unit_constant(left)?,
            _ => return None,
        };
        let target = instruction.jump_target() as usize;
        let string_base = self.loop_array(pc, Some(base)).is_none_or(|array| self.string_base(&array));
        let unread = !self.is_live(pc + 2, destination) && !self.is_live(target, destination);
        if !string_base || !unread || self.hoisted.contains_key(&next) || self.unit_leaders().contains(next) { return None; }
        let within = self.ranges.string_in_bounds(pc);
        let bits = self.tagged(state, base);
        let is_string = { let mut stage = super::super::object_slots::emit(&mut self.builder); quench_runtime::staged::value::is_direct_string(&mut stage, bits) };
        emit_js_guard(&mut self.builder, is_string, exit);
        let key_word = self.tagged(state, index);
        let records = super::super::emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(super::super::JsStepDispatch, head.string_record_base));
        let unit = {
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            let units = quench_runtime::staged::strings::flat_string(&mut stage, records, bits, exit).expect("the emit stage continues past its guards");
            let row = if within {
                quench_runtime::staged::strings::string_unit_at_within(&mut stage, units, key_word, exit)
            } else {
                quench_runtime::staged::strings::string_unit_at(&mut stage, units, key_word, exit)
            };
            row.expect("the emit stage continues past its guards")
        };
        Some(PendingUnit { pc: next, register: destination, unit, constant, negated })
    }

    /// `base[index]` where `base` holds a string: its one-unit string from the ASCII unit table
    /// (`strings::string_char_at`), joined with the element row otherwise. A string the row does
    /// not read (a rope, a wide unit, a unit with no string yet, an index outside the string)
    /// leaves at `exit`, as the row leaves for a value that is no array.
    /// The row may change a register's representation (the key's int32 view) on its own path, so
    /// the join meets the string path's state with the row's (`join_states`).
    fn string_or_row(&mut self, state: &mut State, pc: usize, (destination, base, index, key): (Register, Register, Register, quench_runtime::bytecode::Operand),
        within: bool, exit: ir::Block) -> ir::Value {
        let bits = self.tagged(state, base);
        let is_string = { let mut stage = super::super::object_slots::emit(&mut self.builder); quench_runtime::staged::value::is_direct_string(&mut stage, bits) };
        // The key's tagged word is made before the paths split: the row's host read takes it too,
        // so a word the string path made would not dominate its use.
        let key_word = self.tagged(state, index);
        let (string, row, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        self.branch_if(is_string, (string, &[]), (row, &[]));
        self.builder.switch_to_block(string);
        let records = super::super::emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(super::super::JsStepDispatch, head.string_record_base));
        let table = super::super::emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(super::super::JsStepDispatch, head.ascii_unit_strings));
        let character = {
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            let units = quench_runtime::staged::strings::flat_string(&mut stage, records, bits, exit).expect("the emit stage continues past its guards");
            let row = if within {
                quench_runtime::staged::strings::string_char_at_within(&mut stage, units, key_word, table, exit)
            } else {
                quench_runtime::staged::strings::string_char_at(&mut stage, units, key_word, table, exit)
            };
            row.expect("the emit stage continues past its guards")
        };
        let joined = self.builder.append_block_param(join, ir::types::I64);
        self.goto(join, &[character]);
        let mut string_state = state.clone();
        self.builder.switch_to_block(row);
        let Step::Value(value) = self.step(state, frame::Edges::ValueGuarded { miss: exit }, Operation::ElementRead { base, key, destination })
            else { unreachable!("an element read yields a value or leaves at its miss edge") };
        self.goto(join, &[value]);
        self.builder.switch_to_block(join);
        // The destination is defined from `joined` after the join.
        string_state.values[usize::from(destination)] = state.values[usize::from(destination)];
        self.join_states(state, &string_state, pc);
        joined
    }

    /// Whether `array` is a hoisted typed array of a fixed kind whose elements are all int32s.
    fn reads_int32(&self, array: LoopArray) -> bool {
        array.views.observed_kind().and_then(NativeKind::new).is_some_and(NativeKind::is_int32)
    }

    /// Whether the hoisted dense `array` holds int32s throughout its loop (`int32_loop`) and the
    /// read at `pc` is one the loop observed as an int32 (`int32_read_at`): such a read takes no tag test.
    fn int32_read(&self, array: LoopArray, pc: usize) -> bool {
        let ArraySource::Register(source) = array.source else { return false };
        self.int32_loop(array) && self.int32_read_at(pc, source) && !self.generic.contains(&pc)
    }

    /// Whether the element read at `pc` reads register `source` as its base and was observed an int32.
    fn int32_read_at(&self, pc: usize, source: Register) -> bool {
        let instruction = self.unit.instructions[pc];
        matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. }))
            && instruction.operand_b().register_index() == Some(source)
            && self.observed.at(pc).is_some_and(|ty| ty.is_int32())
    }

    /// Whether the hoisted dense `array` holds only int32s for its whole loop, so its element
    /// reads need no tag test. The heap keeps a backing's count of values that are no int32
    /// (`OTHERS_FROM_VALUES`, holes included), zero exactly when every element below the length is
    /// an int32, and `hoist_arrays` guards that count at the loop's entry. The count stays zero
    /// because every store of the loop is an int32 through `array`'s own register (`int32_stores`),
    /// and a store past the end leaves to the VM instead of making a hole (`dense_element_write`).
    /// Only an array the loop reads as int32s (`int32_read_at`) qualifies, so the guard never
    /// leaves for an array the loop would not read as one.
    pub(crate) fn int32_loop(&self, array: LoopArray) -> bool {
        let ArraySource::Register(source) = array.source else { return false };
        array.writable && array.views == ElementViews::Dense && self.int32_stores(array, source)
            && (array.header..=array.back).any(|pc| self.int32_read_at(pc, source) && !self.generic.contains(&pc))
    }

    /// Whether every store of the loop `array` belongs to is an int32 stored through `source`, the
    /// array's own register (the loop never redefines it, so it names the same array), and no append
    /// runs: an append or a store through another register could make a hole or a non-int32 element.
    fn int32_stores(&self, array: LoopArray, source: Register) -> bool {
        (array.header..=array.back).all(|pc| {
            if super::native_calls::appends(&self.intrinsics, pc) { return false; }
            match crate::compiled::performed(self.unit.instructions[pc]) {
                Some(quench_runtime::staged::Operation::ElementWrite { value, base, .. }) => base == source
                    && self.facts.at(pc).and_then(|types| types.get(usize::from(value))).is_some_and(|ty| ty.is_subtype(Type::INT32)),
                _ => true,
            }
        })
    }

    /// An element read of a hoisted dense `array` that `int32_read` admits, into `destination`: the
    /// element's word, narrowed to its int32, past the bounds test alone (no hole, no tag test).
    fn int32_element_read(&mut self, state: &mut State, (destination, key): (Register, quench_runtime::bytecode::Operand),
        array: LoopArray, exit: ir::Block) -> bool {
        let Some(index) = self.operand_index(state, key, exit) else { return false };
        let (values, length) = self.hoisted_view(array);
        let word = {
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            let index = stage.uextend32(index);
            let length = stage.uextend32(length);
            let (_, word) = quench_runtime::staged::elements::packed_element(&mut stage, values, length, index, exit)
                .expect("the emit stage continues past its guards");
            word
        };
        let int = super::super::object_slots::emit(&mut self.builder).narrow(word);
        self.set_int(state, destination, int);
        true
    }

    /// An element read of a hoisted typed array of an int32 kind (`reads_int32`) into `destination`
    /// as the int32 word it is: no Number is made of it and converted back. `false` where the
    /// access is not one, with nothing emitted.
    fn integer_element_read(&mut self, state: &mut State, (destination, key): (Register, quench_runtime::bytecode::Operand),
        array: LoopArray, exit: ir::Block) -> bool {
        let Some(index) = self.operand_index(state, key, exit) else { return false };
        let frame_elements::Held::Typed(view) = self.held_array(array, (false, false)) else { return false };
        let word = {
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            let index = stage.uextend32(index);
            typed_read_integer(&mut stage, view, index, exit)
        };
        let Some(word) = word else { return false };
        let int = if self.raw_lane(word) == Lane::I64 { self.raw_unary(Unary::Narrow(Lane::I32), word) } else { word };
        self.set_int(state, destination, int);
        true
    }

    /// Whether the loop array `array` is read only as int32s over a backing no store of its loop can
    /// change, so its entering edge can prove every element an int32 once (`hoist_arrays`). Such an
    /// array is read-only (a writable one's stores uncount its backing), its loop holds no element
    /// store or append (a store may move the backing, which reloads every view around it, or change
    /// its count), and every element read in the loop is observed int32 (a read observed otherwise
    /// would leave at the entry on every run that found a non-int32 element). The loop must read it,
    /// else the entry guard buys nothing.
    pub(crate) fn int_only_array(&self, array: LoopArray) -> bool {
        if array.views != ElementViews::Dense || array.writable { return false; }
        let mut reads = false;
        for pc in array.header..=array.back {
            let instruction = self.unit.instructions[pc];
            // A call or construction may run code that stores into the backing in place, which no
            // reload follows: its loop keeps the per-read test.
            let stores = matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. }))
                || native_calls::appends(&self.intrinsics, pc)
                || quench_runtime::staged::meaning::means(&instruction, quench_runtime::staged::meaning::Meaning::Call)
                || quench_runtime::staged::meaning::means(&instruction, quench_runtime::staged::meaning::Meaning::Construct);
            if stores { return false; }
            if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })) {
                if !self.observed.at(pc).is_some_and(|ty| ty.is_int32()) { return false; }
                reads = true;
            }
        }
        reads
    }

    /// An element read of the int-only array `array` (`int_only_array`) into `destination` as the int32
    /// its word is. No hole and no non-int32 element is present, so the word needs no tag test, and
    /// `Narrow` takes its int32 payload. The index is an int32 register (`int`), the view's length
    /// bounds it unless the access is proved within it (`ranges`). A key that is no register is the
    /// general read's, which takes this path not at all: `false`, with nothing emitted.
    fn proved_int_element_read(&mut self, state: &mut State, pc: usize, (destination, key): (Register, quench_runtime::bytecode::Operand),
        array: LoopArray, exit: ir::Block) -> bool {
        let Some(register) = key.register_index() else { return false };
        let index = self.int(state, register, exit);
        let (values, length) = self.hoisted_view(array);
        let within = self.ranges.in_bounds(pc);
        let word = {
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            quench_runtime::staged::elements::int_element(&mut stage, values, length, index, within, exit)
                .expect("the emit stage continues past its guards")
        };
        let int = self.raw_unary(Unary::Narrow(Lane::I32), word);
        self.set_int(state, destination, int);
        true
    }

    /// Whether the loop's array `array` is a register its entering facts type as a string: such a
    /// base is no array, so its element reads take the string row and it is no hoisted view.
    pub(crate) fn string_base(&self, array: &LoopArray) -> bool {
        let super::loop_arrays::ArraySource::Register(register) = array.source else { return false };
        self.facts.at(array.header).and_then(|types| types.get(usize::from(register)).copied())
            .is_some_and(|ty| ty.is_subtype(Type::language(quench_runtime::ValueType::String)))
    }

    /// The view the hoisted `array`'s entering edge proved, which its element rows read and write through.
    fn held_array(&mut self, array: LoopArray, proved: (bool, bool)) -> frame_elements::Held {
        let (values, length) = self.hoisted_view(array);
        match array.views {
            ElementViews::Dense => frame_elements::Held::Dense(super::super::DenseArrayView {
                values, length, writable: array.writable, uncounted: array.writable, double: None }, proved),
            // A double backing's words are a `Float64Array`'s (`ElementViews::observed_kind`).
            ElementViews::Typed(_) | ElementViews::Double =>
                frame_elements::Held::Typed(TypedView { values, length, kind: self.hoisted_kind(array), within: proved.1 }),
        }
    }

    /// An element store. A Number into a writable hoisted array goes through its view: its row
    /// replaces a present element in place and, where no miss was recorded at the site, leaves
    /// at the miss (`ElementFrame::index_set_host`); a site that missed plans the helper arm
    /// (`dense_element_write`). Any other store is the row, which replaces a present element in
    /// place and otherwise takes the VM's store, which may add the element and so retires every view. A key that is a string names a
    /// property, which the element store never makes: it leaves for the interpreter.
    fn element_write(&mut self, state: &mut State, (pc, instruction): (usize, WideInstruction),
        (value, base, key): (Register, Register, Register)) -> bool {
        let key_type = state.values[usize::from(key)].value.ty;
        if key_type != Type::NEVER && key_type.is_subtype(Type::language(quench_runtime::ValueType::String)) {
            self.exit(state, pc as u32);
            return false;
        }
        let exit = self.guard_exit_for(state, pc as u32, ExitFact::Element);
        let number = state.values[usize::from(value)].value.ty.is_subtype(Type::NUMBER);
        // A reference is stored through a hoisted dense view too, past its barrier gate (`store_gate`);
        // a typed view takes a Number alone.
        let array = self.loop_array(pc, Some(base))
            .filter(|array| array.writable && (number || array.views == ElementViews::Dense));
        match array {
            Some(array) if array.views == ElementViews::Dense && self.left_here =>
                self.dense_element_write(state, array, pc as u32, instruction, number, exit),
            held => {
                self.held_view = held.map(|array| self.held_array(array, (false, self.ranges.in_bounds(pc))));
                self.step(state, frame::Edges::ValueGuarded { miss: exit }, Operation::ElementWrite { value, base, key, strict: instruction.boolean_flag().unwrap_or(false) });
                // A store through a held view leaves where it misses (`index_set_host`) and calls no
                // helper, so no backing moved and no view is read again. Only the general row may
                // call the VM's store, which may move any backing.
                if held.is_none() { self.reload_dense_arrays(pc); }
            }
        }
        state.proofs.arrays.clear();
        state.proofs.fields.clear();
        // The VM's store may write a prototype's element or a mapped argument.
        state.proofs.stable = false;
        true
    }
}

/// Whether the element write `instruction` is strict code's, whose refused store raises (`Operation::ElementWrite::strict`).
fn element_write_strict(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded(instruction), Some(Operation::ElementWrite { strict: true, .. }))
}
