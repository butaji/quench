//! The effect row of each staged `Operation`, from its variant alone.

#![allow(dead_code)] // Stage v2 vocabulary; first consumers are the rows moving under `Concrete` next.

use super::{Effects, Operation, Region, RegionSet};

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
