//! The staged element rows (`staged::element_rows`) lowered: the lane decides by the row's data,
//! not by the opcode it decoded from. A loop's hoisted array (`LoopArray`), whose view its entering
//! edge proved once, is read and written through that view; any other access is the row itself,
//! over the frame's proofs of the array (`frame_elements`), with the exit, the join and the
//! views a store retires left to this lowering.
use super::*;
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

    /// An element read into `destination`: a hoisted array's through its view, any other the row.
    /// The element an observation typed (`number_reads`) is proved after it.
    fn element_read(&mut self, state: &mut State, pc: usize, (destination, base, key): (Register, Register, crate::bytecode::Operand)) -> bool {
        let exit = self.guard_exit(state, pc as u32);
        match self.loop_array(pc, Some(base)) {
            Some(array) => {
                let index = self.operand_int(state, key, exit).expect("a supported key operand has an int32 view");
                self.loop_element_read(state, array, index, (pc, destination), exit);
            }
            None => {
                let Step::Value(value) = self.step(state, frame::Edges::ValueGuarded { miss: exit }, Operation::ElementRead { base, key })
                    else { unreachable!("an element read yields a value or leaves at its miss edge") };
                self.set_tagged(state, destination, value);
            }
        }
        match self.observed.at(pc) {
            Some(ty) if ty == Type::INT32 => { self.int(state, destination, exit); }
            Some(_) => { self.number(state, destination, exit); }
            None => {}
        }
        true
    }

    /// An element store. A Number into a writable hoisted array goes through its view; any other
    /// store is the row, which replaces a present element in place and otherwise takes the VM's
    /// store, which may add the element and so retires every view. A key that is a string names a
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
        match self.loop_array(pc, Some(base)).filter(|array| number && array.writable) {
            Some(array) => self.loop_element_write(state, array, pc as u32, instruction, exit),
            None => {
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
