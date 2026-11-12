//! Hand-written stencil rows for Number loops over tagged `Value` words. A row names where each
//! operand lives: a frame slot holding a tagged word (hole: byte offset), a pinned register
//! holding a raw int32 (rdx, rcx, r8, r9 through the whole chain of stencils: no tag, no
//! store-forwarding), an immediate int32 (hole) or a constant tagged word (hole). A row applies
//! the interpreter's Number semantics (`Value::number` canonicalization) and continues at the
//! next row. Speculating rows take int32 operands only: anything else, an overflow, or a result
//! a pinned register cannot hold leaves through exit 0 before any effect, with the pinned
//! registers as they were at the row's start. General rows take tagged words (pinned operands
//! are tagged on use) and write a frame slot.
//!
//! Holes: 0 is the destination offset, 1 and 2 the operands; a jump polls the interrupt flag
//! pointer at the context offset in hole 0.
use crate::row::{Operand, RowBuilder, Source, Ty};
use crate::{Pins, Stencil, StencilBuilder, generate, stencil_isa};
use cranelift_codegen::ir::{self, InstBuilder, condcodes::{CondCode, FloatCC, IntCC}, types};
use cranelift_codegen::isa::OwnedTargetIsa;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

mod control;
mod float;
mod int;
mod word;
use control::*;
use float::*;
use int::*;
pub use word::{canonical, f64_to_i32, int_of, int_word, is_double, is_int, to_f64, word_of};
use word::*;
pub use float::{deliver, f64_of, index_of, number_word};

const TAG_INT: i64 = 0x7ffd_0000_0000_0000;
const FIRST_TAG: i64 = 0x7ff9_0000_0000_0000;
const TAG_SPAN: i64 = 6 << 48;
const CANONICAL_NAN: i64 = 0x7ff8_0000_0000_0000;
const TAG_SHIFT: i64 = 48;
const WORD_SHIFT: i64 = 32;
/// The high 32 bits of an int word.
const INT_HIGH: i64 = TAG_INT >> WORD_SHIFT;
/// Doubles below this magnitude convert to an i64 exactly.
const I64_LIMIT: f64 = 9.0e18;

/// The holes of an operand: the first and the second operand's frame offset or value.
pub const FIRST: u32 = 1;
pub const SECOND: u32 = 2;

/// Where a row's operand lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// A frame slot: the hole is its byte offset.
    Mem,
    /// A pinned register holding a raw int32.
    Pin(u8),
    /// A pinned xmm register holding a Number as `f64` (an int is converted, a result is not
    /// canonicalized until it is written to a frame slot).
    PinF(u8),
    /// A constant tagged `Value` word: the hole.
    Const,
    /// An int32 constant: the hole holds its low 32 bits.
    Imm,
    /// A frame slot known to hold an int word: read and written as its low 32 bits, the tag
    /// staying in place.
    MemInt,
}

/// The operation of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Kind {
    Move,
    Const,
    Add,
    Sub,
    Mul,
    Div,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    UShr,
    /// Compare-and-jump: go to `taken` unless the relation holds.
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    /// Go to `taken` (and poll the interrupt flag first, for `JumpPoll`).
    Jump,
    JumpPoll,
    /// Go to `taken` unless the operand is a Number; into a pinned destination an int word, which
    /// it loads; for an int slot destination, an int word.
    Guard,
    /// Set the high half of the int slot named by hole 0 to the int tag.
    TagInit,
    /// Write a pinned double (operand `a`) to the frame slot at hole 0 as a canonical Number word.
    Spill,
}

impl Kind {
    /// Whether the kind is a bitwise operation, which computes on int32s only.
    pub fn is_bitwise(self) -> bool {
        (Kind::And..=Kind::UShr).contains(&self)
    }

    /// Whether a row of this kind with a double operand computes in f64.
    pub fn has_float_form(self) -> bool {
        self <= Kind::Ne && !self.is_bitwise()
    }
}

/// One row: operation and operand places; `speculate` rows take int32 operands only and leave
/// through exit 0 otherwise, general rows also handle doubles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Row {
    pub kind: Kind,
    pub dst: Place,
    pub a: Place,
    pub b: Place,
    pub speculate: bool,
}

impl Row {
    /// Whether a double register is read or written: the row computes in `f64`.
    fn floats(self) -> bool {
        [self.dst, self.a, self.b].iter().any(|place| matches!(place, Place::PinF(_)))
    }
}

fn emit(b: &mut StencilBuilder, row: Row) {
    if row.floats() && row.kind.has_float_form() {
        return float_row(b, row);
    }
    match row.kind {
        Kind::Move | Kind::Const => copy_row(b, row),
        Kind::Add | Kind::Sub | Kind::Mul | Kind::Div => arith_row(b, row),
        Kind::And | Kind::Or | Kind::Xor | Kind::Shl | Kind::Shr | Kind::UShr => bitwise_row(b, row),
        Kind::Lt | Kind::Le | Kind::Gt | Kind::Ge | Kind::Eq | Kind::Ne => compare_row(b, row),
        Kind::Jump | Kind::JumpPoll => jump_row(b, row.kind == Kind::JumpPoll),
        Kind::Guard | Kind::TagInit | Kind::Spill => bookkeeping_row(b, row),
    }
}

/// The rows that compute nothing: an entry guard, the tag initialisation, a spill.
fn bookkeeping_row(b: &mut StencilBuilder, row: Row) {
    match row.kind {
        Kind::Guard => guard_row(b, row),
        Kind::TagInit => tag_init_row(b),
        _ => spill_row(b, row),
    }
}

pub fn isa() -> Result<&'static OwnedTargetIsa, String> {
    static ISA: OnceLock<Result<OwnedTargetIsa, String>> = OnceLock::new();
    ISA.get_or_init(stencil_isa).as_ref().map_err(Clone::clone)
}

static GENERATION_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Nanoseconds spent generating stencils so far.
pub fn generation_ns() -> u64 {
    GENERATION_NS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Counts `nanos` of stencil generation done outside this module.
pub fn add_generation_ns(nanos: u64) {
    GENERATION_NS.fetch_add(nanos, std::sync::atomic::Ordering::Relaxed);
}

#[path = "../stencils/library.rs"]
pub mod library;

impl Place {
    /// The code of this place in `library` and the stencil rows.
    pub fn code(self) -> u8 {
        match self {
            Place::Mem => library::MEM,
            Place::MemInt => library::MEMINT,
            Place::Const => library::CONST,
            Place::Imm => library::IMM,
            Place::Pin(i) => library::PIN + i,
            Place::PinF(i) => library::PINF + i,
        }
    }
}

impl Row {
    /// The library's code of this row.
    pub fn code(self) -> library::Code {
        (self.kind as u8, self.dst.code(), self.a.code(), self.b.code(), self.speculate)
    }
}

/// The stencil of `row`, built at compile time by rustc and shared for the life of the process;
/// a row the library does not hold is an error, which declines the region that needs it.
pub fn stencil(row: Row) -> Result<&'static Stencil, String> {
    type Table = Mutex<HashMap<library::Code, &'static Stencil>>;
    static TABLE: OnceLock<Table> = OnceLock::new();
    let code = library::canonical(row.code());
    let mut table = TABLE.get_or_init(Default::default).lock().map_err(|e| e.to_string())?;
    if let Some(found) = table.get(&code) {
        return Ok(found);
    }
    let found = crate::rustc::stencil(&library::name(code)).ok_or_else(|| format!("no stencil row {row:?}"))?;
    let stencil: &'static Stencil = Box::leak(Box::new(found));
    table.insert(code, stencil);
    Ok(stencil)
}

/// The Cranelift-built stencil of `row`, which the rustc stencil is compared with.
#[cfg(test)]
pub fn cranelift_stencil(row: Row) -> Result<Stencil, String> {
    isa().and_then(|isa| generate(isa, |b| emit(b, row)))
}

/// The rustc-built Number rows against the Cranelift-built ones: the same row on the same
/// frame, pinned registers and holes must leave the same successor, registers and frame.
#[cfg(test)]
mod differential {
    use super::*;
    use crate::{Row as Placed, copy_patch};

    const KINDS: [Kind; 23] = [
        Kind::Move, Kind::Const, Kind::Add, Kind::Sub, Kind::Mul, Kind::Div, Kind::And, Kind::Or, Kind::Xor, Kind::Shl, Kind::Shr, Kind::UShr,
        Kind::Lt, Kind::Le, Kind::Gt, Kind::Ge, Kind::Eq, Kind::Ne, Kind::Jump, Kind::JumpPoll, Kind::Guard, Kind::TagInit, Kind::Spill,
    ];

    fn place(code: u8) -> Place {
        match code {
            library::MEM => Place::Mem,
            library::MEMINT => Place::MemInt,
            library::CONST => Place::Const,
            library::IMM => Place::Imm,
            4..=7 => Place::Pin(code - library::PIN),
            _ => Place::PinF(code - library::PINF),
        }
    }

    /// What a sink wrote: four pinned ints, eight doubles, the successor taken.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Result {
        p: [u64; 4],
        f: [u64; 8],
        kind: u32,
    }

    /// `movabs rax, result; mov [rax+..], pins; movsd [rax+..], xmmN; mov dword [rax+96], kind; ret`.
    fn sink(pins: [&[u8]; 4], result: usize, kind: u32) -> Stencil {
        let mut code = vec![0x48, 0xb8];
        code.extend((result as u64).to_le_bytes());
        for p in pins {
            code.extend(p);
        }
        for i in 0..8u8 {
            code.extend([0xf2, 0x0f, 0x11, 0x40 | i << 3, 32 + 8 * i]);
        }
        code.extend([0xc7, 0x40, 96]);
        code.extend(kind.to_le_bytes());
        code.push(0xc3);
        Stencil { raw_len: code.len(), code, relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0 }
    }

    /// The pinned stores of a convention: rdx, rcx, r8, r9 (Cranelift) or r14, r15, rdi, rsi (rustc).
    const CRANELIFT_PINS: [&[u8]; 4] = [&[0x48, 0x89, 0x10], &[0x48, 0x89, 0x48, 8], &[0x4c, 0x89, 0x40, 16], &[0x4c, 0x89, 0x48, 24]];
    const RUSTC_PINS: [&[u8]; 4] = [&[0x4c, 0x89, 0x30], &[0x4c, 0x89, 0x78, 8], &[0x48, 0x89, 0x78, 16], &[0x48, 0x89, 0x70, 24]];
    /// Saves the callee-saved registers, optionally moves the C arguments to the rustc convention,
    /// calls the code at +64 and restores.
    fn thunk(rustc: bool) -> Vec<u8> {
        let mut code = vec![0x53, 0x55, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57];
        if rustc {
            code.extend([0x49, 0x89, 0xfc, 0x49, 0x89, 0xf5, 0x49, 0x89, 0xd6, 0x49, 0x89, 0xcf, 0x4c, 0x89, 0xc7, 0x4c, 0x89, 0xce]);
        }
        code.push(0xe8);
        let end = code.len() + 4;
        code.extend((64 - end as i32).to_le_bytes());
        code.extend([0x41, 0x5f, 0x41, 0x5e, 0x41, 0x5d, 0x41, 0x5c, 0x5d, 0x5b, 0xc3]);
        assert!(code.len() <= 64);
        code
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        /// A tagged word: mostly ints and doubles, sometimes other tags.
        fn word(&mut self) -> u64 {
            let word = self.raw_word();
            // The rustc rows convert doubles up to 2^63 (the Cranelift ones stop at 9e18): the one
            // intended difference, kept out of the inputs.
            if (9.0e18..9.3e18).contains(&f64::from_bits(word).abs()) { 0 } else { word }
        }
        fn raw_word(&mut self) -> u64 {
            const INTS: [i32; 8] = [0, 1, -1, 7, i32::MAX, i32::MIN, 65535, -65536];
            const DOUBLES: [f64; 10] = [0.0, -0.0, 0.5, -2.5, 1e10, 3.0, f64::NAN, f64::INFINITY, 2147483648.0, -2147483649.0];
            match self.next() % 8 {
                0 | 1 | 2 => int_word_of(if self.next() % 3 == 0 { INTS[(self.next() % 8) as usize] } else { self.next() as i32 }),
                3 | 4 => f64::to_bits(DOUBLES[(self.next() % 10) as usize]),
                5 => f64::to_bits((self.next() as i64 >> 20) as f64 * 0.25),
                6 => 0x7ff9_0000_0000_0000 | (self.next() & 0x0007_0000_0000_0000),
                _ => self.next(),
            }
        }
        fn double(&mut self) -> f64 {
            f64::from_bits(self.word())
        }
    }

    fn int_word_of(v: i32) -> u64 {
        TAG_INT as u64 | u64::from(v as u32)
    }

    /// Runs `stencil` chained to the three sinks on the inputs; the result and the frame after.
    fn run(stencil: &Stencil, rustc: bool, holes: [u64; 3], frame: &mut [u64; 16], p: [u64; 4], f: [f64; 8], memory: *mut u8) -> Result {
        let result = Box::new(Result::default());
        let pins = if rustc { RUSTC_PINS } else { CRANELIFT_PINS };
        let address = &*result as *const Result as usize;
        let sinks = [sink(pins, address, 1), sink(pins, address, 2), sink(pins, address, 3)];
        let hole = move |i: u32| holes[i as usize];
        let none: &dyn Fn(u32) -> u64 = &|_| 0;
        let rows = [
            Placed { stencil, hole: &hole, taken: Some(2) },
            Placed { stencil: &sinks[0], hole: none, taken: None },
            Placed { stencil: &sinks[1], hole: none, taken: None },
            Placed { stencil: &sinks[2], hole: none, taken: None },
        ];
        // SAFETY: `memory` is an RWX page; the code is the chain computed above.
        unsafe {
            std::ptr::copy_nonoverlapping(thunk(rustc).as_ptr(), memory, 64);
            let code = memory.add(64);
            let (_, starts) = copy_patch(&rows, code, |_, _| 0);
            let exit = code as usize + starts[3];
            copy_patch(&rows, code, |_, _| exit);
            let run: extern "C" fn(usize, usize, u64, u64, u64, u64, f64, f64, f64, f64, f64, f64, f64, f64) = std::mem::transmute(memory);
            run(frame.as_mut_ptr() as usize, 0, p[0], p[1], p[2], p[3], f[0], f[1], f[2], f[3], f[4], f[5], f[6], f[7]);
        }
        *result
    }

    fn same(a: &Result, b: &Result) -> bool {
        let float = |x: u64, y: u64| x == y || (f64::from_bits(x).is_nan() && f64::from_bits(y).is_nan());
        a.kind == b.kind && a.p == b.p && a.f.iter().zip(&b.f).all(|(x, y)| float(*x, *y))
    }

    #[test]
    fn rustc_rows_match_cranelift_rows() {
        // SAFETY: an anonymous RWX page, unmapped at the end.
        let (one, two) = unsafe {
            let map = |_| libc::mmap(std::ptr::null_mut(), 1 << 16, 7, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0).cast::<u8>();
            (map(0), map(1))
        };
        std::panic::set_hook(Box::new(|_| {}));
        let (mut rng, mut checked, mut failures, mut skipped) = (Rng(0x9e37_79b9_7f4a_7c15), 0, Vec::new(), 0);
        for (_, code) in library::library().into_iter().enumerate().filter(|_| true) {
            let (kind, dst, a, b, speculate) = code;
            // A jump's holes are context offsets; an oracle row with two constant operands folds
            // its own sentinels (the planner folds such rows itself and never asks for one).
            let constant = |p: u8| p == library::CONST || p == library::IMM;
            if kind == Kind::Jump as u8 || kind == Kind::JumpPoll as u8 || (constant(a) && constant(b)) {
                continue;
            }
            let row = Row { kind: KINDS[kind as usize], dst: place(dst), a: place(a), b: place(b), speculate };
            // The oracle's own two-pass verification rejects some rows (a folded hole): skip those.
            let Ok(Ok(cranelift)) = std::panic::catch_unwind(|| cranelift_stencil(row)) else {
                skipped += 1;
                continue;
            };
            let rustc = crate::rustc::stencil(&library::name(code)).unwrap_or_else(|| panic!("{row:?}"));
            for round in 0..24 {
                let slots = [(rng.next() % 14) as usize, (rng.next() % 14) as usize, (rng.next() % 14) as usize];
                let hole = |place: Place, slot: usize, rng: &mut Rng| match place {
                    Place::Const => rng.word(),
                    Place::Imm => u64::from(rng.next() as u32 % 100),
                    _ => (slot * 8) as u64,
                };
                let holes = [(slots[0] * 8) as u64, hole(row.a, slots[1], &mut rng), hole(row.b, slots[2], &mut rng)];
                let base: [u64; 16] = std::array::from_fn(|_| rng.word());
                let p: [u64; 4] = std::array::from_fn(|_| u64::from(rng.next() as u32));
                let f: [f64; 8] = std::array::from_fn(|_| rng.double());
                let (mut fa, mut fb) = (base, base);
                let (x, y) = (run(&cranelift, false, holes, &mut fa, p, f, one), run(&rustc, true, holes, &mut fb, p, f, two));
                checked += 1;
                if !same(&x, &y) || fa != fb {
                    failures.push(format!("{row:?} round {round} holes {holes:x?} p {p:x?}: cranelift {} {:x?} {:x?} / rustc {} {:x?} {:x?} frames equal {}", x.kind, x.p, x.f, y.kind, y.p, y.f, fa == fb));
                    break;
                }
            }
        }
        // SAFETY: the pages mapped above.
        unsafe {
            libc::munmap(one.cast(), 1 << 16);
            libc::munmap(two.cast(), 1 << 16);
        }
        let _ = std::panic::take_hook();
        eprintln!("checked {checked} runs, oracle rows skipped {skipped}");
        assert!(failures.is_empty(), "{} of {checked} runs differ; first: {:#?}", failures.len(), &failures[..failures.len().min(8)]);
    }
}
