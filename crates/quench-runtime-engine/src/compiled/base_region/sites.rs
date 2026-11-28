//! What the VM remembers of the sites a body's rows stand on, read when the body is compiled, and the rows built on it:
//! the `in` row of a key one receiver layout answered (the facts it holds and the folded holders). The `instanceof`
//! row is retired (S7j4a): no fact the walk over the object records needed is read any more.

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt, OperationNamesExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::Vm;
use super::*;
use quench_runtime::vm::in_presence::InPresence;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The operator of a comparison that is a `Binary`, which writes a boolean to a register, or a `JumpBinaryFalse`, which branches
/// where the comparison fails.
fn comparison_operator(instruction: WideInstruction) -> Option<u32> {
    if super::super::meaning_compiled::is_binary(instruction) {
        Some(instruction.binary_operator())
    } else if super::super::meaning_compiled::is_jump_binary_false(instruction) {
        Some(instruction.binary_operator_field())
    } else {
        None
    }
}

/// Whether `instruction` is `key in object`, as a value or as a jump.
fn is_membership(instruction: WideInstruction) -> bool {
    comparison_operator(instruction) == Some(BinaryOperator::In as u32)
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
fn joins(code: &[WideInstruction]) -> Bits {
    let mut joins = Bits::default();
    for instruction in code.iter().filter(|instruction| instruction.op().control_flow_layout().jumps()) {
        joins.insert(instruction.jump_target() as usize);
    }
    joins
}

/// The origin of `register` at `pc` of `code`, whose jump targets are `joins`.
fn origin(code: &[WideInstruction], joins: &Bits, pc: usize, register: Register) -> Origin {
    for at in (0..pc).rev() {
        let instruction = code[at];
        if instruction.written_registers().any(|written| written == register) {
            return Origin::Written(instruction);
        }
        if instruction.op().control_flow_layout().jumps() || joins.contains(at) {
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
                Origin::Written(load) => quench_runtime::staged::Operation::<(), ()>::decode_constant_load(load).map(|(_, index)| index)?,
                _ => return None,
            },
            _ => return None,
        };
        constants.get(key).copied()
    }).collect()
}

/// The facts rows hold, each once, in order.
pub(crate) fn distinct<T: Ord>(facts: impl Iterator<Item = T>) -> Box<[T]> {
    let mut facts: Vec<T> = facts.collect();
    facts.sort_unstable();
    facts.dedup();
    facts.into()
}

/// The exact shapes of the holders whose values the rows hold folded (`Decoded::folded`) or whose native
/// functions they stand on (`Decoded::holder`): the Stable facts the code takes, which a change of a value
/// a prototype of that shape holds, or of its own layout, retires it.
pub(crate) fn held_holders(decoded: &[Decoded]) -> Box<[u32]> {
    let natives = decoded.iter().filter_map(|row| row.holder);
    distinct(decoded.iter().filter_map(|row| row.folded.map(|folded| folded.holder)).chain(natives))
}

/// The prototype chains the folded rows read their answers through (`Folded::chain`) and the adding stores'
/// chain proofs hold for (`adds::Add::chain`): the Stable facts the code takes, which a layout change or the
/// death of a prototype on one retires it (`Heap::subscribe_prototype_chain`).
pub(crate) fn held_chains(decoded: &[Decoded]) -> Box<[quench_runtime::code_dependencies::PrototypeChain]> {
    let added = decoded.iter().filter_map(|row| row.add.and_then(adds::Add::chain));
    distinct(decoded.iter().filter_map(|row| row.folded.map(Folded::chain)).chain(added))
}

/// The atoms the rows hold unnamed by method caches (`Decoded::atom`): the Stable facts the code
/// takes, which a cache naming one retires it.
pub(crate) fn held_atoms(decoded: &[Decoded]) -> Box<[Atom]> {
    distinct(decoded.iter().filter_map(|row| row.atom))
}

/// The row of `key in object` over a register's object, where the VM remembers exactly one receiver layout answering the
/// key (`Known::presences`): the boolean it answered to the register `dst`, while the object has that exact shape and prototype
/// link (`Heap::Held`, the folded read's row over a constant), under the dependency on the layouts of the prototypes
/// `Folded::chain` names, the whole chain the answer read.
pub(crate) fn membership_row(known: Known, pc: u32, instruction: WideInstruction, dst: Register) -> Option<Decoded> {
    let fact = known.presences?.get(pc as usize).copied().flatten().filter(|_| is_membership(instruction))?;
    let object = Some(Source::Register(instruction.operand_c().register_index()?));
    let row = Decoded::heap(Heap::Held, Some(dst), object, None);
    Some(Decoded { hole: Value::boolean(fact.present).to_bits(), folded: Some(Folded { shape: fact.shape, link: fact.link, holder: fact.holder, depth: fact.depth }), ..row })
}

/// The rows of a `JumpBinaryFalse` of `in` the VM remembers an answer for: the comparison's row, writing its boolean to
/// the register `scratch`, then the jump to the instruction's target where that boolean is false.
pub(crate) fn compare_jump(known: Known, pc: u32, instruction: WideInstruction, scratch: Register) -> Option<Vec<Decoded>> {
    matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. })).then_some(())?;
    // S7f DESIGN: an `instanceof` jump has no row since the object-record walk went (S7j4a); its helper step decides it.
    let compared = membership_row(known, pc, instruction, scratch)?;
    let jump = Decoded::number(Kind::JumpFalse, None, Some(Source::Register(scratch)), None, Some(instruction.jump_target()));
    Some(vec![compared, jump])
}

#[quench_macros::ext(CompiledBaseRegionSitesVm)]
impl Vm {
    /// What `key in object` answered at each `in` of `code`, by PC, where its key is a constant string and exactly one
    /// receiver layout answered that key (`Vm::presence_fact`).
    pub(crate) fn membership_facts(&mut self, code: &[WideInstruction]) -> Vec<Option<InPresence>> {
        let keys = membership_keys(code, self.programs.constants(self.active_program));
        keys.into_iter().map(|key| {
            let atom = self.string_key_atom(key?)?;
            self.presence_fact(atom)
        }).collect()
    }
}
