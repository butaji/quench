//! A row's operand places as a staged register frame. Hole 0 is the destination, holes 1 and 2
//! the operands (`DEST`, `LEFT`, `RIGHT` are the staged registers and the hole numbers at once),
//! and a place is a pinned int32 register, a pinned double, a frame slot (a tagged word, or known
//! to hold an int word), an int32 immediate or a constant Number the immediate cannot hold. The
//! staged Number rows (`staged::numeric`) run over this frame as over the interpreter's: it
//! proves an operand in the representation the row asks for (leaving where it is not) and
//! defines the result in its place, converting between a place's representation and the
//! result's (an int32 into a pinned double, a Number into a tagged word or a pinned int32).
//! Only representation is decided here; what an operation computes never is.

use crate::bytecode::{Operand, Register};
use crate::codes::*;
use crate::heap_rows::{payload, word_at};
use crate::layout::{JS_STEP_ARRAY_RECORD_BASE, JS_STEP_EPOCH, JS_STEP_LOCAL_COUNT, JS_STEP_NATIVE_LOCALS, JS_STEP_OBJECT_RECORD_BASE, JS_STEP_RECEIVER};
use crate::staged::{Concrete, Constant, Frame, GlobalFrame, NumericFrame, NumericStage, Stage, value};
use crate::typed_ir::Representation;
use crate::words::*;

pub(crate) const DEST: Register = 0;
pub(crate) const LEFT: Register = HOLE_A as Register;
pub(crate) const RIGHT: Register = HOLE_B as Register;

/// The representation of a place.
const fn rep_of(place: u8) -> Representation {
    match place {
        MEM | CONST => Representation::Tagged,
        MEMINT | IMM | PIN..=LAST_INT_PIN => Representation::Int32,
        _ => Representation::Float64,
    }
}

#[inline(always)]
fn stage() -> Concrete {
    Concrete::from_hole(0)
}

/// The binary64 bits of the int32 in the low half of `value`.
#[inline(always)]
fn widen(value: u64) -> u64 {
    stage().i32_to_f64(value)
}

/// The place's value in its own representation.
#[inline(always)]
fn read_place<const P: u8, const H: u8>(s: &S) -> u64 {
    match P {
        // SAFETY: the frame slot at hole `H` is a live register of the activation.
        MEM => unsafe { (slot::<H>(s) as *const u64).read() },
        MEMINT => u64::from(unsafe { (slot::<H>(s) as *const u32).read() }),
        IMM => u64::from(hole::<H>() as u32),
        PIN..=LAST_INT_PIN => u64::from(s.p[(P - PIN) as usize] as u32),
        CONST => hole64::<H>(),
        _ => s.f[(P - PINF) as usize].to_bits(),
    }
}

/// The place's tagged word, boxing an unboxed representation as `Value::number` does.
#[inline(always)]
pub(crate) fn tagged_place<const P: u8, const H: u8>(s: &S) -> u64 {
    let word = read_place::<P, H>(s);
    match rep_of(P) {
        Representation::Tagged => word,
        Representation::Int32 | Representation::Int64 => value::int_word(&mut stage(), word as u32 as u64),
        Representation::Float64 => canonical(f64::from_bits(word)),
    }
}

/// The int32 an operand holds, proved here: a tagged word must be an int word (`proved` where
/// the caller tested that already), a double an exact int32.
#[inline(always)]
fn int_place<const P: u8, const H: u8>(s: &S, proved: bool) -> Option<u64> {
    let word = read_place::<P, H>(s);
    match rep_of(P) {
        Representation::Int32 | Representation::Int64 => Some(word),
        Representation::Tagged => (proved || has_int_high(word)).then_some(word as u32 as u64),
        Representation::Float64 => {
            let narrow = truncate32(f64::from_bits(word));
            (widen(narrow as u32 as u64) == word).then_some(narrow as u32 as u64)
        }
    }
}

/// The binary64 bits of an operand, proved a Number. A constant place holds a Number the int32
/// immediate cannot (`base_region/plan.rs` sends an int word to `IMM`): its word is the bits.
#[inline(always)]
pub(crate) fn number_place<const P: u8, const H: u8>(s: &S) -> Option<u64> {
    let word = read_place::<P, H>(s);
    match rep_of(P) {
        _ if P == CONST => Some(word),
        Representation::Float64 => Some(word),
        Representation::Int32 | Representation::Int64 => Some(widen(word)),
        Representation::Tagged if value::is_int(&mut stage(), word) => Some(widen(word)),
        Representation::Tagged => value::is_double(&mut stage(), word).then_some(word),
    }
}

/// The int32 a bitwise operand speculates: an int word, or (where `numbers`) a double whose
/// truncation fits an int32, which is its ToInt32.
#[inline(always)]
fn bitwise_place<const P: u8, const H: u8>(s: &S, numbers: bool) -> Option<u64> {
    let word = read_place::<P, H>(s);
    let truncated = |bits: u64| f64_to_i32(f64::from_bits(bits)).map(|int| int as u32 as u64);
    match rep_of(P) {
        Representation::Int32 | Representation::Int64 => Some(word),
        Representation::Float64 => truncated(word),
        Representation::Tagged if has_int_high(word) => Some(word as u32 as u64),
        Representation::Tagged => (numbers && is_double(word)).then(|| truncated(word)).flatten(),
    }
}

/// The int32 an array index operand holds: an int32, or a double equal to one.
#[inline(always)]
pub(crate) fn index_of<const P: u8, const H: u8>(s: &S) -> Option<i32> {
    if rep_of(P) != Representation::Float64 {
        return int_place::<P, H>(s, false).map(|word| word as u32 as i32);
    }
    let f = f64::from_bits(read_place::<P, H>(s));
    let narrow = truncate32(f);
    (f64::from_bits(widen(narrow as u32 as u64)) == f).then_some(narrow)
}

/// The tagged word of a Number operand a store writes.
#[inline(always)]
pub(crate) fn number_word<const P: u8, const H: u8>(s: &S) -> Option<u64> {
    let word = tagged_place::<P, H>(s);
    if P == MEM && !(value::is_int(&mut stage(), word) || value::is_double(&mut stage(), word)) {
        return None;
    }
    Some(word)
}

/// Writes the int32 in the low half of `value` to the destination.
#[inline(always)]
fn put_int<const D: u8>(s: &mut S, value: u64) {
    match D {
        PIN..=LAST_INT_PIN => s.p[(D - PIN) as usize] = value as u32 as usize,
        // SAFETY: the frame slot of hole 0 is writable; an int slot's tag stays in place.
        MEMINT => unsafe { (slot::<0>(s) as *mut u32).write(value as u32) },
        PINF.. => s.f[(D - PINF) as usize] = f64::from_bits(widen(value)),
        _ => unsafe { (slot::<0>(s) as *mut u64).write(value::int_word(&mut stage(), value as u32 as u64)) },
    }
}

/// Writes the tagged `word` to the destination: a pinned register or int slot takes an int
/// word, a pinned double a Number, any other place any word. False, writing nothing, where the
/// place cannot hold it.
#[inline(always)]
fn put_tagged<const D: u8>(s: &mut S, word: u64) -> bool {
    match D {
        PINF.. if value::is_double(&mut stage(), word) => s.f[(D - PINF) as usize] = f64::from_bits(word),
        PINF.. => {
            core::hint::cold_path();
            if !value::is_int(&mut stage(), word) {
                return false;
            }
            put_int::<D>(s, word);
        }
        PIN..=LAST_INT_PIN | MEMINT => {
            if !value::is_int(&mut stage(), word) {
                return false;
            }
            put_int::<D>(s, word);
        }
        // SAFETY: the frame slot of hole 0 is writable.
        _ => unsafe { (slot::<0>(s) as *mut u64).write(word) },
    }
    true
}

/// `deliver`: the destination takes the tagged `word`, else the row leaves.
#[inline(always)]
pub(crate) fn deliver<const D: u8>(mut s: S, word: u64) -> Out {
    if put_tagged::<D>(&mut s, word) { Out::Next(s) } else { Out::Exit(s) }
}

/// Writes `value`, held in `rep`, to the destination; false where the place cannot hold it.
#[inline(always)]
fn put<const D: u8>(s: &mut S, value: u64, rep: Representation) -> bool {
    match rep {
        Representation::Int32 | Representation::Int64 => put_int::<D>(s, value),
        Representation::Float64 if D >= PINF => s.f[(D - PINF) as usize] = f64::from_bits(value),
        Representation::Float64 => return put_tagged::<D>(s, canonical(f64::from_bits(value))),
        Representation::Tagged => return put_tagged::<D>(s, value),
    }
    true
}

/// ToBoolean of an operand: an int32 by its value, a double by its number, a tagged word by the
/// staged rule (`value::tagged_truthiness`, which also decides a record, a boolean and an int);
/// `None` where a heap cell decides, which leaves.
#[inline(always)]
fn truthy<const P: u8, const H: u8>(s: &S) -> Option<bool> {
    let word = read_place::<P, H>(s);
    match rep_of(P) {
        Representation::Int32 | Representation::Int64 => Some(word as u32 != 0),
        Representation::Float64 => Some(stage().number_truthy(word)),
        Representation::Tagged => {
            let (undecided, truthy) = value::tagged_truthiness(&mut stage(), word);
            (!undecided).then_some(truthy)
        }
    }
}

/// A row's three places over the machine state. `proved` records that a dispatch already
/// tested both operands for int words, so a frame slot is read as an int without a second
/// test; `refused` that a result did not fit its place, so the row leaves; `taken` that a
/// branch the row decided goes to its taken successor.
pub(crate) struct Places<const D: u8, const A: u8, const B: u8> {
    pub(crate) s: S,
    pub(crate) proved: bool,
    pub(crate) refused: bool,
    pub(crate) taken: bool,
}

/// `f::<place, hole>` of the place `register` names.
macro_rules! by_place {
    ($register:expr, $f:ident($($arg:expr),*)) => {
        match $register {
            LEFT => $f::<A, HOLE_A>($($arg),*),
            RIGHT => $f::<B, HOLE_B>($($arg),*),
            _ => $f::<D, 0>($($arg),*),
        }
    };
}

impl<const D: u8, const A: u8, const B: u8> Places<D, A, B> {
    #[inline(always)]
    pub(crate) fn new(s: S) -> Self {
        Self { s, proved: false, refused: false, taken: false }
    }

    /// Records that a dispatch proved both operand words int words.
    #[inline(always)]
    pub(crate) fn prove_ints(&mut self) {
        self.proved = true;
    }

    /// Whether both operands are int words, from their tagged words.
    #[inline(always)]
    pub(crate) fn both_int(&self) -> bool {
        both_int(tagged_place::<A, HOLE_A>(&self.s), tagged_place::<B, HOLE_B>(&self.s))
    }

    /// The int32 operand `register` of a bitwise row, proved here.
    #[inline(always)]
    pub(crate) fn bitwise_operand(&self, register: Register, numbers: bool) -> Option<u64> {
        by_place!(register, bitwise_place(&self.s, numbers))
    }

    /// Whether the row may continue: nothing the staged row did was refused by a place.
    #[inline(always)]
    pub(crate) fn writes_fit(&self, step_ok: bool) -> bool {
        step_ok && !self.refused
    }
}

impl<const D: u8, const A: u8, const B: u8> Frame<Concrete> for Places<D, A, B> {
    type Fact = Representation;
    type Control = ();

    #[inline(always)]
    fn fact(&self, register: Register) -> Representation {
        match register {
            LEFT => rep_of(A),
            RIGHT => rep_of(B),
            _ => rep_of(D),
        }
    }

    #[inline(always)]
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        by_place!(register, read_place(&self.s))
    }

    #[inline(always)]
    fn tagged(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        by_place!(register, tagged_place(&self.s))
    }

    #[inline(always)]
    fn write(&mut self, _stage: &mut Concrete, _register: Register, value: u64, fact: Representation) {
        self.refused |= !put::<D>(&mut self.s, value, fact);
    }

    #[inline(always)]
    fn this(&mut self, _stage: &mut Concrete) -> u64 {
        word_at(payload(&self.s), JS_STEP_RECEIVER)
    }
    #[inline(always)]
    fn define_this(&mut self, _stage: &mut Concrete, _destination: Register, this: u64) {
        self.refused |= !put::<D>(&mut self.s, this, Representation::Tagged);
    }
    fn constant(&mut self, _stage: &mut Concrete, _constant: Constant) -> (u64, Representation) {
        unreachable!("a Number row's constants are its places")
    }
    #[inline(always)]
    fn truthiness(&mut self, _stage: &mut Concrete, register: Register) -> Option<bool> {
        by_place!(register, truthy(&self.s))
    }
    #[inline(always)]
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) {
        self.taken = condition;
    }
    fn jump(&mut self, _stage: &mut Concrete) { unreachable!("a row jumps by its kind") }
    #[inline(always)]
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, Representation) {
        (if condition { crate::Value::TRUE } else { crate::Value::FALSE }.to_bits(), Representation::Tagged)
    }

    /// The activation's local slot `slot`, where it has locals and the slot is inside them.
    #[inline(always)]
    fn load_local(&mut self, _stage: &mut Concrete, slot: u16, _miss: Option<()>) -> Option<(u64, Representation)> {
        let locals = local_slot(&self.s, slot)?;
        Some((word_at(locals, 0), Representation::Tagged))
    }

    /// The array and object record tables and the collection epoch the dispatch payload holds, where
    /// both tables are reserved (the lane frame's `heap_records`); `None` where one is absent.
    #[inline(always)]
    fn heap_records(&mut self, _stage: &mut Concrete) -> Option<(u64, u64, u64)> {
        let payload = payload(&self.s);
        let (arrays, objects) = (word_at(payload, JS_STEP_ARRAY_RECORD_BASE), word_at(payload, JS_STEP_OBJECT_RECORD_BASE));
        (arrays != 0 && objects != 0).then(|| (arrays, objects, word_at(payload, JS_STEP_EPOCH)))
    }

    #[inline(always)]
    fn store_local(&mut self, _stage: &mut Concrete, slot: u16, value: u64, _miss: Option<()>) -> bool {
        let Some(locals) = local_slot(&self.s, slot) else { return false };
        // SAFETY: the slot is inside the activation's live local slots (`local_slot`).
        unsafe { (locals as *mut u64).write(value) };
        true
    }
}

/// The address of local slot `slot` of the activation, where the payload holds locals and the
/// slot is inside them.
#[inline(always)]
fn local_slot(s: &S, slot: u16) -> Option<usize> {
    let payload = payload(s);
    let (locals, count) = (word_at(payload, JS_STEP_NATIVE_LOCALS) as usize, word_at(payload, JS_STEP_LOCAL_COUNT));
    (locals != 0 && u64::from(slot) < count).then(|| locals + usize::from(slot) * core::mem::size_of::<u64>())
}

impl<const D: u8, const A: u8, const B: u8> GlobalFrame<Concrete> for Places<D, A, B> {}
impl<const D: u8, const A: u8, const B: u8> crate::staged::PropertyFrame<Concrete> for Places<D, A, B> {}
impl<const D: u8, const A: u8, const B: u8> crate::staged::ElementFrame<Concrete> for Places<D, A, B> {}
impl<const D: u8, const A: u8, const B: u8> crate::staged::CallFrame<Concrete> for Places<D, A, B> {}

impl<const D: u8, const A: u8, const B: u8> NumericFrame<Concrete> for Places<D, A, B> {
    #[inline(always)]
    fn int_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let (register, proved) = (operand.register_index()?, self.proved);
        by_place!(register, int_place(&self.s, proved))
    }

    #[inline(always)]
    fn number_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        by_place!(operand.register_index()?, number_place(&self.s))
    }

    #[inline(always)]
    fn prove_number(&mut self, _stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        by_place!(register, number_place(&self.s)).map(|_| ())
    }

    #[inline(always)]
    fn miss(&self) {}
    #[inline(always)]
    fn int_fact(&self) -> Representation { Representation::Int32 }
    #[inline(always)]
    fn number_fact(&self) -> Representation { Representation::Float64 }
}
