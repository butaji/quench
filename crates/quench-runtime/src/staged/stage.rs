//! The binding-time domain every staged row is written over. Its own file, so the b-jit
//! stencil build (`quench-bjit/build.rs`) compiles the same trait the interpreter and the
//! specializers use.

use crate::typed_ir::Type;
use super::region::{Access, Region};

/// Bytes in one word of memory a stage addresses (a register of a window is one).
const WORD_BYTES: u32 = std::mem::size_of::<u64>() as u32;

/// The byte offset of word `index` from a base.
#[inline(always)]
pub(crate) fn word_offset(index: u16) -> i32 {
    (u32::from(index) * WORD_BYTES) as i32
}

/// The byte count of `index` words.
#[inline(always)]
pub(crate) fn word_bytes(index: u16) -> u64 {
    u64::from(u32::from(index) * WORD_BYTES)
}

/// The arguments a call passes, where the caller's registers hold them.
#[derive(Clone, Copy)]
pub(crate) enum Passed<'a, W> {
    /// `count` consecutive words at `arguments`.
    Window { arguments: W, count: u16 },
    /// The words of the caller's file at `file` that the register numbers `registers` name, in order.
    List { file: W, registers: &'a [u16] },
}

impl<W> Passed<'_, W> {
    /// How many arguments are passed.
    pub(crate) fn count(&self) -> usize {
        match self {
            Self::Window { count, .. } => usize::from(*count),
            Self::List { registers, .. } => registers.len(),
        }
    }
}

/// `fill_window` as one store per register of the window: the prefix, the arguments the window
/// holds room for, then `undefined`.
#[inline(always)]
pub(crate) fn fill_by_rows<S: Stage + ?Sized>(stage: &mut S, top: S::Word, undefined: S::Word, prefix: &[u64],
    passed: Passed<'_, S::Word>, registers: u16) {
    let mut next = 0_u16;
    for bits in prefix.iter().take(usize::from(registers)) {
        let value = stage.constant(*bits);
        stage.store_u64(Region::Frame.mutable(), top, word_offset(next), value);
        next += 1;
    }
    for index in 0..passed.count().min(usize::from(registers - next)) {
        let value = match passed {
            Passed::Window { arguments, .. } => stage.load_u64(Region::Frame.mutable(), arguments, word_offset(index as u16)),
            Passed::List { file, registers } => stage.load_u64(Region::Frame.mutable(), file, word_offset(registers[index])),
        };
        stage.store_u64(Region::Frame.mutable(), top, word_offset(next), value);
        next += 1;
    }
    for register in next..registers {
        stage.store_u64(Region::Frame.mutable(), top, word_offset(register), undefined);
    }
}

/// A binding-time domain: the 64-bit words an operation computes on, its conditions, and where
/// a failed guard goes. A 32-bit load zero-extends.
pub(crate) trait Stage {
    type Word: Copy;
    /// The layout proof a field operation carries (a projection for Emit, `()` for Concrete).
    type Proof: Copy;
    type Cond: Copy;
    /// Where a failed guard leaves: nowhere for the interpreter, which knows the value; a
    /// native exit for a specializer.
    type Miss: Copy;
    /// A value that comes from the bytecode operand or the IC site record (a slot, a shape id,
    /// a field offset, a jump delta): known to Concrete and Emit, a patchable hole to a stencil.
    /// A fixed constant (a tag mask, a header offset) stays a plain Rust value.
    type Imm: Copy + From<u64> + From<u32>;
    /// A per-site operand `v` as this stage's `Imm`: every operand of the bytecode or of an IC
    /// site reaches the stage through here.
    fn site(&mut self, v: u64) -> Self::Imm;
    fn constant(&mut self, bits: u64) -> Self::Word;
    /// The loads and stores say which alias region they touch and whether it can change while
    /// code runs (`Access`): a specializer tags the access so Cranelift may reorder and hoist
    /// it, and Concrete and the stencil stage ignore it.
    fn load_u32(&mut self, at: Access, base: Self::Word, offset: i32) -> Self::Word;
    fn load_u64(&mut self, at: Access, base: Self::Word, offset: i32) -> Self::Word;
    /// An 8-bit load, zero-extended (a narrow string's unit).
    fn load_u8(&mut self, at: Access, base: Self::Word, offset: i32) -> Self::Word;
    /// A 16-bit load, zero-extended (a wide string's unit).
    fn load_u16(&mut self, at: Access, base: Self::Word, offset: i32) -> Self::Word;
    /// A 64-bit load at the per-site offset `offset` (`site`).
    fn load_site(&mut self, at: Access, base: Self::Word, offset: Self::Imm) -> Self::Word;
    /// A load from a table that never changes while the process runs.
    fn load_table(&mut self, at: Access, address: Self::Word) -> Self::Word;
    fn add(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    fn or(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    fn add_imm(&mut self, a: Self::Word, imm: impl Into<Self::Imm>) -> Self::Word;
    fn mul_imm(&mut self, a: Self::Word, imm: impl Into<Self::Imm>) -> Self::Word;
    fn and_imm(&mut self, a: Self::Word, imm: impl Into<Self::Imm>) -> Self::Word;
    fn or_imm(&mut self, a: Self::Word, imm: impl Into<Self::Imm>) -> Self::Word;
    fn shr_imm(&mut self, a: Self::Word, shift: impl Into<Self::Imm>) -> Self::Word;
    /// The low 32 bits shifted right arithmetically, zero-extended to the word.
    fn sar32_imm(&mut self, a: Self::Word, shift: u32) -> Self::Word;
    fn eq_imm(&mut self, a: Self::Word, imm: impl Into<Self::Imm>) -> Self::Cond;
    fn ne_imm(&mut self, a: Self::Word, imm: impl Into<Self::Imm>) -> Self::Cond;
    /// Whether `a` exceeds `imm`, both unsigned.
    fn ugt_imm(&mut self, a: Self::Word, imm: impl Into<Self::Imm>) -> Self::Cond;
    /// Whether `a` is below `b`, both unsigned and of one width.
    fn ult(&mut self, a: Self::Word, b: Self::Word) -> Self::Cond;
    /// A 32-bit word zero-extended to the full word (the full word unchanged).
    fn uextend32(&mut self, a: Self::Word) -> Self::Word;
    fn sub(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    /// `then` where `cond` holds, else `otherwise`, both already computed.
    fn select(&mut self, cond: Self::Cond, then: Self::Word, otherwise: Self::Word) -> Self::Word;
    /// Stores the low 32 bits of `value`.
    fn store_u32(&mut self, at: Access, base: Self::Word, offset: i32, value: Self::Word);
    fn store_u64(&mut self, at: Access, base: Self::Word, offset: i32, value: Self::Word);
    /// A byte (`load_i8`) or a 16-bit element (`load_i16`, `load_u16`'s address need not be
    /// aligned) as an int32 word, sign-extended; `load_i32` is `load_u32`'s bits as an int32.
    fn load_i8(&mut self, at: Access, base: Self::Word, offset: i32) -> Self::Word;
    fn load_i16(&mut self, at: Access, base: Self::Word, offset: i32) -> Self::Word;
    fn load_i32(&mut self, at: Access, base: Self::Word, offset: i32) -> Self::Word;
    /// A binary32 element: an `f32` word, which only `f32_to_f64` and `store_f32` take.
    fn load_f32(&mut self, at: Access, base: Self::Word, offset: i32) -> Self::Word;
    /// A binary64 element: an `f64` word (the IEEE bits in an interpreter's word).
    fn load_f64(&mut self, at: Access, base: Self::Word, offset: i32) -> Self::Word;
    /// Stores the low 8 (`store_u8`) or 16 (`store_u16`) bits of `value`.
    fn store_u8(&mut self, at: Access, base: Self::Word, offset: i32, value: Self::Word);
    fn store_u16(&mut self, at: Access, base: Self::Word, offset: i32, value: Self::Word);
    fn store_f32(&mut self, at: Access, base: Self::Word, offset: i32, value: Self::Word);
    fn store_f64(&mut self, at: Access, base: Self::Word, offset: i32, value: Self::Word);
    /// The `f64` word of an `f32` word, exactly.
    fn f32_to_f64(&mut self, a: Self::Word) -> Self::Word;
    /// The `f32` word nearest an `f64` word, ties to even.
    fn f64_to_f32(&mut self, a: Self::Word) -> Self::Word;
    /// ToUint8Clamp of an `f64` word, as an int32 word: NaN is zero, the rest clamps to 0..=255
    /// and rounds half to even.
    fn clamp_u8(&mut self, a: Self::Word) -> Self::Word;
    /// Whether a store of `value` into object record `record_id` may proceed unobserved and
    /// without a barrier log: no slot representation observes the write, and the record is not
    /// one the barrier logs for this value. The one place the store barrier's layout is read;
    /// a stage without that layout answers no, and the store leaves at its miss.
    fn store_gate(&mut self, record_id: Self::Word, value: Self::Word) -> Self::Cond;
    /// Whether the dependency named by `watch` (a per-site operand) still holds: the interpreter
    /// tests it, a specializer proves it once at publication and its code pays no check (the
    /// assumption registry retires the code when it breaks); a stage without a registry answers
    /// no, and the read leaves at its miss.
    fn assumption(&mut self, watch: Self::Imm) -> Self::Cond;
    /// A computed initializer store's value: the Number operator `operator` (a per-site operand,
    /// the interpreter's operator table) over the Number `passed` and the Number word `constant`,
    /// boxed as `Value::number` does, and whether `passed` was a Number and the operator numeric.
    /// A stage with no arithmetic for the operator answers false, and the field leaves.
    fn number_operator(&mut self, operator: Self::Imm, passed: Self::Word, constant: Self::Word) -> (Self::Word, Self::Cond);
    fn not(&mut self, cond: Self::Cond) -> Self::Cond;
    /// Whether either condition holds.
    fn any(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    /// `then` where `cond` holds, else `otherwise`.
    fn pick(&mut self, cond: Self::Cond, then: Self::Cond, otherwise: Self::Cond) -> Self::Cond;
    /// ToBoolean of the Number whose IEEE double bits are `bits` (`number_is_truthy`).
    fn number_truthy(&mut self, bits: Self::Word) -> Self::Cond;
    /// Whether the shape word `shape` is one of `shapes`, sorted.
    fn shape_in(&mut self, shape: Self::Word, shapes: &[u32]) -> Self::Cond;
    /// `shape_in` of a site's shapes, each a per-site operand: a stage with holes takes one hole
    /// per entry, in entry order whatever the shapes are; Concrete and Emit take `shape_in`.
    fn shape_in_site(&mut self, shape: Self::Word, shapes: &[u32]) -> Self::Cond;
    /// The word `then` computes where `cond` holds, else `otherwise`'s.
    fn choose(&mut self, cond: Self::Cond, then: impl FnOnce(&mut Self) -> Self::Word,
        otherwise: impl FnOnce(&mut Self) -> Self::Word) -> Self::Word;
    /// `choose` whose arms may leave: each runs only on its own path, and a `None` from the taken
    /// arm (a guard inside it that failed) leaves the whole. The interpreter takes one arm as a
    /// plain `if`; a specializer's guard inside an arm is a branch to its miss block, so the arm's
    /// own cold path costs nothing on the other.
    #[inline(always)]
    fn choose_leaving(&mut self, cond: Self::Cond, then: impl FnOnce(&mut Self) -> Option<Self::Word>,
        otherwise: impl FnOnce(&mut Self) -> Option<Self::Word>) -> Option<Self::Word> {
        let left = std::cell::Cell::new(false);
        let word = self.choose(cond, |stage| then(stage).unwrap_or_else(|| { left.set(true); stage.constant(0) }),
            |stage| otherwise(stage).unwrap_or_else(|| { left.set(true); stage.constant(0) }));
        (!left.get()).then_some(word)
    }
    /// Continues where `cond` holds; `None` where it does not, so nothing runs past it.
    fn guard(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
    /// `cond`'s value where this stage knows it: the interpreter always does, a specializer
    /// never does for a value read from the heap.
    fn known(&self, cond: Self::Cond) -> Option<bool>;
    /// The base of `table`.
    fn table(&mut self, table: Table) -> Self::Word;

    /// A call's window of `registers` words at `top`: the `prefix` constants (a bound function's
    /// arguments), then the `passed` arguments, the rest `undefined` (a word the caller made). A
    /// stage with a tuned fill runs it instead (`Concrete`).
    #[inline(always)]
    fn fill_window(&mut self, top: Self::Word, undefined: Self::Word, prefix: &[u64], passed: Passed<'_, Self::Word>,
        registers: u16) {
        fill_by_rows(self, top, undefined, prefix, passed, registers);
    }

    // Stage v2: additions with defaults, so a stage written before them is unchanged.

    /// The fact this stage keeps of `word`; the interpreter keeps none.
    #[inline(always)]
    #[allow(dead_code)]
    fn ty(&self, _word: Self::Word) -> Type {
        Type::ANY
    }

    /// `word` with `ty` proved of it, after a guard that tested it: a later operation on the
    /// word sees the fact. A stage without facts returns the word unchanged.
    #[inline(always)]
    #[allow(dead_code)]
    fn refine(&mut self, word: Self::Word, _ty: Type) -> Self::Word {
        word
    }

    /// Runs `cold` on the path where `cond` holds and nothing is produced (a counter bump, a
    /// poll): out of line for a specializer, run only when the interpreter's `cond` holds.
    #[inline(always)]
    #[allow(dead_code)]
    fn cold_when(&mut self, cond: Self::Cond, cold: impl FnOnce(&mut Self)) {
        if self.known(cond) == Some(true) {
            cold(self);
        }
    }
}

/// The process tables an operation addresses from a base.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Table {
    /// The property arena's values, where a flat object's run starts at its position.
    PropertyValues,
}

impl Table {
    pub(crate) const COUNT: usize = 1;
}
