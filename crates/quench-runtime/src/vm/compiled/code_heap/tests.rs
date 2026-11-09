use super::{Block, Chunk, CodeHeap, CodeMemory, Views, CHUNK_BYTES, CODE_GRANULE, REGION_CHUNKS};

fn chunk(heap: &CodeHeap, block: Block) -> Chunk {
    heap.regions[block.region as usize].chunks[block.chunk()]
}

#[test]
fn small_bodies_share_a_chunk_at_granule_spacing() {
    let mut heap = CodeHeap::new();
    let (first, granted) = heap.allocate(1).unwrap();
    let (second, _) = heap.allocate(CODE_GRANULE + 1).unwrap();
    let (third, _) = heap.allocate(0).unwrap();
    assert_eq!(granted, CODE_GRANULE);
    assert_eq!(second.offset, first.offset + CODE_GRANULE as u32);
    assert_eq!(third.offset, second.offset + 2 * CODE_GRANULE as u32);
    assert_eq!(
        chunk(&heap, first),
        Chunk::Small {
            live: 4 * CODE_GRANULE as u32
        }
    );
}

#[test]
fn freed_blocks_serve_equal_and_smaller_bodies_before_the_bump() {
    let mut heap = CodeHeap::new();
    let (keep, _) = heap.allocate(CODE_GRANULE).unwrap();
    let (hole, granted) = heap.allocate(3 * CODE_GRANULE).unwrap();
    let (_, _) = heap.allocate(CODE_GRANULE).unwrap();
    heap.free(hole, granted);
    let (small, _) = heap.allocate(CODE_GRANULE).unwrap();
    let (rest, _) = heap.allocate(2 * CODE_GRANULE).unwrap();
    assert_eq!(small, hole);
    assert_eq!(rest.offset, hole.offset + CODE_GRANULE as u32);
    assert_eq!(
        chunk(&heap, keep),
        Chunk::Small {
            live: 5 * CODE_GRANULE as u32
        }
    );
}

/// Allocates quarter-chunk bodies until one lands in the second chunk: the first chunk's
/// bodies and that next one.
fn fill_first_chunk(heap: &mut CodeHeap) -> (Vec<(Block, usize)>, (Block, usize)) {
    let mut first = Vec::new();
    while first.last().is_none_or(|&(block, _): &(Block, usize)| block.chunk() == 0) {
        first.push(heap.allocate(CHUNK_BYTES / 4).unwrap());
    }
    let next = first.pop().unwrap();
    (first, next)
}

#[test]
fn a_chunk_without_live_bodies_returns_its_pages() {
    let mut heap = CodeHeap::new();
    let (first_chunk, (next, next_granted)) = fill_first_chunk(&mut heap);
    let views = heap.regions[0].views;
    // SAFETY: the writable view of a live block, opened through the platform's write window
    // (Darwin's MAP_JIT page faults a write made outside it).
    Views::writing(|| unsafe { views.writer(first_chunk[0].0.offset as usize).write(1) });
    for (block, granted) in first_chunk {
        heap.free(block, granted);
    }
    assert_eq!(chunk(&heap, Block { region: 0, offset: 0 }), Chunk::Free);
    // Linux returns the pages on release (`MADV_REMOVE`), so the mapping reads zero at once.
    // Darwin's `MADV_FREE` keeps the contents until the system reclaims them.
    #[cfg(target_os = "linux")]
    {
        // SAFETY: the released pages stay mapped and read as zero.
        assert_eq!(unsafe { views.exec.read() }, 0);
    }
    heap.free(next, next_granted);
    assert_eq!(
        heap.allocate(CODE_GRANULE).unwrap().0,
        next,
        "the open chunk rewinds"
    );
}

#[test]
fn large_bodies_take_whole_chunks_and_free_them_together() {
    let mut heap = CodeHeap::new();
    let (small, _) = heap.allocate(CODE_GRANULE).unwrap();
    let (large, granted) = heap.allocate(2 * CHUNK_BYTES + 1).unwrap();
    assert_eq!(
        (large.chunk(), granted),
        (small.chunk() + 1, 3 * CHUNK_BYTES)
    );
    assert_eq!(chunk(&heap, large), Chunk::Head { chunks: 3 });
    heap.free(large, granted);
    assert_eq!(heap.allocate(CHUNK_BYTES + 1).unwrap().0, large);
    let (huge, _) = heap.allocate((REGION_CHUNKS + 1) * CHUNK_BYTES).unwrap();
    assert_eq!(
        (huge.region, heap.regions[1].chunks.len()),
        (1, REGION_CHUNKS + 1)
    );
}

#[test]
fn published_code_executes_from_the_executable_view() {
    #[cfg(target_arch = "x86_64")]
    let body: &[u8] = &[0xb8, 0x2a, 0, 0, 0, 0xc3]; // mov eax, 42; ret
    #[cfg(target_arch = "aarch64")]
    let body: &[u8] = &[0x40, 0x05, 0x80, 0x52, 0xc0, 0x03, 0x5f, 0xd6]; // mov w0, #42; ret
    let first = CodeMemory::new(body).unwrap();
    let second = CodeMemory::new(body).unwrap();
    assert_eq!(second.pointer() as usize % CODE_GRANULE, 0);
    for code in [&first, &second] {
        // SAFETY: the body is a complete function of this signature.
        let function =
            unsafe { std::mem::transmute::<*mut u8, extern "C" fn() -> u32>(code.pointer()) };
        assert_eq!(function(), 42);
    }
}
