//! What an operation reads, writes and may do, as data (Stage v2): the alias regions a native
//! load or store belongs to, the effect row of an `Operation`, and the targets of a call. A
//! specializer reads them to keep facts across operations (a fact about a region survives
//! until an operation that writes the region), and the interpreter ignores them.

#![allow(dead_code)] // Stage v2 vocabulary; first consumers are the rows moving under `Concrete` next.

use super::{Allocation, Operation};

/// An alias region of memory: an access to one never aliases an access to another.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Region {
    /// The activation's register and local slots.
    Frame,
    /// Property slots of objects.
    ObjectSlots,
    /// A shape word: written only by a shape change, so a load of it is read-only.
    ShapeWord,
    /// Dense and typed array elements.
    Elements,
    /// Heap bookkeeping: allocation runs, tables, collection state.
    HeapMeta,
    /// Per-site feedback counters.
    MissCounters,
}

/// A set of regions, one bit each, so a fact's invalidation is a mask intersection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct RegionSet(pub(crate) u16);

impl RegionSet {
    pub(crate) const NONE: Self = Self(0);

    pub(crate) const fn of(region: Region) -> Self {
        Self(1 << region as u16)
    }

    pub(crate) const fn with(self, region: Region) -> Self {
        Self(self.0 | 1 << region as u16)
    }

    pub(crate) const fn contains(self, region: Region) -> bool {
        self.0 & 1 << region as u16 != 0
    }

    pub(crate) const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

/// An operation's effect row. `may_collect` is separate from `may_alloc`: an allocation whose
/// fast path is inline does not collect; only a slow helper and a safepoint do.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Effects {
    pub(crate) reads: RegionSet,
    pub(crate) writes: RegionSet,
    pub(crate) may_alloc: bool,
    pub(crate) may_collect: bool,
    pub(crate) may_call_js: bool,
    pub(crate) may_throw: bool,
}

impl Effects {
    pub(crate) const NONE: Self = Self { reads: RegionSet::NONE, writes: RegionSet::NONE,
        may_alloc: false, may_collect: false, may_call_js: false, may_throw: false };

    const fn reads(region: Region) -> Self { Self { reads: RegionSet::of(region), ..Self::NONE } }

    const fn writes(region: Region) -> Self { Self { writes: RegionSet::of(region), ..Self::NONE } }

    /// An operation that may run guest code or collect: it reads and writes every region.
    const ANY: Self = Self { reads: RegionSet(!0), writes: RegionSet(!0), may_alloc: true,
        may_collect: true, may_call_js: true, may_throw: true };
}

/// What a call enters: a function whose entry is known (`Direct`), an entry word read at
/// run time (`Indirect`), or a builtin row (`Builtin`).
#[derive(Clone, Copy)]
pub(crate) enum CallTarget<W> {
    Direct(u64),
    Indirect(W),
    Builtin(u32),
}

impl<W, P> Operation<W, P> {
    /// The effect row of the operation, from its variant alone.
    pub(crate) const fn effects(&self) -> Effects {
        match self.memory_effects() {
            Some(effects) => effects,
            None => self.control_effects(),
        }
    }

    /// Effects of the field, element and allocation operations.
    const fn memory_effects(&self) -> Option<Effects> {
        Some(match self {
            Self::ReadField { .. } => Effects { reads: RegionSet::of(Region::ObjectSlots).with(Region::ShapeWord), ..Effects::NONE },
            Self::WriteField { .. } => Effects::writes(Region::ObjectSlots),
            Self::IndexRead { .. } | Self::TypedIndexRead { .. } => Effects::reads(Region::Elements),
            Self::IndexWrite { .. } | Self::TypedIndexWrite { .. } => Effects::writes(Region::Elements),
            Self::MakeObject { .. } | Self::MakeArray { .. } | Self::MakeConstArray { .. } | Self::MakeObjectPair { .. } => Effects {
                writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots), may_alloc: true, ..Effects::NONE },
            // A closure promotes the activation's captured locals into its environment.
            Self::Allocate { allocation: Allocation::Closure { .. }, .. } => Effects {
                writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots).with(Region::Frame), may_alloc: true, ..Effects::NONE },
            Self::Allocate { allocation: Allocation::ObjectLiteral { .. } | Allocation::Template { .. }, .. } => Effects {
                reads: RegionSet::of(Region::Frame), writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots),
                may_alloc: true, ..Effects::NONE },
            _ => return None,
        })
    }

    const fn control_effects(&self) -> Effects {
        match self {
            Self::Move { .. } | Self::LoadConst { .. } | Self::LoadThis { .. } => Effects::NONE,
            Self::LoadLocalPlain { .. } => Effects::reads(Region::Frame),
            Self::StoreLocalPlain { .. } => Effects::writes(Region::Frame),
            Self::Jump | Self::JumpIf { .. } | Self::LogicalNot { .. } | Self::Number(_) => Effects::NONE,
            Self::RequireObjectCoercible { .. } => Effects { may_throw: true, ..Effects::NONE },
            Self::Safepoint { .. } => Effects { may_collect: true, ..Effects::NONE },
            // The memory operations answered in `memory_effects`; anything else is conservative.
            _ => Effects::ANY,
        }
    }
}
