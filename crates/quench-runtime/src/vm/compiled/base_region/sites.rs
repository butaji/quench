//! What the VM remembers of the sites a body's rows stand on, read when the body is compiled: the constructor each
//! `instanceof` names and what each `in` answered. The constructor is only a hint: the `instanceof` row guards its
//! constructor register against the hinted identity each run, so a hint that is wrong (another path reaches the
//! register, a name another scope binds) costs the row's fast arm and nothing else.

use super::*;
use crate::vm::in_presence::InPresence;
use std::collections::BTreeSet;

/// Whether `instruction` is `left instanceof right`, which writes a boolean to a register.
pub(super) fn is_instanceof(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::Binary && instruction.binary_operator() == BinaryOperator::Instanceof as u32
}

/// Whether `instruction` is `key in object`, which writes a boolean to a register.
pub(super) fn is_membership(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::Binary && instruction.binary_operator() == BinaryOperator::In as u32
}

/// Where the value of a register at some PC comes from, along the straight-line code before it.
enum Origin {
    /// No instruction before it writes it: the entry's parameter of that number.
    Entry,
    /// The nearest instruction before it that writes it.
    Written(WideInstruction),
    /// A jump or a join lies between: another path may reach the register.
    Merged,
}

/// The PCs some jump of `code` goes to.
fn joins(code: &[WideInstruction]) -> BTreeSet<usize> {
    code.iter().filter(|instruction| instruction.op().control_flow_layout().jumps()).map(|instruction| instruction.jump_target() as usize).collect()
}

/// The origin of `register` at `pc` of `code`, whose jump targets are `joins`.
fn origin(code: &[WideInstruction], joins: &BTreeSet<usize>, pc: usize, register: Register) -> Origin {
    for at in (0..pc).rev() {
        let instruction = code[at];
        if instruction.written_registers().any(|written| written == register) {
            return Origin::Written(instruction);
        }
        if instruction.op().control_flow_layout().jumps() || joins.contains(&at) {
            return Origin::Merged;
        }
    }
    Origin::Entry
}

/// The constant string key of each `in` of `code`, by PC: the operand itself, or the `LoadConst` just before it.
fn membership_keys(code: &[WideInstruction], constants: &[Value]) -> Vec<Option<Value>> {
    let joins = joins(code);
    (0..code.len()).map(|pc| {
        let instruction = Some(code[pc]).filter(|instruction| is_membership(*instruction))?;
        let key = match instruction.operand_b().kind()? {
            OperandKind::Constant => usize::from(instruction.operand_b().payload()),
            OperandKind::Register => match origin(code, &joins, pc, instruction.operand_b().register_index()?) {
                Origin::Written(load) if load.op().class() == Class::LoadConst => load.constant_index(),
                _ => return None,
            },
            _ => return None,
        };
        constants.get(key).copied()
    }).collect()
}

impl<H: crate::Host> crate::vm::Vm<H> {
    /// The constructor each `instanceof` of `code` names, by PC, where the VM can tell now: the global a by-name load
    /// just before it reads, or the entry parameter its register still holds (`parameters`, one per register from 0).
    pub(in crate::vm::compiled) fn instanceof_constructors(&mut self, program: &ResidualProgram, code: &[WideInstruction], parameters: &[Value]) -> Vec<Option<Value>> {
        let joins = joins(code);
        (0..code.len()).map(|pc| {
            let register = Some(code[pc]).filter(|instruction| is_instanceof(*instruction))?.operand_c().register_index()?;
            let named = match origin(code, &joins, pc, register) {
                Origin::Written(load) if load.op().class() == Class::LoadName => self.global_value(program, load),
                Origin::Entry => parameters.get(usize::from(register)).copied(),
                Origin::Written(_) | Origin::Merged => None,
            };
            named.filter(|constructor| self.heap.function(*constructor).is_some())
        }).collect()
    }

    /// What `key in object` answered at each `in` of `code`, by PC, where its key is a constant string and exactly one
    /// receiver layout answered that key (`Vm::presence_fact`).
    pub(in crate::vm::compiled) fn membership_facts(&mut self, code: &[WideInstruction]) -> Vec<Option<InPresence>> {
        let keys = membership_keys(code, self.programs.constants(self.active_program));
        keys.into_iter().map(|key| {
            let atom = self.string_key_atom(key?)?;
            self.presence_fact(atom)
        }).collect()
    }

    /// The value the global scope binds the name the by-name load `load` reads, now.
    fn global_value(&mut self, program: &ResidualProgram, load: WideInstruction) -> Option<Value> {
        match self.global_name_read(program, load.atom_index(), load.cache_site_index()) {
            crate::vm::field_cache::GlobalNameRead::Value(value, _) => Some(value),
            _ => None,
        }
    }
}
