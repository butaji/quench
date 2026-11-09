//! Isolate allocator work before checking an application workload.
use quench_alloc::{class_of, QuenchAlloc, GRANULE_BYTES, MAX_SMALL_BYTES};
use std::{alloc::{GlobalAlloc, Layout}, hint::black_box, time::Instant};

const LIVE_BLOCKS: usize = 256;
const DEFAULT_ROUNDS: usize = 10_000_000;
const DEFAULT_SIZE: usize = 64;
/// Command-line positions: `primitives [operation] [rounds] [size]`.
const OPERATION_ARGUMENT: usize = 1;
const ROUNDS_ARGUMENT: usize = 2;
const SIZE_ARGUMENT: usize = 3;

/// Classifies every small size in turn; returns the operations run and a checksum.
fn classes(rounds: usize) -> (usize, usize) {
    let mut checksum = 0usize;
    for index in 0..rounds {
        let size = black_box(index % MAX_SMALL_BYTES + 1);
        checksum += black_box(class_of(size).expect("a small size"));
    }
    (rounds, checksum)
}

/// Allocates and frees `LIVE_BLOCKS` blocks of `size` per round; returns the operations run
/// and a checksum.
fn allocate(rounds: usize, size: usize) -> (usize, usize) {
    QuenchAlloc::enable();
    let allocator = QuenchAlloc::new();
    let layout = Layout::from_size_align(size, GRANULE_BYTES).expect("valid layout");
    let mut blocks = [std::ptr::null_mut(); LIVE_BLOCKS];
    let mut checksum = 0usize;
    for _ in 0..rounds {
        for block in &mut blocks {
            // SAFETY: layout is valid; each successful block is freed below.
            *block = unsafe { allocator.alloc(black_box(layout)) };
            assert!(!block.is_null());
            // Touch allocated storage and prevent allocation elision.
            unsafe { (*block).write(black_box(1)) };
        }
        black_box(&blocks);
        for &block in blocks.iter().rev() {
            // SAFETY: every block is live, belongs to layout, and is freed once.
            unsafe {
                checksum += usize::from(block.read());
                allocator.dealloc(block, layout);
            }
        }
    }
    (rounds * LIVE_BLOCKS, checksum)
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let operation = args.get(OPERATION_ARGUMENT).map(String::as_str).unwrap_or("classes");
    let rounds: usize = args.get(ROUNDS_ARGUMENT).map(|value| value.parse().expect("positive rounds")).unwrap_or(DEFAULT_ROUNDS);
    let size: usize = args.get(SIZE_ARGUMENT).map(|value| value.parse().expect("positive allocation size")).unwrap_or(DEFAULT_SIZE);
    assert!(rounds > 0 && size > 0);
    let start = Instant::now();
    let (operations, checksum) = match operation {
        "classes" => classes(rounds),
        "allocate" => allocate(rounds, size),
        _ => panic!("operation must be classes or allocate"),
    };
    let elapsed = start.elapsed().as_nanos();
    println!("{{\"primitive\":\"{operation}\",\"operations\":{operations},\"size\":{size},\"elapsed_ns\":{elapsed},\"ns_per_operation\":{},\"checksum\":{checksum}}}", elapsed as f64 / operations as f64);
}
