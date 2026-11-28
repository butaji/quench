//! Overflow prototype links: prototypes whose record id lies past their family's link window.
//!
//! An object's prototype is a 28-bit link (`PROTO_INDEX_BITS` in `cell.rs`) stored in the `meta`
//! word of its layout. The link space is carved into one window per record family (legacy,
//! array, object, function, promise, iterator, collection), each a power of two aligned to its
//! size, so the hot readers decode a link with a compare and a mask (`ProtoLink::
//! object_record_id`, `staged/object.rs` `RECORD_LINKS`, the JIT's `NATIVE_OBJECT_PROTO_*`).
//!
//! The windows used to cap the record spaces themselves: a record past its window could not be
//! named by a link. Now a record space grows to the full `u32` id width, and a prototype whose
//! id lies past its window takes a link from the overflow range instead: the codes between
//! `PROTO_RECORD_END` and the null/undefined sentinels (16,777,214 codes). An overflow link is an
//! index into this table, which holds the prototype's `Value` bits.
//!
//! - Hot paths are unchanged. Every prototype inside its window keeps its window link, so the
//!   compare-and-mask readers and link-equality guards never see the table. An overflow link
//!   simply fails `object_record_id` (it is outside the object window), so a fast path that
//!   needs the record id falls back to the general decode, which loads from the table.
//! - Links are bits, not meanings. A link already names a heap-local record id: the same meta
//!   bits mean different prototypes in different heaps, and each heap decodes them in its own
//!   record spaces. The table maps a link to `Value` bits, which every heap then reads in its
//!   own spaces, so one process-wide table serves every heap, exactly as the window links do.
//!   Interning keys on the `Value` bits, so equal links always name equal bits.
//! - No reuse. An index is never handed out twice. A layout id, a code dependency (holder
//!   watch, construct plan) or a compiled guard may hold a link after its prototype died; if
//!   the index were reissued to another `Value`, a stale guard could pass on a new prototype.
//!   The table therefore only grows. It costs eight bytes per distinct out-of-window prototype
//!   ever created, which only programs with more than tens of millions of live records of one
//!   family can produce, and its pages are allocated in chunks on first use.
//! - Reads are lock-free (an atomic load from a chunk that never moves); interning takes a lock
//!   and runs only when an out-of-window record first becomes a prototype.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Entries per chunk, allocated on first use.
const CHUNK_ENTRIES: usize = 1 << 12;
/// Chunks: enough for every overflow code.
const CHUNKS: usize = 1 << 12;
/// Overflow entries the table can hold.
pub(super) const OVERFLOW_ENTRIES: usize = CHUNK_ENTRIES * CHUNKS;

static TABLE: [OnceLock<Box<[AtomicU64]>>; CHUNKS] = [const { OnceLock::new() }; CHUNKS];
static INTERNED: Mutex<Option<HashMap<u64, u32>>> = Mutex::new(None);

/// The overflow index naming the `Value` with bits `bits`, interned on first use; `None` when
/// `limit` indices are taken (`limit <= OVERFLOW_ENTRIES`).
pub(super) fn intern(bits: u64, limit: usize) -> Option<u32> {
    let mut interned = INTERNED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let interned = interned.get_or_insert_with(HashMap::new);
    if let Some(&index) = interned.get(&bits) {
        return Some(index);
    }
    let index = interned.len();
    if index >= limit.min(OVERFLOW_ENTRIES) {
        return None;
    }
    let chunk = TABLE[index / CHUNK_ENTRIES]
        .get_or_init(|| (0..CHUNK_ENTRIES).map(|_| AtomicU64::new(0)).collect());
    chunk[index % CHUNK_ENTRIES].store(bits, Ordering::Release);
    let index = index as u32;
    interned.insert(bits, index);
    Some(index)
}

/// The `Value` bits overflow index `index` names (`intern`'s inverse).
#[inline]
pub(super) fn bits(index: u32) -> u64 {
    let index = index as usize;
    TABLE[index / CHUNK_ENTRIES]
        .get()
        .expect("an overflow prototype link names an interned entry")[index % CHUNK_ENTRIES]
        .load(Ordering::Acquire)
}
