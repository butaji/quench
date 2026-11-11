//! Hand-written stencil rows for Number loops over tagged `Value` words. A row names where each
//! operand lives: a frame slot holding a tagged word (hole: byte offset), a pinned register
//! holding a raw int32 (rdx, rcx, r8, r9 through the whole chain of stencils: no tag, no
//! store-forwarding), an immediate int32 (hole) or a constant tagged word (hole). A row applies
//! the interpreter's Number semantics (`Value::number` canonicalization) and continues at the
//! next row. Speculating rows take int32 operands only: anything else, an overflow, or a result
//! a pinned register cannot hold leaves through exit 0 before any effect, with the pinned
//! registers as they were at the row's start. General rows take tagged words (pinned operands
//! are tagged on use) and write a frame slot.
//!
//! Holes: 0 is the destination offset, 1 and 2 the operands; a jump polls the interrupt flag
//! pointer at the context offset in hole 0.
use crate::row::{Operand, RowBuilder, Source, Ty};
use crate::{Pins, Stencil, StencilBuilder, generate, stencil_isa};
use cranelift_codegen::ir::{self, InstBuilder, condcodes::{CondCode, FloatCC, IntCC}, types};
use cranelift_codegen::isa::OwnedTargetIsa;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

mod control;
mod float;
mod int;
mod word;
use control::*;
use float::*;
use int::*;
pub use word::{canonical, f64_to_i32, int_of, int_word, is_double, is_int, to_f64, word_of};
use word::*;
pub use float::{deliver, f64_of, index_of, number_word};

const TAG_INT: i64 = 0x7ffd_0000_0000_0000;
const FIRST_TAG: i64 = 0x7ff9_0000_0000_0000;
const TAG_SPAN: i64 = 6 << 48;
const CANONICAL_NAN: i64 = 0x7ff8_0000_0000_0000;
const TAG_SHIFT: i64 = 48;
const WORD_SHIFT: i64 = 32;
/// The high 32 bits of an int word.
const INT_HIGH: i64 = TAG_INT >> WORD_SHIFT;
/// Doubles below this magnitude convert to an i64 exactly.
const I64_LIMIT: f64 = 9.0e18;

/// The holes of an operand: the first and the second operand's frame offset or value.
pub const FIRST: u32 = 1;
pub const SECOND: u32 = 2;

/// Where a row's operand lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// A frame slot: the hole is its byte offset.
    Mem,
    /// A pinned register holding a raw int32.
    Pin(u8),
    /// A pinned xmm register holding a Number as `f64` (an int is converted, a result is not
    /// canonicalized until it is written to a frame slot).
    PinF(u8),
    /// A constant tagged `Value` word: the hole.
    Const,
    /// An int32 constant: the hole holds its low 32 bits.
    Imm,
    /// A frame slot known to hold an int word: read and written as its low 32 bits, the tag
    /// staying in place.
    MemInt,
}

/// The operation of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Kind {
    Move,
    Const,
    Add,
    Sub,
    Mul,
    Div,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    UShr,
    /// Compare-and-jump: go to `taken` unless the relation holds.
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    /// Go to `taken` (and poll the interrupt flag first, for `JumpPoll`).
    Jump,
    JumpPoll,
    /// Go to `taken` unless the operand is a Number; into a pinned destination an int word, which
    /// it loads; for an int slot destination, an int word.
    Guard,
    /// Set the high half of the int slot named by hole 0 to the int tag.
    TagInit,
    /// Write a pinned double (operand `a`) to the frame slot at hole 0 as a canonical Number word.
    Spill,
}

impl Kind {
    /// Whether the kind is a bitwise operation, which computes on int32s only.
    pub fn is_bitwise(self) -> bool {
        (Kind::And..=Kind::UShr).contains(&self)
    }

    /// Whether a row of this kind with a double operand computes in f64.
    pub fn has_float_form(self) -> bool {
        self <= Kind::Ne && !self.is_bitwise()
    }
}

/// One row: operation and operand places; `speculate` rows take int32 operands only and leave
/// through exit 0 otherwise, general rows also handle doubles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Row {
    pub kind: Kind,
    pub dst: Place,
    pub a: Place,
    pub b: Place,
    pub speculate: bool,
}

impl Row {
    /// Whether a double register is read or written: the row computes in `f64`.
    fn floats(self) -> bool {
        [self.dst, self.a, self.b].iter().any(|place| matches!(place, Place::PinF(_)))
    }
}

fn emit(b: &mut StencilBuilder, row: Row) {
    if row.floats() && row.kind.has_float_form() {
        return float_row(b, row);
    }
    match row.kind {
        Kind::Move | Kind::Const => copy_row(b, row),
        Kind::Add | Kind::Sub | Kind::Mul | Kind::Div => arith_row(b, row),
        Kind::And | Kind::Or | Kind::Xor | Kind::Shl | Kind::Shr | Kind::UShr => bitwise_row(b, row),
        Kind::Lt | Kind::Le | Kind::Gt | Kind::Ge | Kind::Eq | Kind::Ne => compare_row(b, row),
        Kind::Jump | Kind::JumpPoll => jump_row(b, row.kind == Kind::JumpPoll),
        Kind::Guard | Kind::TagInit | Kind::Spill => bookkeeping_row(b, row),
    }
}

/// The rows that compute nothing: an entry guard, the tag initialisation, a spill.
fn bookkeeping_row(b: &mut StencilBuilder, row: Row) {
    match row.kind {
        Kind::Guard => guard_row(b, row),
        Kind::TagInit => tag_init_row(b),
        _ => spill_row(b, row),
    }
}

pub fn isa() -> Result<&'static OwnedTargetIsa, String> {
    static ISA: OnceLock<Result<OwnedTargetIsa, String>> = OnceLock::new();
    ISA.get_or_init(stencil_isa).as_ref().map_err(Clone::clone)
}

static GENERATION_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Nanoseconds spent generating stencils so far.
pub fn generation_ns() -> u64 {
    GENERATION_NS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Counts `nanos` of stencil generation done outside this module.
pub fn add_generation_ns(nanos: u64) {
    GENERATION_NS.fetch_add(nanos, std::sync::atomic::Ordering::Relaxed);
}

/// The stencil of `row`: generated on first use, then shared for the life of the process.
pub fn stencil(row: Row) -> Result<&'static Stencil, String> {
    static TABLE: Mutex<Option<HashMap<Row, &'static Stencil>>> = Mutex::new(None);
    let mut table = TABLE.lock().map_err(|e| e.to_string())?;
    let table = table.get_or_insert_with(HashMap::new);
    if let Some(found) = table.get(&row) {
        return Ok(found);
    }
    let started = std::time::Instant::now();
    let made = isa().and_then(|isa| generate(isa, |b| emit(b, row)));
    GENERATION_NS.fetch_add(started.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    let made = made?;
    let stencil: &'static Stencil = Box::leak(Box::new(made));
    table.insert(row, stencil);
    Ok(stencil)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of every kind generates, with its operands in each kind of place; with
    /// `STENCIL_DUMP=dir`, each row's bytes are written there.
    #[test]
    fn every_kind_generates() {
        let kinds = [
            Kind::Move, Kind::Const, Kind::Add, Kind::Sub, Kind::Mul, Kind::Div, Kind::And, Kind::Or, Kind::Xor,
            Kind::Shl, Kind::Shr, Kind::UShr, Kind::Lt, Kind::Le, Kind::Gt, Kind::Ge, Kind::Eq, Kind::Ne, Kind::Jump,
            Kind::JumpPoll, Kind::Guard, Kind::TagInit, Kind::Spill,
        ];
        for kind in kinds {
            for (dst, a, b) in [(Place::Mem, Place::Mem, Place::Const), (Place::Pin(0), Place::Pin(1), Place::Pin(2)), (Place::Pin(2), Place::Pin(2), Place::Mem), (Place::PinF(0), Place::PinF(1), Place::Const), (Place::Mem, Place::PinF(3), Place::Pin(0)), (Place::PinF(2), Place::Mem, Place::Imm)] {
                for speculate in [true, false] {
                    let row = Row { kind, dst, a, b, speculate };
                    let stencil = stencil(row).unwrap();
                    if let Ok(dir) = std::env::var("STENCIL_DUMP") {
                        let name = format!("{kind:?}-{:?}-{:?}-{:?}-{speculate}", row.dst, row.a, row.b);
                        std::fs::write(format!("{dir}/{name}.bin"), &stencil.code).unwrap();
                    }
                }
            }
        }
    }
}
