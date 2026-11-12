//! Which Number rows the library holds, as codes (the `numeric.rs` constants of `rows.rs`): the
//! one definition `build.rs` generates the stencils from and the runtime looks rows up by. A row
//! outside it has no stencil, and a region that needs one is declined like any other row the
//! library lacks.

pub const MEM: u8 = 0;
pub const MEMINT: u8 = 1;
pub const CONST: u8 = 2;
pub const IMM: u8 = 3;
pub const PIN: u8 = 4;
pub const PINF: u8 = 8;
const NE: u8 = 17;

/// A row as codes: kind, destination, operands, speculate.
pub type Code = (u8, u8, u8, u8, bool);

/// The row whose stencil computes `code`: operands and speculation a row ignores are fixed.
pub fn canonical((kind, dst, a, b, spec): Code) -> Code {
    let floats = [dst, a, b].iter().any(|&p| p >= PINF);
    let (copy, compare, bare) = (kind <= 1, (12..=NE).contains(&kind), kind >= 18);
    if floats && kind <= NE && !(6..=11).contains(&kind) {
        let b = if copy { MEM } else { b };
        return (kind, if compare { MEM } else { dst }, a, b, false);
    }
    match kind {
        _ if copy => (kind, dst, a, MEM, true),
        _ if compare => (kind, MEM, a, b, spec),
        18 | 19 => (kind, MEM, MEM, MEM, true),
        20 => (kind, dst, a, MEM, true),
        21 => (kind, MEM, MEM, MEM, true),
        22 => (kind, MEM, a, MEM, true),
        _ if bare => (kind, dst, a, b, spec),
        _ => (kind, dst, a, b, spec),
    }
}

/// The name of a row's stencil symbol.
pub fn name((kind, dst, a, b, spec): Code) -> String {
    format!("n_{kind}_{dst}_{a}_{b}_{}", u8::from(spec))
}

/// Every row the library holds, canonical, once.
pub fn library() -> Vec<Code> {
    let ints_dst: Vec<u8> = [MEM, MEMINT].into_iter().chain(PIN..PIN + 4).collect();
    let ints_src: Vec<u8> = [MEM, MEMINT, CONST, IMM].into_iter().chain(PIN..PIN + 4).collect();
    let floats_dst: Vec<u8> = (PINF..PINF + 8).chain([MEM]).collect();
    let floats_src: Vec<u8> = [MEM, CONST, MEMINT].into_iter().chain(PIN..PIN + 4).chain(PINF..PINF + 8).collect();
    let mut rows = std::collections::BTreeSet::new();
    let mut product = |kinds: &[u8], dsts: &[u8], srcs: &[u8], specs: &[bool]| {
        for &kind in kinds {
            for &dst in dsts {
                for &a in srcs {
                    for &b in srcs {
                        for &spec in specs {
                            rows.insert(canonical((kind, dst, a, b, spec)));
                        }
                    }
                }
            }
        }
    };
    let (arith, bitwise, compare, copy): (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) = ((2..=4).collect(), (6..=11).collect(), (12..=NE).collect(), vec![0, 1]);
    for kinds in [&arith, &bitwise, &compare, &copy] {
        product(kinds, &ints_dst, &ints_src, &[true, false]);
    }
    for kinds in [&(2..=5).collect::<Vec<u8>>(), &compare, &copy] {
        product(kinds, &floats_dst, &floats_src, &[false]);
    }
    rows.insert(canonical((18, MEM, MEM, MEM, true)));
    rows.insert(canonical((19, MEM, MEM, MEM, true)));
    rows.insert(canonical((21, MEM, MEM, MEM, true)));
    for dst in floats_dst.iter().chain(&ints_dst) {
        rows.insert(canonical((20, *dst, MEM, MEM, true)));
    }
    for a in PINF..PINF + 8 {
        rows.insert(canonical((22, MEM, a, MEM, true)));
    }
    rows.into_iter().collect()
}

/// A heap row as codes: the heap operation (`heap_rows.rs`), entry-record variant, destination
/// and the two operand places.
pub type HeapCode = (u8, bool, u8, u8, u8);

/// The heap row whose stencil computes `code`: the places a row ignores are fixed.
pub fn heap_canonical((heap, entry, dst, a, b): HeapCode) -> HeapCode {
    match heap {
        0 | 3 => (heap, false, dst, MEM, MEM),
        1 => (heap, entry, dst, MEM, b),
        2 => (heap, entry, MEM, a, b),
        _ => (heap, false, MEM, MEM, MEM),
    }
}

/// The name of a heap row's stencil symbol.
pub fn heap_name((heap, entry, dst, a, b): HeapCode) -> String {
    format!("h_{heap}_{}_{dst}_{a}_{b}", u8::from(entry))
}

/// Every heap row the library holds, canonical, once.
pub fn heap_library() -> Vec<HeapCode> {
    let dsts: Vec<u8> = [MEM, MEMINT].into_iter().chain(PIN..PIN + 4).chain(PINF..PINF + 8).collect();
    let srcs: Vec<u8> = [MEM, MEMINT, CONST, IMM].into_iter().chain(PIN..PIN + 4).chain(PINF..PINF + 8).collect();
    let mut rows = std::collections::BTreeSet::new();
    for entry in [false, true] {
        for &dst in &dsts {
            for heap in [0, 1, 3] {
                for &b in &srcs {
                    rows.insert(heap_canonical((heap, entry, dst, MEM, b)));
                }
            }
        }
        for &a in &srcs {
            for &b in &srcs {
                rows.insert(heap_canonical((2, entry, MEM, a, b)));
            }
        }
    }
    rows.insert(heap_canonical((4, false, MEM, MEM, MEM)));
    rows.into_iter().collect()
}
