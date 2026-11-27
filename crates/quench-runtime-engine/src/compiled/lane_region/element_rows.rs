//! The staged element rows (`staged::element_rows`) lowered: the lane decides by the row's data,
//! not by the opcode it decoded from. A loop's hoisted array (`LoopArray`), whose view its entering
//! edge proved once, is read and written through that view; any other access is the row itself,
//! over the frame's proofs of the array (`frame_elements`), with the exit, the join and the
//! views a store retires left to this lowering.
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::typed_elements::{typed_read_integer, TypedView, ViewKind};
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
    /// Number), any other the row. The element the solve proves after it is typed by that proof.
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
            && self.proven_element(pc).is_some_and(|ty| ty.is_int32())
            && self.proved_int_element_read(state, pc, (destination, key), array, exit) { return true; }
        if array.is_none() && self.typed_int_read(state, pc, (destination, base, key), exit) { return true; }
        // An element read the solve proves a Number needs no hole test of its own: a hole is a NaN
        // pattern, which the Number proof's one NaN test sends to the tag check that leaves.
        let packed = self.proven_element(pc).is_some_and(|ty| ty.is_subtype(Type::NUMBER));
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
            _ => self.element_read_row(state, pc, array.is_some(), exit, Operation::ElementRead { base, key, destination }),
        };
        if array.is_some_and(|array| matches!(array.views, ElementViews::Typed(_) | ElementViews::Double)) {
            self.set_number(state, destination, value);
        } else {
            // A tagged element the solve proves a Number is defined with that proof, so its conversion below needs no test.
            match self.proven_element(pc) {
                Some(ty) if ty.is_subtype(Type::NUMBER) => self.define(state, destination, value, RegisterFact::definition(ty, Representation::Tagged)),
                _ => self.set_tagged(state, destination, value),
            }
        }
        match self.proven_element(pc) {
            Some(ty) if ty.is_int32() => { self.int(state, destination, exit); }
            Some(ty) if ty.is_subtype(Type::NUMBER) => { self.number(state, destination, exit); }
            _ => {}
        }
        true
    }

    /// The element read `row`: through the held view of a loop's array, else `row_or_typed`.
    fn element_read_row(&mut self, state: &mut State, pc: usize, held: bool, exit: ir::Block, row: Operation<ir::Value, EmitProof>) -> ir::Value {
        let Operation::ElementRead { base, .. } = row else { unreachable!("an element read row") };
        if !held { return self.row_or_typed(state, pc, base, exit, row).expect("an element read yields a value"); }
        let Step::Value(value) = self.step(state, frame::Edges::ValueGuarded { miss: exit }, row)
            else { unreachable!("an element read yields a value or leaves at its miss edge") };
        value
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
        let value = self.row_or_typed(state, pc, base, exit, Operation::ElementRead { base, key, destination }).expect("an element read yields a value");
        self.goto(join, &[value]);
        self.builder.switch_to_block(join);
        // The destination is defined from `joined` after the join.
        string_state.values[usize::from(destination)] = state.values[usize::from(destination)];
        self.join_states(state, &string_state, pc);
        joined
    }

    /// The element `row` of a base no loop array holds: a direct array's row, any other base's the same row over the
    /// typed view proved here (`typed_arms`). The arms' states meet at the join (`join_states`), and a read's value is joined from them.
    fn row_or_typed(&mut self, state: &mut State, pc: usize, base: Register, exit: ir::Block, row: Operation<ir::Value, EmitProof>) -> Option<ir::Value> {
        let bits = self.tagged(state, base);
        // Every operand's tagged word is made before the arms split, so each arm's view of it is the same word.
        let operands = match row { Operation::ElementRead { key, .. } => [key.register_index(), None], Operation::ElementWrite { value, key, .. } => [Some(value), Some(key)], _ => [None, None] };
        for register in operands.into_iter().flatten() { self.tagged(state, register); }
        let destination = match row { Operation::ElementRead { destination, .. } => Some(destination), _ => None };
        // The kind the interpreter's record observed (`body_views::typed_reads`) proves the view for that kind alone.
        let kind = super::super::body_views::kind_at(self.kinds, pc).and_then(quench_runtime::staged::typed_elements::NativeKind::new);
        if let Some(native) = kind { return self.typed_only(state, (base, bits, native), (row, destination, exit)); }
        let is_array = { let mut stage = super::super::object_slots::emit(&mut self.builder); quench_runtime::staged::value::is_direct_array(&mut stage, bits) };
        let (dense, typed, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        self.branch_if(is_array, (dense, &[]), (typed, &[]));
        let joined = destination.map(|_| self.builder.append_block_param(join, ir::types::I64));
        let entry = state.clone();
        self.builder.switch_to_block(dense);
        self.row_arm(state, (row, destination, exit, join));
        self.builder.switch_to_block(typed);
        let arms = self.typed_arms(&entry, (bits, kind), (row, destination, exit, join));
        self.builder.switch_to_block(join);
        // A register whose arms each made their own tagged word (the key's, beside its int32) is tagged again past the
        // join, as one arm leaves it: a later join with a tagged arm needs it current.
        let retag: Vec<usize> = (0..state.values.len()).filter(|&register| state.values[register].tagged.current() && arms.iter().all(|arm| arm.values[register].tagged.current())).collect();
        for mut incoming in arms {
            if let Some(destination) = destination { incoming.values[usize::from(destination)] = state.values[usize::from(destination)]; }
            self.join_states(state, &incoming, pc);
        }
        for register in retag { if !state.values[register].tagged.current() { self.tagged(state, register as Register); } }
        joined
    }

    /// The element `row` of a base whose record observed the typed array kind `native` alone: the row over that kind's
    /// view, which an earlier access of the same base and kind proved (`Proofs::typed`) or this one proves and keeps. Any
    /// other base, a direct array included, leaves at `exit` before the row's effect: no VM store runs, so no backing
    /// moves and the kept views stay current past a store.
    fn typed_only(&mut self, state: &mut State, (base, bits, native): (Register, ir::Value, quench_runtime::staged::typed_elements::NativeKind), (row, destination, exit): (Operation<ir::Value, EmitProof>, Option<Register>, ir::Block)) -> Option<ir::Value> {
        let view = self.typed_view_kept(state, (base, bits, native), destination.is_none(), exit);
        let held = self.held_view.replace(frame_elements::Held::TypedValues(view));
        let value = self.step(state, frame::Edges::ValueGuarded { miss: exit }, row);
        self.held_view = held;
        match (value, destination) { (Step::Value(value), Some(_)) => Some(value), _ => None }
    }

    /// The view of the typed array `bits` of kind `native` an earlier access proved and no storage change retired
    /// (`Proofs::typed`), else proved here, leaving at `exit`, and kept for the accesses after.
    fn typed_view_kept(&mut self, state: &mut State, (base, bits, native): (Register, ir::Value, quench_runtime::staged::typed_elements::NativeKind), writable: bool, exit: ir::Block) -> TypedView<ir::Value> {
        if let Some(proof) = self.typed_view_of(state, (base, native.kind, writable), exit) {
            return TypedView { values: proof.values, length: proof.length, kind: quench_runtime::staged::typed_elements::ViewKind::Static(native), within: false };
        }
        // A unit that also stores into a typed array proves its views for stores at once: one proof serves both.
        let publish = !writable;
        let writable = writable || self.typed_stores();
        let view = self.typed_view_here((bits, Some(native)), writable, exit, publish);
        // Every register holding the same word (a copy of the base) holds the same view.
        let same: smallvec::SmallVec<[Register; 4]> = (0..state.values.len()).filter(|&register| register == usize::from(base)
            || state.values[register].tagged == super::TaggedView::Value(bits)).map(|register| register as Register).collect();
        state.proofs.typed.retain(|proof| !same.contains(&proof.register));
        for register in same {
            state.proofs.typed.push(super::TypedProof { register, kind: native.kind, values: view.values, length: view.length, writable, kept: false });
        }
        view
    }

    /// Whether the unit holds a typed-only element store (`typed_only_at`).
    fn typed_stores(&self) -> bool {
        // A store's base is often another register holding the same array (a copy the reads do not name): any
        // typed-only store of the unit proves the reads' views for stores too, as one proof serves both.
        self.unit.instructions.iter().enumerate().any(|(pc, instruction)| self.typed_only_at(pc)
            && matches!(crate::compiled::performed(*instruction), Some(Operation::ElementWrite { .. })))
    }

    /// An element read at `pc` of a base no loop array holds, whose record observed one native typed array kind
    /// (`typed_only_at`), over the kept view (`typed_view_kept`): an int32 kind's element as its int32 word
    /// (`typed_read_integer`), any other's as its Number in a float register, with no Number word made and tested
    /// again. `false` where the read is not one, with nothing emitted.
    fn typed_int_read(&mut self, state: &mut State, pc: usize, (destination, base, key): (Register, Register, quench_runtime::bytecode::Operand), exit: ir::Block) -> bool {
        let Some(native) = super::super::body_views::kind_at(self.kinds, pc).and_then(quench_runtime::staged::typed_elements::NativeKind::new) else { return false };
        let bits = self.tagged(state, base);
        if !native.is_int32() {
            // Any other native kind's element is read as the Number it is, into a float register (`typed_read`):
            // no Number word is boxed and tested again.
            let view = self.typed_view_kept(state, (base, bits, native), false, exit);
            let held = self.held_view.replace(frame_elements::Held::Typed(view));
            let read = self.step(state, frame::Edges::ValueGuarded { miss: exit }, Operation::ElementRead { base, key, destination });
            self.held_view = held;
            let Step::Value(number) = read else { unreachable!("an element read yields a value or leaves at its miss edge") };
            self.set_number(state, destination, number);
            return true;
        }
        let Some(index) = self.operand_index(state, key, exit) else { return false };
        let view = self.typed_view_kept(state, (base, bits, native), false, exit);
        let word = {
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            let index = stage.uextend32(index);
            typed_read_integer(&mut stage, view, index, exit)
        }.expect("an int32 kind's static view reads its integer");
        let int = if self.raw_lane(word) == Lane::I64 { self.raw_unary(Unary::Narrow(Lane::I32), word) } else { word };
        self.set_int(state, destination, int);
        true
    }

    /// Whether the element access at `pc` takes `typed_only`: its record observed one native typed array kind.
    pub(super) fn typed_only_at(&self, pc: usize) -> bool {
        super::super::body_views::kind_at(self.kinds, pc).and_then(quench_runtime::staged::typed_elements::NativeKind::new).is_some()
    }

    /// `row` on `state`, leaving at `exit`, whose value (a read's) goes to `join`.
    fn row_arm(&mut self, state: &mut State, (row, destination, exit, join): (Operation<ir::Value, EmitProof>, Option<Register>, ir::Block, ir::Block)) {
        let value = self.step(state, frame::Edges::ValueGuarded { miss: exit }, row);
        self.goto(join, &match (value, destination) { (Step::Value(value), Some(_)) => vec![value], _ => Vec::new() });
    }

    /// The arms of `row_or_typed` past its direct array test, each from `entry`: the row over the typed view proved here
    /// (`Held::TypedValues`); a base that is no typed array leaves at `exit` for a read (as the row's array proof would),
    /// and takes the row's VM store for a write. The arms' states.
    fn typed_arms(&mut self, entry: &State, (bits, kind): (ir::Value, Option<quench_runtime::staged::typed_elements::NativeKind>), (row, destination, exit, join): (Operation<ir::Value, EmitProof>, Option<Register>, ir::Block, ir::Block)) -> Vec<State> {
        let other = if destination.is_some() { exit } else { self.builder.create_block() };
        let view = self.typed_view_here((bits, kind), destination.is_none(), other, destination.is_some());
        let held = self.held_view.replace(frame_elements::Held::TypedValues(view));
        let mut arms = vec![entry.clone()];
        self.row_arm(&mut arms[0], (row, destination, exit, join));
        self.held_view = held;
        if other != exit {
            self.builder.switch_to_block(other);
            arms.push(entry.clone());
            self.row_arm(&mut arms[1], (row, destination, exit, join));
        }
        arms
    }

    /// The view of the typed array `bits` (of any native kind), proved here as a loop's entering edge
    /// proves it (`hoist_typed_view`): a base no loop array holds, which the dense row declined.
    /// Unpublished, a view is the VM's to publish (`hoist_typed_view`'s slow arm), else `miss` is taken at once.
    fn typed_view_here(&mut self, (bits, kind): (ir::Value, Option<quench_runtime::staged::typed_elements::NativeKind>), writable: bool, miss: ir::Block, publish: bool) -> TypedView<ir::Value> {
        let mut declare = |ty| self.builder.declare_var(ty);
        let variables = ArrayVariables { bits: declare(types::I64), values: declare(types::I64), length: declare(types::I64), kind: declare(types::I64), room: declare(types::I32) };
        let access = ViewAccess { kind, writable };
        if publish { self.hoist_typed_view(access, bits, variables, miss) } else { self.prove_typed_view_into(variables, bits, access, miss) }
        let mut used = |variable| self.builder.use_var(variable);
        let kind = match kind { Some(native) => ViewKind::Static(native), None => ViewKind::Dynamic(used(variables.kind)) };
        TypedView { values: used(variables.values), length: used(variables.length), kind, within: false }
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
            && self.proven_element(pc).is_some_and(|ty| ty.is_int32())
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
    /// or pop runs: an append or a store through another register could make a hole or a non-int32
    /// element, and a pop changes the count the view keeps.
    fn int32_stores(&self, array: LoopArray, source: Register) -> bool {
        (array.header..=array.back).all(|pc| {
            if super::native_calls::resizes(&self.intrinsics, pc) { return false; }
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
                || native_calls::resizes(&self.intrinsics, pc)
                || quench_runtime::staged::meaning::means(&instruction, quench_runtime::staged::meaning::Meaning::Call)
                || quench_runtime::staged::meaning::means(&instruction, quench_runtime::staged::meaning::Meaning::Construct);
            if stores { return false; }
            if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })) {
                if !self.proven_element(pc).is_some_and(|ty| ty.is_int32()) { return false; }
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
        let keeps_typed = array.is_none() && self.typed_only_at(pc);
        match array {
            Some(array) if array.views == ElementViews::Dense && self.left_here =>
                self.dense_element_write(state, array, pc as u32, instruction, number, exit),
            held => {
                self.held_view = held.map(|array| self.held_array(array, (false, self.ranges.in_bounds(pc))));
                let row = Operation::ElementWrite { value, base, key, strict: instruction.boolean_flag().unwrap_or(false) };
                if held.is_some() { self.step(state, frame::Edges::ValueGuarded { miss: exit }, row); } else { self.row_or_typed(state, pc, base, exit, row); }
                // A store through a held view leaves where it misses (`index_set_host`) and calls no
                // helper, so no backing moved and no view is read again. Only the general row may
                // call the VM's store, which may move any backing.
                if held.is_none() { self.reload_dense_arrays(pc); }
            }
        }
        state.proofs.arrays.clear();
        // A typed-only store calls no VM store (`typed_only`), so no typed view moved.
        if !keeps_typed { state.proofs.keep_typed_views(); }
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
