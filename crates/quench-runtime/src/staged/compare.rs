//! A conditional jump on a comparison of two operands (`Operation::CompareJump`, the fused
//! `JumpBinaryFalse`): each comparison is decided once over the stage, and its answer picks the
//! control edge through the frame's `branch`, so the interpreter, the lane and every other tier
//! jump on one definition. A pair the words do not decide is the VM's (`CallOut::Decide`).

use super::object::object_record_at;
use super::value::{is_direct_array, is_direct_object, is_direct_string};
use super::{CallOut, Domain, Edge, NumericFrame, NumericStage, Operation, Step};
use crate::bytecode::{Operand, WideInstruction};
use crate::typed_ir::Type;
use crate::value::ValueLayout;
use crate::vm::operations::{equality_binary_operator, equality_operator, NumberBitwise, NumberRelation};
use crate::{Value, ValueType};

/// The `null` or `undefined` a nullish test compares with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Nullish {
    Null,
    Undefined,
}

impl Nullish {
    /// The nullish `value`, where it is one.
    pub(crate) fn of(value: Value) -> Option<Self> {
        [Self::Null, Self::Undefined].into_iter().find(|nullish| nullish.value() == value)
    }

    fn value(self) -> Value {
        match self { Self::Null => Value::NULL, Self::Undefined => Value::UNDEFINED }
    }

    /// The semantic type of the one value.
    fn ty(self) -> Type {
        Type::language(match self { Self::Null => ValueType::Null, Self::Undefined => ValueType::Undefined })
    }
}

/// The `instanceof` operator.
const INSTANCEOF_OPERATOR: u32 = oxc_ast::ast::BinaryOperator::Instanceof as u32;

/// The `in` operator.
const IN_OPERATOR: u32 = oxc_ast::ast::BinaryOperator::In as u32;

/// What a `CompareJump` compares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Comparison {
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
    Nullish { expected: Nullish, strict: bool, negated: bool },
    /// The truth of a bitwise operator's result of two Numbers (`if (flags & MASK)`): its int32
    /// bits are nonzero, so the Number the operator would make is never made.
    Bitwise(NumberBitwise),
    /// `===` or `!==` (`negated`) of the tested operand (the row's `left`) and the Boolean
    /// `expected`: a Boolean has one encoding, so identity decides it.
    Boolean { expected: bool, negated: bool },
    /// `value instanceof constructor` of two registers: its prototype-chain walk is no word test,
    /// so the VM decides it (`CallOut::Decide`), guest code (`@@hasInstance`, a proxy) included.
    Instanceof,
    /// `key in object` of two registers: no word decides it, so the VM does (`CallOut::Decide`,
    /// `Vm::plain_in` for an ordinary object). The decode leaves `in` as `Operator`; a tier that
    /// has the VM's residual names it (`Comparison::by_vm`).
    Membership,
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
            Self::Operator(operator) => Some(operator),
        }
    }

    /// The comparison `operator` names (a `JumpBinaryFalse`'s operator field): the widest Number
    /// relation, an equality, a bitwise test, `instanceof`, else the operator itself. A tier with
    /// more facts than the operator narrows it (the observed Numbers, the proved domain).
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
            | Self::Equality { strict: true, .. })
    }
}

impl<W, P> Operation<W, P> {
    /// The `JumpBinaryFalse` row: it jumps where the comparison fails.
    pub(super) fn decode_compare(instruction: WideInstruction) -> Self {
        let comparison = Comparison::of_operator(instruction.binary_operator_field());
        Self::CompareJump { comparison, left: instruction.operand_b(), right: instruction.operand_c(), negate: true }
    }
}

/// Run the comparison jump `operation`: the edge its answer takes, or `Unresolved` where the
/// tier decides the comparison itself.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let Operation::CompareJump { comparison, left, right, negate } = operation else { return Step::Unresolved };
    let Some(holds) = outcome(stage, frame, comparison, [left, right], negate) else { return Step::Unresolved };
    let taken = if negate { stage.not(holds) } else { holds };
    Step::Control(frame.branch(stage, taken))
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
            nullish(stage, frame, left, (expected, strict), equal_edge)
        }
        Comparison::Boolean { expected, negated } => identity(stage, frame, left, Value::boolean(expected), negated),
        Comparison::Bitwise(operation) => bitwise(stage, frame, [left, right], operation),
        Comparison::Instanceof => instanceof(stage, frame, [left, right]),
        Comparison::Membership => decided_by_vm(stage, frame, [left, right], IN_OPERATOR),
        Comparison::Operator(_) => None,
    }
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
            stage.choose_leaving(decided, |_| Some(decision), |stage| frame.vm_step(stage, call, None).map(|(word, _)| word))?
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

/// The longest prototype chain a word walk follows: a longer one is the VM's decision.
const PROTOTYPE_WALK_LINKS: u32 = 16;

/// Whether `value instanceof constructor` holds. Where the frame holds the constructor's `prototype`
/// as a Stable fact (`NumericFrame::stable_prototype`) the value's own words decide it by a walk
/// of its prototype links (`walked_instanceof`), and the VM the pairs they leave; with no such
/// fact, or no object record table to walk in, it is the VM's decision.
#[inline(always)]
fn instanceof<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operands: [Operand; 2]) -> Option<S::Cond> {
    let [value, constructor] = operands;
    let target = constructor.register_index().and_then(|register| frame.stable_prototype(stage, register));
    let Some(target) = target else { return decided_by_vm(stage, frame, operands, INSTANCEOF_OPERATOR) };
    let Some(records) = frame.object_records(stage) else { return decided_by_vm(stage, frame, operands, INSTANCEOF_OPERATOR) };
    let call = decision_call(operands, INSTANCEOF_OPERATOR)?;
    let word = frame.operand_word(stage, value)?;
    let answer = walked_instanceof(stage, (records, word), target);
    let undecided = stage.eq_imm(answer, Value::DELETED.to_bits());
    let decided = stage.not(undecided);
    let decision = stage.choose_leaving(decided, |_| Some(answer),
        |stage| frame.vm_step(stage, call, None).map(|(word, _)| word))?;
    Some(stage.eq_imm(decision, Value::TRUE.to_bits()))
}

/// `value instanceof` a constructor whose `prototype` link is `target`, by the words: `TRUE` or
/// `FALSE`, or `DELETED` where only the VM decides. A primitive (an immediate, a string) is no
/// instance, as the algorithm decides before it reads `prototype`; a direct object's record is
/// walked (`Stage::walk_prototypes`); any other cell (an array, a function, a proxy) is the VM's.
#[inline(always)]
fn walked_instanceof<S: NumericStage>(stage: &mut S, (records, value): (S::Word, S::Word), target: S::Word) -> S::Word {
    let direct = is_direct_object(stage, value);
    stage.choose(direct, |stage| {
        let id = stage.and_imm(value, u64::from(u32::MAX));
        let record = object_record_at(stage, records, id);
        let (decided, holds) = stage.walk_prototypes(records, record, target, PROTOTYPE_WALK_LINKS);
        let [yes, no, unknown] = [Value::TRUE, Value::FALSE, Value::DELETED].map(|each| stage.constant(each.to_bits()));
        let answer = stage.select(holds, yes, no);
        stage.select(decided, answer, unknown)
    }, |stage| {
        let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
        let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
        let immediate = stage.not(heap);
        let string = is_direct_string(stage, value);
        let primitive = stage.any(immediate, string);
        let (no, unknown) = (stage.constant(Value::FALSE.to_bits()), stage.constant(Value::DELETED.to_bits()));
        stage.select(primitive, no, unknown)
    })
}

/// Whether the bits `operation` makes of the operands' ToInt32 are nonzero (`if (flags & MASK)`).
/// An operand that is no Number leaves the row.
#[inline(always)]
fn bitwise<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, [left, right]: [Operand; 2],
    operation: NumberBitwise) -> Option<S::Cond> {
    let miss = frame.miss();
    let (left, right) = (frame.int32_operand(stage, left, miss)?, frame.int32_operand(stage, right, miss)?);
    let bits = stage.int_bitwise(operation, left, right);
    let zero = stage.i32_is_zero(bits);
    Some(stage.not(zero))
}

/// Whether the tested operand is `expected` (or, loosely, either nullish value): a type that
/// already decides it folds the test, else the word's tag does, and the equal edge refines the
/// register. A heap cell an IsHTMLDDA object may be leaves at the miss edge.
#[inline(always)]
fn nullish<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, tested: Operand,
    (expected, strict): (Nullish, bool), equal_edge: Edge) -> Option<S::Cond> {
    let register = tested.register_index()?;
    if let Some(equal) = frame.register_type(register).and_then(|ty| known_equal(ty, expected, strict)) {
        let word = stage.constant(u64::from(equal));
        return Some(stage.ne_imm(word, 0u64));
    }
    let word = frame.tagged(stage, register);
    let equal = if strict { stage.eq_imm(word, expected.value().to_bits()) } else { loosely_nullish(stage, frame, word)? };
    frame.refine(register, if strict { expected.ty() } else { Type::NULLISH }, equal_edge);
    Some(equal)
}

/// Whether a value of type `ty` is `expected` (loosely, either nullish value) by its type
/// alone: the nullish types are, and objects, Numbers and Booleans are not.
fn known_equal(ty: Type, expected: Nullish, strict: bool) -> Option<bool> {
    let [null, undefined] = [ValueType::Null, ValueType::Undefined].map(Type::language);
    if ty.is_subtype(null) {
        Some(!strict || expected == Nullish::Null)
    } else if ty.is_subtype(undefined) {
        Some(!strict || expected == Nullish::Undefined)
    } else {
        (ty.is_direct_object_family() || ty.is_subtype(Type::NUMBER.join(Type::language(ValueType::Boolean)))).then_some(false)
    }
}

/// Whether `word` is `null` or `undefined`, as `==` with either holds. Where an IsHTMLDDA object
/// can exist, a legacy heap cell outside the object and array records may be it (equal to both),
/// which no word decides: it leaves at the miss edge.
#[inline(always)]
fn loosely_nullish<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, word: S::Word) -> Option<S::Cond> {
    if frame.html_dda_possible() {
        let tag = stage.and_imm(word, ValueLayout::TAG_MASK);
        let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
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
