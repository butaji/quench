//! A packed integer field of an object record whose entry facts a proved region established
//! once (`vm::dispatch_fast::region_state`): its record's exact shape, its plain inline
//! storage, and an integer in the slot. The rows are the packed read and store of `object` and
//! `store` with those guards removed and the integer arms alone, so they run from the same
//! layout constants as every tier's.

use super::object::decode_narrow_integer;
use super::store::{narrow_integer, HALF_BYTES};
use super::region::Region;
use super::Stage;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// The address of packed slot `slot` of the record at `record`.
#[inline(always)]
fn half<S: Stage>(stage: &mut S, record: S::Word, slot: u16) -> S::Word {
    let offset = stage.site(OBJECT.payload_offset as u64 + u64::from(slot) * HALF_BYTES);
    stage.add_imm(record, offset)
}

/// The int32 word the packed integer in `slot` of the record at `record` holds.
#[inline(always)]
pub(crate) fn read_integer<S: Stage>(stage: &mut S, record: S::Word, slot: u16) -> S::Word {
    let address = half(stage, record, slot);
    let narrow = stage.load_u32(Region::Payload.mutable(), address, 0);
    decode_narrow_integer(stage, narrow)
}

/// Stores the int32 word `word` as the packed integer of `slot` of the record at `record`;
/// `None` (leaving at `miss`) where it has no narrow form, changing nothing.
#[inline(always)]
pub(crate) fn write_integer<S: Stage>(stage: &mut S, record: S::Word, slot: u16, word: S::Word, miss: S::Miss) -> Option<()> {
    let (narrow, fits) = narrow_integer(stage, word);
    stage.guard(fits, miss)?;
    let address = half(stage, record, slot);
    stage.store_u32(Region::Payload.mutable(), address, 0, narrow);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{Cell, Heap, Object};
    use crate::staged::Concrete;
    use crate::value::NarrowLayout;
    use crate::Value;

    /// An object of two int properties in packed inline storage.
    fn pair(heap: &mut Heap) -> Value {
        for shape in 1..=2_u32 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        heap.property_add(object, Value::integer(0), 1);
        heap.property_add(object, Value::integer(0), 2);
        object
    }

    /// The integer rows read what the full packed read reads and store what the full store
    /// stores, at every int32 the narrow form holds, and leave at the first one it does not.
    #[test]
    fn the_integer_rows_agree_with_the_packed_read_and_store() {
        let mut heap = Heap::new();
        let object = pair(&mut heap);
        let record = heap.object(object).unwrap() as *const Object as u64;
        let half = 1_i32 << (NarrowLayout::INTEGER_BITS - 1);
        let mut stage = Concrete::new(heap.property_values_base());
        for slot in 0..2_u16 {
            for value in [0, 1, -1, 7, -123_456, half - 1, -half, half, -half - 1, i32::MAX, i32::MIN] {
                let before = (read_integer(&mut stage, record, slot) as u32 as i32, read_integer(&mut stage, record, 1 - slot) as u32 as i32);
                let stored = write_integer(&mut stage, record, slot, u64::from(value as u32), ()).is_some();
                assert_eq!(stored, (-half..half).contains(&value), "{value}");
                let (own, other) = (read_integer(&mut stage, record, slot) as u32 as i32, read_integer(&mut stage, record, 1 - slot) as u32 as i32);
                assert_eq!(own, if stored { value } else { before.0 }, "{value}");
                assert_eq!(other, before.1, "the other half stays");
                // SAFETY: a live record of the shape that holds both slots.
                let full = unsafe { crate::staged::concrete_own_read(heap.object(object).unwrap(), heap.property_values_base(), usize::from(slot)) };
                assert_eq!(full, Value::integer(own));
            }
        }
    }
}
