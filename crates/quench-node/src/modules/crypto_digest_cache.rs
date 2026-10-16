//! Opaque SDK IDs index retained, fetched native Md owners, never synthetic keys.
use super::crypto_digest::{Digest, DigestError, supported_names};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Default)]
pub(crate) struct Registry {
    generation: Option<u64>,
    entries: Vec<Arc<Digest>>,
    aliases: BTreeMap<String, i32>,
}
impl Registry {
    fn insert(&mut self, name: &str) -> Result<Arc<Digest>, DigestError> {
        if let Some(id) = self.aliases.get(name) {
            return self
                .get(*id, self.generation.ok_or(DigestError::Unsupported)?)
                .ok_or(DigestError::Unsupported);
        }
        let fetched = Digest::fetch(name)?;
        self.insert_owner(name, fetched)
    }
    fn insert_owner(&mut self, name: &str, fetched: Digest) -> Result<Arc<Digest>, DigestError> {
        let id = if let Some(id) = self
            .entries
            .iter()
            .position(|digest| digest.identity() == fetched.identity())
        {
            id
        } else {
            let id = self.entries.len();
            self.entries.push(Arc::new(fetched));
            id
        };
        let id = i32::try_from(id).map_err(|_| DigestError::Allocation)?;
        self.aliases.insert(name.into(), id);
        Ok(self.entries[id as usize].clone())
    }
    pub(crate) fn synchronize(
        &mut self,
        read_generation: impl Fn() -> u64,
    ) -> Result<(), DigestError> {
        loop {
            let before = read_generation();
            if self.generation == Some(before) {
                return Ok(());
            }
            let mut next = Self {
                generation: Some(before),
                ..Self::default()
            };
            for name in supported_names() {
                if let Some(digest) = Digest::fetch_preserving_errors(&name) {
                    next.insert_owner(&name, digest)?;
                }
            }
            if before != read_generation() {
                continue;
            }
            next.generation = Some(before);
            *self = next;
            return Ok(());
        }
    }
    pub(crate) fn resolve(
        &mut self,
        name: &str,
        read_generation: impl Fn() -> u64,
    ) -> Result<Arc<Digest>, DigestError> {
        loop {
            self.synchronize(&read_generation)?;
            let before = self.generation;
            let result = self.insert(name);
            if before == Some(read_generation()) {
                return result;
            }
        }
    }
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.aliases.keys().map(String::as_str)
    }
    pub(crate) fn aliases(&self) -> impl Iterator<Item = (&str, i32)> {
        self.aliases.iter().map(|(name, id)| (name.as_str(), *id))
    }
    /// IDs are usable only in this generation. Cached jobs retain Arc<Md> directly
    /// so a later registry refresh cannot invalidate an in-flight digest operation.
    pub(crate) fn get(&self, id: i32, generation: u64) -> Option<Arc<Digest>> {
        (self.generation == Some(generation))
            .then(|| {
                usize::try_from(id)
                    .ok()
                    .and_then(|id| self.entries.get(id).cloned())
            })
            .flatten()
    }
}
