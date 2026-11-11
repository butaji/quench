//! A table of per-realm values, searched by realm global in place: a program runs in one
//! realm almost always, so the lookup is one compare, where a hash costs its own instructions.
use super::Value;

#[derive(Clone)]
pub(super) struct RealmTable<T>(Vec<(Value, T)>);

impl<T> Default for RealmTable<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> RealmTable<T> {
    #[inline(always)]
    pub(super) fn get(&self, realm: &Value) -> Option<&T> {
        self.0.iter().find(|(known, _)| known == realm).map(|(_, value)| value)
    }

    #[inline(always)]
    pub(super) fn get_mut(&mut self, realm: &Value) -> Option<&mut T> {
        self.0.iter_mut().find(|(known, _)| known == realm).map(|(_, value)| value)
    }

    pub(super) fn insert(&mut self, realm: Value, value: T) {
        match self.get_mut(&realm) {
            Some(held) => *held = value,
            None => self.0.push((realm, value)),
        }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&Value, &T)> {
        self.0.iter().map(|(realm, value)| (realm, value))
    }
}

impl<T> std::ops::Index<&Value> for RealmTable<T> {
    type Output = T;

    fn index(&self, realm: &Value) -> &T {
        self.get(realm).expect("the realm has an entry")
    }
}

impl<T: crate::snapshot::Snap> crate::snapshot::Snap for RealmTable<T> {
    fn snap(&self, writer: &mut crate::snapshot::Writer) -> crate::snapshot::SnapshotResult<()> {
        self.0.snap(writer)
    }

    fn unsnap(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        Ok(Self(Vec::unsnap(reader)?))
    }
}
