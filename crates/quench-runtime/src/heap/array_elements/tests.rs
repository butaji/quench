use super::*;

#[test]
fn array_counter_unknown_is_sticky() {
    for (added, removed) in [(0, 0), (1, 0), (0, 1), (1, 1), (OTHERS_UNKNOWN - 1, OTHERS_UNKNOWN - 1)] {
        assert_eq!(recounted(OTHERS_UNKNOWN, added, removed), OTHERS_UNKNOWN);
    }
}

#[test]
fn array_counter_exact_updates_do_not_saturate_intermediate_sums() {
    for (before, added, removed) in [
        (0, 3, 0),
        (5, 1, 3),
        (OTHERS_UNKNOWN - 2, 3, 2),
        (OTHERS_UNKNOWN - 1, OTHERS_UNKNOWN - 1, OTHERS_UNKNOWN - 1),
    ] {
        let expected = u32::try_from(u64::from(before) - u64::from(removed) + u64::from(added)).unwrap();
        assert_eq!(recounted(before, added, removed), expected);
    }
}

#[test]
fn array_counter_unknown_survives_proved_replacements() {
    let mut elements = ArrayElements::from([Value::integer(1), Value::number(0.5)]);
    elements.make_mut().set_others(OTHERS_UNKNOWN);
    let values = elements.unique_values().unwrap();
    for value in [Value::number(1.5), Value::integer(2)] {
        // SAFETY: the backing remains unique and the index is below its length.
        unsafe { ArrayElements::replace_through(values, 1, value) };
        assert_eq!(elements[1], value);
        assert_eq!(elements.header().others, OTHERS_UNKNOWN);
        assert!(!elements.holds_only_int32());
    }
}

#[test]
fn array_counter_unknown_survives_unique_mutations() {
    let mutations: [(&str, fn(&mut UniqueElements)); 4] = [
        ("set", |elements| elements.set(0, Value::integer(3))),
        ("fill", |elements| elements.fill(0..2, Value::integer(0))),
        ("copy", |elements| elements.copy_from(0, &[Value::integer(0), Value::number(2.5)])),
        ("truncate", |elements| elements.truncate(0)),
    ];
    for (name, mutate) in mutations {
        let mut elements = ArrayElements::from([Value::number(0.5), Value::number(1.5)]);
        let unique = elements.make_mut();
        unique.set_others(OTHERS_UNKNOWN);
        mutate(unique);
        assert_eq!(elements.header().others, OTHERS_UNKNOWN, "{name}");
    }
}

#[test]
fn native_layout_matches_header_and_dense_backing() {
    assert_eq!(
        NATIVE_HEADER_POINTER_OFFSET,
        std::mem::offset_of!(ArrayElements, header)
    );
    assert_eq!(
        NATIVE_HEADER_LENGTH_OFFSET,
        std::mem::offset_of!(Header, len)
    );
    assert_eq!(NATIVE_HEADER_VALUES_OFFSET, std::mem::size_of::<Header>());
    assert_eq!(
        NATIVE_HEADER_VALUES_OFFSET % std::mem::align_of::<Value>(),
        0
    );
    assert!(
        NATIVE_HEADER_LENGTH_OFFSET + std::mem::size_of::<u32>()
            <= NATIVE_HEADER_VALUES_OFFSET
    );

    let empty = ArrayElements::new();
    let empty_values = unsafe {
        empty
            .header
            .as_ptr()
            .cast::<u8>()
            .add(NATIVE_HEADER_VALUES_OFFSET)
            .cast::<Value>()
            .cast_const()
    };
    assert_eq!(empty_values, (*empty).as_ptr());
    assert_eq!(empty.len(), 0);

    let values = [Value::integer(7), Value::integer(9)];
    let dense = ArrayElements::from(values);
    let current_values = unsafe {
        dense
            .header
            .as_ptr()
            .cast::<u8>()
            .add(NATIVE_HEADER_VALUES_OFFSET)
            .cast::<Value>()
            .cast_const()
    };
    assert_eq!(current_values, (*dense).as_ptr());
    // The header pointer belongs to this ArrayElements value; re-reading it observes the
    // new header/backing after copy-on-write mutation.
    let mut shared = dense;
    shared.make_mut().push(Value::integer(11));
    let current_values = unsafe {
        shared
            .header
            .as_ptr()
            .cast::<u8>()
            .add(NATIVE_HEADER_VALUES_OFFSET)
            .cast::<Value>()
            .cast_const()
    };
    assert_eq!(current_values, (*shared).as_ptr());
    assert_eq!(shared.len(), 3);
}

#[test]
fn in_place_replacements_keep_the_others_count_exact() {
    // An array storage proof's store writes through the backing (`replace_through`),
    // as an in-place replacement does: the count stays the one authority.
    let mut elements = ArrayElements::from(vec![Value::integer(1), Value::integer(2)]);
    assert!(elements.replace_in_place(0, Value::number(0.5)));
    assert!(!elements.holds_only_int32());
    let values = elements.unique_values().expect("a unique backing");
    // SAFETY: the backing stays unique and both indices are below its length.
    unsafe { ArrayElements::replace_through(values, 0, Value::integer(3)) };
    assert!(elements.holds_only_int32());
    unsafe { ArrayElements::replace_through(values, 1, Value::NULL) };
    assert!(!elements.holds_only_int32());
    let shared = elements.clone();
    assert!(!elements.replace_in_place(1, Value::integer(4)));
    assert!(elements.unique_values().is_none());
    assert_eq!(shared[1], Value::NULL);
}

#[test]
fn every_write_keeps_the_others_count_exact() {
    let ints = || ArrayElements::from(vec![Value::integer(1), Value::integer(2)]);
    assert!(ints().holds_only_int32());
    assert!(!ArrayElements::from(vec![Value::integer(1), Value::number(0.5)]).holds_only_int32());
    assert!(ArrayElements::from_fn(3, |_| Value::integer(0)).holds_only_int32());
    assert!(!ArrayElements::from_fn(3, |slot| if slot == 2 { Value::NULL } else { Value::integer(0) }).holds_only_int32());
    let demotions: [(&str, fn(&mut UniqueElements)); 6] = [
        ("set", |elements| elements.set(0, Value::DELETED)),
        ("push", |elements| elements.push(Value::number(0.5))),
        ("resize", |elements| elements.resize(4, Value::UNDEFINED)),
        ("fill", |elements| elements.fill(0..1, Value::TRUE)),
        ("copy_from", |elements| elements.copy_from(1, &[Value::NULL])),
        ("splice", |elements| elements.splice(0..1, &[Value::FALSE])),
    ];
    for (write, demote) in demotions {
        let mut elements = ints();
        demote(elements.make_mut());
        assert!(!elements.holds_only_int32(), "{write}");
    }
    let mut grown = ints();
    for value in 3..64 {
        grown.make_mut().push(Value::integer(value));
    }
    grown.make_mut().set(5, Value::integer(-5));
    assert!(grown.holds_only_int32(), "int32 writes and growth keep the kind");
    let mut shared = ArrayElements::from(vec![Value::integer(1), Value::NULL]);
    let mut copy = shared.clone();
    copy.make_mut().set(1, Value::integer(2));
    assert_eq!((shared.holds_only_int32(), copy.holds_only_int32()), (false, true), "a detached copy counts its own writes");
    shared.make_mut().truncate(1);
    assert!(shared.holds_only_int32(), "removing the last other value restores Int32");
    let mut holey = ints();
    holey.make_mut().resize(4, Value::DELETED);
    assert!(!holey.holds_only_int32());
    for index in [3, 2] {
        holey.make_mut().set(index, Value::integer(0));
    }
    assert!(holey.holds_only_int32(), "filling every hole with int32s restores Int32");
    holey.make_mut().set(0, Value::number(0.5));
    holey.make_mut().pop();
    assert!(!holey.holds_only_int32());
    holey.make_mut().remove(0);
    assert!(holey.holds_only_int32());
    assert!(ArrayElements::new().holds_only_int32());
}

#[test]
fn front_removals_move_the_header_and_growth_reclaims_the_dropped_room() {
    let mut queue = ArrayElements::from_fn(64, |index| Value::integer(index as i32));
    let bytes = queue.allocated_bytes();
    for expected in 0..40 {
        assert_eq!(queue.make_mut().remove(0), Value::integer(expected));
    }
    assert_eq!((queue.len(), queue.capacity(), queue.allocated_bytes()), (24, 24, bytes));
    queue.make_mut().push(Value::NULL);
    assert_eq!((queue.len(), queue.capacity(), queue.allocated_bytes()), (25, 64, bytes), "a mostly dropped backing compacts in place");
    assert_eq!((queue[0], queue[23], queue[24]), (Value::integer(40), Value::integer(63), Value::NULL));
    assert!(!queue.holds_only_int32());
}

#[test]
fn front_insertions_take_front_room_and_regrow_with_more() {
    let mut deque = ArrayElements::from_fn(32, |index| Value::integer(index as i32));
    for value in 1..=40 {
        deque.make_mut().splice(0..0, &[Value::integer(-value)]);
    }
    deque.make_mut().remove(0);
    deque.make_mut().splice(0..0, &[Value::NULL, Value::integer(-40)]);
    deque.make_mut().push(Value::integer(32));
    let expected: Vec<Value> = std::iter::once(Value::NULL).chain((-40..=32).map(Value::integer)).collect();
    assert_eq!((&*deque, deque.holds_only_int32()), (&expected[..], false));
    drop(deque);
}

#[test]
fn young_runs_admit_reads_and_copy_out_on_write_clone_or_survival() {
    let mut run = vec![Value::UNDEFINED; HEADER_VALUES + 3];
    let at = NonNull::new(run.as_mut_ptr()).unwrap();
    let values = [Value::integer(1), Value::DELETED, Value::integer(3)];
    // SAFETY: the run holds a header and three values, and nothing else uses it.
    let young = || unsafe { ArrayElements::young(at, 3, |index| values[index]) };
    let mut written = young();
    assert!(written.is_young() && !written.holds_only_int32());
    assert_eq!((&*written, written.owner_count(), written.capacity()), (&values[..], 0, 3));
    assert!(written.unique_values().is_none() && !written.replace_in_place(0, Value::integer(9)));
    written.make_mut().set(1, Value::integer(2));
    assert!(!written.is_young() && written.holds_only_int32());
    assert_eq!(written.owner_count(), 1);
    assert_eq!(young()[1], Value::DELETED, "a write copies the run first");
    let original = young();
    let copy = original.clone();
    assert_eq!((copy.owner_count(), &*copy), (1, &values[..]));
    assert_ne!(copy.as_ptr(), original.as_ptr());
    let mut survivor = young();
    survivor.leave_nursery();
    assert_eq!((survivor.is_young(), survivor.owner_count(), &*survivor), (false, 1, &values[..]));
    survivor.leave_nursery();
    assert_eq!(survivor.owner_count(), 1, "an allocation stays where it is");
    drop((original, young()));
    assert_eq!(run[HEADER_VALUES], Value::integer(1), "dropping a young run frees nothing");
}

#[test]
fn shared_backings_copy_on_write_and_empty_arrays_allocate_nothing() {
    let empty = ArrayElements::new();
    assert_eq!(empty.capacity(), 0);
    let mut first = ArrayElements::from(vec![Value::integer(1), Value::integer(2)]);
    let mut second = first.clone();
    assert_eq!(first.as_ptr(), second.as_ptr());
    assert_eq!(first.owner_count(), 2);
    second.make_mut().push(Value::integer(3));
    assert_ne!(first.as_ptr(), second.as_ptr());
    assert_eq!(&*first, &[Value::integer(1), Value::integer(2)]);
    assert_eq!(&*second, &[Value::integer(1), Value::integer(2), Value::integer(3)]);
    first.make_mut().set(0, Value::integer(9));
    assert_eq!(first[0], Value::integer(9));

    let mut grown = ArrayElements::new();
    let unique = grown.make_mut();
    unique.extend((0..10).map(Value::integer));
    assert_eq!(unique.remove(0), Value::integer(0));
    unique.splice(1..3, &[Value::TRUE]);
    unique.truncate(4);
    assert_eq!(&*grown, &[Value::integer(1), Value::TRUE, Value::integer(4), Value::integer(5)]);
    let unique = grown.make_mut();
    assert_eq!(unique.pop(), Some(Value::integer(5)));
    unique.resize(6, Value::NULL);
    assert_eq!(&grown[3..], &[Value::NULL; 3]);
    drop(empty);
}
