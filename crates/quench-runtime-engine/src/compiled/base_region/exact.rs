//! The int32 `+` and `-` a function proves exact, planned as their no-overflow rows
//! (`Kind::AddNoOverflow`, `Kind::SubNoOverflow`): the facts analysis (`facts::analyze_text`) run over
//! the enclosing function's bytecode from pc 0, every register starting unknown. Its facts are program
//! invariants: the function is entered only at pc 0, so every state the interpreter reaches at a PC, and so
//! every state a loop region or resume stub enters with, is one the analysis's fixpoint covers. Its int32
//! types come from constants and int32 operations only, so a `+` that concatenates a string is never an
//! int32 here. A function with exception handlers, a generator or async state, a dynamic scope or an
//! aliasing `arguments` object proves none: the analysis does not model the states those reach.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::facts::{self, Observed, Seeds, Text};
use super::super::lane_region::{leaders, writes};
use quench_runtime::bytecode::{Constant, Function};
use quench_runtime::dataflow::BitSet;

/// The no-overflow kind of an int32 `+` or `-`: the row whose overflow test a proof removed.
pub(crate) fn exact_kind(kind: Kind) -> Option<Kind> {
    match kind {
        Kind::Add => Some(Kind::AddNoOverflow),
        Kind::Sub => Some(Kind::SubNoOverflow),
        _ => None,
    }
}

/// The checked kind of a no-overflow row: the row it falls back to where the library holds no row over the
/// operands' places (a double operand's pinned register), which takes the same int32 operands and tests overflow.
pub(crate) fn checked(kind: Kind) -> Kind {
    match kind {
        Kind::AddNoOverflow => Kind::Add,
        Kind::SubNoOverflow => Kind::Sub,
        other => other,
    }
}

/// The PCs of `function`'s instructions whose int32 arithmetic is exact in every state the analysis reaches
/// The ascending PCs of `function`'s instructions whose int32 arithmetic is exact in every state the analysis reaches
/// from pc 0 with every register unknown; `None` where the analysis models less than the function does: handlers,
/// a generator or async state resumed at a saved PC, a dynamic scope (`eval`), or an `arguments` object that may alias a register.
fn exact_pcs(function: &Function, constants: &[Constant]) -> Option<Box<[u32]>> {
    if !function.handlers.is_empty() || function.is_generator || function.is_async || function.has_dynamic_scope_access() || function.arguments_slot.is_some() {
        return None;
    }
    let instructions: Vec<WideInstruction> = function.instructions().collect();
    let blocks = leaders(&instructions);
    let initial = vec![Type::ANY; usize::from(function.registers)];
    let text = Text { entry: 0, instructions: &instructions, leaders: &blocks, constants, written: &writes,
        own_shape: &|_| None, native_calls: true, raises: &[] };
    let seeds = Seeds { contracts: &[], observed: Observed::NONE, entering: &[], model: None, signless: &[] };
    let mut exact: BitSet = BitSet::with_capacity(instructions.len());
    facts::analyze_text(&text, &initial, seeds).replay(&text, seeds, |pc, point| {
        if point.overflow_free(constants, instructions[pc]) {
            exact.insert(pc);
        }
    });
    Some((0..instructions.len()).filter(|&pc| exact.contains(pc)).map(|pc| pc as u32).collect())
}

/// Plans the no-overflow kind on each caller row (a body's or a region's) whose instruction the analysis proved
/// exact. The analysis runs once per function (`Function::exact_sums`): its facts are invariants of the bytecode,
/// since the function is entered only at pc 0, so every state a region or resume stub enters with is one it covers.
/// Inlined callees' rows keep their checked kinds.
pub(crate) fn promote(function: &Function, constants: &[Constant], origins: &[Origin], rows: &mut [Decoded]) {
    let Some(exact) = function.caches().exact_sums.get_or_init(|| exact_pcs(function, constants)).as_deref() else { return };
    for (row, origin) in rows.iter_mut().zip(origins) {
        let Origin::Caller(pc) = origin else { continue };
        if exact.binary_search(pc).is_ok() && let Some(kind) = exact_kind(row.kind) {
            row.kind = kind;
        }
    }
}
