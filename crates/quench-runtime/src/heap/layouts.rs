//! Object layouts: every fact an object record shares with the other objects of its kind,
//! interned once. An ordinary object's header used to be two words of its own, `meta`
//! (prototype link and flags) and `auxiliary` (shape and storage form), eight bytes per object
//! for facts that thousands of objects repeat. A record now holds one 32-bit layout id, and the
//! pair lives once in this table: the header word `meta | auxiliary << 32` at the id's entry.
//!
//! The table is process-wide and append-only. An entry is written before its id is handed out
//! and never changes after, so a reader on any thread (a marker, compiled code) reads it with a
//! plain load that the compiler may merge and hoist like any read of immutable memory, and a
//! specializer may treat it as read-only (`staged::Table::Layouts`).
//! Ids name bit patterns, not heap objects: two heaps share an entry when their headers are
//! equal, and an entry outlives the objects that used it. Entry 0 is the all-zero header.
//!
//! Interning goes through a global direct-mapped cache of ids before the global map: a cached
//! id is checked against its own entry, so a racy or stale cache slot only misses, and a header
//! write (a shape transition, a prototype change) costs a hash, two loads and a compare in the
//! common case.

use super::cell::{ProtoLink, header_word};
use crate::staged::object_encoding::{
    CAPACITY_CLASSES, CAPACITY_MASK, CAPACITY_SHIFT, DICTIONARY_STORAGE, EXOTIC_ACCESS, FROZEN, NON_EXTENSIBLE, OBJECT_ARRAY, PROTO_INDEX_BITS,
};
use crate::value_vec::{CONSTANT_FORM, SHAPE_LIMIT, SHAPE_MASK, STORAGE_FORM};
use rustc_hash::FxHashMap;
use std::sync::Mutex;
#[cfg(not(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_vendor = "apple"))))]
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU32, Ordering};

/// Distinct headers the table holds: 32 MiB of address space, resident only where written.
pub const LAYOUT_CAPACITY: usize = 1 << 22;

/// On x86-64 Linux the table sits at a fixed low address, mapped before `main`
/// (`map_fixed_table`), so a read of an entry is one instruction with the address as its
/// displacement: no base to load from the GOT, no register to hold it. The address sits above
/// the range the kernel randomizes a non-PIE program's break into (up to 1 GiB past the image)
/// and below 2 GiB, the reach of a sign-extended 32-bit displacement; a break that grows into
/// it only sends `malloc` to `mmap`.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const FIXED_TABLE: usize = crate::staged::layout_offsets::LAYOUT_TABLE_BASE as usize;

#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
const FIXED_TABLE: usize = crate::staged::layout_offsets::LAYOUT_TABLE_BASE as usize;

/// The table's fixed address is what compiled code reads (`staged::Table::Layouts`), so it is
/// mapped before `main` on every target: on Apple AArch64 the constant is a 32-bit unsigned
/// displacement a row adds, and the 32 MiB mapping lies below the image's 4 GiB base.
#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
extern "C" fn map_fixed_table() {
    // SAFETY: an anonymous private mapping at the address compiled code reads; the kernel
    // replaces whatever the hint found, and the check below aborts on any other address.
    let mapped = unsafe {
        libc::mmap(
            FIXED_TABLE as *mut libc::c_void,
            LAYOUT_CAPACITY * size_of::<u64>(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_FIXED,
            -1,
            0,
        )
    };
    if mapped as usize != FIXED_TABLE {
        eprintln!("quench: cannot map the object layout table at {FIXED_TABLE:#x}");
        std::process::abort();
    }
}

#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static MAP_FIXED_TABLE: extern "C" fn() = map_fixed_table;

/// The layout table's range where it sits at its fixed address, mapped before `main`: a dump of
/// the process (`quench::aot::image_dump`) carries its written pages.
pub fn fixed_table_range() -> Option<(usize, usize)> {
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    return Some((FIXED_TABLE, LAYOUT_CAPACITY * size_of::<u64>()));
    #[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
    None
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
extern "C" fn map_fixed_table() {
    const MAP_FIXED_NOREPLACE: libc::c_int = 0x100000;
    // SAFETY: an anonymous private mapping at an address nothing else may hold (NOREPLACE).
    let mapped = unsafe {
        libc::mmap(
            FIXED_TABLE as *mut libc::c_void,
            LAYOUT_CAPACITY * size_of::<u64>(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | MAP_FIXED_NOREPLACE,
            -1,
            0,
        )
    };
    if mapped as usize != FIXED_TABLE {
        eprintln!("quench: cannot map the object layout table at {FIXED_TABLE:#x}");
        std::process::abort();
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[used]
#[unsafe(link_section = ".init_array")]
static MAP_FIXED_TABLE: extern "C" fn() = map_fixed_table;

#[cfg(not(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_vendor = "apple"))))]
#[repr(C, align(4096))]
struct Table(UnsafeCell<[u64; LAYOUT_CAPACITY]>);
// SAFETY: an entry is written once, under `MAP`'s lock, before its id exists anywhere; every
// read of it follows the id's publication, and no entry is written twice.
#[cfg(not(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_vendor = "apple"))))]
unsafe impl Sync for Table {}

// SAFETY: all-zero bits are a valid table; the zeroed static lands in .bss.
#[cfg(not(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_vendor = "apple"))))]
static TABLE: Table = unsafe { std::mem::zeroed() };
static NEXT: AtomicU32 = AtomicU32::new(1);
static MAP: Mutex<Option<FxHashMap<u64, u32>>> = Mutex::new(None);

const CACHE_BITS: u32 = 12;
static CACHE: [AtomicU32; 1 << CACHE_BITS] = [const { AtomicU32::new(0) }; 1 << CACHE_BITS];

/// The base address of the table, entry `id` at `id * 8` past it.
#[inline(always)]
pub fn table_base() -> *const u64 {
    #[cfg(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_vendor = "apple")))]
    return FIXED_TABLE as *const u64;
    #[cfg(not(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_vendor = "apple"))))]
    return TABLE.0.get().cast();
}

/// The header word of layout `id`.
#[inline(always)]
pub(crate) fn word(id: u32) -> u64 {
    debug_assert!((id as usize) < LAYOUT_CAPACITY);
    // SAFETY: every id came from `intern`, below the capacity, and its entry was written
    // before the id was handed out and never changes (`Table`).
    unsafe { *table_base().add(id as usize) }
}

/// The auxiliary word (shape and storage form) of layout `id`: the low half of its header word
/// (`NATIVE_OBJECT_AUXILIARY_OFFSET`). A specializer reads it at compile time from an id it
/// guards, so the residual tests none of its bits. A word past the table (a cleared hint's
/// sentinel) names no layout: its auxiliary word is 0.
#[inline(always)]
pub fn auxiliary(id: u32) -> u32 {
    if id as usize >= LAYOUT_CAPACITY {
        return 0;
    }
    word(id) as u32
}

/// The layout id of the header `word`.
#[inline(always)]
pub(crate) fn intern(word: u64) -> u32 {
    if word == 0 {
        return 0;
    }
    let slot = (word.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> (u64::BITS - CACHE_BITS)) as usize;
    // Every id the cache holds is a written entry, so its check reads a valid word.
    let id = CACHE[slot].load(Ordering::Relaxed);
    if self::word(id) == word {
        return id;
    }
    intern_slow(word, slot)
}

/// The class word's flag bits beyond shape, link and capacity, in header-word positions: the
/// auxiliary flags (constant mark, exotic access, dictionary storage) in the low half, the meta
/// flags (array, non-extensible, frozen) in the high half. `intern_class` takes them as `flags`.
pub(crate) const CLASS_FLAG_MASK: u64 = (CONSTANT_FORM | EXOTIC_ACCESS | DICTIONARY_STORAGE) as u64
    | ((OBJECT_ARRAY | NON_EXTENSIBLE | FROZEN) as u64) << u32::BITS;

/// The class id of an ordinary object with prototype link `link`, exact shape `shape`, `flags`
/// (`CLASS_FLAG_MASK` bits) and capacity class `capacity` (`0..CAPACITY_CLASSES`): the interned
/// header word, so equal classes share one id as every header does.
///
/// Capacity 0 is a record-table class, the only kind interned before cell classes, and its word
/// has no form bit. Capacity 1 to 5 sets the bit positions `NARROW_FORM`, `EXTRAS_FORM` and
/// `INLINE_FORM` (`CAPACITY_SHIFT`); see `class_capacity` for the overlap until R4-E retires them.
pub(crate) fn intern_class(shape: u32, link: ProtoLink, flags: u64, capacity: u32) -> u32 {
    assert!(shape < SHAPE_LIMIT, "object shape table exhausted");
    assert!((capacity as usize) < CAPACITY_CLASSES, "capacity class out of range");
    assert!(flags & !CLASS_FLAG_MASK == 0, "class flags outside the class word");
    assert!(link.bits() & !PROTO_INDEX_BITS == 0, "prototype link outside its field");
    let meta = link.bits() | (flags >> u32::BITS) as u32;
    let auxiliary = shape | flags as u32 | (capacity << CAPACITY_SHIFT);
    intern(header_word(meta, auxiliary))
}

/// The shape of class `id`: the shape bits of its auxiliary word.
#[allow(dead_code)] // R4-C3/C4 consume it; tests below cover it now.
pub(crate) fn class_shape(id: u32) -> u32 {
    auxiliary(id) & SHAPE_MASK
}

/// The prototype link of class `id`: the index bits of its meta word.
#[allow(dead_code)] // R4-C3/C4 consume it; tests below cover it now.
pub(crate) fn class_link(id: u32) -> ProtoLink {
    ProtoLink::from_bits((word(id) >> u32::BITS) as u32 & PROTO_INDEX_BITS)
}

/// The flags of class `id`, in `CLASS_FLAG_MASK` positions.
#[allow(dead_code)] // R4-C3/C4 consume it; tests below cover it now.
pub(crate) fn class_flags(id: u32) -> u64 {
    word(id) & CLASS_FLAG_MASK
}

/// The raw capacity field of class `id`: bits `CAPACITY_SHIFT..+3` of its auxiliary word, whatever
/// else sets them. Only `intern_class` writes it for a capacity class; a legacy form bit reads here too.
#[allow(dead_code)] // R4-C3/C4 consume it; tests below cover it now.
pub(crate) fn class_capacity_bits(id: u32) -> u32 {
    (auxiliary(id) & CAPACITY_MASK) >> CAPACITY_SHIFT
}

/// The capacity class of class `id` (`0..CAPACITY_CLASSES`): its cell holds `2^k - 1` fields.
///
/// R4-E: until the storage forms retire, the capacity field overlaps `NARROW_FORM` (bit 27),
/// `EXTRAS_FORM` (bit 28) and `INLINE_FORM` (bit 29). Every class with a form bit set reads as
/// capacity 0, the record-table class, so every legacy class does. The field is never nonzero
/// without a form bit, so this reads 0 for every class until R4-E makes the field the capacity.
#[allow(dead_code)] // R4-C3/C4 consume it; tests below cover it now.
pub(crate) fn class_capacity(id: u32) -> u32 {
    if auxiliary(id) & STORAGE_FORM != 0 { 0 } else { class_capacity_bits(id) }
}

#[cold]
#[inline(never)]
fn intern_slow(word: u64, slot: usize) -> u32 {
    let mut map = MAP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let map = map.get_or_insert_with(FxHashMap::default);
    let id = *map.entry(word).or_insert_with(|| {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!((id as usize) < LAYOUT_CAPACITY, "object layout table full");
        // SAFETY: a fresh id below the capacity: no reader can name it yet (`Table`).
        unsafe { *table_base().cast_mut().add(id as usize) = word };
        std::sync::atomic::fence(Ordering::Release);
        id
    });
    CACHE[slot].store(id, Ordering::Relaxed);
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_stable_and_reads_back() {
        assert_eq!(intern(0), 0);
        assert_eq!(word(0), 0);
        let a = intern(0x1234_5678_9abc_def0);
        let b = intern(0x0fed_cba9_8765_4321);
        assert_ne!(a, b);
        assert_eq!(intern(0x1234_5678_9abc_def0), a);
        assert_eq!(word(a), 0x1234_5678_9abc_def0);
        assert_eq!(word(b), 0x0fed_cba9_8765_4321);
        let other = std::thread::spawn(|| intern(0x1234_5678_9abc_def0)).join().unwrap();
        assert_eq!(other, a);
    }

    #[test]
    fn class_round_trips_shape_link_and_flags_at_every_capacity() {
        let link = ProtoLink::from_bits(0x0123_4567 & PROTO_INDEX_BITS);
        let flags = CONSTANT_FORM as u64 | ((FROZEN | OBJECT_ARRAY) as u64) << u32::BITS;
        for capacity in 0..CAPACITY_CLASSES as u32 {
            let id = intern_class(1234, link, flags, capacity);
            assert_eq!(class_shape(id), 1234, "capacity {capacity}");
            assert_eq!(class_link(id), link, "capacity {capacity}");
            assert_eq!(class_flags(id), flags, "capacity {capacity}");
            assert_eq!(class_capacity_bits(id), capacity, "capacity {capacity}");
            assert_eq!(intern_class(1234, link, flags, capacity), id, "interning is stable at capacity {capacity}");
        }
    }

    #[test]
    fn capacity_zero_is_the_record_table_class_of_a_plain_link() {
        // Capacity 0 with no flags is the plain header word of the link and shape.
        let id = ProtoLink::NULL.class(5, 0);
        assert_eq!(word(id), header_word(ProtoLink::NULL.bits(), 5));
        assert_eq!(class_capacity(id), 0);
        assert_eq!(class_flags(id), 0);
        assert_eq!(class_shape(id), 5);
    }

    #[test]
    fn distinct_capacities_are_distinct_classes() {
        let ids: Vec<u32> = (0..CAPACITY_CLASSES as u32).map(|capacity| intern_class(9, ProtoLink::NULL, 0, capacity)).collect();
        for (i, a) in ids.iter().enumerate() {
            for b in &ids[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn capacity_reads_as_zero_until_the_form_bits_retire() {
        // R4-E: capacity 1 to 5 set the form bit positions, so the gated accessor reads 0 (see
        // `class_capacity`); the raw field still carries the capacity.
        for capacity in 1..CAPACITY_CLASSES as u32 {
            let id = intern_class(77, ProtoLink::NULL, 0, capacity);
            assert_eq!(class_capacity_bits(id), capacity);
            assert_eq!(class_capacity(id), 0);
        }
    }
}
