//! The rows a stitch spans and the registers live at each: a stitched program (`stitch`) of a
//! function or of a loop region spans its rows, numbered from 0, and holds the liveness it derived
//! for them, so webs, homes and exits are keyed by the row index.
use super::*;

/// Bits in a word of a live set.
pub(crate) const WORD: usize = u64::BITS as usize;

pub(crate) use quench_runtime::vm::engine_types::Live;

#[quench_macros::ext(LiveOps1)]
impl Live {
    /// An empty table over `registers` registers.
    pub(crate) fn new(registers: usize) -> Self {
        Self { words: registers.div_ceil(WORD), bits: Vec::new() }
    }

    /// Appends the next row, with the registers `live` names live.
    pub(crate) fn push(&mut self, live: impl Iterator<Item = Register>) {
        let start = self.bits.len();
        self.bits.resize(start + self.words, 0);
        for register in live {
            self.bits[start + usize::from(register) / WORD] |= 1 << (usize::from(register) % WORD);
        }
    }

    fn is_live(&self, row: u32, register: Register) -> bool {
        let (word, bit) = (usize::from(register) / WORD, usize::from(register) % WORD);
        word < self.words && self.bits.get(row as usize * self.words + word).is_some_and(|bits| bits >> bit & 1 != 0)
    }
}

/// The rows of a stitched program, numbered from 0, and what is live at each.
pub(crate) struct Span<'a> {
    pub region: &'a LoopRegion,
    pub header: u32,
    pub backedge: u32,
    pub live: &'a Live,
}

#[quench_macros::ext(SpanOps1)]
impl<'a> Span<'a> {
    /// The span of a stitched program of `rows` rows over the span `region` holds of its function.
    pub(crate) fn of(region: &'a LoopRegion, rows: usize, live: &'a Live) -> Self {
        Self { region, header: 0, backedge: rows as u32 - 1, live }
    }

    /// Whether the interpreter may read `register` resuming at row `pc`.
    pub(crate) fn is_live(&self, pc: u32, register: Register) -> bool {
        self.live.is_live(pc, register)
    }

    /// The function's instruction at caller PC `record`.
    pub(crate) fn instruction(&self, record: u32) -> Option<WideInstruction> {
        self.region.instructions().get(record as usize).copied()
    }

    pub(crate) fn number_registers(&self) -> &[Register] {
        self.region.number_registers()
    }
}
