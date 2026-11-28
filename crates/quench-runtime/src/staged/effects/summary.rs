//! The region set and effect summary of an operation: which regions it reads or writes, and how it leaves.

use super::*;

/// A set of regions, one bit each, so a fact's invalidation is a mask intersection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RegionSet(pub u16);

impl RegionSet {
    pub const NONE: Self = Self(0);

    pub const fn of(region: Region) -> Self {
        Self(1 << region as u16)
    }

    pub const fn with(self, region: Region) -> Self {
        Self(self.0 | 1 << region as u16)
    }

    pub const fn contains(self, region: Region) -> bool {
        self.0 & 1 << region as u16 != 0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

/// An operation's effect row. `may_collect` is separate from `may_alloc`: an allocation whose
/// fast path is inline does not collect; only a slow helper and a safepoint do.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Effects {
    pub reads: RegionSet,
    pub writes: RegionSet,
    /// The elements of which arrays the row writes (`ElementWrites`). A row that writes the `Elements` region names its
    /// arrays here: `Any` where the kind row cannot name them, narrowed by the settled facts where they can.
    pub elements: ElementWrites,
    /// Whether the row may allocate, which sets the collector's due flag and never collects inline: only a VM
    /// safepoint collects (`may_collect`). Guest code the row may run (`may_call_js`) may allocate too.
    pub may_alloc: bool,
    pub may_collect: bool,
    pub may_call_js: bool,
    pub may_throw: bool,
}

impl Effects {
    pub const NONE: Self = Self { reads: RegionSet::NONE, writes: RegionSet::NONE, elements: ElementWrites::Nothing,
        may_alloc: false, may_collect: false, may_call_js: false, may_throw: false };

    pub const fn reads(region: Region) -> Self { Self { reads: RegionSet::of(region), ..Self::NONE } }

    pub const fn writes(region: Region) -> Self { Self { writes: RegionSet::of(region), ..Self::NONE } }

    /// An operation that may run guest code or collect: it reads and writes every region. The top of every grade.
    pub const ANY: Self = Self { reads: RegionSet(!0), writes: RegionSet(!0), elements: ElementWrites::Any, may_alloc: true,
        may_collect: true, may_call_js: true, may_throw: true };

    /// A row that touches every region and runs nothing: no guest code, throw, collection or allocation. The settled
    /// facts grade a site with it where they prove it cannot leave and allocates nothing (`signatures::facts_grade`).
    /// Met with a kind row (`meet`) it clears those flags and keeps the kind's regions.
    pub(crate) const RUNS_NOTHING: Self = Self { reads: RegionSet(!0), writes: RegionSet(!0), elements: ElementWrites::Any, may_alloc: false,
        may_collect: false, may_call_js: false, may_throw: false };

    /// Whether the row may write `region`.
    pub const fn writes_region(self, region: Region) -> bool {
        self.writes.contains(region)
    }

    /// Whether the row may run guest code, allocate or collect: any of these may move records or change
    /// what a call leaves behind, so a word read across it is not fixed.
    pub const fn may_move_or_call(self) -> bool {
        self.may_call_js || self.may_alloc || self.may_collect
    }

    /// The union of two rows: a region is read or written if either row reads or writes it, and a
    /// flag holds if either does, so a sequence of steps keeps every effect any of them may have.
    pub const fn join(self, other: Self) -> Self {
        Self { reads: RegionSet(self.reads.0 | other.reads.0), writes: RegionSet(self.writes.0 | other.writes.0),
            elements: self.elements.join(other.elements),
            may_alloc: self.may_alloc || other.may_alloc, may_collect: self.may_collect || other.may_collect,
            may_call_js: self.may_call_js || other.may_call_js, may_throw: self.may_throw || other.may_throw }
    }

    /// The regions an object's shape, property slots, record words and payload live in: a write to one of them, outside an
    /// allocation (which writes only the new object's words), changes an existing object's exact shape or prototype link.
    const OBJECT_REGIONS: RegionSet = RegionSet::of(Region::ShapeWord).with(Region::ObjectSlots).with(Region::Records)
        .with(Region::Payload).with(Region::HeapMeta);

    /// Whether a row with this effect may change an existing object's exact shape or prototype link: it may run guest code (a
    /// getter, a setter, a proxy trap), or it writes an object's shape, slots or record words without allocating.
    pub const fn reshapes(self) -> bool {
        self.may_call_js || (!self.may_alloc && self.writes.intersects(Self::OBJECT_REGIONS))
    }

    /// Whether a row with this effect reads and nothing else: it writes no region, runs no guest code and allocates nothing.
    pub const fn reads_alone(self) -> bool {
        self.writes.0 == RegionSet::NONE.0 && matches!(self.elements, ElementWrites::Nothing) && !self.may_call_js && !self.may_alloc
    }

    /// Whether a row with this effect may rewrite the run's stores word: guest code may run in a nested run and specialize or
    /// unspecialize stores. An allocation runs no guest code, so it clears nothing.
    pub const fn clears_stores(self) -> bool {
        self.may_call_js
    }

    /// The meet of a kind row with a grade: a region is read or written only if both allow it, and a flag holds
    /// only if both do. A grade can clear what its kind row may do and never adds an effect, so a site's effects
    /// are its operation's kind row met with what the settled facts prove of the site: the one place a grade is computed.
    pub(crate) fn meet(self, other: Self) -> Self {
        Self { reads: RegionSet(self.reads.0 & other.reads.0), writes: RegionSet(self.writes.0 & other.writes.0),
            elements: self.elements.meet(other.elements),
            may_alloc: self.may_alloc && other.may_alloc, may_collect: self.may_collect && other.may_collect,
            may_call_js: self.may_call_js && other.may_call_js, may_throw: self.may_throw && other.may_throw }
    }

    /// Whether a step with this row can leave the activation: it may throw, run guest code or collect. A site's
    /// leave is this flag of its graded row (`signatures::site_grade`), so a fact that proves a site cannot leave
    /// clears those flags there, and the row's leave is never decided twice.
    pub(crate) const fn can_leave(self) -> bool {
        self.may_throw || self.may_call_js || self.may_collect
    }
}

/// How far a step can take the running activation out of native code (a leave). The lattice is a
/// chain: a function that cannot leave is `Never`; one that can leave only before its first effect
/// is `RestartCall`, so the call starts again from its entry; one that can leave after an effect
/// needs its frame rebuilt from the registers, `Materialize`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Leaves {
    /// No step can leave the activation under the facts.
    Never,
    /// The activation may leave only before its first write or allocation, so the call restarts.
    RestartCall,
    /// A leave needs the frame rebuilt.
    Materialize,
}

impl Leaves {
    /// The larger of the two: the chain's join is its maximum.
    pub const fn join(self, other: Self) -> Self {
        if other as u8 > self as u8 { other } else { self }
    }
}

/// How a step that can leave (`can_leave` of its graded row) leaves, given whether an earlier step of the
/// same function wrote or allocated. Before any
/// write or allocation nothing the call did is visible, so the call restarts from its entry; after one,
/// the frame must be rebuilt. Every tier reads this one rule.
pub(super) const fn leave_of(can_leave: bool, written_before: bool) -> Leaves {
    match (can_leave, written_before) {
        (false, _) => Leaves::Never,
        (true, false) => Leaves::RestartCall,
        (true, true) => Leaves::Materialize,
    }
}
