use crate::identity::HeapRef;

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
        if let Some(set) = self.sets.iter_mut().find(|set| set.domain() == domain) {
            set.remove(reference);
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
    use super::{LifetimeDomain, RootRegistry};
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
    }
}
