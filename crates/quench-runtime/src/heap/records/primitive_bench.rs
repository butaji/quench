//! Record lifecycle kernels. Linear TOD-9 owns commands and interpretation.
//! A batch is one occupancy word; reservation and first allocation are outside timing.

extern crate test;

use super::{OCCUPANCY_WORD_BITS, PromotionAge, RecordSpace};
use crate::heap::{ArrayElements, ArrayObject, Cell, Heap, Object};
use crate::value::Value;
use std::hint::black_box;
use test::Bencher;

const BATCH_RECORDS: usize = OCCUPANCY_WORD_BITS;

fn empty_object() -> Object {
    Object::new(black_box(Value::NULL))
}

fn shared_array(elements: &ArrayElements) -> ArrayObject {
    ArrayObject::new(empty_object(), black_box(elements).clone())
}

#[bench]
fn primitive_record_object_construct_batch(b: &mut Bencher) {
    b.iter(|| {
        for _ in 0..BATCH_RECORDS {
            black_box(empty_object());
        }
    });
}

#[bench]
fn primitive_record_array_shared_construct_batch(b: &mut Bencher) {
    let elements = ArrayElements::from_fn(BATCH_RECORDS, |index| Value::integer(index as i32));
    b.iter(|| {
        for _ in 0..BATCH_RECORDS {
            black_box(shared_array(&elements));
        }
    });
    assert_eq!(elements.owner_count(), 1);
}

fn recycle<T: std::fmt::Debug>(b: &mut Bencher, mut make: impl FnMut() -> T) {
    let mut records: RecordSpace<T> = RecordSpace::default();
    for _ in 0..BATCH_RECORDS {
        records.try_insert(make()).unwrap();
    }
    records.sweep(true, PromotionAge::First, false, |_| {});
    b.iter(|| {
        for _ in 0..BATCH_RECORDS {
            black_box(black_box(&mut records).try_insert(black_box(make())).unwrap());
        }
        black_box(black_box(&mut records).sweep(true, PromotionAge::First, false, |_| {}));
    });
    assert_eq!(records.live_count(), 0);
}

#[bench]
fn primitive_record_object_recycle_batch(b: &mut Bencher) {
    recycle(b, empty_object);
}

#[bench]
fn primitive_record_array_shared_recycle_batch(b: &mut Bencher) {
    let elements = ArrayElements::from_fn(BATCH_RECORDS, |index| Value::integer(index as i32));
    recycle(b, || shared_array(&elements));
    assert_eq!(elements.owner_count(), 1);
}

#[bench]
fn primitive_record_object_mark_sweep_live_batch(b: &mut Bencher) {
    let mut records: RecordSpace<Object> = RecordSpace::default();
    let ids: [_; BATCH_RECORDS] = std::array::from_fn(|_| records.try_insert(empty_object()).unwrap());
    b.iter(|| {
        black_box(&mut records).clear_marks();
        for id in ids {
            black_box(black_box(&mut records).mark(black_box(id)));
        }
        black_box(black_box(&mut records).sweep(true, PromotionAge::First, false, |_| {}));
    });
    assert_eq!(records.live_count(), BATCH_RECORDS);
}

#[bench]
fn primitive_heap_object_allocate_collect_batch(b: &mut Bencher) {
    let mut heap = Heap::new();
    for _ in 0..BATCH_RECORDS {
        heap.alloc(Cell::Object(empty_object()));
    }
    heap.collect([]);
    b.iter(|| {
        for _ in 0..BATCH_RECORDS {
            black_box(black_box(&mut heap).alloc(Cell::Object(empty_object())));
        }
        black_box(black_box(&mut heap).collect([]));
    });
    assert_eq!(heap.live_slots(), 0);
}
