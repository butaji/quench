//! Number rows for loops over tagged `Value` words, the vocabulary the planner names them in
//! (the semantics are `stencils/numeric.rs`). A row names where each operand lives: a frame slot holding a tagged word (hole: byte offset), a pinned register
//! holding a raw int32 (the first integer registers of the row convention, through the whole chain of stencils: no tag, no
//! store-forwarding), an immediate int32 (hole) or a constant tagged word (hole). A row applies
//! the interpreter's Number semantics (`Value::number` canonicalization) and continues at the
//! next row. Speculating rows take int32 operands only: anything else, an overflow, or a result
//! a pinned register cannot hold leaves through exit 0 before any effect, with the pinned
//! registers as they were at the row's start. General rows take tagged words (pinned operands
//! are tagged on use) and write a frame slot.
//!
//! Holes: 0 is the destination offset, 1 and 2 the operands; a jump polls the interrupt flag
//! pointer at the context offset in hole 0.
use crate::Stencil;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

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

#[path = "../stencils/library.rs"]
pub mod library;
#[path = "../stencils/supernodes.rs"]
pub mod supernodes;

impl Place {
    /// The code of this place in `library` and the stencil rows.
    pub fn code(self) -> u8 {
        match self {
            Place::Mem => library::MEM,
            Place::MemInt => library::MEMINT,
            Place::Const => library::CONST,
            Place::Imm => library::IMM,
            Place::Pin(i) => library::PIN + i,
            Place::PinF(i) => library::PINF + i,
        }
    }
}

impl Row {
    /// The library's code of this row.
    pub fn code(self) -> library::Code {
        (self.kind as u8, self.dst.code(), self.a.code(), self.b.code(), self.speculate)
    }
}

/// The stencil of `row`, built at compile time by rustc and shared for the life of the process;
/// a row the library does not hold is an error, which declines the region that needs it.
pub fn stencil(row: Row) -> Result<&'static Stencil, String> {
    type Table = Mutex<HashMap<library::Code, &'static Stencil>>;
    static TABLE: OnceLock<Table> = OnceLock::new();
    let code = library::canonical(row.code());
    let mut table = TABLE.get_or_init(Default::default).lock().map_err(|e| e.to_string())?;
    if let Some(found) = table.get(&code) {
        return Ok(found);
    }
    let found = crate::rustc::stencil(&library::name(code)).ok_or_else(|| format!("no stencil row {row:?}"))?;
    let stencil: &'static Stencil = Box::leak(Box::new(found));
    table.insert(code, stencil);
    Ok(stencil)
}

/// The stencil of the supernode row computing `nodes` (`supernodes.rs`), built at compile time by
/// rustc and shared for the life of the process; `None` for a pair the library does not hold (the
/// pair is then planned as single rows). Misses are remembered too.
pub fn supernode_stencil(nodes: [supernodes::Node; supernodes::NODES]) -> Option<&'static Stencil> {
    type Table = Mutex<HashMap<[supernodes::Node; supernodes::NODES], Option<&'static Stencil>>>;
    static TABLE: OnceLock<Table> = OnceLock::new();
    let mut table = TABLE.get_or_init(Default::default).lock().ok()?;
    *table.entry(nodes).or_insert_with(|| crate::rustc::stencil(&supernodes::name(&nodes)).map(|found| &*Box::leak(Box::new(found))))
}

#[cfg(test)]
mod supernode_tests {
    use super::{supernode_stencil, supernodes};

    /// Every supernode the library names was built, and has the successors a row chains by.
    #[test]
    fn the_supernodes_are_built() {
        for nodes in supernodes::instances() {
            let stencil = supernode_stencil(nodes).unwrap_or_else(|| panic!("{nodes:?}"));
            assert!(stencil.relocs.iter().any(|reloc| crate::class::is_successor(reloc.class)), "{nodes:?}");
        }
    }
}
