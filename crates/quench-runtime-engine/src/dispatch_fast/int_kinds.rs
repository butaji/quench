//! Int32 register kinds of a function's view (D1/D2 of the cheap-ops design): which registers
//! hold an int32 at every read, so an operator record reading only such registers runs a form
//! with no tag tests (`operator_row`'s `OPERANDS_PROVEN*`).
//!
//! A register's kind is the join of the kinds of every instruction that defines it, solved
//! optimistically to a fixpoint over the function (a loop counter's `i = 0; i++` cycle starts
//! int32 and stays so):
//! - `Int32`: always an int32. An int32 `LoadConst`; a `Move` of an `Int32` register; a bitwise
//!   `| & ^ << >>` with at least one Number operand (the other operand is then a Number too, or
//!   the operator throws: a BigInt mixed with a Number is a TypeError), whose result is an int32.
//! - `Speculative`: an int32 until an `IncDec` of it overflows (`i++` past 2^31 - 1), and a Number
//!   always. The interpreter checks it where it is defined, never where it is read: the overflowing
//!   update runs the VM's increment (`Vm::inc_dec_value` or the general loop's `IncDec` arm), which
//!   retires every speculating record of the VM to its checked form before the double is written
//!   (`Vm::retire_int_speculation`), as the lane does after native code (`admission::retire_after_native`).
//! - `Tagged`: anything else, including every register live at the function's entry
//!   (parameters, `this`, values a caller or a resumption put there).
//!
//! The kinds hold only where every write of a register goes through its function's instructions,
//! which the view's own constraints give: no dynamic scope access (a direct `eval` or `with`),
//! no superinstructions (whose nested writes the pass does not decode), and a liveness mask that
//! names the entry-live registers precisely (not windowed).
#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::{ImmediateRole, OperandKind, WideInstruction};
use quench_runtime::compile::liveness::RegisterMasks;
use quench_runtime::value::Value;
use quench_runtime::vm::operations::binary_operator as code;

pub(crate) use quench_runtime::vm::engine_types::Kind;

/// The kinds of a function's registers below the width the analysis covered.
pub(crate) struct IntKinds {
    kinds: Vec<Kind>,
}

impl IntKinds {
    /// `register`'s kind; `Tagged` past the covered registers.
    pub(crate) fn of(&self, register: u16) -> Kind {
        self.kinds.get(usize::from(register)).copied().unwrap_or(Kind::Tagged)
    }

    /// Whether `register` holds an int32 at every read while speculation stands.
    pub(crate) fn proven(&self, register: u16) -> bool {
        self.of(register) != Kind::Tagged
    }
}

/// Whether the speculative kinds stood on this thread: once an update overflowed one, no later
/// view speculates (its loops would overflow again and retire again).
#[thread_local]
static SPECULATION_RETIRED: std::cell::Cell<bool> = std::cell::Cell::new(false);

/// Whether a view may speculate: none overflowed on this thread yet.
pub(crate) fn speculation_allowed() -> bool {
    !SPECULATION_RETIRED.get()
}

/// Notes that a speculative kind overflowed; returns whether it was the first time.
pub(crate) fn note_retired() -> bool {
    !SPECULATION_RETIRED.replace(true)
}

/// The registers `masks` covers per pc, `None` for a windowed mask.
pub(crate) fn covered(masks: &RegisterMasks) -> Option<usize> {
    match masks {
        RegisterMasks::Narrow(_) => Some(u64::BITS as usize),
        RegisterMasks::Half(_) => Some(u128::BITS as usize),
        RegisterMasks::Wide(_) => Some(quench_runtime::compile::liveness::RegisterSet::BITS as usize),
        RegisterMasks::Windowed { .. } => None,
    }
}

/// The kinds of the registers of the function decoded as `decoded`, over its program's
/// `constants`, with its liveness `masks`; `None` where the function admits no kinds (see the
/// module documentation). `speculate` admits `Speculative` kinds.
pub(crate) fn analyze(decoded: &[WideInstruction], constants: &[Value], masks: Option<&RegisterMasks>, registers: usize,
    dynamic_scope: bool, speculate: bool) -> Option<IntKinds> {
    if dynamic_scope || decoded.is_empty() {
        return None;
    }
    let masks = masks?;
    let width = covered(masks)?.min(registers);
    let entry = masks.get(0)?;
    if decoded.iter().any(|instruction| instruction.op().immediate_role() == ImmediateRole::SuperinstructionIndex) {
        return None;
    }
    let mut kinds: Vec<Kind> = (0..width).map(|register| if entry.contains(register) { Kind::Tagged } else { Kind::Int32 }).collect();
    loop {
        let mut changed = false;
        for &instruction in decoded {
            let defined = quench_runtime::compile::liveness::definitions(instruction, &[]);
            if defined.is_empty() {
                continue;
            }
            let single = instruction.defined_register().filter(|&register| defined == quench_runtime::compile::liveness::register_bit(register));
            for register in 0..width {
                if !defined.contains(register) || kinds[register] == Kind::Tagged {
                    continue;
                }
                let kind = match single {
                    Some(own) if usize::from(own) == register => kind_of(instruction, constants, &kinds, speculate),
                    _ => Kind::Tagged,
                };
                if kind > kinds[register] {
                    kinds[register] = kind;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    kinds.iter().any(|kind| *kind != Kind::Tagged).then_some(IntKinds { kinds })
}

/// The kind of the value `instruction` defines in its result register, given the current `kinds`.
fn kind_of(instruction: WideInstruction, constants: &[Value], kinds: &[Kind], speculate: bool) -> Kind {
    let register_kind = |register: u16| kinds.get(usize::from(register)).copied().unwrap_or(Kind::Tagged);
    if instruction.result_flags() != 0 {
        return Kind::Tagged;
    }
    match () {
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) => match constants.get(instruction.constant_index()) {
            Some(constant) if constant.as_int().is_some() => Kind::Int32,
            _ => Kind::Tagged,
        },
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Move) => register_kind(instruction.register_b()),
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::IncDec) && speculate && instruction.numeric_local_target().is_none() => {
            match register_kind(instruction.register_b()) {
                Kind::Tagged => Kind::Tagged,
                _ => Kind::Speculative,
            }
        }
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Binary) && instruction.numeric_local_target().is_none() => {
            let operator = instruction.binary_operator();
            let bitwise = matches!(operator, code::BitwiseOR | code::BitwiseXOR | code::BitwiseAnd | code::ShiftLeft | code::ShiftRight);
            if !bitwise {
                return Kind::Tagged;
            }
            // A Number operand: a register of an int32 kind (a speculative one is a Number too),
            // or a Number constant.
            let number = |operand: quench_runtime::bytecode::Operand| match operand.kind() {
                Some(OperandKind::Register) => operand.register_index().is_some_and(|register| register_kind(register) != Kind::Tagged),
                Some(OperandKind::Constant) => constants.get(usize::from(operand.payload())).is_some_and(|constant| constant.as_number().is_some()),
                _ => false,
            };
            if number(instruction.operand_b()) || number(instruction.operand_c()) { Kind::Int32 } else { Kind::Tagged }
        }
        _ => Kind::Tagged,
    }
}
