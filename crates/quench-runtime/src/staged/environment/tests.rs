use super::*;
use crate::Value;

#[test]
fn a_dense_capture_reads_ordinary_values_and_leaves_past_the_run_or_at_a_marker() {
    let values = [Value::UNDEFINED, Value::DELETED, Value::number(2.5)];
    // SAFETY: `values` holds three live values.
    let read = |slot| unsafe { concrete_dense_capture(values.as_ptr(), values.len(), slot) };
    assert_eq!(read(0).map(Value::to_bits), Some(Value::UNDEFINED.to_bits()));
    assert_eq!(read(1).map(Value::to_bits), None, "an internal marker leaves");
    assert_eq!(read(2).map(Value::to_bits), Some(Value::number(2.5).to_bits()));
    assert_eq!(read(3).map(Value::to_bits), None, "past the run leaves");
}

/// The capture row names its slot only through holes: two slots, one op trace.
#[test]
fn capture_slot_is_a_hole_not_part_of_the_trace() {
    let trace = |slot: u16| {
        let mut stage = crate::staged::trace::Trace::default();
        let (values, len) = (stage.constant(0), stage.constant(0));
        dense_capture(&mut stage, values, len, slot, ()).expect("a trace never misses");
        stage
    };
    let (first, second) = (trace(2), trace(9));
    assert_eq!(first.ops, second.ops);
    assert_eq!((first.holes, second.holes), (vec![2, 16], vec![9, 72]));
}

/// The store writes where `dense_plain_mut` finds a place, and leaves a marker or a slot past the run as it is.
#[test]
fn an_environment_slot_store_writes_initialized_slots_only() {
    use crate::heap::EnvironmentSlots;
    let mut slots = EnvironmentSlots::from_values([Value::number(7.0), Value::DELETED].into_iter());
    // SAFETY: `slots` is live and unborrowed.
    let store = |slots: &mut EnvironmentSlots, slot| unsafe { concrete_environment_slot_store(slots, slot, Value::number(9.0)) };
    assert!(store(&mut slots, 0));
    assert!(!store(&mut slots, 1), "a dead zone is the helper's");
    assert!(!store(&mut slots, 2), "past the run leaves");
    let dense = slots.dense().expect("a new storage is dense");
    assert_eq!(dense.dense_plain(0).map(Value::to_bits), Some(Value::number(9.0).to_bits()));
    assert_eq!(dense.dense_plain(1).map(Value::to_bits), None);
}

/// The layout constants and the row read what `EnvironmentSlots::dense_plain` reads.
#[test]
fn an_environment_slot_reads_as_the_dense_storage_does() {
    use crate::heap::EnvironmentSlots;
    let slots = EnvironmentSlots::from_values([Value::number(7.0), Value::DELETED, Value::UNDEFINED].into_iter());
    let dense = slots.dense().expect("a new storage is dense");
    for slot in 0..4u16 {
        // SAFETY: `slots` is live.
        let read = unsafe { concrete_environment_slot(&slots, slot) };
        assert_eq!(read.map(Value::to_bits), dense.dense_plain(usize::from(slot)).map(Value::to_bits), "slot {slot}");
    }
}

/// A binding's first value defines over its dead zone or an earlier iteration's value; a slot another environment shares
/// or past the run leaves, nothing written; opening the dead zone writes the hole.
#[test]
fn an_environment_slot_defines_over_a_hole_or_a_value_and_opens_its_dead_zone() {
    use crate::heap::EnvironmentSlots;
    // Slot 2 shares with the environment the copy was made from; the fresh slots own.
    let source = EnvironmentSlots::from_values([Value::DELETED, Value::number(7.0), Value::number(1.0)].into_iter());
    let owner = Value::from_bits(crate::value::ValueLayout::HEAP_TAG | 1);
    let mut slots = source.iteration_copy(owner, &[0, 1], crate::bytecode::Resets::listed(&[]), |_, _| None).expect("a copy");
    // SAFETY: `slots` is live and unborrowed.
    let define = |slots: &mut EnvironmentSlots, slot| unsafe { concrete_environment_slot_define(slots, slot, Value::number(9.0)) };
    assert!(define(&mut slots, 0), "a dead zone takes the first value");
    assert!(define(&mut slots, 1), "an earlier iteration's value is replaced");
    assert!(!define(&mut slots, 2), "a shared slot is the helper's");
    assert!(!define(&mut slots, 3), "past the run leaves");
    // SAFETY: as above.
    assert!(unsafe { concrete_environment_slot_open(&mut slots, 0) });
    let dense = slots.dense().expect("a new storage is dense");
    assert_eq!(dense.dense_plain(0).map(Value::to_bits), None, "the dead zone is open");
    assert_eq!(dense.dense_plain(1).map(Value::to_bits), Some(Value::number(9.0).to_bits()));
}
