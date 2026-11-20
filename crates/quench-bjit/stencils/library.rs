//! Which Number rows the library holds, as codes (the constants of `codes.rs`): the one
//! definition `build.rs` generates the stencils from and the runtime looks rows up by. A row
//! outside it has no stencil, and a region that needs one is declined like any other row the
//! library lacks.

#[path = "codes.rs"]
mod codes;
pub use codes::*;
/// The heap rows' names (`row_names.rs`), mounted beside the library by the lib and the build script.
pub use super::row_names::*;

use std::collections::BTreeSet;

/// A row as codes: kind, destination, operands, speculate.
pub type Code = (u8, u8, u8, u8, bool);

/// The operands a kind reads, the others fixed to `MEM`.
fn read_operands(kind: u8, dst: u8, a: u8, b: u8) -> (u8, u8, u8) {
    match kind {
        _ if is_copy(kind) => (dst, a, MEM),
        _ if is_compare(kind) => (MEM, a, b),
        JUMP | JUMP_POLL | JUMP_WATCH | POLL | TAG_INIT => (MEM, MEM, MEM),
        GUARD | UNBOX | UNBOX_INT | NOT | NEG | CLZ32 | MATH_FIRST..=MATH_LAST => (dst, a, MEM),
        JUMP_TRUE | JUMP_FALSE | SPILL => (MEM, a, MEM),
        BIT_TEST => (MEM, a, b),
        NULLISH_EQ | NULLISH_NE | BOOL_TRUE | BOOL_FALSE => (MEM, a, MEM),
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
    for dst in int_pins() {
        rows.insert(canonical((UNBOX_INT, dst, MEM, MEM, true)));
    }
    for dst in double_pins() {
        rows.insert(canonical((UNBOX, dst, MEM, MEM, true)));
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
    product(rows, &[NEG, SQRT, FLOOR, CEIL, TRUNC, ABS, ROUND, FROUND, SIGN], &dsts, &srcs, &[true]);
    // `Math.clz32` writes an int32 (0..=32) to an int destination, from an int32 or a Number.
    for &dst in &places.ints_dst {
        rows.extend(srcs.iter().map(|&a| canonical((CLZ32, dst, a, MEM, true))));
    }
}

/// The jumps a staged compare names with no Number relation: `if (a & b)` over the int32 and Number operands a
/// bitwise row reads, and the nullish and identity tests over every operand a truthiness is read from.
fn tests(places: &Places, rows: &mut BTreeSet<Code>) {
    let bit_operands: Vec<u8> = places.ints_src.iter().copied().chain(double_pins()).collect();
    product(rows, &[BIT_TEST], &[MEM], &bit_operands, &[true]);
    let tested: Vec<u8> = [MEM, MEMINT].into_iter().chain(int_pins()).chain(double_pins()).collect();
    for &a in &tested {
        rows.extend([NULLISH_EQ, NULLISH_NE, BOOL_TRUE, BOOL_FALSE].map(|kind| canonical((kind, MEM, a, MEM, true))));
    }
}

/// `switch`'s case slot (`INT_CASE`): an int destination and int operand, the minimum as the immediate.
fn int_case(places: &Places, rows: &mut BTreeSet<Code>) {
    for &dst in &places.ints_dst {
        for &a in &places.ints_src {
            rows.insert(canonical((INT_CASE, dst, a, IMM, true)));
        }
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
    int_case(&places, &mut rows);
    truthiness(&places, &mut rows);
    tests(&places, &mut rows);
    rows.into_iter().collect()
}

/// A heap row as codes: the heap operation (`heap_rows.rs`), entry-record variant, destination
/// and the two operand places.
pub type HeapCode = (u8, bool, u8, u8, u8);

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

/// Whether a heap code has a row: the typed kinds of `NON_NATIVE_KINDS` have none.
fn has_row(heap: u8) -> bool {
    let typed = |first: u8| heap.checked_sub(first).filter(|&kind| kind < TYPED_KINDS);
    match typed(TYPED_INDEX_ROW).or_else(|| typed(TYPED_STORE_ROW)) {
        Some(kind) => !NON_NATIVE_KINDS.contains(&kind),
        None => true,
    }
}

/// The uses of a heap code's row (`row_catalog.rs`): a code no catalog row names reads nothing.
fn heap_uses(heap: u8) -> Uses {
    let unread = Uses { entry: Entry::Never, dst: Slot::Unread, a: Slot::Unread, b: Slot::Unread };
    HEAP_TABLES.iter().flat_map(|table| table.iter()).find(|(first, count, _)| (*first..*first + *count).contains(&heap)).map_or(unread, |(_, _, row)| *row)
}

/// The heap row whose stencil computes `code`: the places a row ignores are fixed.
pub fn heap_canonical((heap, entry, dst, a, b): HeapCode) -> HeapCode {
    let row = heap_uses(heap);
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

/// The code of every heap row the catalog lists (`row_catalog.rs`), including the typed kinds the library builds no row for.
#[allow(dead_code)] // Read by the runtime catalog test (`base_rows.rs`), not the build.
pub fn heap_row_codes() -> Vec<u8> {
    HEAP_TABLES.iter().flat_map(|table| table.iter()).flat_map(|&(first, count, _)| first..first + count).collect()
}

/// Every heap row the library holds, canonical, once: each group's rows over both entry flags and every
/// combination of the places its operands range over (`heap_canonical` folds the ones a row ignores).
pub fn heap_library() -> Vec<HeapCode> {
    let places = Places::all();
    let mut rows = BTreeSet::new();
    for table in HEAP_TABLES {
        for &(first, count, row) in table.iter() {
            let (dst_set, a_set, b_set) = (row.dst.set(&places), row.a.set(&places), row.b.set(&places));
            for heap in (first..first + count).filter(|&heap| has_row(heap)) {
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
    }
    rows.into_iter().collect()
}
