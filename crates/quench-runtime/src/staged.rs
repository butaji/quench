//! Operation semantics written once, over a binding-time domain (`Stage`).
//!
//! This is Futamura's first projection made literal through Ershov's mixed computation. Each
//! operation below is one Rust function, generic over the stage that runs it:
//!
//! - The interpreter runs it on machine words (`Concrete`). Every condition is known there, so
//!   only the arm taken runs.
//! - A specializer runs it on IR values, which emits the residual code (`Emit` in
//!   `vm/compiled/object_slots.rs`). A condition it cannot know becomes a guard, or both arms.
//!
//! A plain Rust argument (a slot, a storage form a site proved) is static, so a branch on it
//! leaves no code. A rule written here is the rule of every tier, which is what makes the
//! native tiers the interpreter specialized, not a second implementation of it
//! (`docs/v2/specialization.md`). `staged_coverage` in this module's tests keeps hand-written
//! decodes of the same layouts from reappearing elsewhere.

use crate::bytecode::Register;
use crate::typed_ir::Type;

pub(crate) mod numeric;
pub(crate) use numeric::{Domain, NumberOperation, NumericFrame, NumericStage, Overflow};
pub(crate) mod elements;
pub(crate) mod environment;
pub(crate) mod object;
pub(crate) mod value;
pub(crate) use object::{Form, Layout, PackedAccess, Slots, concrete_own_read,
    own_layout, read_slot};

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
    fn constant(&mut self, bits: u64) -> Self::Word;
    fn load_u32(&mut self, base: Self::Word, offset: i32) -> Self::Word;
    fn load_u64(&mut self, base: Self::Word, offset: i32) -> Self::Word;
    /// A load from a table that never changes while the process runs.
    fn load_table(&mut self, address: Self::Word) -> Self::Word;
    fn add(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    fn or(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    fn add_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn mul_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn and_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn or_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn shr_imm(&mut self, a: Self::Word, shift: u32) -> Self::Word;
    /// The low 32 bits shifted right arithmetically, zero-extended to the word.
    fn sar32_imm(&mut self, a: Self::Word, shift: u32) -> Self::Word;
    fn eq_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    fn ne_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    /// Whether `a` exceeds `imm`, both unsigned.
    fn ugt_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    /// Whether `a` is below `b`, both unsigned and of one width.
    fn ult(&mut self, a: Self::Word, b: Self::Word) -> Self::Cond;
    /// A 32-bit word zero-extended to the full word (the full word unchanged).
    fn uextend32(&mut self, a: Self::Word) -> Self::Word;
    fn sub(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    /// `then` where `cond` holds, else `otherwise`, both already computed.
    fn select(&mut self, cond: Self::Cond, then: Self::Word, otherwise: Self::Word) -> Self::Word;
    /// Stores the low 32 bits of `value`.
    fn store_u32(&mut self, base: Self::Word, offset: i32, value: Self::Word);
    fn store_u64(&mut self, base: Self::Word, offset: i32, value: Self::Word);
    fn not(&mut self, cond: Self::Cond) -> Self::Cond;
    /// The word `then` computes where `cond` holds, else `otherwise`'s.
    fn choose(&mut self, cond: Self::Cond, then: impl FnOnce(&mut Self) -> Self::Word,
        otherwise: impl FnOnce(&mut Self) -> Self::Word) -> Self::Word;
    /// Continues where `cond` holds; `None` where it does not, so nothing runs past it.
    fn guard(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
    /// `cond`'s value where this stage knows it: the interpreter always does, a specializer
    /// never does for a value read from the heap.
    fn known(&self, cond: Self::Cond) -> Option<bool>;
    /// The base of `table`.
    fn table(&mut self, table: Table) -> Self::Word;
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

/// A constant an instruction materializes: its value and the semantic type a stage that
/// keeps type facts records for it.
#[derive(Clone, Copy)]
pub(crate) struct Constant {
    pub(crate) value: crate::Value,
    pub(crate) ty: Type,
}

impl Constant {
    /// A constant whose type the stage does not keep.
    #[inline(always)]
    pub(crate) const fn untyped(value: crate::Value) -> Self {
        Self { value, ty: Type::ANY }
    }
}

/// A register frame: each register's word and the static fact the stage keeps about it. A
/// stage with no register facts uses `()`.
pub(crate) trait Frame<S: Stage> {
    /// The static fact a stage keeps about a register.
    type Fact: Copy;

    /// The fact `register` currently carries.
    fn fact(&self, register: Register) -> Self::Fact;

    /// `register`'s word in the representation its fact names; tagged where it is tagged.
    fn read(&mut self, stage: &mut S, register: Register) -> S::Word;

    /// `register`'s tagged word (`Value::to_bits`), boxing an unboxed representation.
    #[inline(always)]
    fn tagged(&mut self, stage: &mut S, register: Register) -> S::Word {
        self.read(stage, register)
    }

    /// Define `register` with `value`, carrying `fact`.
    fn write(&mut self, stage: &mut S, register: Register, value: S::Word, fact: Self::Fact);

    /// Copy `source` into `destination` with its value and every fact derived from it; the
    /// word copied.
    #[inline(always)]
    fn move_register(&mut self, stage: &mut S, destination: Register, source: Register) -> S::Word {
        let fact = self.fact(source);
        let value = self.read(stage, source);
        self.write(stage, destination, value, fact);
        value
    }

    /// The running activation's `this` word: the bound value, or `Value::DELETED` where the
    /// activation does not hold it.
    fn this(&mut self, stage: &mut S) -> S::Word;

    /// Define `destination` with the activation's `this`, carrying every fact proved of it.
    fn define_this(&mut self, stage: &mut S, destination: Register, this: S::Word);

    /// `register`'s value from `constant`, with the fact the stage keeps of it.
    fn constant(&mut self, stage: &mut S, constant: Constant) -> (S::Word, Self::Fact);

    /// The proven slot `slot` of `base`'s object, with the fact the read proves; `None` where
    /// the frame has no heap or the slot is not proved, leaving the instruction to the host.
    #[inline(always)]
    fn field_read(&mut self, _stage: &mut S, _base: Register, _slot: u16, _ty: Type, _proof: S::Proof) -> Option<(S::Word, Self::Fact)> {
        None
    }

    /// Write `value` to the proven slot; `None` where the frame has no heap or the slot is
    /// not writable, leaving the instruction to the host.
    #[inline(always)]
    fn field_write(&mut self, _stage: &mut S, _base: Register, _slot: u16, _ty: Type, _value: S::Word, _proof: S::Proof) -> Option<()> {
        None
    }

    /// The dense array element at `index`, under the proof; a hole, a miss or an index past
    /// the view leaves `None` to the caller's cold path.
    #[inline(always)]
    fn index_read(&mut self, _stage: &mut S, _index: S::Word, _proof: S::Proof) -> Option<S::Word> { None }

    /// Store `value` at the dense array element `index`, under the proof.
    #[inline(always)]
    fn index_write(&mut self, _stage: &mut S, _index: S::Word, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// Where control goes: the edge the interpreter returns, or the branch a specializer
    /// emits.
    type Control;

    /// ToBoolean of `register` from the frame's facts; `None` where a value's heap cell
    /// decides it, leaving the test to the interpreter's fallback.
    fn truthiness(&mut self, stage: &mut S, register: Register) -> Option<S::Cond>;

    /// Continue at the jump target when `condition` holds, else after this instruction.
    fn branch(&mut self, stage: &mut S, condition: S::Cond) -> Self::Control;

    /// Continue at the jump target.
    fn jump(&mut self, stage: &mut S) -> Self::Control;
}

/// An instruction `step` runs, with the operands a tier decoded.
#[derive(Clone, Copy)]
pub(crate) enum Operation<W, P> {
    /// Copy register `source` into `destination` with its facts.
    Move { destination: Register, source: Register },
    /// Define `destination` from `constant`.
    LoadConst { destination: Register, constant: Constant },
    /// Continue at the jump target.
    Jump,
    /// Continue at the jump target when register `register` is truthy, or when it is falsy
    /// where `taken` is false; otherwise continue after this instruction.
    JumpIf { register: Register, taken: bool },
    /// Define `destination` with the activation's `this`; where the activation does not
    /// hold it, leave at the frame's miss edge.
    LoadThis { destination: Register },
    /// A Number operation (`numeric`).
    Number(NumberOperation),
    /// Continue where register `register` is neither `null` nor `undefined`; those leave at
    /// the frame's miss edge, where the interpreter throws the TypeError
    /// (RequireObjectCoercible).
    RequireObjectCoercible { register: Register },
    /// The proven slot `slot` of `base`'s object, under `proof`, as a value (`Frame::field_read`).
    ReadField { base: Register, slot: u16, ty: Type, proof: P },
    /// Write `value` to the proven slot `slot` of `base`'s object under `proof`.
    WriteField { base: Register, slot: u16, ty: Type, value: W, proof: P },
    /// A dense array element at `index`, under `proof`; a hole or miss leaves `Unresolved`.
    IndexRead { index: W, proof: P },
    /// Store `value` at dense array element `index`, under `proof`.
    IndexWrite { index: W, value: W, proof: P },
}

/// Which of an instruction's two control edges it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Edge {
    /// The jump target.
    Target,
    /// The instruction after this one.
    Following,
}

/// A staged instruction's outcome: the word it produced, or where control goes.
pub(crate) enum Step<W, C> {
    /// The operation produced `W`; control falls through.
    Value(W),
    /// The operation transferred control to `C`.
    Control(C),
    /// The operation left at its miss edge (a failed guard), or a value's heap cell decides
    /// its truthiness: the interpreter must take its fallback path.
    Unresolved,
}

/// Run `operation` on `frame`; its word or where control goes.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Move { destination, source } => Step::Value(frame.move_register(stage, destination, source)),
        Operation::LoadConst { destination, constant } => {
            let (value, fact) = frame.constant(stage, constant);
            frame.write(stage, destination, value, fact);
            Step::Value(value)
        }
        Operation::Jump => Step::Control(frame.jump(stage)),
        Operation::JumpIf { register, taken } => match frame.truthiness(stage, register) {
            Some(condition) => {
                let condition = if taken { condition } else { stage.not(condition) };
                Step::Control(frame.branch(stage, condition))
            }
            None => Step::Unresolved,
        },
        Operation::Number(operation) => numeric::step(stage, frame, operation),
        Operation::RequireObjectCoercible { register } => require_object_coercible(stage, frame, register),
        Operation::LoadThis { destination } => load_this(stage, frame, destination),
        heap => return heap_step(stage, frame, heap),
    }
}

/// The heap operations: a field read or write. A frame with no heap for the operation leaves
/// it `Unresolved` for its host path.
#[inline(always)]
fn heap_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::ReadField { base, slot, ty, proof } => match frame.field_read(stage, base, slot, ty, proof) {
            Some((value, _)) => Step::Value(value),
            None => Step::Unresolved,
        },
        Operation::WriteField { base, slot, ty, value, proof } =>
            match frame.field_write(stage, base, slot, ty, value, proof) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            },
        Operation::IndexRead { index, proof } => match frame.index_read(stage, index, proof) {
            Some(value) => Step::Value(value),
            None => Step::Unresolved,
        },
        Operation::IndexWrite { index, value, proof } => match frame.index_write(stage, index, value, proof) {
            Some(()) => Step::Value(value),
            None => Step::Unresolved,
        },
        _ => Step::Unresolved,
    }
}

/// The activation's `this` into `destination`, where the activation holds it.
#[inline(always)]
fn load_this<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let this = frame.this(stage);
    let bound = stage.ne_imm(this, crate::Value::DELETED.to_bits());
    match stage.guard(bound, miss) {
        Some(()) => {
            frame.define_this(stage, destination, this);
            Step::Value(this)
        }
        None => Step::Unresolved,
    }
}

/// RequireObjectCoercible of `register`: its tagged word where it is neither `null` nor
/// `undefined`; those leave at the frame's miss edge.
#[inline(always)]
fn require_object_coercible<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    register: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let value = frame.tagged(stage, register);
    let null = stage.eq_imm(value, crate::Value::NULL.to_bits());
    let undefined = stage.eq_imm(value, crate::Value::UNDEFINED.to_bits());
    let nullish = stage.either(null, undefined);
    match stage.guard_false(nullish, miss) {
        Some(()) => Step::Value(value),
        None => Step::Unresolved,
    }
}

/// The interpreter's stage: machine words, read from live memory. It knows every condition,
/// so a guard never fails and each `choose` runs one arm.
#[derive(Clone, Copy)]
pub(crate) struct Concrete {
    tables: [u64; Table::COUNT],
}

impl Concrete {
    #[inline(always)]
    pub(crate) fn new(property_values: *const crate::Value) -> Self {
        Self { tables: [property_values as u64] }
    }
}

impl Stage for Concrete {
    type Word = u64;
    type Proof = ();
    type Cond = bool;
    type Miss = ();

    #[inline(always)]
    fn constant(&mut self, bits: u64) -> u64 { bits }
    #[inline(always)]
    fn load_u32(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: a staged operation loads only from a live record or table at a layout offset.
        u64::from(unsafe { ((base as *const u8).offset(offset as isize) as *const u32).read() })
    }
    #[inline(always)]
    fn load_u64(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`; records and arena values are 8-byte aligned.
        unsafe { ((base as *const u8).offset(offset as isize) as *const u64).read() }
    }
    #[inline(always)]
    fn load_table(&mut self, address: u64) -> u64 { self.load_u64(address, 0) }
    #[inline(always)]
    fn add(&mut self, a: u64, b: u64) -> u64 { a.wrapping_add(b) }
    #[inline(always)]
    fn or(&mut self, a: u64, b: u64) -> u64 { a | b }
    #[inline(always)]
    fn add_imm(&mut self, a: u64, imm: u64) -> u64 { a.wrapping_add(imm) }
    #[inline(always)]
    fn mul_imm(&mut self, a: u64, imm: u64) -> u64 { a.wrapping_mul(imm) }
    #[inline(always)]
    fn and_imm(&mut self, a: u64, imm: u64) -> u64 { a & imm }
    #[inline(always)]
    fn or_imm(&mut self, a: u64, imm: u64) -> u64 { a | imm }
    #[inline(always)]
    fn shr_imm(&mut self, a: u64, shift: u32) -> u64 { a >> shift }
    #[inline(always)]
    fn sar32_imm(&mut self, a: u64, shift: u32) -> u64 { u64::from(((a as u32 as i32) >> shift) as u32) }
    #[inline(always)]
    fn eq_imm(&mut self, a: u64, imm: u64) -> bool { a == imm }
    #[inline(always)]
    fn ne_imm(&mut self, a: u64, imm: u64) -> bool { a != imm }
    #[inline(always)]
    fn ugt_imm(&mut self, a: u64, imm: u64) -> bool { a > imm }
    #[inline(always)]
    fn ult(&mut self, a: u64, b: u64) -> bool { a < b }
    #[inline(always)]
    fn uextend32(&mut self, a: u64) -> u64 { a }
    #[inline(always)]
    fn sub(&mut self, a: u64, b: u64) -> u64 { a.wrapping_sub(b) }
    #[inline(always)]
    fn select(&mut self, cond: bool, then: u64, otherwise: u64) -> u64 { if cond { then } else { otherwise } }
    #[inline(always)]
    fn store_u32(&mut self, base: u64, offset: i32, value: u64) {
        // SAFETY: a staged operation stores only into a live record or backing at a layout offset.
        unsafe { ((base as *mut u8).offset(offset as isize) as *mut u32).write(value as u32) }
    }
    #[inline(always)]
    fn store_u64(&mut self, base: u64, offset: i32, value: u64) {
        // SAFETY: as `store_u32`; values and backings are 8-byte aligned.
        unsafe { ((base as *mut u8).offset(offset as isize) as *mut u64).write(value) }
    }
    #[inline(always)]
    fn not(&mut self, cond: bool) -> bool { !cond }
    #[inline(always)]
    fn choose(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> u64, otherwise: impl FnOnce(&mut Self) -> u64) -> u64 {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn guard(&mut self, cond: bool, _miss: ()) -> Option<()> { cond.then_some(()) }
    #[inline(always)]
    fn known(&self, cond: bool) -> Option<bool> { Some(cond) }
    #[inline(always)]
    fn table(&mut self, table: Table) -> u64 { self.tables[table as usize] }
}

#[cfg(test)]
mod tests;
