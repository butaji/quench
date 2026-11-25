//! The call rows (`stencils/call_rows.rs`) as a planner names them: which row, and its stencil.
//! The contracts (registers, stack, holes, successors) are the doc of `call_rows.rs`; the
//! record the direct rows read is `call_record::CallRecord`.

use crate::call_record::MAX_DIRECT_ARGC;
use crate::numeric::Place;
use crate::Stencil;
use std::sync::OnceLock;

/// A call row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallRow {
    /// The helper call: value holes 0 (the call pc) and 1 (the helper's address); successors
    /// NEXT (completed), TAKEN (a step that branched), EXIT0 (missed: leave at the pc, no spill),
    /// EXIT1 (leave with (status, word) in the first two pinned ints: `LEAVE_STATUS`).
    Helper,
    /// `Helper` with the instruction the planner decoded in value holes 2 and 3 (`WideInstruction::from_words`),
    /// for the helper that runs it without decoding the record again (`call_helper_packed`).
    HelperPacked,
    /// The direct call of `argc` arguments (at most `MAX_DIRECT_ARGC`) through an entry cell:
    /// value hole 0 is the `CallRecord`'s address; successors NEXT (refused: the helper row),
    /// TAKEN (completed), EXIT0 (a leave inside the callee, `LEAVE_STATUS`). `checked` compares the
    /// callee register with the record's callee; the unchecked row (a callee a Stable fact holds, whose
    /// change retires the body) does not read the register. `pinned` says a leading argument is passed
    /// from a pinned register (the row reads the record's places); else every argument is in the window
    /// and the `_window` row, which reads no places, is named (argc 0 has only the general row).
    Direct { argc: u8, this: ThisRow, checked: bool, pinned: bool },
    /// The pinned register `Place::Pin` or `Place::PinF` back from the tagged word a spill wrote
    /// to the frame slot at hole 0; successor NEXT.
    Reload(Place),
    /// The unconditional leave at the instruction: successor EXIT0 alone, every register as it
    /// came (the exit stub writes the live pinned ones back).
    Here,
    /// A builtin of a native the callee is, run on its staged row (`stencils/builtin_rows.rs`): value
    /// hole 0 is the `CallRecord`'s address, whose `callee` is the identity the callee register must hold;
    /// successors TAKEN (completed: the result is in the call's register), NEXT (a miss, nothing written:
    /// the helper row the planner lays out behind it).
    Builtin(BuiltinRow),
}

/// The builtins a call row runs on their staged rows (`stencils/builtin_rows.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinRow {
    /// `String.prototype.charCodeAt` of a flat string.
    CharCodeAt,
    /// `String.prototype.charAt` of a flat string, at an index of an ASCII unit.
    CharAt,
    /// `Number.isInteger` of a Number.
    IsInteger,
    /// `Math.imul` of two int32 operands.
    Imul,
    /// `Array.prototype.push` of one value onto a dense array (`staged::builtins::array_push_one`).
    Push,
    /// `Array.prototype.pop` of a dense array (`staged::builtins::array_pop`).
    Pop,
    /// `Array.isArray` of an ordinary array (`staged::builtins::is_plain_array`).
    IsArray,
}

impl BuiltinRow {
    /// Every builtin row, each with its own stencil.
    pub const ALL: [BuiltinRow; 7] = [
        BuiltinRow::CharCodeAt, BuiltinRow::CharAt, BuiltinRow::IsInteger, BuiltinRow::Imul,
        BuiltinRow::Push, BuiltinRow::Pop, BuiltinRow::IsArray,
    ];

    /// The row's name in the stencil object, after `builtin_`.
    pub const fn symbol(self) -> &'static str {
        match self {
            BuiltinRow::CharCodeAt => "char_code_at",
            BuiltinRow::CharAt => "char_at",
            BuiltinRow::IsInteger => "is_integer",
            BuiltinRow::Imul => "imul",
            BuiltinRow::Push => "push",
            BuiltinRow::Pop => "pop",
            BuiltinRow::IsArray => "is_array",
        }
    }

    /// The arguments the builtin reads after its receiver's window: the count the planner must match.
    pub const fn arguments(self) -> u16 {
        match self {
            BuiltinRow::Imul => 2,
            BuiltinRow::Pop => 0,
            _ => 1,
        }
    }
}

/// How a direct row binds the callee's `this`: the receiver as it is (strict), boxed or the
/// global for nullish (sloppy), or none for an arrow, whose body reads its environment's (lexical).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThisRow { Strict, Sloppy, Lexical }

/// The routine a leave through a (status, word) pair ends in (`stencils/call_rows.rs`).
pub const LEAVE_STATUS: &str = "leave_status";

impl CallRow {
    /// The row's symbol in the stencil object, or why no such row exists.
    pub fn name(self) -> Result<String, String> {
        match self {
            CallRow::Helper => Ok("call_helper".to_string()),
            CallRow::HelperPacked => Ok("call_helper_packed".to_string()),
            CallRow::Here => Ok("exit_here".to_string()),
            CallRow::Direct { argc, this, checked, pinned } if usize::from(argc) <= MAX_DIRECT_ARGC => {
                let mode = match this { ThisRow::Strict => "strict", ThisRow::Sloppy => "sloppy", ThisRow::Lexical => "lexical" };
                let window = if pinned || argc == 0 { "" } else { "_window" };
                Ok(format!("call_direct_a{argc}_{mode}{}{window}", if checked { "" } else { "_unchecked" }))
            }
            CallRow::Reload(place @ (Place::Pin(_) | Place::PinF(_))) => Ok(format!("reload_{}", place.code())),
            CallRow::Builtin(row) => Ok(format!("builtin_{}", row.symbol())),
            other => Err(format!("no call row {other:?}")),
        }
    }
}

/// The stencil of `row`, built at compile time by rustc and shared for the life of the process.
pub fn stencil(row: CallRow) -> Result<&'static Stencil, String> {
    static TABLE: crate::rustc::Memo<CallRow> = OnceLock::new();
    crate::rustc::memoized(&TABLE, row, || {
        let name = row.name()?;
        crate::rustc::stencil(&name).ok_or_else(|| format!("no stencil row {name}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EXIT, NEXT, TAKEN};

    fn rows() -> Vec<CallRow> {
        let direct = (0..=MAX_DIRECT_ARGC as u8).flat_map(|argc| [ThisRow::Strict, ThisRow::Sloppy, ThisRow::Lexical].into_iter()
            .flat_map(move |this| [true, false].into_iter().flat_map(move |checked| [true, false].map(move |pinned| CallRow::Direct { argc, this, checked, pinned }))));
        let pins = (0..crate::PINNED as u8).map(Place::Pin).chain((0..crate::PINNED_F as u8).map(Place::PinF)).map(CallRow::Reload);
        [CallRow::Helper, CallRow::HelperPacked, CallRow::Here].into_iter().chain(direct).chain(pins).collect()
    }

    /// Every row the planner can name was built, and its successors and holes are the contract's.
    #[test]
    fn every_call_row_is_built_with_its_contract_holes() {
        for row in rows() {
            let stencil = stencil(row).unwrap_or_else(|e| panic!("{row:?}: {e}"));
            let successors: Vec<u32> = stencil.relocs.iter().filter(|r| crate::class::is_successor(r.class)).map(|r| r.index).collect();
            let values: Vec<u32> = stencil.relocs.iter().filter(|r| !crate::class::is_successor(r.class)).map(|r| r.index).collect();
            match row {
                CallRow::Helper | CallRow::HelperPacked => {
                    assert!([NEXT, TAKEN, EXIT, EXIT + 1].iter().all(|s| successors.contains(s)), "{successors:?}");
                    assert!(values.contains(&0) && values.contains(&1), "{values:?}");
                    if row == CallRow::HelperPacked {
                        assert!(values.contains(&2) && values.contains(&3), "{values:?}");
                    }
                }
                CallRow::Here => {
                    assert_eq!(successors, [EXIT]);
                    assert!(values.is_empty() && stencil.imm32.is_empty());
                }
                CallRow::Direct { .. } => {
                    assert!([NEXT, TAKEN, EXIT].iter().all(|s| successors.contains(s)), "{successors:?}");
                    assert!(values.contains(&0) && values.iter().all(|v| *v <= 1), "the record address and the call pc are the value holes: {values:?}");
                }
                CallRow::Reload(_) => {
                    assert!(successors.iter().all(|s| *s == NEXT), "{successors:?}");
                    assert!(stencil.imm32.iter().any(|field| field.index == 0), "the slot offset is hole 0");
                }
                CallRow::Builtin(_) => {
                    assert!([NEXT, TAKEN].iter().all(|s| successors.contains(s)), "{successors:?}");
                    assert!(values.contains(&0), "the record address is the value hole: {values:?}");
                }
            }
        }
        assert!(crate::rustc::stencil(LEAVE_STATUS).is_some());
    }

    #[test]
    fn rows_beyond_the_contract_do_not_exist() {
        assert!(CallRow::Direct { argc: MAX_DIRECT_ARGC as u8 + 1, this: ThisRow::Strict, checked: true, pinned: true }.name().is_err());
        assert!(CallRow::Reload(Place::Mem).name().is_err());
    }
}
