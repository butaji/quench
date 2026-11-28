#![allow(unused_imports)]
use super::*;
use crate::staged::Concrete;
use crate::staged::typed_elements::{NativeKind, TypedView, ViewKind};

/// The interpreter's view of the typed array `base` among the record pages of `directory`: the proof
/// without a frame; `None` for any other value, a record the VM has not published (or has
/// withdrawn) or one whose published address is stale.
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn concrete_typed_view(directory: (*const usize, usize), base: crate::Value) -> Option<TypedView<u64>> {
    concrete_typed_access(directory, base, ViewAccess::READ)
}

/// The interpreter's view of the typed array `base` proved for `access` (a kind the site observed,
/// and whether it stores), among the record pages of `directory`: the element row's proof without
/// a frame, a store's `unshared` test included.
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn concrete_typed_access((directory, pages): (*const usize, usize), base: crate::Value, access: ViewAccess)
    -> Option<TypedView<u64>> {
    let relocations = crate::heap::buffer_relocations_address();
    let records = TypedRecords { directory: directory.addr() as u64, pages: pages as u64, relocations: relocations.addr() as u64 };
    prove_typed_view(&mut Concrete::pure(), records, base.to_bits(), access, ())
}
