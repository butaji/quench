//! Storage the function-lowering passes use and drop. None of it escapes into a finished
//! `Function`, so it is cleared between passes and functions rather than freed: the compiler
//! that lowers a program's functions one after another reuses the same buffers.

use crate::bytecode::{Instr, SourceSpan};

/// Compiled source bytes per instruction a function's code is reserved for before it is
/// emitted: one instruction per six source bytes, the ratio the lowering's bodies measure at.
const SOURCE_BYTES_PER_INSTRUCTION: u32 = 6;

/// A finished table's spare capacity is released past one of this many parts of its capacity.
const TRIMMED_SLACK_DIVISOR: usize = 4;

/// The most instructions reserved up front for one function. A body past it grows as it
/// emits, so a pathological span cannot reserve far more than the body it compiles to.
const MAX_RESERVED_INSTRUCTIONS: usize = 1 << 16;

/// The temporary buffers of the lowering passes, owned by the `Compiler` that drives them.
#[derive(Default)]
pub(crate) struct CompileScratch {
    /// The old-to-new pc map a relocating pass fills (`rewrite::relocate`).
    remap: Vec<usize>,
    /// The instruction storage a relocating pass replaced last. The next pass writes its
    /// output into it, so a function's passes share one buffer instead of allocating one each.
    spare_code: Vec<Instr>,
}

impl CompileScratch {
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
