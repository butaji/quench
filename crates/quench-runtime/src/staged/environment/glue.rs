#![allow(unused_imports)]
use super::*;
use crate::staged::{Concrete, Stage};
use crate::staged::region::Region;

#[cfg(test)]
/// The interpreter's `environment_slot_open` over the storage at `storage`: whether it wrote.
///
/// # Safety
/// As `concrete_environment_slot_store`.
#[inline(always)]
pub unsafe fn concrete_environment_slot_open(storage: *mut crate::heap::EnvironmentSlots, slot: u16) -> bool {
    let mut stage = Concrete::pure();
    environment_slot_open(&mut stage, storage as u64, slot, ()).is_some()
}

/// The interpreter's `environment_slot_define` over the storage at `storage`: whether it wrote.
///
/// # Safety
/// As `concrete_environment_slot_store`.
#[inline(always)]
pub unsafe fn concrete_environment_slot_define(storage: *mut crate::heap::EnvironmentSlots, slot: u16, value: crate::Value) -> bool {
    let mut stage = Concrete::pure();
    environment_slot_define(&mut stage, storage as u64, slot, value.to_bits(), ()).is_some()
}

#[cfg(test)]
/// The interpreter's `environment_slot` over the storage at `storage`.
///
/// # Safety
/// `storage` addresses a live `EnvironmentSlots`.
#[inline(always)]
pub unsafe fn concrete_environment_slot(storage: *const crate::heap::EnvironmentSlots, slot: u16) -> Option<crate::Value> {
    let mut stage = Concrete::pure();
    environment_slot(&mut stage, storage as u64, slot, ()).map(crate::Value::from_bits)
}

/// The interpreter's `environment_slot_store` over the storage at `storage`: whether it wrote.
///
/// # Safety
/// `storage` addresses a live `EnvironmentSlots` no other reference reads, and the caller owns the store's barrier.
#[inline(always)]
pub unsafe fn concrete_environment_slot_store(storage: *mut crate::heap::EnvironmentSlots, slot: u16, value: crate::Value) -> bool {
    let mut stage = Concrete::pure();
    environment_slot_store(&mut stage, storage as u64, slot, value.to_bits(), ()).is_some()
}
