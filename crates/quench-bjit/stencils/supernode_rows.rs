//! A supernode row: two constituent rows run in sequence inside one function, so LLVM inlines
//! both bodies and the value between them stays in the pinned register the first row delivers
//! to. Both bodies are the staged rows themselves (`numeric::run`, `heap_rows::run`), unchanged:
//! the supernode is the constituents' composition, not a second definition of either.
//!
//! The holes are the constituents' own (0 destination, 1 and 2 operands). `supernodes.rs` only
//! admits variants whose constituents read disjoint hole roles, so each hole has one meaning.
//! A first row that fails leaves through exit 0 with nothing written; a second row that fails
//! leaves through exit 0 with only the pinned register between them written, which no value live
//! at the first row's PC occupies (the interpreter then runs both instructions from that PC).

use crate::codes::FIRST_HEAP;
use crate::heap_rows;
use crate::numeric::{self, Out, S};

#[inline(always)]
pub(crate) fn run<
    const FIRST: u8,
    const K0: u8,
    const ENTRY0: bool,
    const D0: u8,
    const A0: u8,
    const B0: u8,
    const SPEC0: bool,
    const K1: u8,
    const D1: u8,
    const A1: u8,
    const B1: u8,
    const SPEC1: bool,
>(
    s: S,
) -> Out {
    let first = if FIRST == FIRST_HEAP { heap_rows::run::<K0, ENTRY0, D0, A0, B0>(s) } else { numeric::run::<K0, D0, A0, B0, SPEC0>(s) };
    match first {
        Out::Next(s) => numeric::run::<K1, D1, A1, B1, SPEC1>(s),
        stopped => stopped,
    }
}
