//! The rows a stitch spans and the registers live at each. A loop region spans its own PCs and
//! asks the region's liveness; a whole function's stitched program (`stitch`) spans its rows,
//! numbered from 0, and holds the liveness it derived for them, so webs, homes and exits are keyed
//! by the row index in both.
use super::*;

/// Bits in a word of a live set.
pub(super) const WORD: usize = u64::BITS as usize;

/// The registers the interpreter may read resuming at each row, one set per row, flat.
pub(super) struct Live {
    words: usize,
    bits: Vec<u64>,
}

impl Live {
    /// An empty table over `registers` registers.
    pub(super) fn new(registers: usize) -> Self {
        Self { words: registers.div_ceil(WORD), bits: Vec::new() }
    }

    /// Appends the next row, with the registers `live` names live.
    pub(super) fn push(&mut self, live: impl Iterator<Item = Register>) {
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

/// The PCs a region's rows are numbered by, and what is live at each.
pub(super) struct Span<'a> {
    region: &'a LoopRegion,
    pub(super) header: u32,
    pub(super) backedge: u32,
    /// The stitched program's liveness; none for a loop region, which asks its own.
    live: Option<&'a Live>,
}

impl<'a> Span<'a> {
    /// The span of a loop region: its own PCs.
    pub(super) fn of(region: &'a LoopRegion) -> Self {
        Self { region, header: region.header, backedge: region.backedge, live: None }
    }

    /// The span of a stitched program of `rows` rows over the function `region` holds.
    pub(super) fn stitched(region: &'a LoopRegion, rows: usize, live: &'a Live) -> Self {
        Self { region, header: 0, backedge: rows as u32 - 1, live: Some(live) }
    }

    /// Whether the interpreter may read `register` resuming at row `pc`.
    pub(super) fn is_live(&self, pc: u32, register: Register) -> bool {
        self.live.map_or_else(|| self.region.is_live(pc, register), |live| live.is_live(pc, register))
    }

    pub(super) fn number_registers(&self) -> &[Register] {
        self.region.number_registers()
    }
}
