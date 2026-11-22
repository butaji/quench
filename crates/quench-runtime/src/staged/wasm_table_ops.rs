//! Wasm table, element-segment and data-segment operations as data: which opcode is which
//! operation, the registers each one names (its table or segment bindings, its index, length
//! and delta words, the reference it stores), and whether it reads the function's memory. Every
//! tier reads this one table through `Step::Table`. The interpreter and the u-jit both run the
//! operation's semantics once, over the VM's tables (`Vm::wasm_table_op` in
//! `vm/compiled/wasm_table_ops_stage.rs`): the interpreter decodes its registers into the words
//! it names here, the u-jit passes the same words to a callout.

use crate::bytecode::{Class, Op, WideInstruction};

/// The table, element-segment, data-segment and memory-init operations a function body runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum TableOp {
    /// `a = table.size b`.
    Size,
    /// `a = table.get b[c]`: the reference bits of the element.
    Get,
    /// `table.set b[c] = a`.
    Set,
    /// `a = table.grow b by pair.0, initialized to c`: the size before, or -1 where it cannot grow.
    Grow,
    /// `table.fill a[pair.0] = b` for `c` elements.
    Fill,
    /// `table.copy a[pair.0] <- b[pair.1]` for `c` elements, `b` another table.
    Copy,
    /// `table.init a[pair.0] <- element segment b[pair.1]` for `c` elements.
    Init,
    /// `memory.init a[pair.0] <- data segment b[pair.1]` for `c` bytes, `a` the function's memory.
    MemoryInit,
    /// `data.drop` and `elem.drop`: the segment bound at environment slot `imm` is dropped.
    SegmentDrop,
}

impl TableOp {
    /// Every operation, in declaration order.
    pub(crate) const ALL: [Self; 9] = [
        Self::Size,
        Self::Get,
        Self::Set,
        Self::Grow,
        Self::Fill,
        Self::Copy,
        Self::Init,
        Self::MemoryInit,
        Self::SegmentDrop,
    ];

    /// The operation `op` is, if it is one of these.
    pub const fn of(op: Op) -> Option<Self> {
        match op.class() {
            Class::WasmTableSize => Some(Self::Size),
            Class::WasmTableGet => Some(Self::Get),
            Class::WasmTableSet => Some(Self::Set),
            Class::WasmTableGrow => Some(Self::Grow),
            Class::WasmTableFill => Some(Self::Fill),
            Class::WasmTableCopy => Some(Self::Copy),
            Class::WasmTableInit => Some(Self::Init),
            Class::WasmMemoryInit => Some(Self::MemoryInit),
            Class::WasmSegmentDrop => Some(Self::SegmentDrop),
            _ => None,
        }
    }

    /// The tag a callout receives the operation by.
    pub const fn tag(self) -> u64 {
        self as u64
    }

    /// The operation of `tag`, as `tag` names it.
    pub fn from_tag(tag: u64) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.tag() == tag)
    }

    /// The registers of the bindings the operation names, first then second: a table, a
    /// segment or (for `memory.init`) a segment, and the second table or segment of a copy or
    /// an init. A binding register holds a `Slot::Binding`, the environment slot it names.
    pub fn bindings(self, instruction: WideInstruction) -> (Option<u16>, Option<u16>) {
        match self {
            Self::Size | Self::Get | Self::Set | Self::Grow | Self::MemoryInit => {
                (Some(instruction.b()), None)
            }
            Self::Fill => (Some(instruction.a()), None),
            Self::Copy | Self::Init => (Some(instruction.a()), Some(instruction.b())),
            Self::SegmentDrop => (None, None),
        }
    }

    /// The registers of the word operands, in the order `Vm::wasm_table_op` takes them: an
    /// index, a length, a delta, or a destination, a source and a length. Each is a 32-bit word.
    pub fn words(self, instruction: WideInstruction) -> [Option<u16>; 3] {
        let (first, second) = instruction.register_pair();
        match self {
            Self::Size | Self::SegmentDrop => [None, None, None],
            Self::Get | Self::Set => [Some(instruction.c()), None, None],
            Self::Grow => [Some(first), None, None],
            Self::Fill => [Some(first), Some(instruction.c()), None],
            Self::Copy | Self::Init | Self::MemoryInit => {
                [Some(first), Some(second), Some(instruction.c())]
            }
        }
    }

    /// The register of the reference the operation stores or initializes with, if any: an `I64`
    /// word holding the reference's bits.
    pub fn value(self, instruction: WideInstruction) -> Option<u16> {
        match self {
            Self::Set => Some(instruction.a()),
            Self::Fill => Some(instruction.b()),
            Self::Grow => Some(instruction.c()),
            _ => None,
        }
    }
}
