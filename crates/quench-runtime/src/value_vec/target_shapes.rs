//! Historical pointee shapes, never live record-access certificates.
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TargetShape(u32);

impl TargetShape {
    pub(crate) const NONE: Self = Self(u32::MAX);
    pub(crate) const ANY: Self = Self(u32::MAX - 1);
    pub(crate) fn unclassified(value: crate::Value) -> Self {
        if value.is_null() || value.is_undefined() { Self::NONE } else { Self::ANY }
    }
    pub(crate) fn exact(shape: u32) -> Self {
        assert!(shape <= super::SHAPE_MASK);
        Self(shape)
    }
    pub(crate) fn shape(self) -> Option<u32> { (self.0 <= super::SHAPE_MASK).then_some(self.0) }
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
        let states = [TargetShape::NONE, TargetShape::exact(0), TargetShape::exact(1), TargetShape::ANY];
        for a in states {
            assert_eq!(a.join(a), a);
            assert_eq!(a.join(TargetShape::NONE), a);
            for b in states {
                assert_eq!(a.join(b), b.join(a));
                for c in states { assert_eq!(a.join(b).join(c), a.join(b.join(c))); }
            }
        }
    }

    #[test]
    fn reverse_links_survive_middle_head_removal_and_owner_reuse() {
        let mut targets = TargetShapes::default();
        for owner in 0..4 { targets.join(owner, TargetShape::exact(2)); }
        targets.join(4, TargetShape::exact(3));
        targets.reset_owner(1); // middle
        targets.reset_owner(3); // head
        targets.join(1, TargetShape::exact(3));
        let mut retired = Vec::new();
        targets.depart(2, |owner| retired.push(owner));
        retired.sort_unstable();
        assert_eq!(retired, [0, 2]);
        assert_eq!(targets.get(1), TargetShape::exact(3));
        assert_eq!(targets.get(3), TargetShape::NONE);
        assert!(!targets.join(0, TargetShape::exact(2)));
        retired.clear();
        targets.depart(3, |owner| retired.push(owner));
        retired.sort_unstable();
        assert_eq!(retired, [1, 4]);
        targets.reset();
        targets.join(0, TargetShape::exact(3));
        retired.clear();
        targets.depart(3, |owner| retired.push(owner));
        assert_eq!(retired, [0]);
    }
}

type Link = Option<NonZeroU32>;

/// Each owner belongs to at most one target shape's reverse list. Indexed links
/// permit O(1) joining/unlinking; a target departure visits only its owners.
#[derive(Default)]
pub(super) struct TargetShapes {
    targets: Vec<TargetShape>,
    heads: Vec<Link>,
    next: Vec<Link>,
    previous: Vec<Link>,
}

impl TargetShapes {
    #[inline(always)]
    pub(super) fn get(&self, owner: u32) -> TargetShape {
        self.targets.get(owner as usize).copied().unwrap_or(TargetShape::NONE)
    }

    #[inline(never)]
    fn set(&mut self, owner: u32, target: TargetShape) {
        let index = owner as usize;
        if let Some(shape) = self.get(owner).shape() {
            match self.previous[index] {
                Some(previous) => self.next[(previous.get() - 1) as usize] = self.next[index],
                None => self.heads[shape as usize] = self.next[index],
            }
            if let Some(next) = self.next[index] { self.previous[(next.get() - 1) as usize] = self.previous[index]; }
        }
        let length = self.targets.len().max(index + 1);
        self.targets.resize(length, TargetShape::NONE);
        self.next.resize(length, None);
        self.previous.resize(length, None);
        self.targets[index] = target;
        self.next[index] = None;
        self.previous[index] = None;
        if let Some(shape) = target.shape() {
            self.heads.resize(self.heads.len().max(shape as usize + 1), None);
            let link = NonZeroU32::new(owner.checked_add(1).expect("shape owner fits a link"));
            let head = &mut self.heads[shape as usize];
            self.next[index] = *head;
            if let Some(next) = *head { self.previous[(next.get() - 1) as usize] = link; }
            *head = link;
        }
    }

    #[inline(always)]
    pub(super) fn join(&mut self, owner: u32, incoming: TargetShape) -> bool {
        let held = self.get(owner);
        if held == incoming || held == TargetShape::ANY { return false; }
        let joined = held.join(incoming);
        if joined == held { return false; }
        self.set(owner, joined);
        true
    }

    pub(super) fn reset_owner(&mut self, owner: u32) {
        if self.get(owner) != TargetShape::NONE { self.set(owner, TargetShape::NONE); }
    }

    #[inline(always)]
    pub(super) fn depart(&mut self, shape: u32, mut retire: impl FnMut(u32)) {
        while let Some(owner) = self.heads.get(shape as usize).copied().flatten() {
            let owner = owner.get() - 1;
            self.set(owner, TargetShape::ANY);
            retire(owner);
        }
    }

    pub(super) fn reset(&mut self) {
        self.targets.clear(); self.heads.clear(); self.next.clear(); self.previous.clear();
    }

    pub(super) fn memory_bytes(&self) -> usize {
        self.targets.capacity() * size_of::<TargetShape>()
            + (self.heads.capacity() + self.next.capacity() + self.previous.capacity()) * size_of::<Link>()
    }
}
