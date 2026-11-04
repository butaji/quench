#![feature(test)]

extern crate test;

use quench_alloc::{class_of, owns, QuenchAlloc, GRANULE_BYTES};
use std::alloc::{handle_alloc_error, GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::ptr::NonNull;
use test::Bencher;

// Amortize the harness boundary over a batch; these are simultaneous live blocks.
const BATCH_BLOCKS: usize = 64;
// Model word-backed runtime buffers without tying the fixture to allocator policy.
const BUFFER_ALIGNMENT: usize = std::mem::align_of::<usize>();
const LIVE_BYTE: u8 = 0xa5;

struct Block<'a, A: GlobalAlloc> {
    allocator: &'a A,
    pointer: NonNull<u8>,
    layout: Layout,
}

impl<'a, A: GlobalAlloc> Block<'a, A> {
    fn new(allocator: &'a A, layout: Layout) -> Self {
        // SAFETY: every fixture layout is nonzero and valid.
        let pointer = unsafe { allocator.alloc(layout) };
        let pointer = NonNull::new(pointer).unwrap_or_else(|| handle_alloc_error(layout));
        Self { allocator, pointer, layout }
    }

    fn resize(&mut self, layout: Layout) {
        // SAFETY: this block owns the allocation under its current layout. The
        // fixture keeps alignment unchanged and requests a nonzero size.
        let pointer = unsafe {
            self.allocator.realloc(self.pointer.as_ptr(), self.layout, layout.size())
        };
        let pointer = NonNull::new(pointer).unwrap_or_else(|| handle_alloc_error(layout));
        self.pointer = pointer;
        self.layout = layout;
    }

    fn validate_route(&self, expected_owned: bool) {
        assert_eq!(self.pointer.as_ptr() as usize % self.layout.align(), 0);
        assert_eq!(owns(self.pointer.as_ptr()), expected_owned);
    }

    fn initialize(&mut self, length: usize) {
        assert!(length <= self.layout.size());
        // SAFETY: the block exclusively owns at least `length` writable bytes.
        unsafe { self.pointer.as_ptr().write_bytes(LIVE_BYTE, length) };
    }

    fn validate_contents(&self, length: usize) {
        assert!(length <= self.layout.size());
        // SAFETY: setup initialized this prefix; resize must preserve it.
        let bytes = unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), length) };
        assert!(bytes.iter().all(|&byte| byte == LIVE_BYTE));
    }
}

impl<A: GlobalAlloc> Drop for Block<'_, A> {
    fn drop(&mut self) {
        // SAFETY: the pointer and its authoritative current layout belong together.
        unsafe { self.allocator.dealloc(self.pointer.as_ptr(), self.layout) };
    }
}

fn layout(bytes: usize) -> Layout {
    assert_ne!(bytes, 0);
    const { assert!(BUFFER_ALIGNMENT <= GRANULE_BYTES) };
    Layout::from_size_align(bytes, BUFFER_ALIGNMENT).unwrap()
}

fn blocks<A: GlobalAlloc>(allocator: &A, layout: Layout) -> [Block<'_, A>; BATCH_BLOCKS] {
    std::array::from_fn(|_| Block::new(allocator, layout))
}

fn allocation<A: GlobalAlloc>(b: &mut Bencher, allocator: &A, bytes: usize, expected_owned: bool) {
    let layout = layout(bytes);
    {
        let mut live = blocks(allocator, layout);
        for block in &mut live {
            block.validate_route(expected_owned);
            block.initialize(bytes);
        }
        for block in &live {
            block.validate_contents(bytes);
        }
    }
    // No payload initialization in timing: 64 alloc/free pairs plus ownership
    // bookkeeping. Setup above warms the route and checks its live allocations.
    b.iter(|| {
        let live = blocks(black_box(allocator), black_box(layout));
        black_box(&live);
        drop(live);
    });
}

fn resize_cycle<A: GlobalAlloc>(
    b: &mut Bencher,
    allocator: &A,
    initial_bytes: usize,
    other_bytes: usize,
    expected_owner: impl Fn(usize) -> bool,
) {
    let initial = layout(initial_bytes);
    let other = layout(other_bytes);
    let preserved = initial_bytes.min(other_bytes);
    let mut live = blocks(allocator, initial);
    for block in &mut live {
        block.validate_route(expected_owner(initial_bytes));
        block.initialize(preserved);
        block.resize(other);
        block.validate_route(expected_owner(other_bytes));
        block.validate_contents(preserved);
        block.resize(initial);
        block.validate_route(expected_owner(initial_bytes));
        block.validate_contents(preserved);
    }
    // Allocation, initial writes and final free are outside timing. A batch has
    // 64 outward and 64 return reallocations, preserving the initialized prefix.
    b.iter(|| {
        let initial = black_box(initial);
        let other = black_box(other);
        for block in &mut live {
            block.resize(other);
        }
        black_box(&live);
        for block in &mut live {
            block.resize(initial);
        }
        black_box(&live);
    });
    for block in &live {
        block.validate_route(expected_owner(initial_bytes));
        block.validate_contents(preserved);
    }
}

macro_rules! allocation_cases {
    ($(($quench:ident, $system:ident, $bytes:expr)),* $(,)?) => {$(
        #[bench]
        fn $quench(b: &mut Bencher) {
            QuenchAlloc::enable();
            allocation(b, &QuenchAlloc::new(), $bytes, class_of($bytes).is_some());
        }

        #[bench]
        fn $system(b: &mut Bencher) {
            allocation(b, &System, $bytes, false);
        }
    )*};
}

macro_rules! resize_cases {
    ($(($quench:ident, $system:ident, $initial:expr, $other:expr)),* $(,)?) => {$(
        #[bench]
        fn $quench(b: &mut Bencher) {
            QuenchAlloc::enable();
            resize_cycle(b, &QuenchAlloc::new(), $initial, $other, |bytes| class_of(bytes).is_some());
        }

        #[bench]
        fn $system(b: &mut Bencher) {
            resize_cycle(b, &System, $initial, $other, |_| false);
        }
    )*};
}

// Fixed request bytes are experimental inputs, not allocator-policy constants.
// Keep them fixed when changing class geometry so baseline/candidate do equal work.
// 152 observed the medium sizes; 1008/1040 bracket the original 1024-byte boundary.
allocation_cases! {
    (primitive_alloc_quench_1024, primitive_alloc_system_1024, 1024),
    (primitive_alloc_quench_1040, primitive_alloc_system_1040, 1040),
    (primitive_alloc_quench_1536, primitive_alloc_system_1536, 1536),
    (primitive_alloc_quench_4096, primitive_alloc_system_4096, 4096),
    (primitive_alloc_quench_16384, primitive_alloc_system_16384, 16384),
}

resize_cases! {
    (primitive_resize_quench_1008_1024, primitive_resize_system_1008_1024, 1008, 1024),
    (primitive_resize_quench_1024_1040, primitive_resize_system_1024_1040, 1024, 1040),
    (primitive_resize_quench_1536_4096, primitive_resize_system_1536_4096, 1536, 4096),
}
