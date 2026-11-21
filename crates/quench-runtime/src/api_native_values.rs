//! `NativeContext` reads and writes of BigInt, Date and ArrayBuffer detach state for hosts
//! (Node-API); none of them runs guest code.
use super::{NativeContext, RootedError};
use crate::{Host, RootId};

/// What `NativeContext::detach_array_buffer_rooted` did.
pub use crate::vm::Detach;

impl NativeContext<'_> {
    /// A BigInt's sign (true when negative) and magnitude as little-endian 64-bit words;
    /// `None` when `value` is not a BigInt.
    pub fn bigint_words_rooted(&mut self, value: RootId) -> Result<Option<(bool, Vec<u64>)>, RootedError> {
        let value = self.vm.embedding_value(value).map_err(|error| self.error(error))?;
        Ok(self.vm.bigint_words_embedding(value))
    }

    /// The BigInt `-magnitude` when `negative`, else `magnitude`, from little-endian words.
    pub fn bigint_from_words_rooted(&mut self, negative: bool, words: &[u64]) -> RootId {
        let integer = self.vm.bigint_from_words_embedding(negative, words);
        let root = self.vm.root(integer);
        self.roots.push(root);
        root
    }

    /// A Date whose time value is TimeClip(`milliseconds`).
    pub fn date_rooted(&mut self, milliseconds: f64) -> RootId {
        let date = self.vm.date_embedding(milliseconds);
        let root = self.vm.root(date);
        self.roots.push(root);
        root
    }

    /// A Date's time value; `None` when `value` is not a Date.
    pub fn date_value_rooted(&mut self, value: RootId) -> Result<Option<f64>, RootedError> {
        let value = self.vm.embedding_value(value).map_err(|error| self.error(error))?;
        Ok(self.vm.date_value_embedding(value))
    }

    /// DetachArrayBuffer(`value`) with an undefined key.
    pub fn detach_array_buffer_rooted(&mut self, value: RootId) -> Result<Detach, RootedError> {
        let value = self.vm.embedding_value(value).map_err(|error| self.error(error))?;
        Ok(self.vm.detach_array_buffer_embedding(value))
    }

    /// Whether `value` is a detached ArrayBuffer.
    pub fn array_buffer_detached_rooted(&mut self, value: RootId) -> Result<bool, RootedError> {
        let value = self.vm.embedding_value(value).map_err(|error| self.error(error))?;
        Ok(self.vm.array_buffer_detached_embedding(value))
    }
}
