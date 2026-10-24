//! Generation-owned native digest registry and independent raw native inventory.
use super::crypto_digest::{supported_names, Digest, DigestError};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Default)]
pub(crate) struct Registry {
    generation: Option<u64>,
    first_id: u32,
    next_id: u32,
    entries: Vec<Arc<Digest>>,
    aliases: BTreeMap<String, (String, i32)>,
    inventory: Vec<String>,
}
impl Registry {
    fn insert(&mut self, name: &str) -> Result<Arc<Digest>, DigestError> {
        let name = name.split('\0').next().unwrap_or("");
        let fetched = Digest::fetch(name)?;
        if let Some((_, id)) = self.aliases.get(&name.to_ascii_lowercase()) {
            return self
                .get(*id, self.generation.ok_or(DigestError::Unsupported)?)
                .ok_or(DigestError::Unsupported);
        }
        self.insert_owner(name, fetched)
    }
    fn alias(&mut self, name: &str, id: i32) {
        // Case-insensitive native map retains the spelling of its first key.
        self.aliases
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| (name.into(), id))
            .1 = id;
    }
    fn insert_owner(&mut self, name: &str, fetched: Digest) -> Result<Arc<Digest>, DigestError> {
        let index = if let Some(index) = self
            .entries
            .iter()
            .position(|digest| digest.identity() == fetched.identity())
        {
            index
        } else {
            if self.next_id == u32::MAX {
                return Err(DigestError::Allocation);
            }
            let index = self.entries.len();
            let id = self.next_id as i32;
            self.next_id += 1;
            for alias in fetched.aliases() {
                self.alias(&alias, id);
            }
            self.entries.push(Arc::new(fetched));
            index
        };
        let id = (self.first_id + index as u32) as i32;
        self.alias(name, id);
        Ok(self.entries[index].clone())
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
                first_id: self.next_id,
                next_id: self.next_id,
                ..Self::default()
            };
            for name in supported_names() {
                if let Some(digest) = Digest::fetch_preserving_errors(&name) {
                    next.insert_owner(&name, digest)?;
                    next.inventory.push(name);
                }
            }
            if before != read_generation() {
                continue;
            }
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
    pub(crate) fn resolve_cached(
        &mut self,
        name: &str,
        id: Option<i32>,
        read_generation: impl Fn() -> u64,
    ) -> Result<(Arc<Digest>, i32), DigestError> {
        loop {
            self.synchronize(&read_generation)?;
            let before = self.generation.ok_or(DigestError::Unsupported)?;
            if let Some((digest, id)) = id
                .filter(|id| *id != -1)
                .and_then(|id| self.get(id, before).map(|digest| (digest, id)))
            {
                if before == read_generation() {
                    return Ok((digest, id));
                }
                continue;
            }
            let digest = self.resolve(name, &read_generation)?;
            let index = self
                .entries
                .iter()
                .position(|entry| entry.identity() == digest.identity())
                .ok_or(DigestError::Unsupported)?;
            let id = (self.first_id + index as u32) as i32;
            if self.generation == Some(read_generation()) {
                return Ok((digest, id));
            }
        }
    }
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.inventory.iter().map(String::as_str)
    }
    pub(crate) fn aliases(&self) -> impl Iterator<Item = (&str, i32)> {
        self.aliases.values().map(|(name, id)| (name.as_str(), *id))
    }
    pub(crate) fn get(&self, id: i32, generation: u64) -> Option<Arc<Digest>> {
        if self.generation != Some(generation) {
            return None;
        }
        (id as u32)
            .checked_sub(self.first_id)
            .and_then(|index| self.entries.get(index as usize))
            .cloned()
    }
}
