//! Run bookkeeping of the arena: the free lists of released runs, the nursery's young range,
//! and the reservations that keep a predicted run's room.
use super::*;

/// A size class's released runs, linked through each run's first value: a released run's
/// values are dead, so its list costs no memory of its own. As a `Vec<u32>` per class, the
/// lists held 2.2 MB at TypeScript's peak, the full collection that releases millions of runs.
///
/// The list is `unreleased` runs at the head, then released ones: a run is pushed at the head
/// unreleased, `pop_free` takes the head, and `return_free_pages` gives back the pages of the
/// first `unreleased` runs and then counts them released. So a run whose pages are already
/// given back is never walked again, and every link stays the one `push_free` wrote.
#[derive(Clone, Copy, Debug)]
pub(super) struct FreeRuns {
    pub(super) head: u32,
    pub(super) len: u32,
    pub(super) unreleased: u32,
}

impl FreeRuns {
    pub(super) const EMPTY: Self = Self { head: u32::MAX, len: 0, unreleased: 0 };

    pub(super) fn is_empty(self) -> bool {
        self.len == 0
    }

    pub(super) fn len(self) -> usize {
        self.len as usize
    }
}

/// The runs of new objects (`ValueArena::young_run_padded`), bumped from one range of the
/// arena, `[floor, end)`: those taken since the last collection are `[floor, top)`. A dead
/// young object releases nothing; its collection moves the few young runs whose owners
/// survive (`ValueArena::evacuate_young`) and takes back the rest of `[floor, top)` at once,
/// so a dead young object's storage costs nothing to reclaim and the collection's cost follows
/// its survivors (a scavenger's bump allocation and evacuation, over immix's line reclaim).
///
/// Only an object's creation takes a young run: any other range comes from `allocate`, so
/// every owner of a young run is a young object, which the sweep shows (`RecordSweep::Keep`).
/// Young runs are ordinary size-class runs, so forgetting the nursery (`Nursery::default`) is
/// always sound: its dead runs then wait for a compaction.
///
/// The range stays where it is: once something else ends the arena it no longer grows, and
/// young runs past its end are ordinary ones. Retiring it to start again at the arena's end
/// handed its unused values to the free lists as runs of classes few allocations asked for:
/// on acorn-wtb they held 15K idle values more than the old path's at the peak, 0.2 MB of
/// max RSS.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Nursery {
    pub(super) floor: usize,
    pub(super) top: usize,
    pub(super) end: usize,
}

/// The nursery's young range, `[floor, top)`, which a collection's releases leave as it is.
#[derive(Clone, Copy, Debug)]
pub(crate) struct YoungRuns {
    pub(crate) floor: usize,
    pub(crate) top: usize,
}

impl YoungRuns {
    /// Whether `vector` holds one of these runs.
    #[inline(always)] pub(crate) fn hold(self, vector: ValueVec) -> bool {
        !vector.is_dictionary() && vector.start().wrapping_sub(self.floor) < self.top - self.floor
    }
}

/// A young run taken past the class of its length by a growth that predicts more adds
/// (`ValueArena::grow`, `reserve_predicted_run`): `[start, end)` was bumped from the nursery whole, so no
/// other run holds any of it until the young cycle ends, and the run's owner may grow into it
/// without moving (`ValueArena::reserved_room`). A run's capacity is otherwise a fact of its
/// length (`ordered_capacity`), which every other reader still sees: the words past it are
/// only the owner's to append into, and a release, an evacuation or a compaction moves or
/// frees no more than that capacity, leaving the rest as dead nursery words. Valid while
/// `cycle` is the arena's `reservation_cycle`, which every event that frees or moves young
/// runs advances; an entry another reservation evicts only loses the room, never a value.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Reservation {
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) cycle: u32,
}

impl Reservation {
    #[inline(always)]
    pub(super) fn index(start: usize) -> usize {
        // Fibonacci hashing: reserved runs lie a few classes apart.
        ((start as u32).wrapping_mul(0x9E37_79B9) >> (32 - RESERVATIONS.trailing_zeros())) as usize
    }
}
