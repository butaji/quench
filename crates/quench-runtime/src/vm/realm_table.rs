//! A table keyed by realm (its global object): a program has one realm, or a few, so the
//! lookup on every builtin call that reads its realm's intrinsics is a compare of the first
//! key, where a hash map hashes and probes.
use crate::value::Value;

/// Entries in insertion order, the first realm first.
pub(super) struct RealmTable<V> {
    entries: Vec<(Value, V)>,
}

impl<V> Default for RealmTable<V> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<V> RealmTable<V> {
    #[inline(always)]
    pub(super) fn get(&self, realm: &Value) -> Option<&V> {
        self.entries.iter().find(|(key, _)| key == realm).map(|(_, value)| value)
    }

    #[inline(always)]
    pub(super) fn get_mut(&mut self, realm: &Value) -> Option<&mut V> {
        self.entries.iter_mut().find(|(key, _)| key == realm).map(|(_, value)| value)
    }

    /// Sets `realm`'s entry, returning the one it replaces.
    pub(super) fn insert(&mut self, realm: Value, value: V) -> Option<V> {
        match self.get_mut(&realm) {
            Some(held) => Some(std::mem::replace(held, value)),
            None => {
                self.entries.push((realm, value));
                None
            }
        }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&Value, &V)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}

impl<V> std::ops::Index<&Value> for RealmTable<V> {
    type Output = V;

    fn index(&self, realm: &Value) -> &V {
        self.get(realm).expect("the realm has an entry")
    }
}

impl<V: crate::snapshot::Snap> crate::snapshot::Snap for RealmTable<V> {
    fn snap(&self, writer: &mut crate::snapshot::Writer) -> crate::snapshot::SnapshotResult<()> {
        self.entries.snap(writer)
    }

    fn unsnap(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        Ok(Self { entries: Vec::unsnap(reader)? })
    }
}
