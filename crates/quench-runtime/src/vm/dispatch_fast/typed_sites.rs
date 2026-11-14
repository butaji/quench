//! Typed-array index sites specialized by element kind. A site that read or stored an element
//! proved its receiver's kind, which the handler then holds as a constant (`TYPED_ARRAYS` row):
//! the element's width and conversion are decided here, once. The lane run proves the
//! receiver's bytes once too (`Heap::prove_typed_storage`), so that an access is the proof's
//! base, kind and bound compared and one load or store (`Vm::proved_element`,
//! `Vm::proved_store`). A receiver no proof covers takes the checked entries
//! (`Vm::typed_element_at`, `Vm::typed_element_set`), and any other access, or a kind without
//! a handler, the general typed handler.
use super::*;
use crate::heap::{TYPED_ARRAYS, TypedArrayKind as K};

/// The handler `site` instantiates for `$kind`, or `$general` for a kind with no instantiation.
macro_rules! by_kind {
    ($kind:expr, $site:ident, $general:ident) => {
        match $kind {
            K::Uint8 => $site::<H, { K::Uint8 as usize }> as Handler,
            K::Uint8Clamped => $site::<H, { K::Uint8Clamped as usize }> as Handler,
            K::Uint16 => $site::<H, { K::Uint16 as usize }> as Handler,
            K::Uint32 => $site::<H, { K::Uint32 as usize }> as Handler,
            K::Int8 => $site::<H, { K::Int8 as usize }> as Handler,
            K::Int16 => $site::<H, { K::Int16 as usize }> as Handler,
            K::Int32 => $site::<H, { K::Int32 as usize }> as Handler,
            K::Float32 => $site::<H, { K::Float32 as usize }> as Handler,
            K::Float64 => $site::<H, { K::Float64 as usize }> as Handler,
            K::BigInt64 | K::BigUint64 | K::Float16 => $general::<H> as Handler,
        }
    };
}

/// The handler of a site that read an element of a typed array of `kind`.
pub(super) fn read_handler<H: Host>(kind: K) -> Handler {
    by_kind!(kind, lane_get_index_typed_kind, lane_get_index_typed)
}

/// The handler of a site that stored a Number into a typed array of `kind`.
pub(super) fn store_handler<H: Host>(kind: K) -> Handler {
    by_kind!(kind, lane_set_index_typed_kind, lane_set_index_typed)
}

/// A non-negative int32 index, as `Vm::array_index_key` reads one.
#[inline(always)]
fn int32_index(key: Value) -> Option<u32> {
    u32::try_from(key.as_int()?).ok()
}

/// `lane_get_index_typed_kind`'s read where the lane run proved no bytes of `base`: it proves
/// them, or reads through the checked entry.
#[inline(never)]
fn read_unproved<H: Host, const KIND: usize>(cx: *mut LaneContext<H>, register: u16, base: Value, index: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if vm.typed_array_kind(base) != Some(TYPED_ARRAYS[KIND].kind) {
        return None;
    }
    // SAFETY: `cx` is the running lane's context.
    let proofs = unsafe { &mut (*cx).proofs };
    if vm.heap.prove_typed_storage(proofs, register, base)
        && let Some(elements) = proofs.typed_elements::<KIND>(register, base)
    {
        return Some(Vm::<H>::proved_element::<KIND>(elements, index));
    }
    vm.typed_element_at::<KIND>(base, index)
}

/// `lane_set_index_typed_kind`'s store where the lane run proved no writable bytes of `base`.
#[inline(never)]
fn store_unproved<H: Host, const KIND: usize>(cx: *mut LaneContext<H>, register: u16, base: Value, index: u32, value: Value) -> bool {
    let vm = unsafe { vm(cx) };
    if vm.typed_array_kind(base) != Some(TYPED_ARRAYS[KIND].kind) {
        return false;
    }
    // SAFETY: `cx` is the running lane's context.
    let proofs = unsafe { &mut (*cx).proofs };
    if vm.heap.prove_typed_storage(proofs, register, base)
        && let Some(elements) = proofs.typed_elements::<KIND>(register, base)
        && Vm::<H>::proved_store::<KIND>(elements, index, value)
    {
        return true;
    }
    vm.typed_element_set::<KIND>(base, index, value)
}

script_handler! {
    /// `lane_get_index_typed` at a site of the kind `KIND`: the element at an int32 index,
    /// or `undefined` past the end; any other read takes `lane_get_index_typed`.
    fn lane_get_index_typed_kind<KIND: usize>(cx, r, ip, i, view) {
        let (base, key) = (r.get(i.b), r.get(i.c));
        // SAFETY: `cx` is the running lane's context.
        let proofs = unsafe { &(*cx).proofs };
        let value = int32_index(key).and_then(|index| match proofs.typed_elements::<KIND>(i.b, base) {
            Some(elements) => Some(Vm::<H>::proved_element::<KIND>(elements, index)),
            None => read_unproved::<H, KIND>(cx, i.b, base, index),
        });
        let Some(value) = value else {
            sibling!(lane_get_index_typed::<H>, r, ip, view)
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_set_index_typed` at a site of the kind `KIND`: a Number stored at an int32 index,
    /// or nothing past the end; any other store takes `lane_set_index_typed`.
    fn lane_set_index_typed_kind<KIND: usize>(cx, r, ip, i, view) {
        let (base, key, value) = (r.get(i.b), r.get(i.c), r.get(i.a));
        // SAFETY: `cx` is the running lane's context.
        let proofs = unsafe { &(*cx).proofs };
        let stored = int32_index(key).is_some_and(|index| match proofs.typed_elements::<KIND>(i.b, base) {
            Some(elements) if Vm::<H>::proved_store::<KIND>(elements, index, value) => true,
            _ => store_unproved::<H, KIND>(cx, i.b, base, index, value),
        });
        if !stored {
            sibling!(lane_set_index_typed::<H>, r, ip, view)
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}
