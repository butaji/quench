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

/// Writes `initial` to the `count` registers at `registers`, as whole blocks of `BLOCK`: the
/// first unconditionally, since most windows fit it.
///
/// # Safety
/// `registers` has room for `fill_room::<BLOCK>(count)` values, none of them live.
#[inline(always)]
unsafe fn fill_rounded_window<const BLOCK: usize>(registers: *mut Value, count: usize, initial: Value) {
    // SAFETY: the room holds at least one block.
    unsafe { registers.cast::<[Value; BLOCK]>().write_unaligned([initial; BLOCK]) };
    for block in 1..count.div_ceil(BLOCK) {
        // SAFETY: the block ends within the room the caller provides.
        unsafe {
            registers
                .add(block * BLOCK)
                .cast::<[Value; BLOCK]>()
                .write_unaligned([initial; BLOCK]);
        }
    }
}
// The former production implementation is a comparison control, not a runtime policy.
trait WindowFill {
    fn extent(width: usize) -> usize;
    unsafe fn fill(destination: *mut Value, width: usize, initial: Value);
}
struct Exact;
// Retained186's source loop, which LLVM lowers to a pattern-fill library call.
struct ElementLoop;
impl WindowFill for ElementLoop {
    fn extent(width: usize) -> usize { width }
    unsafe fn fill(destination: *mut Value, width: usize, initial: Value) {
        for index in 0..width {
            // SAFETY: the comparison control has the same logical range contract.
            unsafe { destination.add(index).write_unaligned(initial) };
        }
    }
}
struct Rounded<const BLOCK: usize>;
impl WindowFill for Exact {
    fn extent(width: usize) -> usize { width }
    unsafe fn fill(destination: *mut Value, width: usize, initial: Value) {
        unsafe { fill_window(destination, width, initial) };
    }
}
impl<const BLOCK: usize> WindowFill for Rounded<BLOCK> {
    fn extent(width: usize) -> usize { fill_room::<BLOCK>(width) }
    unsafe fn fill(destination: *mut Value, width: usize, initial: Value) {
        unsafe { fill_rounded_window::<BLOCK>(destination, width, initial) };
    }
}

fn window<F: WindowFill>(b: &mut Bencher, width: usize, argc: u16) {
    assert!(usize::from(argc) <= width && usize::from(argc) <= ARGUMENTS.len());
    // Equal stride for every policy keeps address spacing and live storage fixed.
    let stride = fill_room::<LARGE_BLOCK>(width) + 1;
    let mut storage = vec![CANARY; WINDOWS * stride];
    b.iter(|| {
        let destination = black_box(storage.as_mut_slice());
        let width = black_box(width);
        let argc = black_box(argc);
        let arguments = black_box(&ARGUMENTS);
        for slots in destination.chunks_exact_mut(stride) {
            // SAFETY: each distinct chunk has room for every fill policy and arguments;
            // arguments live in a separate immutable array.
            unsafe {
                F::fill(slots.as_mut_ptr(), width, Value::UNDEFINED);
                copy_arguments(slots.as_mut_ptr(), arguments.as_ptr(), argc);
            }
        }
        black_box(destination);
    });
    for slots in storage.chunks_exact(stride) {
        assert_eq!(&slots[..usize::from(argc)], &ARGUMENTS[..usize::from(argc)]);
        assert!(slots[usize::from(argc)..F::extent(width)].iter().all(|v| *v == Value::UNDEFINED));
        assert!(slots[F::extent(width)..].iter().all(|v| *v == CANARY));
    }
}

macro_rules! windows {
    ($(($small:ident, $large:ident, $exact:ident, $element:ident, $width:expr, $argc:expr)),* $(,)?) => {$(
        #[bench]
        fn $small(b: &mut Bencher) { window::<Rounded<SMALL_BLOCK>>(b, $width, $argc); }
        #[bench]
        fn $large(b: &mut Bencher) { window::<Rounded<LARGE_BLOCK>>(b, $width, $argc); }
        #[bench]
        fn $exact(b: &mut Bencher) { window::<Exact>(b, $width, $argc); }
        #[bench]
        fn $element(b: &mut Bencher) { window::<ElementLoop>(b, $width, $argc); }
    )*};
}

windows! {
    (primitive_window_fill8_width4, primitive_window_fill16_width4, primitive_window_fill_exact_width4, primitive_window_fill_element_width4, 4, 0),
    (primitive_window_fill8_width6, primitive_window_fill16_width6, primitive_window_fill_exact_width6, primitive_window_fill_element_width6, 6, 0),
    (primitive_window_fill8_width7, primitive_window_fill16_width7, primitive_window_fill_exact_width7, primitive_window_fill_element_width7, 7, 0),
    (primitive_window_fill8_width5, primitive_window_fill16_width5, primitive_window_fill_exact_width5, primitive_window_fill_element_width5, 5, 0),
    (primitive_window_fill8_width8, primitive_window_fill16_width8, primitive_window_fill_exact_width8, primitive_window_fill_element_width8, 8, 0),
    (primitive_window_fill8_width15, primitive_window_fill16_width15, primitive_window_fill_exact_width15, primitive_window_fill_element_width15, 15, 0),
    (primitive_window_fill8_width16, primitive_window_fill16_width16, primitive_window_fill_exact_width16, primitive_window_fill_element_width16, 16, 0),
    (primitive_window_fill8_width17, primitive_window_fill16_width17, primitive_window_fill_exact_width17, primitive_window_fill_element_width17, 17, 0),
    (primitive_window_fill8_width14, primitive_window_fill16_width14, primitive_window_fill_exact_width14, primitive_window_fill_element_width14, 14, 0),
    (primitive_window_fill8_width21, primitive_window_fill16_width21, primitive_window_fill_exact_width21, primitive_window_fill_element_width21, 21, 0),
    (primitive_window_fill8_width23, primitive_window_fill16_width23, primitive_window_fill_exact_width23, primitive_window_fill_element_width23, 23, 0),
    (primitive_window_fill8_width42, primitive_window_fill16_width42, primitive_window_fill_exact_width42, primitive_window_fill_element_width42, 42, 0),
    (primitive_window_copy8_width5, primitive_window_copy16_width5, primitive_window_copy_exact_width5, primitive_window_copy_element_width5, 5, 2),
    (primitive_window_copy8_width14, primitive_window_copy16_width14, primitive_window_copy_exact_width14, primitive_window_copy_element_width14, 14, 2),
    (primitive_window_copy8_width21, primitive_window_copy16_width21, primitive_window_copy_exact_width21, primitive_window_copy_element_width21, 21, 4),
}

#[test]
fn primitive_window_fill_bounds_and_argument_prefix() {
    fn check<F: WindowFill>(initial: Value) {
        let maximum = usize::from(u16::MAX);
        for width in (0..=LARGE_BLOCK * LARGE_BLOCK).chain([maximum - 1, maximum]) {
            let room = F::extent(width);
            for argc in 0..=ARGUMENTS.len().min(width) {
                let mut storage = vec![CANARY; room + 2];
                // SAFETY: interior holds exactly the declared room, with canaries on
                // either side; the argument source is separate and long enough.
                unsafe {
                    let target = storage.as_mut_ptr().add(1);
                    F::fill(target, width, initial);
                    copy_arguments(target, ARGUMENTS.as_ptr(), argc as u16);
                }
                assert_eq!(storage[0], CANARY);
                assert_eq!(storage[room + 1], CANARY);
                assert_eq!(&storage[1..1 + argc], &ARGUMENTS[..argc]);
                assert!(storage[1 + argc..1 + room].iter().all(|v| *v == initial));
            }
        }
    }
    for initial in [Value::UNDEFINED, Value::TRUE, Value::integer(0), Value::integer(17)] {
        check::<Exact>(initial);
        check::<ElementLoop>(initial);
        check::<Rounded<SMALL_BLOCK>>(initial);
        check::<Rounded<LARGE_BLOCK>>(initial);
    }
}

#[test]
fn primitive_window_fill_preserves_unaligned_storage_contract() {
    const BYTE_CANARY: u8 = 0xa5;
    for width in 0..=LARGE_BLOCK * 2 {
        let bytes = width * size_of::<Value>();
        let mut storage = vec![BYTE_CANARY; bytes + align_of::<Value>() + 1];
        let offset = if storage.as_ptr().addr().is_multiple_of(align_of::<Value>()) { 1 } else { 0 };
        let initial = Value::integer(17);
        // SAFETY: the byte allocation holds exactly the logical extent at this deliberately
        // unaligned address. Values are only written/read through unaligned raw operations.
        unsafe {
            let target = storage.as_mut_ptr().add(offset).cast::<Value>();
            fill_window(target, width, initial);
            for index in 0..width { assert_eq!(target.add(index).read_unaligned(), initial); }
        }
        assert!(storage[..offset].iter().all(|byte| *byte == BYTE_CANARY));
        assert!(storage[offset + bytes..].iter().all(|byte| *byte == BYTE_CANARY));
    }
}
