//! Actual shared window fill and argument-copy operations. Each timed batch writes
//! distinct windows; opacity surrounds the batch rather than each individual store.

extern crate test;

use super::{Value, copy_arguments, fill_room, fill_window};
use std::hint::black_box;
use test::Bencher;

const SMALL_BLOCK: usize = 8;
const LARGE_BLOCK: usize = 2 * SMALL_BLOCK;
const WINDOWS: usize = 64;
const CANARY: Value = Value::NULL;
const ARGUMENTS: [Value; 4] = [Value::TRUE, Value::FALSE, Value::NULL, Value::UNDEFINED];

fn window<const BLOCK: usize>(b: &mut Bencher, width: usize, argc: u16) {
    assert!(usize::from(argc) <= width && usize::from(argc) <= ARGUMENTS.len());
    // Equal stride for both block policies keeps address spacing and live storage fixed.
    let stride = fill_room::<LARGE_BLOCK>(width) + 1;
    let mut storage = vec![CANARY; WINDOWS * stride];
    b.iter(|| {
        let destination = black_box(storage.as_mut_slice());
        let width = black_box(width);
        let argc = black_box(argc);
        let arguments = black_box(&ARGUMENTS);
        for slots in destination.chunks_exact_mut(stride) {
            // SAFETY: each distinct chunk has room for either fill policy and arguments;
            // arguments live in a separate immutable array.
            unsafe {
                fill_window::<BLOCK>(slots.as_mut_ptr(), width, Value::UNDEFINED);
                copy_arguments(slots.as_mut_ptr(), arguments.as_ptr(), argc);
            }
        }
        black_box(destination);
    });
    for slots in storage.chunks_exact(stride) {
        assert_eq!(&slots[..usize::from(argc)], &ARGUMENTS[..usize::from(argc)]);
        assert!(slots[usize::from(argc)..fill_room::<BLOCK>(width)].iter().all(|v| *v == Value::UNDEFINED));
        assert!(slots[fill_room::<BLOCK>(width)..].iter().all(|v| *v == CANARY));
    }
}

macro_rules! windows {
    ($(($small:ident, $large:ident, $width:expr, $argc:expr)),* $(,)?) => {$(
        #[bench]
        fn $small(b: &mut Bencher) { window::<SMALL_BLOCK>(b, $width, $argc); }
        #[bench]
        fn $large(b: &mut Bencher) { window::<LARGE_BLOCK>(b, $width, $argc); }
    )*};
}

windows! {
    (primitive_window_fill8_width5, primitive_window_fill16_width5, 5, 0),
    (primitive_window_fill8_width8, primitive_window_fill16_width8, 8, 0),
    (primitive_window_fill8_width14, primitive_window_fill16_width14, 14, 0),
    (primitive_window_fill8_width21, primitive_window_fill16_width21, 21, 0),
    (primitive_window_fill8_width23, primitive_window_fill16_width23, 23, 0),
    (primitive_window_fill8_width42, primitive_window_fill16_width42, 42, 0),
    (primitive_window_copy8_width5, primitive_window_copy16_width5, 5, 2),
    (primitive_window_copy8_width14, primitive_window_copy16_width14, 14, 2),
    (primitive_window_copy8_width21, primitive_window_copy16_width21, 21, 4),
}

#[test]
fn primitive_window_fill_bounds_and_argument_prefix() {
    fn check<const BLOCK: usize>() {
        let maximum = usize::from(u16::MAX);
        for width in (0..=LARGE_BLOCK * LARGE_BLOCK).chain([maximum - 1, maximum]) {
            let room = fill_room::<BLOCK>(width);
            for argc in 0..=ARGUMENTS.len().min(width) {
                let mut storage = vec![CANARY; room + 2];
                // SAFETY: interior holds exactly the declared room, with canaries on
                // either side; the argument source is separate and long enough.
                unsafe {
                    let target = storage.as_mut_ptr().add(1);
                    fill_window::<BLOCK>(target, width, Value::UNDEFINED);
                    copy_arguments(target, ARGUMENTS.as_ptr(), argc as u16);
                }
                assert_eq!(storage[0], CANARY);
                assert_eq!(storage[room + 1], CANARY);
                assert_eq!(&storage[1..1 + argc], &ARGUMENTS[..argc]);
                assert!(storage[1 + argc..1 + room].iter().all(|v| *v == Value::UNDEFINED));
            }
        }
    }
    check::<SMALL_BLOCK>();
    check::<LARGE_BLOCK>();
}
