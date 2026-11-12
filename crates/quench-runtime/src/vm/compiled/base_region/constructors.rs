//! The constructors a body's `instanceof`s name, as the VM hints them when the body is compiled. A hint is only a
//! candidate: the `instanceof` row guards its constructor register against the hinted identity each run, so a
//! hint that is wrong (another path reaches the register, a name another scope binds) costs the row's fast arm and
//! nothing else.

use super::*;
use std::collections::BTreeSet;

/// Whether `instruction` is `left instanceof right`, which writes a boolean to a register.
pub(super) fn is_instanceof(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::Binary && instruction.binary_operator() == BinaryOperator::Instanceof as u32
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

impl<H: crate::Host> crate::vm::Vm<H> {
    /// The constructor each `instanceof` of `code` names, by PC, where the VM can tell now: the global a by-name load
    /// just before it reads, or the entry parameter its register still holds (`parameters`, one per register from 0).
    pub(in crate::vm::compiled) fn instanceof_constructors(&mut self, program: &ResidualProgram, code: &[WideInstruction], parameters: &[Value]) -> Vec<Option<Value>> {
        let joins: BTreeSet<usize> = code.iter().filter(|instruction| instruction.op().control_flow_layout().jumps()).map(|instruction| instruction.jump_target() as usize).collect();
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

    /// The value the global scope binds the name the by-name load `load` reads, now.
    fn global_value(&mut self, program: &ResidualProgram, load: WideInstruction) -> Option<Value> {
        match self.global_name_read(program, load.atom_index(), load.cache_site_index()) {
            crate::vm::field_cache::GlobalNameRead::Value(value, _) => Some(value),
            _ => None,
        }
    }
}
