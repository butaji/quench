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
        JUMP | JUMP_POLL | TAG_INIT => (MEM, MEM, MEM),
        GUARD => (dst, a, MEM),
        SPILL => (MEM, a, MEM),
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
    for kinds in [&arith, &bitwise, &compare, &copy] {
        product(rows, kinds, &places.ints_dst, &places.ints_src, &[true, false]);
    }
    let double_arith: Vec<u8> = (ADD..=DIV).collect();
    for kinds in [&double_arith, &compare, &copy] {
        product(rows, kinds, &places.floats_dst, &places.floats_src, &[false]);
    }
}

/// Jumps, entry tagging, guards and spills.
fn control(places: &Places, rows: &mut BTreeSet<Code>) {
    for kind in [JUMP, JUMP_POLL, TAG_INIT] {
        rows.insert(canonical((kind, MEM, MEM, MEM, true)));
    }
    for &dst in places.floats_dst.iter().chain(&places.ints_dst) {
        rows.insert(canonical((GUARD, dst, MEM, MEM, true)));
    }
    for &a in &places.floats_pins {
        rows.insert(canonical((SPILL, MEM, a, MEM, true)));
    }
}

/// Every row the library holds, canonical, once.
pub fn library() -> Vec<Code> {
    let places = Places::all();
    let mut rows = BTreeSet::new();
    bitwise_over_doubles(&places, &mut rows);
    products(&places, &mut rows);
    control(&places, &mut rows);
    rows.into_iter().collect()
}

/// A heap row as codes: the heap operation (`heap_rows.rs`), entry-record variant, destination
/// and the two operand places.
pub type HeapCode = (u8, bool, u8, u8, u8);

/// The heap row whose stencil computes `code`: the places a row ignores are fixed.
pub fn heap_canonical((heap, entry, dst, a, b): HeapCode) -> HeapCode {
    match heap {
        CAPTURE_ROW | FIELD_ROW => (heap, false, dst, MEM, MEM),
        INDEX_ROW => (heap, entry, dst, MEM, b),
        STORE_ROW => (heap, entry, MEM, a, b),
        _ => (heap, false, MEM, MEM, MEM),
    }
}

/// The name of a heap row's stencil symbol.
pub fn heap_name((heap, entry, dst, a, b): HeapCode) -> String {
    format!("h_{heap}_{}_{dst}_{a}_{b}", u8::from(entry))
}

/// Every heap row the library holds, canonical, once.
pub fn heap_library() -> Vec<HeapCode> {
    let places = Places::all();
    let everywhere = |sources: &[u8]| -> Vec<u8> { sources.iter().copied().chain(double_pins()).collect() };
    let (dsts, srcs) = (everywhere(&places.ints_dst), everywhere(&places.ints_src));
    let mut rows = BTreeSet::new();
    for entry in [false, true] {
        for &dst in &dsts {
            for heap in [CAPTURE_ROW, INDEX_ROW, FIELD_ROW] {
                rows.extend(srcs.iter().map(|&b| heap_canonical((heap, entry, dst, MEM, b))));
            }
        }
        for (&a, &b) in srcs.iter().flat_map(|a| srcs.iter().map(move |b| (a, b))) {
            rows.insert(heap_canonical((STORE_ROW, entry, MEM, a, b)));
        }
    }
    rows.insert(heap_canonical((ARRAY_ROW, false, MEM, MEM, MEM)));
    rows.into_iter().collect()
}
