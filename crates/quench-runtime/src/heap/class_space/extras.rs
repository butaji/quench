//! The extras half of a live class cell's header (a split of `heap::class_space`). The header's low half is the cell's
//! class and its high half the cell's extras id (`ExtrasSpace`, `NO_EXTRAS` for none). A class edit writes the low
//! half alone (`write_class`), so a cell keeps its extras through every class transition.

use super::{ClassSpace, EXTRAS_SHIFT, header_extras, header_with_class};
use crate::heap::extras_space::NO_EXTRAS;
use crate::staged::object_encoding::CLASS_HEADER_BYTES;

/// The extras id of the live cell at granule `g` in the class space at `base` (`live_class_at`, read in place by a
/// parallel mark round).
#[inline(always)]
pub(in crate::heap) fn extras_id_at(base: usize, g: u32) -> u32 {
    // SAFETY: a live cell's granule of the reserved class space, whose header is a readable u64.
    let header = unsafe { *((base + g as usize * CLASS_HEADER_BYTES) as *const u64) };
    header_extras(header)
}

impl ClassSpace {
    /// Writes live cell `g`'s class, keeping its extras id: the header's low half only.
    #[inline(always)]
    pub(in crate::heap) fn write_class(&mut self, g: u32, class: u32) {
        let header = self.header(g);
        self.write_header(g, header_with_class(header, class));
    }

    /// Live cell `g`'s extras id (`NO_EXTRAS` where it has none).
    #[inline(always)]
    pub(in crate::heap) fn extras_id(&self, g: u32) -> u32 {
        header_extras(self.header(g))
    }

    /// Sets live cell `g`'s extras id, keeping its class. `NO_EXTRAS` clears it.
    pub(in crate::heap) fn set_extras_id(&mut self, g: u32, id: u32) {
        let header = self.header(g);
        let kept = header & u32::MAX as u64;
        let high = if id == NO_EXTRAS { 0 } else { (id as u64) << EXTRAS_SHIFT };
        self.write_header(g, kept | high);
    }
}
