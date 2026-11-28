//! The concrete element writes and counts the interpreter runs on raw backing pointers.

use super::*;

/// `concrete_replace_element` of the elements at `first + step * k` by `values`'s items, the
/// backing's count of others recounted once for all of them.
///
/// # Safety
/// As `concrete_replace_element`, for every index.
#[inline(always)]
pub unsafe fn concrete_replace_elements(backing: *mut crate::Value, first: isize, step: isize, values: impl Iterator<Item = crate::Value>) {
    let mut stage = Concrete::pure();
    let others = stage.load_u32(Region::Elements.mutable(), backing as u64, crate::heap::OTHERS_FROM_VALUES);
    let counted = others != u64::from(crate::heap::OTHERS_UNKNOWN);
    let (mut added, mut removed) = (0u64, 0u64);
    for (k, value) in values.enumerate() {
        // SAFETY: the caller's contract.
        let address = unsafe { backing.offset(first + step * k as isize) };
        if counted {
            added += other(&mut stage, value.to_bits());
            removed += other(&mut stage, unsafe { address.read() }.to_bits());
        }
        replace_uncounted_element(&mut stage, address as u64, value.to_bits());
    }
    if added != removed {
        let others = recount(&mut stage, others, added, removed);
        stage.store_u32(Region::Elements.mutable(), backing as u64, crate::heap::OTHERS_FROM_VALUES, others);
    }
}

/// `concrete_replace_elements` of the contiguous elements from `first` by `values`, in
/// passes the compiler vectorizes: count the others leaving and arriving, then copy.
///
/// # Safety
/// As `concrete_replace_element`, for every index.
#[inline(always)]
pub unsafe fn concrete_replace_run(backing: *mut crate::Value, first: usize, values: &[u64]) {
    let mut stage = Concrete::pure();
    // SAFETY: the caller's contract.
    let elements = unsafe { std::slice::from_raw_parts_mut(backing.add(first).cast::<u64>(), values.len()) };
    let others = stage.load_u32(Region::Elements.mutable(), backing as u64, crate::heap::OTHERS_FROM_VALUES);
    if others != u64::from(crate::heap::OTHERS_UNKNOWN) {
        let count = |bits: &[u64]| bits.iter().map(|&bits| other(&mut Concrete::pure(), bits)).sum::<u64>();
        let (added, removed) = (count(values), count(elements));
        if added != removed {
            let others = recount(&mut stage, others, added, removed);
            stage.store_u32(Region::Elements.mutable(), backing as u64, crate::heap::OTHERS_FROM_VALUES, others);
        }
    }
    elements.copy_from_slice(values);
}

/// The interpreter's reading of `other`.
#[inline(always)]
pub fn concrete_other(value: crate::Value) -> u32 {
    other(&mut Concrete::pure(), value.to_bits()) as u32
}

/// The interpreter's reading of `recount`.
#[inline(always)]
pub fn concrete_recount(others: u32, added: u32, removed: u32) -> u32 {
    recount(&mut Concrete::pure(), u64::from(others), u64::from(added), u64::from(removed)) as u32
}

/// Element `index` of the direct array `base` among the array records at `record_base`, of any
/// backing: a wide one's value, a double backing's Number (`Stage::box_raw_number`) or a
/// narrow backing's value decoded (`object::decode_narrow`); `None` (leaving at `miss`) for any
/// other base, past the elements or at a hole.
#[inline(always)]
pub fn any_backing_element<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    use crate::heap::{DOUBLE_LEN, ENCODED_LEN};
    let Backing { values, length, .. } = flagged_backing(stage, record_base, base, miss, BackingAccess::MUTABLE)?;
    // A narrow or double backing flags one of its length's top two bits (`ArrayElements::is_encoded`).
    let encoded = stage.ugt_imm(length, u64::from(!ENCODED_LEN));
    stage.choose_leaving(encoded, #[inline(always)] |stage| {
        let count = stage.and_imm(length, u64::from(!ENCODED_LEN));
        let in_bounds = stage.ult(index, count);
        stage.guard(in_bounds, miss)?;
        let flag = stage.and_imm(length, u64::from(DOUBLE_LEN));
        let double = stage.ne_imm(flag, 0u32);
        stage.choose_leaving(double, #[inline(always)] |stage| double_element(stage, values, index, miss), #[inline(always)] |stage| {
            // A narrow backing (`ArrayElements::is_narrow`): the element decoded from its narrow form.
            let offset = stage.uextend32(index);
            let offset = stage.mul_imm(offset, std::mem::size_of::<u32>() as u64);
            let address = stage.add(values, offset);
            let narrow = stage.load_u32(Region::Elements.mutable(), address, 0);
            let value = super::super::object::decode_narrow(stage, narrow);
            let present = stage.ne_imm(value, crate::Value::DELETED.to_bits());
            stage.guard(present, miss)?;
            Some(value)
        })
    }, #[inline(always)] |stage| dense_element(stage, values, length, index, miss).map(|(_, value)| value))
}
