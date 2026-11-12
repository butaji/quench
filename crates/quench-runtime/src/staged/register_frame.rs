//! The register frame every staged operation runs on, and an operation's outcome: its own file
//! so the b-jit stencil build (`quench-bjit/build.rs`) compiles the same trait the interpreter
//! and the specializers use.

use super::effects::{CallTarget, Effects};
use crate::bytecode::Register;
use crate::typed_ir::Type;
use super::Stage;

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

    /// The array literal `destination` of the `count` program constants from `constant`.
    #[inline(always)]
    fn allocate_const_array(&mut self, _stage: &mut S, _destination: Register, _constant: S::Word, _count: S::Word, _proof: S::Proof) -> Option<S::Word> { None }

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


/// A conditional jump on the register's truthiness, taken when it is `taken`; unresolved where
/// the frame cannot tell its truthiness.
#[inline(always)]
pub(crate) fn jump_if<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, register: Register, taken: bool) -> Step<S::Word, F::Control> {
    let Some(condition) = frame.truthiness(stage, register) else { return Step::Unresolved };
    let condition = if taken { condition } else { stage.not(condition) };
    Step::Control(frame.branch(stage, condition))
}

/// `destination` defined with the logical negation of `source`'s truthiness.
#[inline(always)]
pub(crate) fn logical_not<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let Some(truthy) = frame.truthiness(stage, source) else { return Step::Unresolved };
    let falsy = stage.not(truthy);
    let (value, fact) = frame.boolean(stage, falsy);
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}

/// `destination` defined with the activation's `this`, where it holds one (not the deleted
/// marker); `miss` where it does not.
#[inline(always)]
pub(crate) fn load_this<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, miss: S::Miss) -> Step<S::Word, F::Control> {
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

/// `destination` defined from local slot `slot`; `None` where the frame cannot read it.
#[inline(always)]
pub(crate) fn read_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, slot: u16, miss: Option<S::Miss>) -> Option<S::Word> {
    let (value, fact) = frame.load_local(stage, slot, miss)?;
    frame.write(stage, destination, value, fact);
    Some(value)
}

/// `source`'s tagged word written to local slot `slot`; `None` where the frame cannot write it.
#[inline(always)]
pub(crate) fn write_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, source: Register, slot: u16, miss: Option<S::Miss>) -> Option<S::Word> {
    let value = frame.tagged(stage, source);
    frame.store_local(stage, slot, value, miss).then_some(value)
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
