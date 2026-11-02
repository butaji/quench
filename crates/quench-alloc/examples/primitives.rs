//! Isolate allocator work before checking an application workload.
use quench_alloc::{class_of, QuenchAlloc, GRANULE_BYTES, MAX_SMALL_BYTES};
use std::{alloc::{GlobalAlloc, Layout}, hint::black_box, time::Instant};

const LIVE_BLOCKS: usize = 256;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let operation = args.get(1).map(String::as_str).unwrap_or("classes");
    let rounds: usize = args.get(2).map(|value| value.parse().expect("positive rounds")).unwrap_or(10_000_000);
    let size: usize = args.get(3).map(|value| value.parse().expect("positive allocation size")).unwrap_or(64);
    assert!(rounds > 0 && size > 0);
    let mut checksum = 0usize;
    let start = Instant::now();
    let operations = match operation {
        "classes" => {
            for index in 0..rounds {
                let size = black_box(index % MAX_SMALL_BYTES + 1);
                checksum += black_box(class_of(size).expect("a small size"));
            }
            rounds
        }
        "allocate" => {
            QuenchAlloc::enable();
            let allocator = QuenchAlloc::new();
            let layout = Layout::from_size_align(size, GRANULE_BYTES).expect("valid layout");
            let mut blocks = [std::ptr::null_mut(); LIVE_BLOCKS];
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
            rounds * LIVE_BLOCKS
        }
        _ => panic!("operation must be classes or allocate"),
    };
    let elapsed = start.elapsed().as_nanos();
    println!("{{\"primitive\":\"{operation}\",\"operations\":{operations},\"size\":{size},\"elapsed_ns\":{elapsed},\"ns_per_operation\":{},\"checksum\":{checksum}}}", elapsed as f64 / operations as f64);
}
