//! The `NumericFrame` trait: the operand, fact and miss interface a Number frame provides.

use super::*;

/// Machine primitives for the two Number representations: int32 words hold the unsigned
/// low 32 bits, float64 words hold their IEEE bits.

/// A frame whose operands a numeric operation proves in its representation and whose result
/// it defines with that representation's fact.
pub trait NumericFrame<S: Stage>: Frame<S> + super::super::GlobalFrame<S> + super::super::PropertyFrame<S> + super::super::ElementFrame<S> + super::super::CallFrame<S> {
    /// The shard module of this frame type's out-of-line `step` (`step_out_of_line`), 0 to 3: frame types with
    /// large instances name different shards, so that each instance gets its own codegen unit.
    const STEP_SHARD: u8 = 0;
    /// The int32 word of `operand`, proved here; `None` where it is none (a frame whose
    /// facts prove it, or whose guard branches away at `miss`, always answers `Some`).
    fn int_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> Option<super::super::IntOf<S>>;
    /// The binary64 word of `operand`, proved here; `None` where it is no Number.
    fn number_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> Option<super::super::NumberOf<S>>;
    /// The int32 `operand` holds where the frame already proved it one, read with no test; `None`
    /// where it did not (a frame that keeps no facts never has).
    #[inline(always)]
    fn held_int(&mut self, _stage: &mut S, _operand: Operand) -> Option<super::super::IntOf<S>> { None }
    /// The program constant `operand` names, where it names one: the value a numeric operation folds into
    /// its stage (`int32_word`, `float_word`: `i32_constant`, `f64_constant`) in place of reading it, in every
    /// tier. `None` for a register or a local, and where the frame holds no constant table.
    #[inline(always)]
    fn constant_operand(&mut self, _operand: Operand) -> Option<crate::Value> { None }
    /// Continue where `register` holds a Number; `None` where it does not, so nothing runs
    /// past it. A specializer may prove a narrower representation its facts favor (an
    /// untyped register taken for an int32), leaving at `miss` for the rest.
    fn prove_number(&mut self, stage: &mut S, register: Register, miss: S::Miss) -> Option<()>;
    fn miss(&self) -> S::Miss;

    /// The tagged word of `operand`: a register's, or a constant the frame holds; `None` for any
    /// other operand, which leaves the row to the interpreter.
    #[inline(always)]
    fn operand_word(&mut self, stage: &mut S, operand: Operand) -> Option<S::Word> {
        operand.register_index().map(|register| self.tagged(stage, register))
    }

    /// `operand`'s ToInt32, as a bitwise operator reads it: an int32 as it is, a Number by
    /// truncation; `None` where it is no Number, which leaves the row to the interpreter. A frame
    /// that proves int32 operands only reads them as such.
    #[inline(always)]
    fn int32_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> Option<super::super::IntOf<S>> {
        self.int_operand(stage, operand, miss)
    }

    /// The encoded prototype link (`ProtoLink::bits`) of `constructor`'s `prototype`, where the
    /// frame holds it as a Stable fact together with the constructor's absent `Symbol.hasInstance`
    /// and its plain, unbound call: a dependency on both retires the code that relied on them
    /// where one changes, so an `instanceof` of `constructor` walks the value's chain by words.
    /// `None` where the frame holds no such fact, which leaves `instanceof` to the VM.
    #[inline(always)]
    fn stable_prototype(&mut self, _stage: &mut S, _constructor: Register) -> Option<S::Word> { None }

    /// Whether an IsHTMLDDA object can exist (the host provides one): a heap cell other than an
    /// object or array record may then be it, which no word decides.
    #[inline(always)]
    fn html_dda_possible(&self) -> bool { true }

    /// The miss edge a local access uses, where this frame guards it. A frame whose locals
    /// are plain storage reads and writes them without a guard returns `None`, and its
    /// `load_local`/`store_local` then ignore the edge.
    #[inline(always)]
    fn local_miss(&self) -> Option<S::Miss> { Some(self.miss()) }

    /// The word `write` takes for the int32 `word` this frame defines with `int_fact`: the
    /// int32 word itself, unless the frame keeps its registers tagged.
    #[inline(always)]
    fn int_result(&mut self, _stage: &mut S, word: S::Word) -> S::Word { word }

    fn int_fact(&self) -> Self::Fact;
    fn number_fact(&self) -> Self::Fact;

    /// The fact a Number takes where the frame keeps Numbers tagged: an int32 boxed in place and a binary64
    /// canonicalized (`box_number`), defined once by `number_arithmetic`. `None` where the frame keeps every
    /// Number unboxed, and a row defines its binary64 result with `number_fact` alone.
    #[inline(always)]
    fn tagged_number_fact(&self) -> Option<Self::Fact> { None }

    /// Write `word`, of `fact`, to frame local slot `local` (a numeric local target); `false` where this
    /// frame keeps no locals of its own.
    #[inline(always)]
    fn store_numeric_local(&mut self, _stage: &mut S, _local: Register, _word: S::Word, _fact: Self::Fact) -> bool { false }

    /// Whether a Number row its words leave takes the VM's general operator in place (`CallOut::Binary`,
    /// `ToNumeric`, `IncDec`). A frame that keeps declining answers `false`, and the row stays unresolved.
    #[inline(always)]
    fn general_operators(&self) -> bool { false }

    /// Whether a store row its plan leaves takes the VM's store in place (`CallOut::StoreField`,
    /// `StoreElement`). A frame that keeps declining answers `false`, and the row stays unresolved.
    #[inline(always)]
    fn general_stores(&self) -> bool { false }

    /// Whether a load row its plan leaves takes the VM's read in place (`CallOut::LoadField`, `LoadElement`).
    /// A frame that keeps declining answers `false`, and the row stays unresolved.
    #[inline(always)]
    fn general_loads(&self) -> bool { false }
}
