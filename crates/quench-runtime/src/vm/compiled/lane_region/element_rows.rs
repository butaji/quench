//! The staged element rows (`staged::element_rows`) lowered: the lane decides by the row's data,
//! not by the opcode it decoded from. A loop's hoisted array (`LoopArray`), whose view its entering
//! edge proved once, is read and written through that view; any other access is the row itself,
//! over the frame's proofs of the array (`frame_elements`), with the exit, the join and the
//! views a store retires left to this lowering.
use super::*;
use crate::staged::typed_elements::TypedView;
use crate::staged::{Operation, Step};

impl Lowering<'_, '_> {
    /// The row of `instruction` where it is an element access whose operands this unit holds.
    fn element_row(&self, instruction: WideInstruction) -> Option<Operation<ir::Value, EmitProof>> {
        let row = Operation::decode(instruction, |_| None)?;
        let held = |register: Register| register < self.unit.registers;
        let supported = match row {
            Operation::ElementRead { base, key } => held(base) && self.operand_supported(key),
            Operation::ElementWrite { value, base, key } => [value, base, key].into_iter().all(held),
            _ => false,
        };
        supported.then_some(row)
    }

    /// Whether `instruction` is a row `element_step` lowers.
    pub(super) fn is_element_row(&self, instruction: WideInstruction) -> bool {
        self.element_row(instruction).is_some()
    }

    /// The element row at `pc`, lowered by its definition; whether control falls through.
    pub(super) fn element_step(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        match self.element_row(instruction).expect("an element row decodes") {
            Operation::ElementRead { base, key } => self.element_read(state, pc, (instruction.result_register(), base, key)),
            Operation::ElementWrite { value, base, key } => self.element_write(state, (pc, instruction), (value, base, key)),
            _ => unreachable!("an element row is a read or a write"),
        }
    }

    /// An element read into `destination`: a hoisted array's through its view (a typed array's a
    /// Number), any other the row. The element an observation typed (`number_reads`) is proved after it.
    fn element_read(&mut self, state: &mut State, pc: usize, (destination, base, key): (Register, Register, crate::bytecode::Operand)) -> bool {
        let exit = self.guard_exit(state, pc as u32);
        let array = self.loop_array(pc, Some(base));
        // An element read as a Number (`number_reads`) needs no hole test of its own: a hole is a NaN
        // pattern, which the Number proof's one NaN test sends to the tag check that leaves.
        let packed = self.observed.at(pc).is_some();
        self.held_view = array.map(|array| self.held_array(array, packed));
        let Step::Value(value) = self.step(state, frame::Edges::ValueGuarded { miss: exit }, Operation::ElementRead { base, key })
            else { unreachable!("an element read yields a value or leaves at its miss edge") };
        if array.is_some_and(|array| array.views == ElementViews::Typed) {
            self.set_number(state, destination, value);
        } else {
            self.set_tagged(state, destination, value);
        }
        match self.observed.at(pc) {
            Some(ty) if ty == Type::INT32 => { self.int(state, destination, exit); }
            Some(_) => { self.number(state, destination, exit); }
            None => {}
        }
        true
    }

    /// The view the hoisted `array`'s entering edge proved, which its element rows read and write through.
    fn held_array(&mut self, array: LoopArray, packed: bool) -> frame_elements::Held {
        let (values, length) = self.hoisted_view(array);
        match array.views {
            ElementViews::Dense => frame_elements::Held::Dense(super::super::DenseArrayView {
                values, length, writable: array.writable, uncounted: array.writable }, packed),
            ElementViews::Typed => frame_elements::Held::Typed(TypedView { values, length, kind: self.hoisted_kind(array) }),
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
        if key_type != Type::NEVER && key_type.is_subtype(Type::language(crate::ValueType::String)) {
            self.exit(state, pc as u32);
            return false;
        }
        let exit = self.guard_exit(state, pc as u32);
        let number = state.values[usize::from(value)].value.ty.is_subtype(Type::NUMBER);
        let array = self.loop_array(pc, Some(base)).filter(|array| number && array.writable);
        match array {
            Some(array) if array.views == ElementViews::Dense && self.left_here =>
                self.dense_element_write(state, array, pc as u32, instruction, exit),
            held => {
                self.held_view = held.map(|array| self.held_array(array, false));
                self.step(state, frame::Edges::ValueGuarded { miss: exit }, Operation::ElementWrite { value, base, key });
                self.reload_dense_arrays(pc);
            }
        }
        state.proofs.arrays.clear();
        state.proofs.fields.clear();
        // The VM's store may write a prototype's element or a mapped argument.
        state.proofs.stable = false;
        true
    }
}
