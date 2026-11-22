//! A conditional jump on a comparison of two operands (`Operation::CompareJump`, the fused
//! `JumpBinaryFalse`): each comparison is decided once over the stage, and its answer picks the
//! control edge through the frame's `branch`, so the interpreter, the lane and every other tier
//! jump on one definition. A pair the words do not decide is the VM's (`CallOut::Decide`).

use super::object::instance_decision;
use super::value::{is_direct_array, is_direct_object};
use super::{CallOut, Domain, Edge, NumericFrame, NumericStage, Operation, Step};
#[cfg(not(stencil_build))]
use crate::bytecode::WideInstruction;
use crate::bytecode::Operand;
use crate::typed_ir::{Type, language_bit};
use crate::vm::operations::{equality_binary_operator, equality_operator, NumberBitwise, NumberRelation};
use crate::{Value, ValueType};

/// The nullish `value` (`null` or `undefined`) a nullish test compares with, where it is one.
pub fn nullish_of(value: Value) -> Option<ValueType> {
    if value == Value::NULL {
        Some(ValueType::Null)
    } else if value == Value::UNDEFINED {
        Some(ValueType::Undefined)
    } else {
        None
    }
}

/// The value of the nullish language type `kind`.
#[inline]
fn nullish_value(kind: ValueType) -> Value {
    match kind {
        ValueType::Null => Value::NULL,
        ValueType::Undefined => Value::UNDEFINED,
        _ => unreachable!("nullish_value of a type that is not nullish"),
    }
}

/// The `instanceof` operator.
pub const INSTANCEOF_OPERATOR: u32 = crate::vm::operations::binary_operator::Instanceof;

/// The `in` operator.
pub const IN_OPERATOR: u32 = crate::vm::operations::binary_operator::In;

/// What a `CompareJump` compares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Comparison {
    /// A Number relation of two operands the frame proves Numbers, in the `domain` it proves (the
    /// decode names the widest, `Float64`): statically where its facts do, else by the tag test
    /// `NumericFrame::number_operand` makes once at the miss edge, so an operand that is no Number
    /// leaves the row.
    Relation { relation: NumberRelation, domain: Domain },
    /// `==` or `===` (`strict`), negated for `!=` and `!==`, of operands of any type: the words
    /// decide the common pairs and the VM the rest.
    Equality { strict: bool, negated: bool },
    /// `==` or `===` (`strict`), negated for `!=` and `!==`, of the tested operand (the row's
    /// `left`) and `expected`, a constant the tier proved the other operand is: a tag decides it,
    /// and the register's type where it already does.
    Nullish { expected: ValueType, strict: bool, negated: bool },
    /// The truth of a bitwise operator's result of two Numbers (`if (flags & MASK)`): its int32
    /// bits are nonzero, so the Number the operator would make is never made.
    Bitwise(NumberBitwise),
    /// `===` or `!==` (`negated`) of the tested operand (the row's `left`) and the Boolean
    /// `expected`: a Boolean has one encoding, so identity decides it.
    // No decode makes it since the boolean-constant jump handler went with
    // specialize_register_operands; the lane re-adopts it at the Ir pass (a constant-operand
    // decode that sees the constant's kind).
    Boolean { expected: bool, negated: bool },
    /// `value instanceof constructor` of two registers: its prototype-chain walk is no word test,
    /// so the VM decides it (`CallOut::Decide`), guest code (`@@hasInstance`, a proxy) included.
    Instanceof,
    /// `key in object` of two registers: no word decides it, so the VM does (`CallOut::Decide`,
    /// `Vm::plain_in` for an ordinary object). The decode leaves `in` as `Operator`; a tier that
    /// has the VM's residual names it (`Comparison::by_vm`).
    Membership,
    /// A relation (`<`, `<=`, `>`, `>=`) of operands a tier proved are not both Numbers (a string
    /// constant): the VM's decision by its own operator (`CallOut::Decide`, `Vm::string_relation`),
    /// which leaves the row where a conversion would run guest code. The decode never names it.
    Decided(u32),
    /// Any other operator (`in` to a tier with no VM): no Number relation names it, so a tier
    /// decides it by the operator, and the step is `Unresolved` for the interpreter's exact path.
    Operator(u32),
}

impl Comparison {
    /// The binary operator the VM decides this comparison by; `None` for a Number relation,
    /// which no VM step decides.
    pub(crate) fn operator(self) -> Option<u32> {
        match self {
            Self::Relation { .. } | Self::Nullish { .. } | Self::Boolean { .. } | Self::Bitwise(_) => None,
            Self::Equality { strict, negated } => Some(equality_binary_operator(strict, negated)),
            Self::Instanceof => Some(INSTANCEOF_OPERATOR),
            Self::Membership => Some(IN_OPERATOR),
            Self::Decided(operator) | Self::Operator(operator) => Some(operator),
        }
    }

    #[cfg(not(stencil_build))]
    /// The comparison `operator` names (a `JumpBinaryFalse`'s operator field): the widest Number
    /// relation, an equality, a bitwise test, `instanceof`, else the operator itself. A tier with
    /// more facts than the operator narrows it (the observed Numbers, the proved domain).
    #[inline(always)]
    pub(crate) fn of_operator(operator: u32) -> Self {
        equality_operator(operator)
            .map(|(strict, negated)| Self::Equality { strict, negated })
            .or_else(|| NumberRelation::from_operator(operator)
                .map(|relation| Self::Relation { relation, domain: Domain::Float64 }))
            .or_else(|| NumberBitwise::from_operator(operator).map(Self::Bitwise))
            .or_else(|| (operator == INSTANCEOF_OPERATOR).then_some(Self::Instanceof))
            .unwrap_or(Self::Operator(operator))
    }

    /// The comparison where the VM's residual decides what no word does: `in`, which the decode
    /// leaves as an operator, is `Membership`; every other comparison is itself.
    pub(crate) const fn by_vm(self) -> Self {
        match self {
            Self::Operator(IN_OPERATOR) => Self::Membership,
            comparison => comparison,
        }
    }

    /// Whether deciding it may run guest code (a conversion, a trap), which retires what a
    /// tier proved of objects: every comparison but a Number relation, a bitwise test of Numbers,
    /// a nullish test and a strict equality.
    pub(crate) const fn runs_guest_code(self) -> bool {
        !matches!(self, Self::Relation { .. } | Self::Bitwise(_) | Self::Nullish { .. } | Self::Boolean { .. }
            | Self::Equality { strict: true, .. } | Self::Decided(_))
    }
}

#[cfg(not(stencil_build))]
impl<W, P> Operation<W, P> {
    #[cfg(not(stencil_build))]
    /// The `JumpBinaryFalse` row: it jumps where the comparison fails. Inlined into the row,
    /// which decodes its record on every run.
    #[inline(always)]
    pub(super) fn decode_compare(instruction: WideInstruction) -> Self {
        let comparison = Comparison::of_operator(instruction.binary_operator_field());
        Self::CompareJump { comparison, left: instruction.operand_b(), right: instruction.operand_c(), negate: true }
    }
}

/// Run the comparison jump `operation`: the edge its answer takes, or `Unresolved` where the
/// tier decides the comparison itself.
#[inline(always)]
pub(super) fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let Operation::CompareJump { comparison, left, right, negate } = operation else { return Step::Unresolved };
    let holds = match outcome(stage, frame, comparison, [left, right], negate) {
        Some(holds) => holds,
        None => match by_vm(stage, frame, comparison, [left, right]) {
            Some(holds) => holds,
            None => return Step::Unresolved,
        },
    };
    let taken = if negate { stage.not(holds) } else { holds };
    Step::Control(frame.branch(stage, taken))
}

/// `step_of` for any operation.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// Whether `comparison` of `operands` holds, for a jump that `negate`s it; `None` where only the
/// interpreter decides it.
#[inline(always)]
fn outcome<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, comparison: Comparison,
    [left, right]: [Operand; 2], negate: bool) -> Option<S::Cond> {
    match comparison {
        Comparison::Relation { relation, domain } =>
            super::numeric::relation_holds(stage, frame, left, right, relation, domain),
        Comparison::Equality { strict, negated } => equality(stage, frame, [left, right], (strict, negated)),
        Comparison::Nullish { expected, strict, negated } => {
            let equal_edge = if negated == negate { Edge::Target } else { Edge::Following };
            let equal = nullish(stage, frame, left, (expected, strict), equal_edge)?;
            Some(if negated { stage.not(equal) } else { equal })
        }
        Comparison::Boolean { expected, negated } => identity(stage, frame, left, Value::boolean(expected), negated),
        Comparison::Bitwise(operation) => bitwise(stage, frame, [left, right], operation),
        Comparison::Instanceof => instanceof(stage, frame, [left, right]),
        Comparison::Membership => decided_by_vm(stage, frame, [left, right], IN_OPERATOR),
        Comparison::Decided(operator) => decided_by_vm(stage, frame, [left, right], operator),
        Comparison::Operator(_) => None,
    }
}

/// Whether `comparison` of `[left, right]` holds where the words leave it, as the VM decides it, where the frame
/// takes the general operators (`NumericFrame::general_operators`). A relation, an operator, or a comparison of a
/// constant operand is `CallOut::Binary` (which takes constants); an equality the words leave is `CallOut::Equality`;
/// a bitwise test is `CallOut::Decide` over two registers and `CallOut::DecideOperand` otherwise. A comparison of two
/// registers the VM already decided (`decided_by_vm`) is no second ask. `None` where the frame keeps declining.
#[inline(always)]
fn by_vm<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, comparison: Comparison,
    [left, right]: [Operand; 2]) -> Option<S::Cond> {
    if !frame.general_operators() { return None; }
    by_vm_operator(stage, frame, comparison, [left, right])
}

/// No stencil frame takes the VM's general operators, and the stencil build has no parser's operator codes.
#[cfg(stencil_build)]
#[inline(always)]
fn by_vm_operator<S: NumericStage, F: NumericFrame<S>>(_stage: &mut S, _frame: &mut F, _comparison: Comparison,
    _operands: [Operand; 2]) -> Option<S::Cond> {
    None
}

/// `by_vm` of a frame that takes the VM's general operators.
#[cfg(not(stencil_build))]
#[inline(always)]
fn by_vm_operator<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, comparison: Comparison,
    [left, right]: [Operand; 2]) -> Option<S::Cond> {
    let both_registers = left.register_index().is_some() && right.register_index().is_some();
    match comparison {
        Comparison::Relation { relation, .. } => binary_decision(stage, frame, [left, right], relation.binary_operator()),
        Comparison::Operator(operator) => binary_decision(stage, frame, [left, right], operator),
        Comparison::Instanceof | Comparison::Membership | Comparison::Decided(_) if !both_registers =>
            binary_decision(stage, frame, [left, right], comparison.operator()?),
        // An equality the words leave (a constant operand the frame's words do not hold): the VM's equality.
        Comparison::Equality { strict, negated } => {
            let (decision, _) = frame.vm_step(stage, CallOut::Equality { left, right, strict, negated }, None)?;
            Some(stage.eq_imm(decision, Value::TRUE.to_bits()))
        }
        Comparison::Bitwise(operation) => {
            let call = bitwise_decision([left, right], operation.binary_operator());
            let (decision, _) = frame.vm_step(stage, call, None)?;
            Some(stage.eq_imm(decision, Value::TRUE.to_bits()))
        }
        _ => None,
    }
}

/// The VM's decision of `left operator right` by `CallOut::Binary`, whose operands may be constants: its boolean.
#[inline(always)]
fn binary_decision<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, [left, right]: [Operand; 2],
    operator: u32) -> Option<S::Cond> {
    let (decision, _) = frame.vm_step(stage, CallOut::Binary { left, right, operator }, None)?;
    Some(stage.eq_imm(decision, Value::TRUE.to_bits()))
}

/// Whether the pair is equal (unequal for `negated`): the words decide it where they can
/// (`generic::decide`), the VM's `CallOut::Decide` otherwise; an operand that is no register
/// leaves a pair the words do not decide at the frame's miss edge.
#[inline(always)]
fn equality<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, [left, right]: [Operand; 2],
    (strict, negated): (bool, bool)) -> Option<S::Cond> {
    let words = [frame.operand_word(stage, left)?, frame.operand_word(stage, right)?];
    let (decided, equal) = super::generic::decide(stage, words, strict);
    let holds = if negated { stage.not(equal) } else { equal };
    let (decision, _) = frame.boolean(stage, holds);
    let value = match [left.register_index(), right.register_index()] {
        [Some(left), Some(right)] => {
            let call = CallOut::Decide { left, right, operator: equality_binary_operator(strict, negated) };
            stage.choose_leaving(decided, |_| Some(decision), #[inline(always)] |stage| frame.vm_step(stage, call, None).map(|(word, _)| word))?
        }
        // A constant operand the words do not decide: the VM's equality, which takes constants (`CallOut::Equality`).
        _ if frame.general_operators() => {
            let call = CallOut::Equality { left, right, strict, negated };
            stage.choose_leaving(decided, |_| Some(decision), #[inline(always)] |stage| frame.vm_step(stage, call, None).map(|(word, _)| word))?
        }
        _ => {
            let miss = frame.miss();
            stage.guard(decided, miss)?;
            decision
        }
    };
    Some(stage.eq_imm(value, Value::TRUE.to_bits()))
}

/// Whether the tested operand is the one-encoding constant `expected` (not, for `negated`), by
/// identity.
#[inline(always)]
fn identity<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, tested: Operand, expected: Value,
    negated: bool) -> Option<S::Cond> {
    let word = frame.operand_word(stage, tested)?;
    let same = stage.eq_imm(word, expected.to_bits());
    Some(if negated { stage.not(same) } else { same })
}

/// The VM's decision of a bitwise test: `Decide` over two registers, `DecideOperand` where an operand is a constant.
fn bitwise_decision([left, right]: [Operand; 2], operator: u32) -> CallOut {
    match (left.register_index(), right.register_index()) {
        (Some(left), Some(right)) => CallOut::Decide { left, right, operator },
        _ => CallOut::DecideOperand { left, right, operator },
    }
}

/// The VM's decision of `left operator right`: the call-out, whose operands are registers.
fn decision_call([left, right]: [Operand; 2], operator: u32) -> Option<CallOut> {
    Some(CallOut::Decide { left: left.register_index()?, right: right.register_index()?, operator })
}

/// Whether `left operator right` holds, as the VM decides it; `None` where an operand is no
/// register (the call-out carries registers) or the frame has no VM to ask.
#[inline(always)]
fn decided_by_vm<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operands: [Operand; 2],
    operator: u32) -> Option<S::Cond> {
    let (decision, _) = frame.vm_step(stage, decision_call(operands, operator)?, None)?;
    Some(stage.eq_imm(decision, Value::TRUE.to_bits()))
}

/// Whether `value instanceof constructor` holds. Subsumes the hand `instanceof` relations of the lane
/// (`lane_instanceof_value`, `lane_instanceof_value_walk`, and the jump forms `lane_instanceof_jump_true`,
/// `lane_jump_instanceof_false`, which are this comparison under a `JumpTrue` or `JumpFalse`). The frame's own proof of the constructor's chain
/// (`Frame::instance_of`: a published watch, or an ordinary walk that runs no guest code) decides it
/// first where it holds. Else, where the frame holds the constructor's `prototype` as a Stable fact
/// (`NumericFrame::stable_prototype`) the value's own words decide it by a walk of its prototype links
/// (`object::instance_decision`), and the VM the pairs they leave; with no such fact, or no object record table
/// to walk in, it is the VM's decision.
#[inline(always)]
fn instanceof<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operands: [Operand; 2]) -> Option<S::Cond> {
    let [value, constructor] = operands;
    let left = frame.operand_word(stage, value);
    if let (Some(left), Some(right)) = (left, frame.operand_word(stage, constructor))
        && let Some(holds) = frame.instance_of(stage, left, right)
    {
        return Some(holds);
    }
    let target = constructor.register_index().and_then(|register| frame.stable_prototype(stage, register));
    let Some(target) = target else { return decided_by_vm(stage, frame, operands, INSTANCEOF_OPERATOR) };
    let Some(records) = frame.object_records(stage) else { return decided_by_vm(stage, frame, operands, INSTANCEOF_OPERATOR) };
    let call = decision_call(operands, INSTANCEOF_OPERATOR)?;
    let (decided, holds) = instance_decision(stage, records, left?, target);
    let decision = stage.choose_leaving(decided, |stage| {
        let [yes, no] = [Value::TRUE, Value::FALSE].map(|each| stage.constant(each.to_bits()));
        Some(stage.select(holds, yes, no))
    }, #[inline(always)] |stage| frame.vm_step(stage, call, None).map(|(word, _)| word))?;
    Some(stage.eq_imm(decision, Value::TRUE.to_bits()))
}

/// Whether the bits `operation` makes of the operands' ToInt32 are nonzero (`if (flags & MASK)`).
/// An operand that is no Number leaves the row.
#[inline(always)]
fn bitwise<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, [left, right]: [Operand; 2],
    operation: NumberBitwise) -> Option<S::Cond> {
    let miss = frame.miss();
    let (left, right) = (super::numeric::bitwise_word(stage, frame, left, miss)?, super::numeric::bitwise_word(stage, frame, right, miss)?);
    let bits = stage.int_bitwise(operation, left, right);
    let zero = stage.i32_is_zero(bits);
    Some(stage.not(zero))
}

/// Whether the tested operand is `expected` (or, loosely, either nullish value): a type that
/// already decides it folds the test, else the word's tag does, and the equal edge refines the
/// register. A heap cell an IsHTMLDDA object may be leaves at the miss edge.
#[inline(always)]
fn nullish<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, tested: Operand,
    (expected, strict): (ValueType, bool), equal_edge: Edge) -> Option<S::Cond> {
    let register = tested.register_index()?;
    if let Some(equal) = frame.register_type(register).and_then(|ty| known_equal(ty, expected, strict)) {
        let word = stage.constant(u64::from(equal));
        return Some(stage.ne_imm(word, 0u64));
    }
    let word = frame.tagged(stage, register);
    let equal = if strict { stage.eq_imm(word, nullish_value(expected).to_bits()) } else { loosely_nullish(stage, frame, word)? };
    frame.refine(register, if strict { Type::language(expected) } else { Type::NULLISH }, equal_edge);
    Some(equal)
}

/// Whether a value of type `ty` is `expected` (loosely, either nullish value) by its type
/// alone: the nullish types are, and objects, Numbers and Booleans are not.
#[inline(always)]
fn known_equal(ty: Type, expected: ValueType, strict: bool) -> Option<bool> {
    // Language-set tests only (`within_languages`), so a stencil's constant type folds them.
    if ty.within_languages(Type::language(ValueType::Null)) {
        Some(!strict || matches!(expected, ValueType::Null))
    } else if ty.within_languages(Type::language(ValueType::Undefined)) {
        Some(!strict || matches!(expected, ValueType::Undefined))
    } else {
        // Number or Boolean, as `Type::NUMBER.join(Type::language(ValueType::Boolean))`, as a constant a
        // stencil row folds (the join is not inlined there).
        const NUMBER_OR_BOOLEAN: Type = Type::languages(language_bit(ValueType::Number) | language_bit(ValueType::Boolean));
        (ty.is_direct_object_family() || ty.is_subtype(NUMBER_OR_BOOLEAN)).then_some(false)
    }
}

/// Whether `word` is `null` or `undefined`, as `==` with either holds. Where an IsHTMLDDA object
/// can exist, a legacy heap cell outside the object and array records may be it (equal to both),
/// which no word decides: it leaves at the miss edge.
#[inline(always)]
fn loosely_nullish<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, word: S::Word) -> Option<S::Cond> {
    if frame.html_dda_possible() {
        let heap = super::value::is_heap(stage, word);
        let (object, array) = (is_direct_object(stage, word), is_direct_array(stage, word));
        let record = stage.either(object, array);
        let unrecorded = stage.not(record);
        let cell = stage.both(heap, unrecorded);
        let miss = frame.miss();
        stage.guard_false(cell, miss)?;
    }
    Some(super::generic::nullish(stage, word))
}

#[cfg(test)]
mod tests;
