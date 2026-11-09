//! Slot pointee observations: `null`/`undefined` only, or any value.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TargetShape(u32);

impl TargetShape {
    pub(crate) const NONE: Self = Self(u32::MAX);
    pub(crate) const ANY: Self = Self(u32::MAX - 1);
    pub(crate) fn unclassified(value: crate::Value) -> Self {
        if value.is_null() || value.is_undefined() { Self::NONE } else { Self::ANY }
    }
    pub(crate) fn join(self, other: Self) -> Self {
        if self == Self::NONE { other }
        else if other == Self::NONE || self == other { self }
        else { Self::ANY }
    }
}

impl super::ValueArena {
    /// Whether `slot` of `shape` already holds pointees of any shape, so a store's target
    /// joins nothing (`TargetShape::ANY` is the top).
    #[inline(always)]
    pub(crate) fn slot_target_settled(&self, shape: u32, slot: usize) -> bool {
        self.tracking && slot < self.shape_length(shape) && self.targets.get(self.slot_owner(shape, slot)) == TargetShape::ANY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_are_monotone_and_nullish_is_neutral() {
        let states = [TargetShape::NONE, TargetShape::ANY];
        for a in states {
            assert_eq!(a.join(a), a);
            assert_eq!(a.join(TargetShape::NONE), a);
            for b in states {
                assert_eq!(a.join(b), b.join(a));
                for c in states { assert_eq!(a.join(b).join(c), a.join(b.join(c))); }
            }
        }
    }
}

/// Each slot owner's pointee observation: whether it ever held a value other than `null`
/// or `undefined`.
#[derive(Default)]
pub(super) struct TargetShapes {
    targets: Vec<TargetShape>,
}

impl TargetShapes {
    #[inline(always)]
    pub(super) fn get(&self, owner: u32) -> TargetShape {
        self.targets.get(owner as usize).copied().unwrap_or(TargetShape::NONE)
    }

    #[inline(always)]
    pub(super) fn join(&mut self, owner: u32, incoming: TargetShape) -> bool {
        let held = self.get(owner);
        if held == incoming || held == TargetShape::ANY { return false; }
        let index = owner as usize;
        if index >= self.targets.len() { self.targets.resize(index + 1, TargetShape::NONE); }
        self.targets[index] = held.join(incoming);
        true
    }

    pub(super) fn reset_owner(&mut self, owner: u32) {
        if let Some(target) = self.targets.get_mut(owner as usize) { *target = TargetShape::NONE; }
    }

    pub(super) fn reset(&mut self) {
        self.targets.clear();
    }

    pub(super) fn memory_bytes(&self) -> usize {
        self.targets.capacity() * size_of::<TargetShape>()
    }
}
