//! The proven-arguments entry of a body region: a second entry behind the stubs, which a call whose arguments the caller
//! proved (`CallRecord::proven_args`) enters. It runs the unchecked unboxing row of each pinned guard, then jumps past the
//! guard rows. A Mem slot guard, or a MemInt check, writes nothing, so the entry skips it.

use super::*;

/// The proven entry's rows: each pinned guard's unboxing row and holes, and the first row past the guards (`past`).
pub(super) struct Proven {
    unboxes: Vec<(&'static Stencil, [u64; HOLES])>,
    past: usize,
}

impl Proven {
    /// The proven entry of the entry rows `entry`, or `None` for a loop region, which has none.
    pub(super) fn new(layout: &Layout, entry: &[EntryRow]) -> Option<Proven> {
        (layout.ending == Ending::Body).then(|| Proven {
            unboxes: entry.iter().filter_map(|row| row.unbox.map(|stencil| (stencil, row.holes))).collect(),
            past: entry.iter().take_while(|row| row.guards()).count(),
        })
    }

    /// The holes of the entry's rows: each unboxing row's, then the jump's (none).
    pub(super) fn holes(&self) -> Vec<[u64; HOLES]> {
        self.unboxes.iter().map(|(_, holes)| *holes).chain(std::iter::once([0; HOLES])).collect()
    }

    /// The number of rows the entry appends: its unboxing rows and its jump.
    pub(super) fn len(&self) -> usize {
        self.unboxes.len() + 1
    }

    /// Appends the entry's rows to `rows`, their holes taken from `holes` from index `at`.
    pub(super) fn place<'a>(&self, rows: &mut Vec<Placed<'a>>, holes: &'a [[u64; HOLES]], at: usize) -> Result<(), String> {
        let skip = numeric::stencil(numeric::Row { kind: Kind::Jump, dst: Place::Mem, a: Place::Mem, b: Place::Mem, speculate: true })?;
        rows.extend(self.unboxes.iter().enumerate().map(|(k, (stencil, _))| Placed { stencil: *stencil, hole: &holes[at + k], taken: None, cold: true }));
        rows.push(Placed { stencil: skip, hole: &holes[at + self.unboxes.len()], taken: Some(self.past), cold: true });
        Ok(())
    }
}
