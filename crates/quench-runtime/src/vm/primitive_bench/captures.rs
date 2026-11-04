//! Already-resolved dense capture reads; neither environment walks nor generic fallback.

use super::{BATCH_VALUES, Bencher, Value, black_box};
use crate::heap::{EnvironmentScopeRef, EnvironmentSlotState, EnvironmentSlots, Heap};

const FIRST: u8 = 0;
const LAST: u8 = 1;
const PAST_END: u8 = 2;
const EMPTY: u8 = 3;
const UNINITIALIZED: u8 = 4;
const SHARED: u8 = 5;
const UNDEFINED: u8 = 6;

fn dense_read<const VIEW: bool, const CASE: u8>(b: &mut Bencher) {
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
    let expected = match CASE {
        FIRST | LAST | UNDEFINED => Some(value),
        PAST_END | EMPTY | UNINITIALIZED | SHARED => None,
        _ => unreachable!(),
    };
    let view_read = |values: &[Value], index: usize| {
        let value = *values.get(index)?;
        (!value.is_internal_marker()).then_some(value)
    };
    assert_eq!(slots.dense_plain(index), expected);
    assert_eq!(view_read(view, index), expected);
    b.iter(|| {
        for _ in 0..BATCH_VALUES {
            let result = if VIEW {
                view_read(black_box(view), black_box(index))
            } else {
                black_box(slots).dense_plain(black_box(index))
            };
            black_box(result);
        }
    });
    assert_eq!(slots.dense_plain(index), expected);
    assert_eq!(view_read(view, index), expected);
}

#[bench]
fn primitive_capture_dense_first(b: &mut Bencher) { dense_read::<false, FIRST>(b); }
#[bench]
fn primitive_capture_view_first(b: &mut Bencher) { dense_read::<true, FIRST>(b); }
#[bench]
fn primitive_capture_dense_last(b: &mut Bencher) { dense_read::<false, LAST>(b); }
#[bench]
fn primitive_capture_view_last(b: &mut Bencher) { dense_read::<true, LAST>(b); }
#[bench]
fn primitive_capture_dense_past_end(b: &mut Bencher) { dense_read::<false, PAST_END>(b); }
#[bench]
fn primitive_capture_view_past_end(b: &mut Bencher) { dense_read::<true, PAST_END>(b); }
#[bench]
fn primitive_capture_dense_empty(b: &mut Bencher) { dense_read::<false, EMPTY>(b); }
#[bench]
fn primitive_capture_view_empty(b: &mut Bencher) { dense_read::<true, EMPTY>(b); }
#[bench]
fn primitive_capture_dense_uninitialized(b: &mut Bencher) { dense_read::<false, UNINITIALIZED>(b); }
#[bench]
fn primitive_capture_view_uninitialized(b: &mut Bencher) { dense_read::<true, UNINITIALIZED>(b); }
#[bench]
fn primitive_capture_dense_shared(b: &mut Bencher) { dense_read::<false, SHARED>(b); }
#[bench]
fn primitive_capture_view_shared(b: &mut Bencher) { dense_read::<true, SHARED>(b); }
#[bench]
fn primitive_capture_dense_undefined(b: &mut Bencher) { dense_read::<false, UNDEFINED>(b); }
#[bench]
fn primitive_capture_view_undefined(b: &mut Bencher) { dense_read::<true, UNDEFINED>(b); }
