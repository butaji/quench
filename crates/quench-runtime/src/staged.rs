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

use crate::bytecode::{Op, Register, WideInstruction};
use crate::typed_ir::Type;

mod concrete;
pub(crate) mod numeric;
mod heap;
pub(crate) use concrete::Concrete;
pub(crate) use numeric::{Domain, NumberOperation, NumericFrame, NumericStage, Overflow};
pub(crate) mod elements;
pub(crate) mod environment;
pub(crate) mod ic;
pub(crate) mod object;
pub(crate) mod value;
mod steps;
pub(crate) use steps::step;
pub(crate) mod effects;
#[allow(unused_imports)] // the Stage v2 vocabulary: stencil and UltraJIT stages use the rest
pub(crate) use effects::{CallTarget, Effects, Region, RegionSet};
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
    /// A value that comes from the bytecode operand or the IC site record (a slot, a shape id,
    /// a field offset, a jump delta): known to Concrete and Emit, a patchable hole to a stencil.
    /// A fixed constant (a tag mask, a header offset) stays a plain Rust value.
    type Imm: Copy + From<u64> + From<u32>;
    /// A per-site operand `v` as this stage's `Imm`: every operand of the bytecode or of an IC
    /// site reaches the stage through here.
    fn site(&mut self, v: u64) -> Self::Imm;
    fn constant(&mut self, bits: u64) -> Self::Word;
    fn load_u32(&mut self, base: Self::Word, offset: i32) -> Self::Word;
    fn load_u64(&mut self, base: Self::Word, offset: i32) -> Self::Word;
    /// A 64-bit load at the per-site offset `offset` (`site`).
    fn load_site(&mut self, base: Self::Word, offset: Self::Imm) -> Self::Word;
    /// A load from a table that never changes while the process runs.
    fn load_table(&mut self, address: Self::Word) -> Self::Word;
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
    fn store_u32(&mut self, base: Self::Word, offset: i32, value: Self::Word);
    fn store_u64(&mut self, base: Self::Word, offset: i32, value: Self::Word);
    fn not(&mut self, cond: Self::Cond) -> Self::Cond;
    /// Whether either condition holds.
    fn any(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    /// `then` where `cond` holds, else `otherwise`.
    fn pick(&mut self, cond: Self::Cond, then: Self::Cond, otherwise: Self::Cond) -> Self::Cond;
    /// ToBoolean of the Number whose IEEE double bits are `bits` (`number_is_truthy`).
    fn number_truthy(&mut self, bits: Self::Word) -> Self::Cond;
    /// Whether the shape word `shape` is one of `shapes`, sorted.
    fn shape_in(&mut self, shape: Self::Word, shapes: &[u32]) -> Self::Cond;
    /// `shape_in` of a site's shapes, each a per-site operand: a stage with holes compares
    /// against each; Concrete and Emit take `shape_in`.
    fn shape_in_site(&mut self, shape: Self::Word, shapes: &[u32]) -> Self::Cond;
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

    // Stage v2: additions with defaults, so a stage written before them is unchanged.

    /// `load_u64` from `region`, which a specializer tags on the access so a load survives
    /// a store to another region. A stage with no alias information ignores it.
    #[inline(always)]
    #[allow(dead_code)]
    fn load_in(&mut self, _region: Region, base: Self::Word, offset: i32) -> Self::Word {
        self.load_u64(base, offset)
    }

    /// `store_u64` to `region`.
    #[inline(always)]
    #[allow(dead_code)]
    fn store_in(&mut self, _region: Region, base: Self::Word, offset: i32, value: Self::Word) {
        self.store_u64(base, offset, value)
    }

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

    /// The typed array element at `index`, as a Number word.
    #[inline(always)]
    fn typed_index_read(&mut self, _stage: &mut S, _index: S::Word, _proof: S::Proof) -> Option<S::Word> { None }

    /// Store the Number `value` at typed array element `index`.
    #[inline(always)]
    fn typed_index_write(&mut self, _stage: &mut S, _index: S::Word, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// The object literal `destination`, written with the frame's fact; `None` where the
    /// frame has no heap allocator.
    #[inline(always)]
    fn allocate_object(&mut self, _stage: &mut S, _destination: Register, _proof: S::Proof) -> Option<S::Word> { None }

    /// The array literal `destination`, of `length` holes or `elements` values.
    #[inline(always)]
    fn allocate_array(&mut self, _stage: &mut S, _destination: Register, _length: S::Word, _elements: S::Word, _proof: S::Proof) -> Option<S::Word> { None }

    /// The two-property object literal `destination` of literal `site`'s shape.
    #[inline(always)]
    fn allocate_object_pair(&mut self, _stage: &mut S, _destination: Register, _site: u32, _first: S::Word, _second: S::Word,
        _proof: S::Proof) -> Option<S::Word> { None }

    /// Continue where no interrupt is requested; `None` leaves at the frame's miss edge.
    #[inline(always)]
    fn safepoint(&mut self, _stage: &mut S, _proof: S::Proof) -> Option<()> { Some(()) }

    /// The element store's host helper; `None` where the frame has no helper table.
    #[inline(always)]
    fn index_set_helper(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// The lane's own global-name read (`LaneHelper::GlobalGet`).
    #[inline(always)]
    fn global_read(&mut self, _stage: &mut S, _destination: Register, _atom: u32, _site: u16, _proof: S::Proof) -> Option<()> { None }

    /// The VM's field-store hit (`LaneHelper::FieldSet`).
    #[inline(always)]
    fn field_store_miss(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// A call of `target` with `args` and effect row `effects`; a non-returning status leaves
    /// at `leave`. `None` where the frame has no call protocol (Stage v2).
    #[inline(always)]
    #[allow(dead_code)]
    fn call(&mut self, _stage: &mut S, _target: CallTarget<S::Word>, _args: &[S::Word], _effects: Effects,
        _leave: S::Miss) -> Option<S::Word> { None }

    /// Leave the function with `value` (Stage v2); `None` where the frame cannot.
    #[inline(always)]
    #[allow(dead_code)]
    fn ret(&mut self, _stage: &mut S, _value: S::Word) -> Option<Self::Control> { None }

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

    /// The canonical boolean `condition`, with the fact this stage keeps of it.
    fn boolean(&mut self, stage: &mut S, condition: S::Cond) -> (S::Word, Self::Fact);

    /// Read local slot `slot` with the fact this stage keeps of it; `None` where this
    /// frame cannot read the activation's locals (the tier then leaves at its exact PC).
    #[inline(always)]
    fn load_local(&mut self, _stage: &mut S, _slot: u16, _miss: Option<S::Miss>)
        -> Option<(S::Word, Self::Fact)> { None }

    /// Write `value` to local slot `slot`; `false` where this frame cannot write them.
    #[inline(always)]
    fn store_local(&mut self, _stage: &mut S, _slot: u16, _value: S::Word, _miss: Option<S::Miss>)
        -> bool { false }
}

/// An instruction `step` runs, with the operands a tier decoded.
#[derive(Clone, Copy)]
pub(crate) enum Operation<W, P> {
    /// Copy register `source` into `destination` with its facts.
    Move { destination: Register, source: Register },
    /// Define `destination` from `constant`.
    LoadConst { destination: Register, constant: Constant },
    /// Define `destination` from local slot `slot`.
    LoadLocalPlain { destination: Register, slot: u16 },
    /// Write `source` to local slot `slot`, and to `result` where the instruction has one.
    StoreLocalPlain { source: Register, slot: u16, result: Option<Register> },
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
    /// Define `destination` with the logical negation of register `source`'s truthiness.
    LogicalNot { destination: Register, source: Register },
    /// The proven slot `slot` of `base`'s object, under `proof`, as a value (`Frame::field_read`).
    ReadField { base: Register, slot: u16, ty: Type, proof: P },
    /// Write `value` to the proven slot `slot` of `base`'s object under `proof`.
    WriteField { base: Register, slot: u16, ty: Type, value: W, proof: P },
    /// A dense array element at `index`, under `proof`; a hole or miss leaves `Unresolved`.
    IndexRead { index: W, proof: P },
    /// Store `value` at dense array element `index`, under `proof`.
    IndexWrite { index: W, value: W, proof: P },
    /// A typed array element at `index`, as a Number word, under `proof`.
    TypedIndexRead { index: W, proof: P },
    /// Store the Number `value` at typed array element `index`, under `proof`.
    TypedIndexWrite { index: W, value: W, proof: P },
    /// The object literal `destination`, allocated by the frame's heap; a due collection
    /// leaves at the frame's miss edge.
    MakeObject { destination: Register, proof: P },
    /// The array literal `destination`: `length` holes, or `elements` values.
    MakeArray { destination: Register, length: W, elements: W, proof: P },
    /// The two-property object literal `destination` of literal `site`'s shape, holding
    /// `first` and `second`; a due collection leaves at the frame's miss edge.
    MakeObjectPair { destination: Register, site: u32, first: W, second: W, proof: P },
    /// Continue unless an interrupt or a due collection is requested (`Frame::safepoint`).
    Safepoint { proof: P },
    /// The element store's host helper for `base[key] = value`, under `proof`; the frame's
    /// miss edge takes a failed store.
    IndexSetHelper { base: W, key: W, value: W, proof: P },
    /// The global-name read of `atom` through cache `site`, defined into `destination`; a
    /// miss leaves at the proof's edge.
    GlobalRead { destination: Register, atom: u32, site: u16, proof: P },
    /// The VM field-store helper for `object.atom = value` through cache `site`.
    FieldStoreMiss { object: W, atom: u32, site: u16, value: W, proof: P },
}
impl<W, P> Operation<W, P> {
    /// The staged operation `instruction` denotes, with the operands the catalog fixes.
    /// `None` where the opcode has no staged operation, or where its constant is not
    /// available to this tier: the caller then takes its own exact path. The heap
    /// operations are the frame's (`Frame::field_read`); a tier decodes those itself.
    pub(crate) fn decode(instruction: WideInstruction,
        constant: impl FnOnce(usize) -> Option<Constant>) -> Option<Self> {
        Some(match instruction.op() {
            Op::Move => Self::Move {
                destination: instruction.result_register(), source: instruction.register_b() },
            Op::LoadConst => Self::LoadConst {
                destination: instruction.result_register(),
                constant: constant(instruction.constant_index())? },
            Op::LoadLocalPlain => Self::LoadLocalPlain {
                destination: instruction.result_register(),
                slot: u16::try_from(instruction.local_slot()).ok()? },
            Op::StoreLocalPlain => Self::StoreLocalPlain {
                source: instruction.register_a(),
                slot: u16::try_from(instruction.local_slot()).ok()?,
                result: instruction.optional_register_b() },
            Op::Jump => Self::Jump,
            Op::JumpFalse => Self::JumpIf {
                register: instruction.register_a(), taken: false },
            Op::JumpTrue => Self::JumpIf {
                register: instruction.register_a(), taken: true },
            Op::LoadThis => Self::LoadThis {
                destination: instruction.result_register() },
            Op::RequireObjectCoercible => Self::RequireObjectCoercible {
                register: instruction.register_b() },
            Op::ToNumeric => Self::Number(NumberOperation::ToNumeric {
                destination: instruction.result_register(), source: instruction.register_b() }),
            Op::Unary if instruction.unary_operator()
                == oxc_ast::ast::UnaryOperator::LogicalNot as u32 => Self::LogicalNot {
                    destination: instruction.result_register(), source: instruction.register_b() },
            _ => return None,
        })
    }
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

#[cfg(test)]
mod tests;
