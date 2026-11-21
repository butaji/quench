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

/// The holes of the destination and of the operands: the destination's frame offset is hole 0, then the first and
/// the second operand's frame offset or value.
const DESTINATION: u32 = 0;
pub const FIRST: u32 = DESTINATION + 1;
pub const SECOND: u32 = FIRST + 1;

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
    Move = library::MOVE,
    Const = library::COPY,
    Add = library::ADD,
    Sub = library::SUB,
    Mul = library::MUL,
    Div = library::DIV,
    And = library::AND,
    Or = library::OR,
    Xor = library::XOR,
    Shl = library::SHL,
    Shr = library::SHR,
    UShr = library::USHR,
    /// Compare-and-jump: go to `taken` unless the relation holds.
    Lt = library::LT,
    Le = library::LE,
    Gt = library::GT,
    Ge = library::GE,
    Eq = library::EQ,
    Ne = library::NE,
    /// Go to `taken` (and poll the interrupt flag first, for `JumpPoll`, which also counts the
    /// edge in the loop entry record; for `JumpWatch`, which counts nothing, a function body
    /// having no such record).
    Jump = library::JUMP,
    JumpPoll = library::JUMP_POLL,
    /// Go to `taken` unless the operand is a Number; into a pinned destination an int word, which
    /// it loads; for an int slot destination, an int word.
    Guard = library::GUARD,
    /// `Guard` into a pinned destination with its tag test left out (`stencils/numeric.rs`'s `unbox_row`).
    Unbox = library::UNBOX,
    UnboxInt = library::UNBOX_INT,
    /// Set the high half of the int slot named by hole 0 to the int tag.
    TagInit = library::TAG_INIT,
    /// Write a pinned double (operand `a`) to the frame slot at hole 0 as a canonical Number word.
    Spill = library::SPILL,
    JumpWatch = library::JUMP_WATCH,
    /// Poll the interrupt flag, then continue at the next row: a conditional branch back is
    /// preceded by it, as an unconditional one is by its own poll.
    Poll = library::POLL,
    /// Go to `taken` when the operand is truthy (`JumpTrue`) or falsy (`JumpFalse`); a heap value
    /// whose cell decides its truthiness leaves.
    JumpTrue = library::JUMP_TRUE,
    JumpFalse = library::JUMP_FALSE,
    /// The destination takes the logical negation of the operand's truthiness (`Not`), or the
    /// Number negation of a Number operand (`Neg`).
    Not = library::NOT,
    Neg = library::NEG,
    /// `%`, the staged Number remainder.
    Rem = library::REM,
    /// `Math.sqrt`, `Math.floor`, `Math.ceil`, `Math.trunc`, `Math.abs` and `Math.round` of a Number
    /// (`codes::SQRT..=codes::ROUND`).
    Sqrt = library::SQRT,
    Floor = library::FLOOR,
    Ceil = library::CEIL,
    Trunc = library::TRUNC,
    Abs = library::ABS,
    Round = library::ROUND,
    /// `Math.fround` and `Math.sign` of one Number (`codes::FROUND`, `codes::SIGN`).
    Fround = library::FROUND,
    Sign = library::SIGN,
    /// `Math.min`, `Math.max` of two Numbers and `Math.imul` of two int32s (`codes::MIN..=codes::IMUL`).
    Min = library::MIN,
    Max = library::MAX,
    Imul = library::IMUL,
    /// `Math.clz32` of one Number (`codes::CLZ32`).
    Clz32 = library::CLZ32,
    /// int32 `+` and `-` a tier proved exact (`staged::Overflow::Impossible`): no overflow test, int operands only.
    AddNoOverflow = library::ADD_NO_OVERFLOW,
    /// `switch`'s case slot over dense int labels (`stencils/numeric.rs` `int_case_row`).
    IntCase = library::INT_CASE,
    SubNoOverflow = library::SUB_NO_OVERFLOW,
    /// The jumps of the tests a staged compare names with no Number relation (`library::BIT_TEST` and the rest).
    BitTest = library::BIT_TEST,
    NullishEq = library::NULLISH_EQ,
    NullishNe = library::NULLISH_NE,
    BoolTrue = library::BOOL_TRUE,
    BoolFalse = library::BOOL_FALSE,
}

impl Kind {
    /// Whether the kind is a bitwise operation, which computes on int32s only.
    pub fn is_bitwise(self) -> bool {
        (Kind::And..=Kind::UShr).contains(&self)
    }

    /// Whether the int32 operation gives the same result with its operands swapped, so the planner
    /// may put an immediate second, where a row encodes it into the instruction.
    pub fn commutes(self) -> bool {
        matches!(self, Kind::Add | Kind::AddNoOverflow | Kind::Mul | Kind::And | Kind::Or | Kind::Xor)
    }

    /// The int32 second operand that makes the operation its first operand unchanged (`x | 0`,
    /// `x + 0`, `x * 1`, `x & -1`, `x << 0`): partial evaluation on a known immediate folds such a row
    /// to a move. `>>>` has none: `x >>> 0` is a uint32, which an int32 register cannot hold.
    pub fn neutral(self) -> Option<i32> {
        match self {
            Kind::Add | Kind::Sub | Kind::AddNoOverflow | Kind::SubNoOverflow | Kind::Or | Kind::Xor | Kind::Shl | Kind::Shr => Some(0),
            Kind::Mul => Some(1),
            Kind::And => Some(-1),
            _ => None,
        }
    }

    /// Whether a row of this kind with a double operand computes in f64.
    pub fn has_float_form(self) -> bool {
        (self <= Kind::Ne && !self.is_bitwise()) || self == Kind::Rem
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

// The lib reads a subset of the shared codes: the generated `PLACE_CODE_COUNT` and the runtime's
// `TypedArrayKind::width` serve the stencil crate and the runtime only.
#[allow(dead_code)]
#[path = "../stencils/library.rs"]
// The stencils and the build script share these files and each reads its own part: the lib does not read every
// count, kind method or runtime operator table they mount (`codes.rs`, `supernode_rules.rs`).
#[allow(dead_code)]
pub mod library;
#[path = "../stencils/site_bits.rs"]
pub mod site_bits;
use library::TYPED_KINDS;
#[path = "../stencils/row_names.rs"]
pub mod row_names;
#[path = "../stencils/supernodes.rs"]
#[allow(dead_code)]
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
    static TABLE: crate::rustc::Memo<library::Code> = OnceLock::new();
    let code = library::canonical(row.code());
    crate::rustc::memoized(&TABLE, code, || crate::rustc::stencil(&library::name(code)).ok_or_else(|| format!("no stencil row {row:?}")))
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
        for nodes in supernodes::instances(supernodes::SUPERNODES, supernodes::FAMILIES) {
            let stencil = supernode_stencil(nodes).unwrap_or_else(|| panic!("{nodes:?}"));
            assert!(stencil.relocs.iter().any(|reloc| crate::class::is_successor(reloc.class)), "{nodes:?}");
        }
    }
}
