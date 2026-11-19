//! Storage the function-lowering passes use and drop. None of it escapes into a finished
//! `Function`, so it is cleared between passes and functions rather than freed: the compiler
//! that lowers a program's functions one after another reuses the same buffers.

use super::liveness::RegisterSet;
use super::object_registers::Aliases;
use crate::bitset::BitSet;
use crate::bytecode::{Instr, Register, SourceSpan, Successors};

/// Compiled source bytes per instruction a function's code is reserved for before it is
/// emitted. A span includes its nested functions' bodies, which compile to their own records,
/// so the ratio is set low (one instruction per twelve bytes, unmeasured): under-reserving costs
/// a doubling, over-reserving costs the trim's copy at finish. Tune against a bundle profile.
const SOURCE_BYTES_PER_INSTRUCTION: u32 = 12;

/// A finished table's spare capacity is released past one of this many parts of its capacity.
const TRIMMED_SLACK_DIVISOR: usize = 4;

/// The most instructions reserved up front for one function. A body past it grows as it
/// emits, so a pathological span cannot reserve far more than the body it compiles to.
const MAX_RESERVED_INSTRUCTIONS: usize = 1 << 12;

/// The temporary buffers of the lowering passes, owned by the `Compiler` that drives them.
///
/// Two kinds. A buffer a pass takes out (`mem::take`) and gives back, or hands on inside a
/// `CodeLiveness`, is recycled when that analysis ends (`CodeLiveness::recycle`). The others
/// are cleared and filled in place by the pass that owns them, and keep their storage between
/// passes and functions.
#[derive(Default)]
pub(crate) struct CompileScratch {
    /// The old-to-new pc map a relocating pass fills (`rewrite::relocate`).
    remap: Vec<usize>,
    /// The instruction storage a relocating pass replaced last. The next pass writes its
    /// output into it, so a function's passes share one buffer instead of allocating one each.
    spare_code: Vec<Instr>,
    /// Each instruction's (uses, definitions) masks (`liveness::transfers_into`).
    pub(in crate::compile) transfers: Vec<(RegisterSet, RegisterSet)>,
    /// The successor table of the code being analysed (`Successors::refill`).
    pub(in crate::compile) successors: Successors,
    /// The transfers narrowed to one word, and to two, for a solve at that width (`analyze_narrowed`).
    pub(in crate::compile) narrowed_word: Vec<(u64, u64)>,
    pub(in crate::compile) narrowed_pair: Vec<(u128, u128)>,
    /// The reads one `Move`'s copy-propagation scan rewrites (`copies::scan_move`).
    pub(in crate::compile) readers: Vec<(usize, Instr)>,
    /// The pcs a copy-propagation pass rewrote, and the readers it already forwarded.
    pub(in crate::compile) changed: Vec<usize>,
    pub(in crate::compile) rewritten_readers: BitSet,
    /// The alias facts of each pc, which object promotion solves (`object_registers::replacement`).
    pub(in crate::compile) incoming: Vec<Option<Aliases>>,
    /// The atoms a function's by-name accesses resolve (`frame_registers::promote`).
    pub(in crate::compile) by_name: Vec<u32>,
    /// The local slots a function's code accesses (`frame_registers::promote`).
    pub(in crate::compile) accessed: BitSet,
    /// The register each promoted local slot takes (`frame_registers::promote`).
    pub(in crate::compile) registers: Vec<Option<Register>>,
}

impl CompileScratch {
    /// Keeps `transfers`, a finished transfer table, as the next analysis's buffer.
    pub(crate) fn recycle_transfers(&mut self, mut transfers: Vec<(RegisterSet, RegisterSet)>) {
        transfers.clear();
        if transfers.capacity() > self.transfers.capacity() {
            self.transfers = transfers;
        }
    }

    /// Keeps `successors`, a finished successor table, as the next analysis's buffer.
    pub(crate) fn recycle_successors(&mut self, successors: Successors) {
        self.successors = successors;
    }

    /// An empty instruction buffer with room for `capacity`, recycled from the last pass.
    pub(crate) fn code_buffer(&mut self, capacity: usize) -> Vec<Instr> {
        let mut code = std::mem::take(&mut self.spare_code);
        code.clear();
        code.reserve(capacity);
        code
    }

    /// Keeps `code`, the instructions a pass replaced, as the next pass's buffer.
    pub(crate) fn recycle_code(&mut self, mut code: Vec<Instr>) {
        code.clear();
        if code.capacity() > self.spare_code.capacity() {
            self.spare_code = code;
        }
    }

    /// The pc map of a pass over `length` instructions: `length + 1` entries, all zero, in
    /// the buffer the last pass left.
    pub(crate) fn remap(&mut self, length: usize) -> &mut Vec<usize> {
        self.remap.clear();
        self.remap.resize(length + 1, 0);
        &mut self.remap
    }
}

/// Releases the spare capacity of a finished table once it exceeds a quarter of the table's
/// capacity. A smaller slack is kept: reallocating to exact size copies the whole table for a
/// few spare slots.
pub(crate) fn trim_slack<T>(items: &mut Vec<T>) {
    if items.capacity() - items.len() > items.capacity() / TRIMMED_SLACK_DIVISOR {
        items.shrink_to_fit();
    }
}

/// The instructions a function compiled from `span` reserves before it emits any.
pub(crate) fn code_capacity_hint(span: Option<SourceSpan>) -> usize {
    span.map_or(0, |span| (span.end.saturating_sub(span.start) / SOURCE_BYTES_PER_INSTRUCTION) as usize)
        .min(MAX_RESERVED_INSTRUCTIONS)
}
