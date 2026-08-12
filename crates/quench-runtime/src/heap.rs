use crate::identity::HeapRef;

#[derive(Debug)]
pub struct HeapArena<T> {
    values: Vec<Option<T>>,
    free: Vec<u32>,
}

impl<T> Default for HeapArena<T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> HeapArena<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allocate(&mut self, value: T) -> HeapRef {
        if let Some(index) = self.free.pop() {
            self.values[index as usize] = Some(value);
            return HeapRef(index);
        }
        let index = u32::try_from(self.values.len()).unwrap_or(u32::MAX);
        self.values.push(Some(value));
        HeapRef(index)
    }

    pub fn get(&self, reference: HeapRef) -> Option<&T> {
        self.values
            .get(reference.0 as usize)
            .and_then(Option::as_ref)
    }

    pub fn get_mut(&mut self, reference: HeapRef) -> Option<&mut T> {
        self.values
            .get_mut(reference.0 as usize)
            .and_then(Option::as_mut)
    }

    pub fn reclaim(&mut self, reference: HeapRef) -> Option<T> {
        let value = self.values.get_mut(reference.0 as usize)?.take()?;
        self.free.push(reference.0);
        Some(value)
    }

    pub fn live_len(&self) -> usize {
        self.values.iter().filter(|value| value.is_some()).count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifetimeDomain {
    Realm,
    Module,
    Request,
    Temporary,
    Continuation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSet {
    domain: LifetimeDomain,
    refs: Vec<HeapRef>,
}

impl RootSet {
    pub fn new(domain: LifetimeDomain) -> Self {
        Self {
            domain,
            refs: Vec::new(),
        }
    }

    pub fn domain(&self) -> LifetimeDomain {
        self.domain
    }

    pub fn insert(&mut self, reference: HeapRef) {
        if !self.refs.contains(&reference) {
            self.refs.push(reference);
        }
    }

    pub fn remove(&mut self, reference: HeapRef) {
        self.refs.retain(|candidate| *candidate != reference);
    }

    pub fn iter(&self) -> impl Iterator<Item = HeapRef> + '_ {
        self.refs.iter().copied()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RootRegistry {
    sets: Vec<RootSet>,
}

impl RootRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn roots(&self, domain: LifetimeDomain) -> impl Iterator<Item = HeapRef> + '_ {
        self.sets
            .iter()
            .filter(move |set| set.domain() == domain)
            .flat_map(RootSet::iter)
    }

    pub fn all_roots(&self) -> impl Iterator<Item = HeapRef> + '_ {
        self.sets.iter().flat_map(RootSet::iter)
    }

    pub fn contains(&self, domain: LifetimeDomain, reference: HeapRef) -> bool {
        self.sets
            .iter()
            .find(|set| set.domain() == domain)
            .is_some_and(|set| set.iter().any(|candidate| candidate == reference))
    }

    pub fn add(&mut self, domain: LifetimeDomain, reference: HeapRef) {
        self.set(domain).insert(reference);
    }

    pub fn remove(&mut self, domain: LifetimeDomain, reference: HeapRef) {
        let Some(index) = self.sets.iter().position(|set| set.domain() == domain) else {
            return;
        };
        self.sets[index].remove(reference);
        if self.sets[index].iter().next().is_none() {
            self.sets.remove(index);
        }
    }

    pub fn clear(&mut self, domain: LifetimeDomain) {
        self.sets.retain(|set| set.domain() != domain);
    }

    fn set(&mut self, domain: LifetimeDomain) -> &mut RootSet {
        if let Some(index) = self.sets.iter().position(|set| set.domain() == domain) {
            return &mut self.sets[index];
        }
        let index = self.sets.len();
        self.sets.push(RootSet::new(domain));
        &mut self.sets[index]
    }
}

#[cfg(test)]
mod tests {
    use super::{HeapArena, LifetimeDomain, RootRegistry};
    use crate::identity::HeapRef;

    #[test]
    fn root_domains_are_enumerable_and_reclaimable() {
        let mut registry = RootRegistry::new();
        registry.add(LifetimeDomain::Realm, HeapRef(1));
        registry.add(LifetimeDomain::Request, HeapRef(2));
        registry.add(LifetimeDomain::Request, HeapRef(2));
        assert!(registry.contains(LifetimeDomain::Request, HeapRef(2)));
        assert_eq!(registry.all_roots().count(), 2);
        registry.clear(LifetimeDomain::Request);
        assert!(!registry.contains(LifetimeDomain::Request, HeapRef(2)));
        assert_eq!(registry.all_roots().collect::<Vec<_>>(), vec![HeapRef(1)]);
        registry.remove(LifetimeDomain::Realm, HeapRef(1));
        assert_eq!(registry.all_roots().count(), 0);
    }

    #[test]
    fn arena_reuses_reclaimed_heap_references() {
        let mut arena = HeapArena::new();
        let first = arena.allocate(String::from("first"));
        assert_eq!(arena.get(first).map(String::as_str), Some("first"));
        assert_eq!(arena.reclaim(first).as_deref(), Some("first"));
        let second = arena.allocate(String::from("second"));
        assert_eq!(first, second);
        if let Some(value) = arena.get_mut(second) {
            value.push('!');
        }
        assert_eq!(arena.get(second).map(String::as_str), Some("second!"));
        assert_eq!(arena.live_len(), 1);
    }
}
