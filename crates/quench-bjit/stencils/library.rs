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
    for kinds in [&arith, &bitwise, &compare, &copy, &vec![REM], &vec![IMUL]] {
        product(rows, kinds, &places.ints_dst, &places.ints_src, &[true, false]);
    }
    let double_arith: Vec<u8> = (ADD..=DIV).collect();
    for kinds in [&double_arith, &compare, &copy, &vec![MIN, MAX]] {
        product(rows, kinds, &places.floats_dst, &places.floats_src, &[false]);
    }
}

/// The int32 `+` and `-` the planner proved exact (`ADD_NO_OVERFLOW`, `SUB_NO_OVERFLOW`): int operands and an int
/// destination only, always speculating, as no overflow leaves them.
fn exact_arithmetic(places: &Places, rows: &mut BTreeSet<Code>) {
    product(rows, &[ADD_NO_OVERFLOW, SUB_NO_OVERFLOW], &places.ints_dst, &places.ints_src, &[true]);
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
    product(rows, &[NEG, SQRT, FLOOR, CEIL, TRUNC, ABS, ROUND], &dsts, &srcs, &[true]);
    // `Math.clz32` writes an int32 (0..=32) to an int destination, from an int32 or a Number.
    for &dst in &places.ints_dst {
        rows.extend(srcs.iter().map(|&a| canonical((CLZ32, dst, a, MEM, true))));
    }
}

/// Every row the library holds, canonical, once.
pub fn library() -> Vec<Code> {
    let places = Places::all();
    let mut rows = BTreeSet::new();
    bitwise_over_doubles(&places, &mut rows);
    products(&places, &mut rows);
    exact_arithmetic(&places, &mut rows);
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

/// The places one operand of a heap row ranges over: a row that does not read the operand fixes it to `MEM`.
#[derive(Clone, Copy)]
enum Slot {
    Unread,
    /// Any destination: every int place and every double register.
    Dst,
    /// A typed element read's destination: a double register or `MEM`.
    Float,
    /// Any operand: every int place, the constant and immediate, and every double register.
    Src,
}

impl Slot {
    /// The places the slot ranges over.
    fn set(self, places: &Places) -> Vec<u8> {
        match self {
            Slot::Unread => vec![MEM],
            Slot::Dst => places.ints_dst.iter().copied().chain(double_pins()).collect(),
            Slot::Float => places.floats_dst.clone(),
            Slot::Src => places.ints_src.iter().copied().chain(double_pins()).collect(),
        }
    }
}

/// The places a heap row reads: its entry flag and the places each operand ranges over.
#[derive(Clone, Copy)]
struct Uses {
    entry: Entry,
    dst: Slot,
    a: Slot,
    b: Slot,
}

const fn uses(entry: Entry, dst: Slot, a: Slot, b: Slot) -> Uses {
    Uses { entry, dst, a, b }
}

/// Each group of heap rows with the operands it reads, and the places each reads. Every row of `heap_library`
/// is named in exactly one group; a row in none reads nothing. Adding a heap kind is one entry here, and adding
/// a place to a `Slot` set reaches every row that reads it.
const HEAP_USES: &[(Uses, &[std::ops::RangeInclusive<u8>])] = &[
    // The frame, environment, global and constant reads of one destination.
    (uses(Entry::Never, Slot::Dst, Slot::Unread, Slot::Unread), &[CAPTURE_ROW..=CAPTURE_ROW, ENV_ROW..=ENV_ROW, FIELD_ROW..=FIELD_ROW, LENGTH_ROW..=LENGTH_ROW, THIS_ROW..=THIS_ROW,
        LOAD_LOCAL_ROW..=LOAD_LOCAL_ROW, LOAD_CHECKED_ROW..=LOAD_CHECKED_ROW, GLOBAL_READ_ROW..=GLOBAL_READ_ROW, LEXICAL_READ_ROW..=LEXICAL_READ_ROW,
        CONSTANT_ROW..=CONSTANT_ROW, CALLEE_CAPTURE_ROW..=CALLEE_CAPTURE_ROW, EMPTY_OBJECT_ROW..=EMPTY_OBJECT_ROW, EMPTY_ARRAY_ROW..=EMPTY_ARRAY_ROW]),
    (uses(Entry::Kept, false, true, false), &[GLOBAL_STORE_ROW..=GLOBAL_STORE_ROW, SITE_STORE_ROW..=SITE_STORE_ROW, THIS_STORE_ROW..=THIS_STORE_ROW,
        IC_SET_ROW..=IC_SET_ROW, IC_SET_INT_ROW..=IC_SET_INT_ROW, IC_KNOWN_SET_ROW..=IC_KNOWN_SET_INT_ROW]),
    // The reads of one destination from a site, the receiver's site, a held field or an inline cache.
    (uses(Entry::Kept, Slot::Dst, Slot::Unread, Slot::Unread), &[SITE_ROW..=SITE_ROW, THIS_SITE_ROW..=THIS_SITE_ROW, HELD_ROW..=HELD_ROW, IC_GET_ROW..=IC_GET_ROW, IC_GET_HOLDER_ROW..=IC_GET_HOLDER_ROW, IC_KNOWN_GET_ROW..=IC_KNOWN_GET_ROW]),
    (uses(Entry::Kept, Slot::Dst, Slot::Unread, Slot::Src), &[INDEX_ROW..=INDEX_ROW]),
    // `charCodeAt` reads its receiver's frame slot and an index place; `parseInt` reads its string's frame slot.
    (uses(Entry::Never, Slot::Dst, Slot::Unread, Slot::Src), &[CHAR_CODE_AT_ROW..=CHAR_CODE_AT_ROW]),
    (uses(Entry::Never, Slot::Dst, Slot::Unread, Slot::Unread), &[PARSE_INT_ROW..=PARSE_INT_ROW]),
    // A typed element read (a double destination) and store, and the typed count.
    (uses(Entry::Always, Slot::Float, Slot::Unread, Slot::Src), &[TYPED_INDEX_ROW..=TYPED_INDEX_LAST]),
    (uses(Entry::Always, Slot::Unread, Slot::Src, Slot::Src), &[TYPED_STORE_ROW..=TYPED_STORE_LAST]),
    (uses(Entry::Always, Slot::Dst, Slot::Unread, Slot::Unread), &[TYPED_LENGTH_ROW..=TYPED_LENGTH_ROW]),
    (uses(Entry::Kept, Slot::Unread, Slot::Src, Slot::Src), &[STORE_ROW..=STORE_ROW]),
    // The writes of one operand to a local, a capture, an environment or an append.
    (uses(Entry::Never, Slot::Unread, Slot::Src, Slot::Unread), &[STORE_LOCAL_ROW..=STORE_LOCAL_ROW, STORE_CHECKED_ROW..=STORE_CHECKED_ROW, CAPTURE_SET_ROW..=CAPTURE_SET_ROW,
        ENV_SET_ROW..=ENV_SET_ROW, ENV_DEFINE_ROW..=ENV_DEFINE_ROW, APPEND_ROW..=APPEND_ROW, LEXICAL_WRITE_ROW..=LEXICAL_WRITE_ROW]),
    // The property adds, which read no operand the holes do not carry.
    (uses(Entry::Kept, Slot::Unread, Slot::Unread, Slot::Unread), &[ADD_PACKED_ROW..=ADD_PACKED_ROW, ADD_ARENA_ROW..=ADD_ARENA_ROW, ADD_EITHER_ROW..=ADD_EITHER_ROW,
        THIS_ADD_PACKED_ROW..=THIS_ADD_PACKED_ROW, THIS_ADD_ARENA_ROW..=THIS_ADD_ARENA_ROW, THIS_ADD_EITHER_ROW..=THIS_ADD_EITHER_ROW]),
    // The rows that read nothing but their holes.
    (uses(Entry::Never, Slot::Unread, Slot::Unread, Slot::Unread), &[NAME_EITHER_ROW..=NAME_EITHER_ROW, NAME_ARENA_ROW..=NAME_ARENA_ROW, ARRAY_ROW..=ARRAY_ROW, DECLARED_ROW..=DECLARED_ROW, CAPTURE_IDENTITY_ROW..=CAPTURE_IDENTITY_ROW, PROTOTYPE_ROW..=PROTOTYPE_ROW, IC_PROOF_ROW..=IC_PROOF_ROW,
        IDENTITY_ROW..=IDENTITY_ROW, LIVE_ROW..=LIVE_ROW, REALM_ROW..=REALM_ROW, OBJECT_ROW..=OBJECT_ROW, KEY_ROW..=KEY_ROW, FOR_IN_ROW..=FOR_IN_ROW,
        TYPEOF_INDEX_ROW..=TYPEOF_INDEX_ROW, TYPEOF_STRING_ROW..=TYPEOF_STRING_ROW, INSTANCEOF_ROW..=INSTANCEOF_ROW, TYPEOF_IS_ROW..=TYPEOF_IS_ROW, TYPEOF_ISNOT_ROW..=TYPEOF_ISNOT_ROW]),
    // The staged rows over two register operands into a frame slot (`staged_rows.rs`): a remainder, a relation or an
    // equality as its Boolean or number. The destination is always a frame slot (`MEM`), which the planner requires.
    (uses(Entry::Never, Slot::Unread, Slot::Src, Slot::Src), &[STAGED_REMAINDER..=STAGED_REMAINDER, STAGED_RELVAL_LT..=STAGED_RELVAL_NE, STAGED_LOOSE_EQ..=STAGED_STRICT_NE]),
    // The staged rows over one register operand (`~`, and `RequireObjectCoercible`'s checked operand).
    (uses(Entry::Never, Slot::Unread, Slot::Src, Slot::Unread), &[STAGED_BITWISE_NOT..=STAGED_BITWISE_NOT, STAGED_COERCIBLE..=STAGED_COERCIBLE]),
    // The staged rows that read no operand: `InitializeTdz` of a frame local's slot (the hole) and `void`'s `undefined`.
    (uses(Entry::Never, Slot::Unread, Slot::Unread, Slot::Unread), &[STAGED_INIT_LOCAL..=STAGED_INIT_LOCAL, STAGED_VOID..=STAGED_VOID]),
];

/// Whether a heap code has a row: the typed kinds of `NON_NATIVE_KINDS` have none.
fn has_row(heap: u8) -> bool {
    let typed = |first: u8| heap.checked_sub(first).filter(|&kind| kind < TYPED_KINDS);
    match typed(TYPED_INDEX_ROW).or_else(|| typed(TYPED_STORE_ROW)) {
        Some(kind) => !NON_NATIVE_KINDS.contains(&kind),
        None => true,
    }
}

/// The heap row whose stencil computes `code`: the places a row ignores are fixed.
pub fn heap_canonical((heap, entry, dst, a, b): HeapCode) -> HeapCode {
    let row = HEAP_USES.iter().find(|(_, rows)| rows.iter().any(|rows| rows.contains(&heap))).map_or(uses(Entry::Never, Slot::Unread, Slot::Unread, Slot::Unread), |(row, _)| *row);
    let entry = match row.entry {
        Entry::Never => false,
        Entry::Kept => entry,
        Entry::Always => true,
    };
    let read = |slot: Slot, place: u8| if matches!(slot, Slot::Unread) { MEM } else { place };
    (heap, entry, read(row.dst, dst), read(row.a, a), read(row.b, b))
}

/// The name of a heap row's stencil symbol.
pub fn heap_name((heap, entry, dst, a, b): HeapCode) -> String {
    format!("h_{heap}_{}_{dst}_{a}_{b}", u8::from(entry))
}

/// Every heap row the library holds, canonical, once: each group's rows over both entry flags and every
/// combination of the places its operands range over (`heap_canonical` folds the ones a row ignores).
pub fn heap_library() -> Vec<HeapCode> {
    let places = Places::all();
    let mut rows = BTreeSet::new();
    for (row, ranges) in HEAP_USES {
        let (dst_set, a_set, b_set) = (row.dst.set(&places), row.a.set(&places), row.b.set(&places));
        for heap in ranges.iter().flat_map(|range| range.clone()).filter(|&heap| has_row(heap)) {
            for entry in [false, true] {
                for &dst in &dst_set {
                    for &a in &a_set {
                        for &b in &b_set {
                            rows.insert(heap_canonical((heap, entry, dst, a, b)));
                        }
                    }
                }
            }
        }
    }
    typed_rows(&dsts, &srcs, &mut rows);
    for &dst in &dsts {
        rows.extend([THIS_ROW, LOAD_LOCAL_ROW, LOAD_CHECKED_ROW, ENV_ROW, LEXICAL_READ_ROW, CALLEE_CAPTURE_ROW, EMPTY_OBJECT_ROW, EMPTY_ARRAY_ROW].map(|heap| heap_canonical((heap, false, dst, MEM, MEM))));
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
