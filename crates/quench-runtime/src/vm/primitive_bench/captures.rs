//! Already-resolved dense capture reads; neither environment walks nor generic fallback.

use super::{BATCH_VALUES, Bencher, Value, black_box};
use crate::heap::{EnvironmentScopeRef, EnvironmentSlotState, EnvironmentSlots, Heap};
use std::{marker::PhantomData, mem::ManuallyDrop};

const OWNER: u8 = 0;
const SLICE: u8 = 1;
const THIN: u8 = 2;

/// Benchmark-only borrowed descriptor: reuse the owner's storage access without owning
/// or dropping its allocation. No mutable access or lifetime erasure is exposed.
struct BorrowedSlots<'a> {
    slots: ManuallyDrop<EnvironmentSlots>,
    borrow: PhantomData<&'a EnvironmentSlots>,
}

impl<'a> BorrowedSlots<'a> {
    fn new(slots: &'a EnvironmentSlots) -> Self {
        const { assert!(size_of::<BorrowedSlots<'static>>() == size_of::<usize>()) };
        // SAFETY: the descriptor is copied, not its elements. ManuallyDrop prevents a
        // second drop, and the borrow keeps the original alive and immutable. All reads
        // use EnvironmentSlots' existing layout and marker/bounds checks.
        Self { slots: ManuallyDrop::new(unsafe { std::ptr::read(slots) }), borrow: PhantomData }
    }
}

fn view_read(values: &[Value], index: usize) -> Option<Value> {
    let value = *values.get(index)?;
    (!value.is_internal_marker()).then_some(value)
}

const FIRST: u8 = 0;
const LAST: u8 = 1;
const PAST_END: u8 = 2;
const EMPTY: u8 = 3;
const UNINITIALIZED: u8 = 4;
const SHARED: u8 = 5;
const UNDEFINED: u8 = 6;

fn dense_read<const STORAGE: u8, const CASE: u8>(b: &mut Bencher) {
    let width = match CASE { EMPTY => 0, LAST | PAST_END => BATCH_VALUES, _ => 1 };
    let index = match CASE { LAST => width - 1, PAST_END => width, _ => 0 };
    let value = match CASE {
        UNINITIALIZED => Value::DELETED,
        UNDEFINED => Value::UNDEFINED,
        _ => Value::integer(42),
    };
    let mut slots = EnvironmentSlots::from(vec![value; width]);
    let mut heap = Heap::new();
    if CASE == SHARED {
        let owner = heap.alloc_environment(
            Value::NULL, 1, slots.clone(), EnvironmentScopeRef::of_program(0),
        ).0;
        slots = slots.iteration_copy(owner, &[], &[], |_, _| unreachable!()).unwrap();
        assert!(matches!(slots.state(index), Some(EnvironmentSlotState::Shared(value)) if value == owner));
    }
    let slots = slots.dense().expect("these fixtures must remain dense");
    let (pointer, length) = slots.native_dense_view().unwrap();
    // SAFETY: the owning slots stay alive and unchanged throughout this benchmark. There
    // is no collection, storage replacement or guest call in the timed loop.
    let view = unsafe { std::slice::from_raw_parts(pointer, length) };
    let thin = BorrowedSlots::new(slots);
    let expected = match CASE {
        FIRST | LAST | UNDEFINED => Some(value),
        PAST_END | EMPTY | UNINITIALIZED | SHARED => None,
        _ => unreachable!(),
    };
    assert_eq!(slots.dense_plain(index), expected);
    assert_eq!(view_read(view, index), expected);
    assert_eq!(thin.slots.dense_plain(index), expected);
    b.iter(|| {
        for _ in 0..BATCH_VALUES {
            let result = match STORAGE {
                OWNER => black_box(slots).dense_plain(black_box(index)),
                SLICE => view_read(black_box(view), black_box(index)),
                THIN => {
                    let borrowed = black_box(BorrowedSlots::new(slots));
                    borrowed.slots.dense_plain(black_box(index))
                }
                _ => unreachable!(),
            };
            black_box(result);
        }
    });
    assert_eq!(slots.dense_plain(index), expected);
    assert_eq!(view_read(view, index), expected);
    assert_eq!(thin.slots.dense_plain(index), expected);
}

#[bench]
fn primitive_capture_dense_first(b: &mut Bencher) { dense_read::<OWNER, FIRST>(b); }
#[bench]
fn primitive_capture_view_first(b: &mut Bencher) { dense_read::<SLICE, FIRST>(b); }
#[bench]
fn primitive_capture_thin_first(b: &mut Bencher) { dense_read::<THIN, FIRST>(b); }
#[bench]
fn primitive_capture_dense_last(b: &mut Bencher) { dense_read::<OWNER, LAST>(b); }
#[bench]
fn primitive_capture_view_last(b: &mut Bencher) { dense_read::<SLICE, LAST>(b); }
#[bench]
fn primitive_capture_thin_last(b: &mut Bencher) { dense_read::<THIN, LAST>(b); }
#[bench]
fn primitive_capture_dense_past_end(b: &mut Bencher) { dense_read::<OWNER, PAST_END>(b); }
#[bench]
fn primitive_capture_view_past_end(b: &mut Bencher) { dense_read::<SLICE, PAST_END>(b); }
#[bench]
fn primitive_capture_thin_past_end(b: &mut Bencher) { dense_read::<THIN, PAST_END>(b); }
#[bench]
fn primitive_capture_dense_empty(b: &mut Bencher) { dense_read::<OWNER, EMPTY>(b); }
#[bench]
fn primitive_capture_view_empty(b: &mut Bencher) { dense_read::<SLICE, EMPTY>(b); }
#[bench]
fn primitive_capture_thin_empty(b: &mut Bencher) { dense_read::<THIN, EMPTY>(b); }
#[bench]
fn primitive_capture_dense_uninitialized(b: &mut Bencher) { dense_read::<OWNER, UNINITIALIZED>(b); }
#[bench]
fn primitive_capture_view_uninitialized(b: &mut Bencher) { dense_read::<SLICE, UNINITIALIZED>(b); }
#[bench]
fn primitive_capture_thin_uninitialized(b: &mut Bencher) { dense_read::<THIN, UNINITIALIZED>(b); }
#[bench]
fn primitive_capture_dense_shared(b: &mut Bencher) { dense_read::<OWNER, SHARED>(b); }
#[bench]
fn primitive_capture_view_shared(b: &mut Bencher) { dense_read::<SLICE, SHARED>(b); }
#[bench]
fn primitive_capture_thin_shared(b: &mut Bencher) { dense_read::<THIN, SHARED>(b); }
#[bench]
fn primitive_capture_dense_undefined(b: &mut Bencher) { dense_read::<OWNER, UNDEFINED>(b); }
#[bench]
fn primitive_capture_view_undefined(b: &mut Bencher) { dense_read::<SLICE, UNDEFINED>(b); }
#[bench]
fn primitive_capture_thin_undefined(b: &mut Bencher) { dense_read::<THIN, UNDEFINED>(b); }

fn dependent_read<const STORAGE: u8>(b: &mut Bencher) {
    // One complete ring per batch. Each loaded integer chooses the following slot.
    let slots = EnvironmentSlots::from((0..BATCH_VALUES)
        .map(|index| Value::integer(((index + 1) % BATCH_VALUES) as i32))
        .collect::<Vec<_>>());
    let slots = slots.dense().unwrap();
    let (pointer, length) = slots.native_dense_view().unwrap();
    // SAFETY: slots remain alive and immutable; the timed loop has no safepoint.
    let view = unsafe { std::slice::from_raw_parts(pointer, length) };
    for index in 0..BATCH_VALUES {
        let expected = Some(Value::integer(((index + 1) % BATCH_VALUES) as i32));
        assert_eq!(slots.dense_plain(index), expected);
        assert_eq!(view_read(view, index), expected);
        assert_eq!(BorrowedSlots::new(slots).slots.dense_plain(index), expected);
    }
    let mut cursor = 0;
    b.iter(|| {
        for _ in 0..BATCH_VALUES {
            let value = match STORAGE {
                OWNER => black_box(slots).dense_plain(black_box(cursor)),
                SLICE => view_read(black_box(view), black_box(cursor)),
                THIN => black_box(BorrowedSlots::new(slots)).slots.dense_plain(black_box(cursor)),
                _ => unreachable!(),
            };
            cursor = value.unwrap().as_int().unwrap() as usize;
        }
        black_box(cursor);
    });
    assert_eq!(cursor, 0);
}

#[bench]
fn primitive_capture_dense_dependent(b: &mut Bencher) { dependent_read::<OWNER>(b); }
#[bench]
fn primitive_capture_view_dependent(b: &mut Bencher) { dependent_read::<SLICE>(b); }
#[bench]
fn primitive_capture_thin_dependent(b: &mut Bencher) { dependent_read::<THIN>(b); }

/// Batch-level opacity control. Descriptor and cursor barriers are outside the read loop;
/// the returned cursor and sum retain every read without per-probe barrier spills.
fn batch_read<const STORAGE: u8, const DEPENDENT: bool>(b: &mut Bencher) {
    let slots = EnvironmentSlots::from((0..BATCH_VALUES)
        .map(|index| Value::integer(((index + 1) % BATCH_VALUES) as i32))
        .collect::<Vec<_>>());
    let slots = slots.dense().unwrap();
    let (pointer, length) = slots.native_dense_view().unwrap();
    // SAFETY: the owner is live and immutable for the whole benchmark; no safepoints run.
    let view = unsafe { std::slice::from_raw_parts(pointer, length) };
    fn run<const DEPENDENT: bool>(mut cursor: usize, mut read: impl FnMut(usize) -> Option<Value>) -> (usize, usize) {
        let mut sum = 0;
        for index in 0..BATCH_VALUES {
            cursor = read(if DEPENDENT { cursor } else { index }).unwrap().as_int().unwrap() as usize;
            sum += cursor;
        }
        (cursor, sum)
    }
    let expected = (0, BATCH_VALUES * (BATCH_VALUES - 1) / 2);
    assert_eq!(run::<DEPENDENT>(0, |index| slots.dense_plain(index)), expected);
    assert_eq!(run::<DEPENDENT>(0, |index| view_read(view, index)), expected);
    assert_eq!(run::<DEPENDENT>(0, |index| BorrowedSlots::new(slots).slots.dense_plain(index)), expected);
    let mut result = expected;
    b.iter(|| {
        let cursor = black_box(result.0);
        result = match STORAGE {
            OWNER => {
                let slots = black_box(slots);
                run::<DEPENDENT>(cursor, |index| slots.dense_plain(index))
            }
            SLICE => {
                let view = black_box(view);
                run::<DEPENDENT>(cursor, |index| view_read(view, index))
            }
            THIN => {
                let thin = black_box(BorrowedSlots::new(slots));
                run::<DEPENDENT>(cursor, |index| thin.slots.dense_plain(index))
            }
            _ => unreachable!(),
        };
        black_box(result);
    });
    assert_eq!(result, expected);
}

#[bench]
fn primitive_capture_batch_dense_independent(b: &mut Bencher) { batch_read::<OWNER, false>(b); }
#[bench]
fn primitive_capture_batch_view_independent(b: &mut Bencher) { batch_read::<SLICE, false>(b); }
#[bench]
fn primitive_capture_batch_thin_independent(b: &mut Bencher) { batch_read::<THIN, false>(b); }
#[bench]
fn primitive_capture_batch_dense_dependent(b: &mut Bencher) { batch_read::<OWNER, true>(b); }
#[bench]
fn primitive_capture_batch_view_dependent(b: &mut Bencher) { batch_read::<SLICE, true>(b); }
#[bench]
fn primitive_capture_batch_thin_dependent(b: &mut Bencher) { batch_read::<THIN, true>(b); }
