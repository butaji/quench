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
use crate::heap_rows::{context_word, payload, word_at};
use crate::layout::{CONTEXT_THIS, JS_STEP_ARRAY_RECORD_BASE, JS_STEP_EPOCH, JS_STEP_LOCAL_COUNT, JS_STEP_NATIVE_LOCALS, JS_STEP_OBJECT_RECORD_BASE, JS_STEP_STRING_RECORD_BASE};
use crate::staged::{Concrete, Constant, Float64, Int32, Frame, GlobalFrame, NumericFrame, NumericStage, Stage, Step, string_length, value};
use crate::typed_ir::{Representation, Type};
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
pub(crate) fn widen(value: u64) -> u64 {
    stage().i32_to_f64(value)
}

/// The place's value in its own representation.
#[inline]
pub(crate) fn read_place<const P: u8, const H: u8>(s: &S) -> u64 {
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
#[inline]
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
#[inline]
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
#[inline]
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
#[inline]
pub(crate) fn number_word<const P: u8, const H: u8>(s: &S) -> Option<u64> {
    let word = tagged_place::<P, H>(s);
    if P == MEM && !(value::is_int(&mut stage(), word) || value::is_double(&mut stage(), word)) {
        return None;
    }
    Some(word)
}

/// Writes the int32 in the low half of `value` to the destination.
#[inline]
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
#[inline]
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

/// `deliver` of a `word` the fact store proved a Number (an int word or a double, `ic_rows::known_get`): a
/// pinned double takes it with no tag test and no exit, an int word converted; any other place as `deliver`.
#[inline(always)]
pub(crate) fn deliver_number<const D: u8>(mut s: S, word: u64) -> Out {
    match D {
        PINF.. => {
            let bits = if is_double(word) { word } else { widen(word) };
            s.f[(D - PINF) as usize] = f64::from_bits(bits);
            Out::Next(s)
        }
        _ => deliver::<D>(s, word),
    }
}

/// The row's outcome for the staged step `step` over `frame`: `next` where the step produced its word
/// and every result fit its place, else the row leaves with the state as it was.
#[inline(always)]
pub(crate) fn settle(frame: Places, step: Step<u64, ()>, next: fn(S) -> Out) -> Out {
    match step {
        Step::Value(_) if !frame.refused => next(frame.s),
        _ => Out::Exit(frame.s),
    }
}

/// The row's outcome: continue where the staged work was done and every result fit its place, else leave with
/// the registers as they were.
#[inline(always)]
pub(crate) fn finish(frame: Places, done: bool) -> Out {
    if frame.writes_fit(done) { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

/// `finish` for a fast arm: where it continues it hops past the helper step laid out after the row (`hop`).
#[inline(always)]
pub(crate) fn finish_fast(frame: Places, done: bool) -> Out {
    hop(finish(frame, done))
}

/// The outcome of a chained row's staged step over its frame `s`: a value continues through `hit` (the row's delivery),
/// and a chained row's hit skips the site's other entries (TAKEN); a leave continues at the site's next entry where the
/// row is chained (NEXT), else leaves through EXIT0 with nothing written.
#[inline(always)]
pub(crate) fn finish_chained<const CHAINED: bool>(s: S, step: Step<u64, ()>, hit: impl FnOnce(S, u64) -> Out) -> Out {
    match step {
        Step::Value(word) => match hit(s, word) {
            Out::Next(s) if CHAINED => Out::Taken(s),
            other => other,
        },
        _ if CHAINED => Out::Next(s),
        _ => Out::Exit(s),
    }
}

/// The object register a site row names: the word at its frame offset, which sits above the destination's in hole 0
/// (`SITE_OBJECT_AT`).
#[inline(always)]
pub(crate) fn site_object(s: &S) -> u64 {
    word_at(s.frame, (hole64::<0>() >> SITE_OBJECT_AT) as usize)
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
#[inline]
fn truthy<const P: u8, const H: u8>(s: &S) -> Option<bool> {
    let word = read_place::<P, H>(s);
    match rep_of(P) {
        Representation::Int32 | Representation::Int64 => Some(word as u32 != 0),
        Representation::Float64 => Some(stage().number_truthy(word)),
        Representation::Tagged => {
            let (undecided, truthy) = value::tagged_truthiness(&mut stage(), word);
            if undecided { string_truthy(s, word) } else { Some(truthy) }
        }
    }
}

/// ToBoolean of a flat string `word` names: its length is not zero, read from its record as the string rows read it
/// (`string_length::flat_string`, the interpreter's `concrete_string_length`). `None` for any other heap value, or a
/// string form the record does not hold flat (a rope, a slice), which leaves the row.
#[inline(always)]
fn string_truthy(s: &S, word: u64) -> Option<bool> {
    let mut stage = stage();
    let records = word_at(payload(s), JS_STEP_STRING_RECORD_BASE);
    let string = string_length::flat_string(&mut stage, records, word, ())?;
    Some(string.length != 0)
}

/// A row's three places over the machine state. `proved` records that a dispatch already
/// tested both operands for int words, so a frame slot is read as an int without a second
/// test; `refused` that a result did not fit its place, so the row leaves; `taken` that a
/// branch the row decided goes to its taken successor.
///
/// The places are fields, not type parameters: every row builds its frame from its own constants
/// (`new`), and the row inlines the whole staged step, so LLVM folds each `at_place!` to the row's one
/// arm. As type parameters they made the staged chain an instance per row's place triple (47,750
/// instances in a Number shard of 1536 rows, most of its compile time in rustc's collector).
pub(crate) struct Places {
    pub(crate) s: S,
    pub(crate) proved: bool,
    pub(crate) refused: bool,
    pub(crate) taken: bool,
    pub(crate) d: u8,
    pub(crate) a: u8,
    pub(crate) b: u8,
}

/// `$body` with `$P` the constant of the place code `$place` (a run-time value that the row's constants
/// fold): one arm per place code, so a generic `f::<$P, _>` names the instance of that place.
macro_rules! at_place {
    ($place:expr, |$P:ident| $body:expr) => {
        match $place {
            0 => { const $P: u8 = 0; $body }
            1 => { const $P: u8 = 1; $body }
            2 => { const $P: u8 = 2; $body }
            3 => { const $P: u8 = 3; $body }
            4 => { const $P: u8 = 4; $body }
            5 => { const $P: u8 = 5; $body }
            6 => { const $P: u8 = 6; $body }
            7 => { const $P: u8 = 7; $body }
            8 => { const $P: u8 = 8; $body }
            9 => { const $P: u8 = 9; $body }
            10 => { const $P: u8 = 10; $body }
            11 => { const $P: u8 = 11; $body }
            12 => { const $P: u8 = 12; $body }
            13 => { const $P: u8 = 13; $body }
            14 => { const $P: u8 = 14; $body }
            _ => { const $P: u8 = 15; $body }
        }
    };
}
pub(crate) use at_place;
const _: () = assert!(PLACE_CODE_COUNT as usize == 16, "at_place! names every place code");

/// `f::<place, hole>` of the place `register` names.
macro_rules! by_place {
    ($self:ident, $register:expr, $f:ident($($arg:expr),*)) => {
        match $register {
            LEFT => at_place!($self.a, |P| $f::<P, HOLE_A>($($arg),*)),
            RIGHT => at_place!($self.b, |P| $f::<P, HOLE_B>($($arg),*)),
            _ => at_place!($self.d, |P| $f::<P, 0>($($arg),*)),
        }
    };
}

impl Places {
    #[inline(always)]
    pub(crate) fn new<const D: u8, const A: u8, const B: u8>(s: S) -> Self {
        Self { s, proved: false, refused: false, taken: false, d: D, a: A, b: B }
    }

    /// Records that a dispatch proved both operand words int words.
    #[inline(always)]
    pub(crate) fn prove_ints(&mut self) {
        self.proved = true;
    }

    /// Whether both operands are int words, from their tagged words.
    #[inline(always)]
    pub(crate) fn both_int(&self) -> bool {
        both_int(at_place!(self.a, |P| tagged_place::<P, HOLE_A>(&self.s)), at_place!(self.b, |P| tagged_place::<P, HOLE_B>(&self.s)))
    }

    /// Whether the row may continue: nothing the staged row did was refused by a place.
    #[inline(always)]
    pub(crate) fn writes_fit(&self, step_ok: bool) -> bool {
        step_ok && !self.refused
    }
}

impl Frame<Concrete> for Places {
    type Fact = Representation;
    type Control = ();

    #[inline(always)]
    fn fact(&self, register: Register) -> Representation {
        match register {
            LEFT => rep_of(self.a),
            RIGHT => rep_of(self.b),
            _ => rep_of(self.d),
        }
    }

    /// The type a register's place proves: an int32 place (`rep_of` Int32/Int64) an int32, a double place a Number; a tagged
    /// place proves nothing. The staged steps skip the tag tests this answers (`staged::numeric`, `staged::compare`).
    #[inline(always)]
    fn register_type(&self, register: Register) -> Option<crate::typed_ir::Type> {
        let place = match register {
            LEFT => self.a,
            RIGHT => self.b,
            DEST => self.d,
            _ => return None,
        };
        match place {
            CONST | IMM => None,
            _ => match rep_of(place) {
                Representation::Int32 | Representation::Int64 => Some(Type::INT32),
                Representation::Float64 => Some(Type::NUMBER),
                Representation::Tagged => None,
            },
        }
    }

    #[inline(always)]
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        by_place!(self, register, read_place(&self.s))
    }

    #[inline(always)]
    fn tagged(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        by_place!(self, register, tagged_place(&self.s))
    }

    #[inline(always)]
    fn write(&mut self, _stage: &mut Concrete, _register: Register, value: u64, fact: Representation) {
        self.refused |= !at_place!(self.d, |P| put::<P>(&mut self.s, value, fact));
    }

    #[inline(always)]
    fn this(&mut self, _stage: &mut Concrete) -> u64 {
        context_word(self.s.frame, CONTEXT_THIS)
    }
    #[inline(always)]
    fn define_this(&mut self, _stage: &mut Concrete, _destination: Register, this: u64) {
        self.refused |= !at_place!(self.d, |P| put::<P>(&mut self.s, this, Representation::Tagged));
    }
    /// A program constant as its tagged word (`Constant::value`, the runtime's `Value` bits): the staged rows read
    /// `void`'s `undefined` this way, where a Number row's constants are its places.
    #[inline(always)]
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, Representation) {
        (constant.value.to_bits(), Representation::Tagged)
    }
    #[inline(always)]
    fn truthiness(&mut self, _stage: &mut Concrete, register: Register) -> Option<bool> {
        by_place!(self, register, truthy(&self.s))
    }
    #[inline(always)]
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) {
        self.taken = condition;
    }
    fn jump(&mut self, _stage: &mut Concrete) { self.taken = true; }

    // The array operations' answers (`places_heap`).
    #[inline(always)]
    fn direct_array(&mut self, stage: &mut Concrete, source: Register) -> Option<()> { self.heap_direct_array(stage, source) }

    #[inline(always)]
    fn array_length(&mut self, stage: &mut Concrete, source: Register) -> Option<(u64, Representation)> { self.heap_array_length(stage, source) }

    #[inline(always)]
    fn typed_length(&mut self, stage: &mut Concrete, source: Register) -> Option<(u64, Representation)> { self.heap_typed_length(stage, source) }

    #[inline(always)]
    fn array_append(&mut self, stage: &mut Concrete, array: Register, value: u64) -> Option<()> { self.heap_array_append(stage, array, value) }

    /// ForInNext's fast arm (`places_names`).
    #[inline(always)]
    fn vm_step(&mut self, stage: &mut Concrete, operation: crate::staged::CallOut, _result: Option<Register>) -> Option<(u64, Representation)> {
        self.call_out_fast(stage, operation)
    }

    /// The typeof string table at hole 2 (`places_names`).
    #[inline(always)]
    fn typeof_string(&mut self, stage: &mut Concrete, name: u64) -> Option<(u64, Representation)> { self.typeof_entry(stage, name) }

    #[inline(always)]
    fn capture_slot(&mut self, stage: &mut Concrete, depth: u16, slot: u16, root: bool) -> Option<(u64, Representation)> {
        self.capture_read(stage, depth, slot, root)
    }

    #[inline(always)]
    fn store_capture(&mut self, stage: &mut Concrete, depth: u16, slot: u16, value: u64) -> Option<()> {
        self.capture_store(stage, depth, slot, value)
    }

    #[inline(always)]
    fn environment(&mut self, _stage: &mut Concrete) -> Option<(u64, Representation)> { self.environment_storage() }

    #[inline(always)]
    fn environment_write(&mut self, stage: &mut Concrete, slot: u16, value: u64, initialize: bool) -> Option<()> {
        self.environment_store(stage, slot, value, initialize)
    }

    /// A pending interrupt or due collection leaves the row (`numeric::interrupted`).
    #[inline(always)]
    fn safepoint(&mut self, _stage: &mut Concrete, _proof: ()) -> Option<()> {
        (!crate::numeric::interrupted(&self.s)).then_some(())
    }
    #[inline(always)]
    fn invalidation(&mut self, _stage: &mut Concrete) -> Option<u64> {
        Some(crate::places_guard::invalidation(&self.s))
    }
    #[inline(always)]
    fn realm_globals(&mut self, _stage: &mut Concrete) -> Option<u64> {
        Some(crate::places_guard::realm_globals(&self.s))
    }
    #[inline(always)]
    fn count_edge(&mut self, _stage: &mut Concrete) -> Option<()> {
        crate::places_guard::count_edge(&self.s).then_some(())
    }
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

impl crate::staged::PropertyFrame<Concrete> for Places {
    type Plans = crate::staged::NoPlans;
    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }
}

impl NumericFrame<Concrete> for Places {
    /// ToInt32 of the operand: an int place as it is, a double truncated (`bitwise_place`).
    #[inline(always)]
    fn int32_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<Int32<u64>> {
        // Fact: the place's fixed representation, or `bitwise_place`'s tag test and ToInt32.
        by_place!(self, operand.register_index()?, bitwise_place(&self.s, true)).map(|word| unsafe { Int32::proven(word) })
    }

    #[inline(always)]
    fn int_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<Int32<u64>> {
        let (register, proved) = (operand.register_index()?, self.proved);
        // Fact: the place's fixed representation, the caller's guard (`proved`), or `int_place`'s tag test.
        by_place!(self, register, int_place(&self.s, proved)).map(|word| unsafe { Int32::proven(word) })
    }

    #[inline(always)]
    fn number_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<Float64<u64>> {
        // Fact: the place's fixed representation, or `number_place`'s tag test.
        by_place!(self, operand.register_index()?, number_place(&self.s)).map(|word| unsafe { Float64::proven(word) })
    }

    #[inline(always)]
    fn prove_number(&mut self, _stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        by_place!(self, register, number_place(&self.s)).map(|_| ())
    }

    #[inline(always)]
    fn miss(&self) {}
    #[inline(always)]
    fn int_fact(&self) -> Representation { Representation::Int32 }
    #[inline(always)]
    fn number_fact(&self) -> Representation { Representation::Float64 }
}
