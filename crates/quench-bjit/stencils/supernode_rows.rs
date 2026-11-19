//! A supernode row: two constituent rows run in sequence inside one function, so LLVM inlines
//! both bodies and the value between them stays in the pinned register the first row delivers
//! to. Both bodies are the staged rows themselves (`numeric::run`, `heap_rows::run`), unchanged:
//! the supernode is the constituents' composition, not a second definition of either.
//!
//! The generated list names each constituent's type (`supernode_rules::macro_args`): a `NumberRow`
//! or a `HeapRow` of the variant's own kind, entry and places, so the stencil runs the rows it was
//! built for and no dispatch picks between them.
//!
//! The holes are the constituents' own (0 destination, 1 and 2 operands). `supernodes.rs` only
//! admits variants whose constituents read disjoint hole roles, so each hole has one meaning.
//! A first row that fails leaves through exit 0 with nothing written; a second row that fails
//! leaves through exit 0 with only the pinned register between them written, which no value live
//! at the first row's PC occupies (the interpreter then runs both instructions from that PC).

use crate::heap_rows;
use crate::numeric::{self, Out, S};

/// One constituent row of a supernode: its own staged step over the frame it is given.
pub(crate) trait Constituent {
    fn run(s: S) -> Out;
}

/// A Number constituent: the Number row of kind `K` over its places (`numeric::run`).
pub(crate) struct NumberRow<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>;

impl<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool> Constituent for NumberRow<K, D, A, B, SPEC> {
    #[inline(always)]
    fn run(s: S) -> Out {
        numeric::run::<K, D, A, B, SPEC>(s)
    }
}

/// A heap constituent: the heap row of operation `K` with its entry variant over its places (`heap_rows::run`).
pub(crate) struct HeapRow<const K: u8, const ENTRY: bool, const D: u8, const A: u8, const B: u8>;

impl<const K: u8, const ENTRY: bool, const D: u8, const A: u8, const B: u8> Constituent for HeapRow<K, ENTRY, D, A, B> {
    #[inline(always)]
    fn run(s: S) -> Out {
        heap_rows::run::<K, ENTRY, D, A, B>(s)
    }
}

/// The supernode: the first constituent's step, then the second's where the first continues at NEXT; any other
/// outcome of the first leaves as it is.
#[inline(always)]
pub(crate) fn run<First: Constituent, Second: Constituent>(s: S) -> Out {
    match First::run(s) {
        Out::Next(s) => Second::run(s),
        stopped => stopped,
    }
}
