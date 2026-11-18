//! Which Number rows the library holds, as codes (the constants of `codes.rs`): the one
//! definition `build.rs` generates the stencils from and the runtime looks rows up by. A row
//! outside it has no stencil, and a region that needs one is declined like any other row the
//! library lacks.

#[path = "codes.rs"]
mod codes;
pub use codes::*;

use std::collections::BTreeSet;

/// A row as codes: kind, destination, operands, speculate.
pub type Code = (u8, u8, u8, u8, bool);

/// The operands a kind reads, the others fixed to `MEM`.
fn read_operands(kind: u8, dst: u8, a: u8, b: u8) -> (u8, u8, u8) {
    match kind {
        _ if is_copy(kind) => (dst, a, MEM),
        _ if is_compare(kind) => (MEM, a, b),
        JUMP | JUMP_POLL | JUMP_WATCH | POLL | TAG_INIT => (MEM, MEM, MEM),
        GUARD | NOT | NEG | SQRT => (dst, a, MEM),
        JUMP_TRUE | JUMP_FALSE | SPILL => (MEM, a, MEM),
        _ => (dst, a, b),
    }
}

/// Whether the row speculates: double rows never do (a bitwise row reading a pinned double
/// still speculates its int32 side), a copy or control row always does.
fn speculates(kind: u8, places: [u8; 3], spec: bool) -> bool {
    let floats = places.iter().any(|&place| place >= PINF);
    if floats && kind <= NE && !is_bitwise(kind) {
        false
    } else if is_copy(kind) || kind >= JUMP {
        true
    } else {
        spec
    }
}

/// The row whose stencil computes `code`: operands and speculation a row ignores are fixed.
pub fn canonical((kind, dst, a, b, spec): Code) -> Code {
    // The operands a kind ignores are fixed first, so the float test sees only the ones it reads.
    let (dst, a, b) = read_operands(kind, dst, a, b);
    (kind, dst, a, b, speculates(kind, [dst, a, b], spec))
}

/// The name of a row's stencil symbol.
pub fn name((kind, dst, a, b, spec): Code) -> String {
    format!("n_{kind}_{dst}_{a}_{b}_{}", u8::from(spec))
}

/// The places an operand or destination may be in.
struct Places {
    ints_dst: Vec<u8>,
    ints_src: Vec<u8>,
    floats_dst: Vec<u8>,
    floats_src: Vec<u8>,
    floats_pins: Vec<u8>,
}

fn int_pins() -> impl Iterator<Item = u8> {
    PIN..PIN + INT_PINS
}

fn double_pins() -> impl Iterator<Item = u8> {
    PINF..PINF + DOUBLE_PINS
}

impl Places {
    fn all() -> Places {
        Places {
            ints_dst: [MEM, MEMINT].into_iter().chain(int_pins()).collect(),
            ints_src: [MEM, MEMINT, CONST, IMM].into_iter().chain(int_pins()).collect(),
            floats_dst: double_pins().chain([MEM]).collect(),
            floats_src: [MEM, CONST, MEMINT].into_iter().chain(int_pins()).chain(double_pins()).collect(),
            floats_pins: double_pins().collect(),
        }
    }
}

/// A bitwise row reads a pinned double as its ToInt32 (`x | 0`): either operand may be one.
fn bitwise_over_doubles(places: &Places, rows: &mut BTreeSet<Code>) {
    for kind in AND..=USHR {
        for &dst in &places.ints_dst {
            let sources = || places.ints_src.iter().chain(&places.floats_pins);
            for (a, b) in sources().flat_map(|&a| sources().map(move |&b| (a, b))) {
                if a >= PINF || b >= PINF {
                    rows.insert(canonical((kind, dst, a, b, true)));
                    // A general row (an int32 speculation that failed) writes a frame word.
                    rows.insert(canonical((kind, MEM, a, b, false)));
                }
            }
        }
    }
}

/// Every kind in `kinds` over every destination and operand pair from `dsts` and `srcs`.
fn product(rows: &mut BTreeSet<Code>, kinds: &[u8], dsts: &[u8], srcs: &[u8], specs: &[bool]) {
    for &kind in kinds {
        for &dst in dsts {
            for (&a, &b) in srcs.iter().flat_map(|a| srcs.iter().map(move |b| (a, b))) {
                rows.extend(specs.iter().map(|&spec| canonical((kind, dst, a, b, spec))));
            }
        }
    }
}

/// The arithmetic, bitwise, compare and copy rows over ints and over doubles.
fn products(places: &Places, rows: &mut BTreeSet<Code>) {
    let arith: Vec<u8> = (ADD..=MUL).collect();
    let bitwise: Vec<u8> = (AND..=USHR).collect();
    let compare: Vec<u8> = (LT..=NE).collect();
    let copy: Vec<u8> = (MOVE..=COPY).collect();
    for kinds in [&arith, &bitwise, &compare, &copy, &vec![REM]] {
        product(rows, kinds, &places.ints_dst, &places.ints_src, &[true, false]);
    }
    let double_arith: Vec<u8> = (ADD..=DIV).collect();
    for kinds in [&double_arith, &compare, &copy] {
        product(rows, kinds, &places.floats_dst, &places.floats_src, &[false]);
    }
}

/// Jumps, entry tagging, guards and spills (of a pinned double to a canonical word, of a pinned
/// int32 back to a tagged int word).
fn control(places: &Places, rows: &mut BTreeSet<Code>) {
    for kind in [JUMP, JUMP_POLL, JUMP_WATCH, POLL, TAG_INIT] {
        rows.insert(canonical((kind, MEM, MEM, MEM, true)));
    }
    for &dst in places.floats_dst.iter().chain(&places.ints_dst) {
        rows.insert(canonical((GUARD, dst, MEM, MEM, true)));
    }
    for a in int_pins().chain(double_pins()) {
        rows.insert(canonical((SPILL, MEM, a, MEM, true)));
    }
}

/// The truthiness jumps over every operand a truthiness is read from, the logical negation into
/// a frame slot, and the Number negation over every destination.
fn truthiness(places: &Places, rows: &mut BTreeSet<Code>) {
    let tested: Vec<u8> = [MEM, MEMINT].into_iter().chain(int_pins()).chain(double_pins()).collect();
    for &a in &tested {
        rows.extend([JUMP_TRUE, JUMP_FALSE].map(|kind| canonical((kind, MEM, a, MEM, true))));
        rows.insert(canonical((NOT, MEM, a, MEM, true)));
    }
    let dsts: Vec<u8> = places.ints_dst.iter().chain(&places.floats_dst).copied().collect();
    let srcs: Vec<u8> = places.ints_src.iter().copied().chain(double_pins()).collect();
    product(rows, &[NEG, SQRT], &dsts, &srcs, &[true]);
}

/// Every row the library holds, canonical, once.
pub fn library() -> Vec<Code> {
    let places = Places::all();
    let mut rows = BTreeSet::new();
    bitwise_over_doubles(&places, &mut rows);
    products(&places, &mut rows);
    control(&places, &mut rows);
    truthiness(&places, &mut rows);
    rows.into_iter().collect()
}

/// A heap row as codes: the heap operation (`heap_rows.rs`), entry-record variant, destination
/// and the two operand places.
pub type HeapCode = (u8, bool, u8, u8, u8);

/// How a heap row reads its entry flag: it ignores it, keeps the site's, or is always an entry.
#[derive(Clone, Copy)]
enum Entry {
    Never,
    Kept,
    Always,
}

/// The places a heap row reads: its entry flag and which of its destination and operands.
#[derive(Clone, Copy)]
struct Uses {
    entry: Entry,
    dst: bool,
    a: bool,
    b: bool,
}

const fn uses(entry: Entry, dst: bool, a: bool, b: bool) -> Uses {
    Uses { entry, dst, a, b }
}

/// Each group of heap rows with the places it reads; a row in none reads nothing.
const HEAP_USES: &[(Uses, &[std::ops::RangeInclusive<u8>])] = &[
    (uses(Entry::Never, true, false, false), &[CAPTURE_ROW..=CAPTURE_ROW, ENV_ROW..=ENV_ROW, FIELD_ROW..=FIELD_ROW, LENGTH_ROW..=LENGTH_ROW, THIS_ROW..=THIS_ROW,
        LOAD_LOCAL_ROW..=LOAD_LOCAL_ROW, LOAD_CHECKED_ROW..=LOAD_CHECKED_ROW, GLOBAL_READ_ROW..=GLOBAL_READ_ROW, LEXICAL_READ_ROW..=LEXICAL_READ_ROW,
        CONSTANT_ROW..=CONSTANT_ROW, CALLEE_CAPTURE_ROW..=CALLEE_CAPTURE_ROW]),
    (uses(Entry::Kept, false, true, false), &[GLOBAL_STORE_ROW..=GLOBAL_STORE_ROW, SITE_STORE_ROW..=SITE_STORE_ROW, THIS_STORE_ROW..=THIS_STORE_ROW,
        IC_SET_ROW..=IC_SET_ROW, IC_SET_INT_ROW..=IC_SET_INT_ROW]),
    (uses(Entry::Kept, true, false, false), &[SITE_ROW..=SITE_ROW, THIS_SITE_ROW..=THIS_SITE_ROW, HELD_ROW..=HELD_ROW, IC_GET_ROW..=IC_GET_ROW, IC_GET_HOLDER_ROW..=IC_GET_HOLDER_ROW]),
    (uses(Entry::Kept, true, false, true), &[INDEX_ROW..=INDEX_ROW]),
    (uses(Entry::Always, true, false, true), &[TYPED_INDEX_ROW..=TYPED_INDEX_LAST]),
    (uses(Entry::Always, false, true, true), &[TYPED_STORE_ROW..=TYPED_STORE_LAST]),
    (uses(Entry::Always, true, false, false), &[TYPED_LENGTH_ROW..=TYPED_LENGTH_ROW]),
    (uses(Entry::Kept, false, true, true), &[STORE_ROW..=STORE_ROW]),
    (uses(Entry::Never, false, true, false), &[STORE_LOCAL_ROW..=STORE_LOCAL_ROW, STORE_CHECKED_ROW..=STORE_CHECKED_ROW, CAPTURE_SET_ROW..=CAPTURE_SET_ROW,
        ENV_SET_ROW..=ENV_SET_ROW, ENV_DEFINE_ROW..=ENV_DEFINE_ROW,
        APPEND_ROW..=APPEND_ROW, LEXICAL_WRITE_ROW..=LEXICAL_WRITE_ROW]),
    (uses(Entry::Kept, false, false, false), &[ADD_PACKED_ROW..=ADD_PACKED_ROW, ADD_ARENA_ROW..=ADD_ARENA_ROW, ADD_EITHER_ROW..=ADD_EITHER_ROW,
        THIS_ADD_PACKED_ROW..=THIS_ADD_PACKED_ROW, THIS_ADD_ARENA_ROW..=THIS_ADD_ARENA_ROW, THIS_ADD_EITHER_ROW..=THIS_ADD_EITHER_ROW]),
];

/// The heap row whose stencil computes `code`: the places a row ignores are fixed.
pub fn heap_canonical((heap, entry, dst, a, b): HeapCode) -> HeapCode {
    let row = HEAP_USES.iter().find(|(_, rows)| rows.iter().any(|rows| rows.contains(&heap))).map_or(uses(Entry::Never, false, false, false), |(row, _)| *row);
    let entry = match row.entry {
        Entry::Never => false,
        Entry::Kept => entry,
        Entry::Always => true,
    };
    (heap, entry, if row.dst { dst } else { MEM }, if row.a { a } else { MEM }, if row.b { b } else { MEM })
}

/// The name of a heap row's stencil symbol.
pub fn heap_name((heap, entry, dst, a, b): HeapCode) -> String {
    format!("h_{heap}_{}_{dst}_{a}_{b}", u8::from(entry))
}

/// The inline-cache rows (`ic_rows.rs`), each as the last entry of its site (`entry` false) and
/// as an entry followed by another (`entry` true, which hands a miss to the next row).
fn cache_rows(dsts: &[u8], srcs: &[u8], rows: &mut BTreeSet<HeapCode>) {
    for chained in [false, true] {
        for &dst in dsts {
            rows.extend([IC_GET_ROW, IC_GET_HOLDER_ROW].map(|heap| heap_canonical((heap, chained, dst, MEM, MEM))));
        }
        for &a in srcs {
            rows.extend([IC_SET_ROW, IC_SET_INT_ROW].map(|heap| heap_canonical((heap, chained, MEM, a, MEM))));
        }
    }
}

/// The typed element rows: each native kind's read over a Number destination (the place of a Number the row
/// reads) and its store, and the count read.
fn typed_rows(dsts: &[u8], srcs: &[u8], rows: &mut BTreeSet<HeapCode>) {
    let floats = Places::all().floats_dst;
    for kind in (0..TYPED_KINDS).filter(|kind| !NON_NATIVE_KINDS.contains(kind)) {
        for &dst in &floats {
            rows.extend(srcs.iter().map(|&b| heap_canonical((TYPED_INDEX_ROW + kind, true, dst, MEM, b))));
        }
        for (&a, &b) in srcs.iter().flat_map(|a| srcs.iter().map(move |b| (a, b))) {
            rows.insert(heap_canonical((TYPED_STORE_ROW + kind, true, MEM, a, b)));
        }
    }
    rows.extend(dsts.iter().map(|&dst| heap_canonical((TYPED_LENGTH_ROW, true, dst, MEM, MEM))));
}

/// Every heap row the library holds, canonical, once.
pub fn heap_library() -> Vec<HeapCode> {
    let places = Places::all();
    let everywhere = |sources: &[u8]| -> Vec<u8> { sources.iter().copied().chain(double_pins()).collect() };
    let (dsts, srcs) = (everywhere(&places.ints_dst), everywhere(&places.ints_src));
    let mut rows = BTreeSet::new();
    for entry in [false, true] {
        for &dst in &dsts {
            for heap in [CAPTURE_ROW, INDEX_ROW, FIELD_ROW, SITE_ROW, LENGTH_ROW, THIS_SITE_ROW, HELD_ROW, GLOBAL_READ_ROW, CONSTANT_ROW] {
                rows.extend(srcs.iter().map(|&b| heap_canonical((heap, entry, dst, MEM, b))));
            }
        }
        for (&a, &b) in srcs.iter().flat_map(|a| srcs.iter().map(move |b| (a, b))) {
            rows.insert(heap_canonical((STORE_ROW, entry, MEM, a, b)));
        }
        for heap in [GLOBAL_STORE_ROW, SITE_STORE_ROW, THIS_STORE_ROW] {
            rows.extend(srcs.iter().map(|&a| heap_canonical((heap, entry, MEM, a, MEM))));
        }
    }
    typed_rows(&dsts, &srcs, &mut rows);
    for &dst in &dsts {
        rows.extend([THIS_ROW, LOAD_LOCAL_ROW, LOAD_CHECKED_ROW, ENV_ROW, LEXICAL_READ_ROW, CALLEE_CAPTURE_ROW].map(|heap| heap_canonical((heap, false, dst, MEM, MEM))));
    }
    for heap in [STORE_LOCAL_ROW, STORE_CHECKED_ROW, CAPTURE_SET_ROW, ENV_SET_ROW, ENV_DEFINE_ROW, APPEND_ROW, LEXICAL_WRITE_ROW] {
        rows.extend(srcs.iter().map(|&a| heap_canonical((heap, false, MEM, a, MEM))));
    }
    for heap in ADD_ROWS {
        rows.extend([false, true].map(|chained| heap_canonical((heap, chained, MEM, MEM, MEM))));
    }
    rows.extend(NAME_ROWS.map(|heap| heap_canonical((heap, false, MEM, MEM, MEM))));
    rows.extend([ARRAY_ROW, IDENTITY_ROW, LIVE_ROW, REALM_ROW, OBJECT_ROW, KEY_ROW, FOR_IN_ROW, TYPEOF_INDEX_ROW, TYPEOF_STRING_ROW, INSTANCEOF_ROW, TYPEOF_IS_ROW, TYPEOF_ISNOT_ROW].map(|heap| heap_canonical((heap, false, MEM, MEM, MEM))));
    cache_rows(&dsts, &srcs, &mut rows);
    rows.into_iter().collect()
}
