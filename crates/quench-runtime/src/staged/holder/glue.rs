//! The interpreter's readings of the holder and own-constant rows through `Concrete::pure`:
//! the `concrete_*` wrappers of `holder.rs`, which name the runtime `Object` and `PublishedCode`.

#![allow(unused_imports)]
use super::*;

#[cfg(not(stencil_build))]
/// The interpreter's reading of `holder_read`.
///
/// # Safety
/// `watch` is the address of a live watch (`Rc::as_ptr` of a hint's), which outlives the call.
#[inline(always)]
pub unsafe fn concrete_holder_read(record: &crate::heap::Object, guard: u64,
    watch: *const crate::code_dependencies::PublishedCode<()>, value: crate::Value) -> Option<crate::Value> {
    let mut stage = crate::staged::Concrete::pure();
    holder_read(&mut stage, record as *const crate::heap::Object as u64, guard, watch as u64, value.to_bits(), ())
        .map(crate::Value::from_bits)
}

#[cfg(not(stencil_build))]
/// The interpreter's reading of `own_constant_read` for the object part `record`.
///
/// # Safety
/// `watch` is the address of a live watch (`Rc::as_ptr` of a hint's), which outlives the call.
#[inline(always)]
pub unsafe fn concrete_own_constant_read(record: &crate::heap::Object, expected: u64,
    watch: *const crate::code_dependencies::PublishedCode<()>, value: crate::Value) -> Option<crate::Value> {
    let mut stage = crate::staged::Concrete::pure();
    crate::staged::ic::own_constant_read(&mut stage, record as *const crate::heap::Object as u64, expected, watch as u64, value.to_bits(), ())
        .map(crate::Value::from_bits)
}
