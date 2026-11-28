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
use crate::staged::object_encoding::{CAPACITY_CLASSES, DICTIONARY_STORAGE, EXOTIC_ACCESS, FROZEN, NON_EXTENSIBLE, OBJECT_ARRAY, PROTO_INDEX_BITS, region_field_capacity};
use crate::value_vec::{SHAPE_LIMIT, SHAPE_MASK};
use rustc_hash::FxHashMap;
use std::sync::Mutex;
#[cfg(not(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_vendor = "apple"))))]
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Distinct headers the table holds: 32 MiB of address space, resident only where written.
pub const LAYOUT_CAPACITY: usize = 1 << 22;

/// Class ids are below the capacity, so they never carry the class key bit (`staged::field_cache_layout::CLASS_KEY`).
const _: () = assert!(LAYOUT_CAPACITY <= crate::staged::field_cache_layout::CLASS_KEY as usize);

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
/// the process (`native_link::image_dump`) carries its written pages.
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

/// The class word's flag bits beyond shape and link, in header-word positions: the
/// auxiliary flags (exotic access, dictionary storage) in the low half, the meta
/// flags (array, non-extensible, frozen) in the high half. `intern_class` takes them as `flags`.
pub(crate) const CLASS_FLAG_MASK: u64 = (EXOTIC_ACCESS | DICTIONARY_STORAGE) as u64
    | ((OBJECT_ARRAY | NON_EXTENSIBLE | FROZEN) as u64) << u32::BITS;

/// The capacity-region bits of a class word, in its auxiliary word: bits 22 to 24 hold the cell's region `k`
/// (1 to 5, `staged::object_encoding::CAPACITY_CLASSES`) as `k << CLASS_REGION_SHIFT`. Shapes occupy bits 0 to 21
/// (`value_vec::SHAPE_MASK`), and no record word sets these bits, so a class word's region is only ever a class's.
/// The region is part of the class's identity: two classes equal in shape, link and flags but in different regions
/// are different ids, so every cell of one class holds one capacity, and a cell keeps its region through its
/// transitions (`class_region` is read from the class it leaves).
pub(crate) use crate::staged::object_encoding::{CLASS_REGION_MASK, CLASS_REGION_SHIFT};

/// The class id of an ordinary object with prototype link `link`, exact shape `shape`, `flags` (`CLASS_FLAG_MASK`
/// bits) and capacity region `region` (1 to 5, a class cell's region): the interned header word, so equal classes
/// share one id as every header does. The region is in the class word (`CLASS_REGION_MASK`), so it is part of the
/// class identity; the cell's capacity is `class_space::region_capacity` of its granule, which equals
/// `region_field_capacity(region)`.
pub(crate) fn intern_class(shape: u32, link: ProtoLink, flags: u64, region: u32) -> u32 {
    assert!(shape < SHAPE_LIMIT, "object shape table exhausted");
    assert!(flags & !CLASS_FLAG_MASK == 0, "class flags outside the class word");
    assert!(link.bits() & !PROTO_INDEX_BITS == 0, "prototype link outside its field");
    assert!((1..CAPACITY_CLASSES as u32).contains(&region), "a class cell's region is a cell region");
    let meta = link.bits() | (flags >> u32::BITS) as u32;
    let auxiliary = shape | flags as u32 | (region << CLASS_REGION_SHIFT);
    intern(header_word(meta, auxiliary))
}

/// The shape of class `id`: the shape bits of its auxiliary word.
pub(crate) fn class_shape(id: u32) -> u32 {
    auxiliary(id) & SHAPE_MASK
}

/// The capacity region of class `id` (1 to 5): the region bits of its auxiliary word. Only the ids of cell classes
/// (`intern_class`) carry one; a record's layout reads 0.
pub(crate) fn class_region(id: u32) -> u32 {
    (auxiliary(id) & CLASS_REGION_MASK) >> CLASS_REGION_SHIFT
}

/// The spill slot of a class cell of region `region` holding `fields` fields: `None` where the fields fit the
/// region's capacity, else the capacity less one, the slot whose field word holds the spill pointer. The class
/// records the point at which its own fields spill (`Heap::class_spill_from`); the shape length is the table's.
pub(crate) fn spill_from(region: u32, fields: usize) -> Option<u32> {
    let capacity = region_field_capacity(region);
    (fields > capacity).then(|| capacity as u32 - 1)
}

/// Whether class `id` is dictionary storage (`DICTIONARY_STORAGE`): its cell's slot 0 holds the dictionary word, and
/// its fields are the table's (`heap::dictionary_cell`).
#[inline(always)]
pub(crate) fn class_is_dictionary(id: u32) -> bool {
    class_flags(id) & DICTIONARY_STORAGE as u64 != 0
}

/// The spill slot of a live cell of class `class` holding `fields` fields: `None` for a dictionary class, whose slot 0
/// is the dictionary word and whose fields are the table's, else `spill_from` of the class's region.
pub(crate) fn class_spill(class: u32, fields: usize) -> Option<u32> {
    if class_is_dictionary(class) { None } else { spill_from(class_region(class), fields) }
}

/// The prototype link of class `id`: the index bits of its meta word.
pub(crate) fn class_link(id: u32) -> ProtoLink {
    ProtoLink::from_bits((word(id) >> u32::BITS) as u32 & PROTO_INDEX_BITS)
}

/// The flags of class `id`, in `CLASS_FLAG_MASK` positions.
#[allow(dead_code)] // R4-C3/C4 consume it; tests below cover it now.
pub(crate) fn class_flags(id: u32) -> u64 {
    word(id) & CLASS_FLAG_MASK
}

/// The method vectors of the classes, dense by layout id: the key is the header a class's instances carry (`word`), not the
/// class id, so a synthetic class (`AotFacts::classes`) takes the vector of its layout like any other class. Entry `layout` is
/// the address of that layout's vector, whose slot `k` is the `CalleeRecord` address of the method at prototype slot `k`
/// (`AotFacts::prototype_layout`). Filled by the loader (`set_class_methods`), read by compiled code at
/// `class_methods_base()` as the closed-set indirect call's chain: header layout id, then `[layout]`, then `[slot]`.
static CLASS_METHODS: [AtomicUsize; LAYOUT_CAPACITY] = [const { AtomicUsize::new(0) }; LAYOUT_CAPACITY];

/// The base of the class method table: entry `class` at `class * size_of::<usize>()` past it.
#[inline(always)]
pub fn class_methods_base() -> *const AtomicUsize {
    CLASS_METHODS.as_ptr()
}

/// The address of class `class`'s method vector, 0 where none is filled (or the id is past the table).
pub fn class_methods(class: u32) -> usize {
    CLASS_METHODS.get(class as usize).map_or(0, |entry| entry.load(Ordering::Acquire))
}

/// Fills class `class`'s method vector at `vector`, whose slots are `CalleeRecord` addresses and which must outlive the process.
pub fn set_class_methods(class: u32, vector: usize) {
    assert!((class as usize) < LAYOUT_CAPACITY, "class method table full");
    CLASS_METHODS[class as usize].store(vector, Ordering::Release);
}

/// The words of every layout id interned so far, id 1 first: a heap snapshot keeps them, since its class cells'
/// headers name classes by id (`restore_words`).
pub(crate) fn snapshot_words() -> Vec<u64> {
    let _map = MAP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    (1..NEXT.load(Ordering::Relaxed)).map(word).collect()
}

/// Gives each of a snapshot's layout words (`snapshot_words`) its id again, so the restored class cells' headers name
/// the classes they named when written. False, writing nothing more, where this process already gave one of those ids
/// another word or one of those words another id: the snapshot then does not fit this process.
pub(crate) fn restore_words(words: &[u64]) -> bool {
    let mut map = MAP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let map = map.get_or_insert_with(FxHashMap::default);
    let next = NEXT.load(Ordering::Relaxed) as usize;
    let fits = words.len() < LAYOUT_CAPACITY && words.iter().enumerate().all(|(index, &word)| {
        let id = index as u32 + 1;
        let held = map.get(&word).copied();
        if (id as usize) < next { self::word(id) == word } else { held.is_none() }
    });
    if !fits { return false; }
    for (index, &word) in words.iter().enumerate().skip(next.saturating_sub(1)) {
        let id = index as u32 + 1;
        // SAFETY: an id at or past `NEXT`, below the capacity: no reader names it yet (`Table`).
        unsafe { *table_base().cast_mut().add(id as usize) = word };
        map.insert(word, id);
    }
    std::sync::atomic::fence(Ordering::Release);
    NEXT.fetch_max(words.len() as u32 + 1, Ordering::Relaxed);
    true
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
    fn snapshot_layout_ids_restore_where_they_fit() {
        let a = intern(0x0aaa_0000_0000_0001);
        let words = snapshot_words();
        assert_eq!(words[a as usize - 1], 0x0aaa_0000_0000_0001);
        // The same process fits its own snapshot: every id already holds its word.
        assert!(restore_words(&words));
        // A snapshot naming a word under an id this process gave another word does not fit.
        let mut taken = words.clone();
        taken[a as usize - 1] = 0x0bbb_0000_0000_0002;
        assert!(!restore_words(&taken));
        // Ids past this process's own are given their words, and intern then finds them. Other tests intern in
        // parallel, so the snapshot is retaken until no id was handed out between the read and the restore.
        let grown = (0..8).find_map(|_| {
            let mut grown = snapshot_words();
            grown.push(0x0ccc_0000_0000_0003);
            restore_words(&grown).then_some(grown)
        }).expect("no interleaved intern in eight tries");
        assert_eq!(intern(0x0ccc_0000_0000_0003) as usize, grown.len());
    }

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
    fn class_round_trips_shape_link_and_flags() {
        let link = ProtoLink::from_bits(0x0123_4567 & PROTO_INDEX_BITS);
        let flags = EXOTIC_ACCESS as u64 | ((FROZEN | OBJECT_ARRAY) as u64) << u32::BITS;
        let id = intern_class(1234, link, flags, 3);
        assert_eq!(class_shape(id), 1234);
        assert_eq!(class_link(id), link);
        assert_eq!(class_flags(id), flags);
        assert_eq!(class_region(id), 3);
        assert_eq!(intern_class(1234, link, flags, 3), id, "interning is stable");
        assert_ne!(intern_class(1235, link, flags, 3), id, "a shape is part of the class");
        assert_ne!(intern_class(1234, link, flags, 4), id, "a region is part of the class");
    }

    #[test]
    fn plain_class_is_the_plain_header_word_of_link_and_shape() {
        let id = ProtoLink::NULL.class(5, 1);
        assert_eq!(word(id), header_word(ProtoLink::NULL.bits(), 5 | (1 << CLASS_REGION_SHIFT)));
        assert_eq!(class_flags(id), 0);
        assert_eq!(class_shape(id), 5);
        assert_eq!(class_region(id), 1);
    }

    #[test]
    fn spill_is_the_last_slot_of_an_overfull_region() {
        assert_eq!(spill_from(1, 1), None);
        assert_eq!(spill_from(1, 2), Some(0));
        assert_eq!(spill_from(3, 7), None);
        assert_eq!(spill_from(3, 8), Some(6));
    }
}
