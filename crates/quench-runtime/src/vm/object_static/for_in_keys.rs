//! The own keys a for-in last listed from a receiver's shape (`Vm::for_in_own_key_listed`).

use super::super::Value;

/// The own keys a for-in last listed from a receiver's shape (`Vm::own_for_in_keys`), with
/// the receiver, that shape and the collection epoch: while the receiver keeps the shape,
/// in the same epoch (which no reused handle or compacted shape shares), each listed key is
/// still an own enumerable property (`Vm::for_in_own_key_listed`). `next` is the index of
/// the key the loop checks next, as it checks them in listed order.
pub(in crate::vm) struct ForInOwnKeys {
    object: Value,
    shape: u32,
    epoch: u64,
    keys: Vec<Value>,
    next: std::cell::Cell<usize>,
}

impl ForInOwnKeys {
    /// Whether `key` is listed: the one after the last key checked, as a loop checking its
    /// keys in turn asks, without a scan; any other key by a scan.
    fn lists_key(&self, key: Value) -> bool {
        let next = self.next.get();
        let position = if self.keys.get(next) == Some(&key) {
            Some(next)
        } else {
            self.keys.iter().position(|&listed| listed == key)
        };
        position.inspect(|&index| self.next.set(index + 1)).is_some()
    }

    pub(in crate::vm) fn new(object: Value, shape: u32, epoch: u64, keys: Vec<Value>) -> Self {
        Self { object, shape, epoch, keys, next: std::cell::Cell::new(0) }
    }

    /// Whether `key` is listed for `object` while it keeps the listed shape in the listed
    /// collection epoch.
    pub(in crate::vm) fn lists(&self, object: Value, shape: u32, epoch: u64, key: Value) -> bool {
        self.object == object && self.shape == shape && self.epoch == epoch && self.lists_key(key)
    }
}
