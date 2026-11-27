//! A per-realm table: one entry per realm, keyed by the realm's global object. A program has
//! one realm, rarely a few (ShadowRealm, `$262.createRealm`), so a lookup is a compare of the
//! first key, where a hash map hashed the key and probed its control bytes on every builtin
//! call that consults a realm's intrinsics (a RegExp call read three of them).
use crate::snapshot::{Reader, Snap, SnapshotResult, Writer};
use crate::value::Value;

pub(crate) struct RealmTable<T>(Vec<(Value, T)>);

impl<T> Default for RealmTable<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> RealmTable<T> {
    #[inline(always)]
    pub(crate) fn get(&self, realm: &Value) -> Option<&T> {
        self.0.iter().find(|(key, _)| key == realm).map(|(_, value)| value)
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, realm: &Value) -> Option<&mut T> {
        self.0.iter_mut().find(|(key, _)| key == realm).map(|(_, value)| value)
    }

    /// Sets `realm`'s entry, returning the one it replaces.
    pub(crate) fn insert(&mut self, realm: Value, value: T) -> Option<T> {
        match self.get_mut(&realm) {
            Some(slot) => Some(std::mem::replace(slot, value)),
            None => {
                self.0.push((realm, value));
                None
            }
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&Value, &T)> {
        self.0.iter().map(|(key, value)| (key, value))
    }

    /// Keeps the entries `keep` accepts (`FxHashMap::retain`'s shape).
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&Value, &mut T) -> bool) {
        self.0.retain_mut(|(key, value)| keep(key, value));
    }
}

impl<T> std::ops::Index<&Value> for RealmTable<T> {
    type Output = T;
    fn index(&self, realm: &Value) -> &T {
        self.get(realm).expect("realm has an entry")
    }
}

/// The same bytes a map writes: the count, then each key and value.
impl<T: Snap> Snap for RealmTable<T> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.len(self.0.len());
        self.0.iter().try_for_each(|(key, value)| {
            key.snap(writer)?;
            value.snap(writer)
        })
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let len = reader.len()?;
        let mut table = Self::default();
        for _ in 0..len {
            table.insert(Value::unsnap(reader)?, T::unsnap(reader)?);
        }
        Ok(table)
    }
}
