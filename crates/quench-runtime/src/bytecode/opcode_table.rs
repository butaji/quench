//! Opcode-table facts that belong to the table as a whole, not to one opcode: its width and
//! the diagnostics name of each entry.
use super::Op;

/// One entry per opcode, for a table keyed by an opcode value.
pub(crate) const OPCODE_COUNT: usize = Op::COUNT;

/// The opcode's name for diagnostics; a value outside the table names no opcode.
pub(crate) fn opcode_name(opcode: usize) -> &'static str {
    Op::NAMES.get(opcode).copied().unwrap_or("unknown-opcode")
}
