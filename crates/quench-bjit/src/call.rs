//! The call rows (`stencils/call_rows.rs`) as a planner names them: which row, and its stencil.
//! The contracts (registers, stack, holes, successors) are the doc of `call_rows.rs`; the
//! record the direct rows read is `call_record::CallRecord`.

use crate::call_record::MAX_DIRECT_ARGC;
use crate::numeric::Place;
use crate::Stencil;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// A call row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallRow {
    /// The helper call: value holes 0 (the call pc) and 1 (the helper's address); successors
    /// NEXT (completed), EXIT0 (missed: leave at the pc, no spill), EXIT1 (leave with
    /// (status, word) in the first two pinned ints: `LEAVE_STATUS`).
    Helper,
    /// The direct call of `argc` arguments (at most `MAX_DIRECT_ARGC`) through an entry cell:
    /// value hole 0 is the `CallRecord`'s address; successors NEXT (refused: the helper row),
    /// TAKEN (completed), EXIT0 (a leave inside the callee, `LEAVE_STATUS`).
    Direct { argc: u8, sloppy: bool },
    /// The pinned register `Place::Pin` or `Place::PinF` back from the tagged word a spill wrote
    /// to the frame slot at hole 0; successor NEXT.
    Reload(Place),
}

/// The routine a leave through a (status, word) pair ends in (`stencils/call_rows.rs`).
pub const LEAVE_STATUS: &str = "leave_status";

impl CallRow {
    /// The row's symbol in the stencil object, or why no such row exists.
    pub fn name(self) -> Result<String, String> {
        match self {
            CallRow::Helper => Ok("call_helper".to_string()),
            CallRow::Direct { argc, sloppy } if usize::from(argc) <= MAX_DIRECT_ARGC => {
                Ok(format!("call_direct_a{argc}_{}", if sloppy { "sloppy" } else { "strict" }))
            }
            CallRow::Reload(place @ (Place::Pin(_) | Place::PinF(_))) => Ok(format!("reload_{}", place.code())),
            other => Err(format!("no call row {other:?}")),
        }
    }
}

/// The stencil of `row`, built at compile time by rustc and shared for the life of the process.
pub fn stencil(row: CallRow) -> Result<&'static Stencil, String> {
    static TABLE: OnceLock<Mutex<HashMap<CallRow, &'static Stencil>>> = OnceLock::new();
    let mut table = TABLE.get_or_init(Default::default).lock().map_err(|e| e.to_string())?;
    if let Some(found) = table.get(&row) {
        return Ok(found);
    }
    let name = row.name()?;
    let found = crate::rustc::stencil(&name).ok_or_else(|| format!("no stencil row {name}"))?;
    let stencil: &'static Stencil = Box::leak(Box::new(found));
    table.insert(row, stencil);
    Ok(stencil)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EXIT, NEXT, TAKEN};

    fn rows() -> Vec<CallRow> {
        let direct = (0..=MAX_DIRECT_ARGC as u8).flat_map(|argc| [false, true].map(|sloppy| CallRow::Direct { argc, sloppy }));
        let pins = (0..crate::PINNED as u8).map(Place::Pin).chain((0..crate::PINNED_F as u8).map(Place::PinF)).map(CallRow::Reload);
        [CallRow::Helper].into_iter().chain(direct).chain(pins).collect()
    }

    /// Every row the planner can name was built, and its successors and holes are the contract's.
    #[test]
    fn every_call_row_is_built_with_its_contract_holes() {
        for row in rows() {
            let stencil = stencil(row).unwrap_or_else(|e| panic!("{row:?}: {e}"));
            let successors: Vec<u32> = stencil.relocs.iter().filter(|r| r.pc_relative).map(|r| r.index).collect();
            let values: Vec<u32> = stencil.relocs.iter().filter(|r| !r.pc_relative).map(|r| r.index).collect();
            match row {
                CallRow::Helper => {
                    assert!([NEXT, EXIT, EXIT + 1].iter().all(|s| successors.contains(s)), "{successors:?}");
                    assert!(values.contains(&0) && values.contains(&1), "{values:?}");
                }
                CallRow::Direct { .. } => {
                    assert!([NEXT, TAKEN, EXIT].iter().all(|s| successors.contains(s)), "{successors:?}");
                    assert!(!values.is_empty() && values.iter().all(|v| *v == 0), "the record address is the one value hole: {values:?}");
                }
                CallRow::Reload(_) => {
                    assert!(successors.iter().all(|s| *s == NEXT), "{successors:?}");
                    assert!(stencil.imm32.iter().any(|field| field.index == 0), "the slot offset is hole 0");
                }
            }
        }
        assert!(crate::rustc::stencil(LEAVE_STATUS).is_some());
    }

    #[test]
    fn rows_beyond_the_contract_do_not_exist() {
        assert!(CallRow::Direct { argc: MAX_DIRECT_ARGC as u8 + 1, sloppy: false }.name().is_err());
        assert!(CallRow::Reload(Place::Mem).name().is_err());
    }
}
