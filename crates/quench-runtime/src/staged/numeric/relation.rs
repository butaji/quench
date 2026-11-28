//! Number relations (`<`, `<=`, `===`, ...) and the Boolean they define.

use super::*;

/// A relation of two operands in `domain`; its predicate word.
#[inline(always)]
pub(super) fn relation<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand, relation: NumberRelation, domain: Domain) -> Step<S::Word, F::Control> {
    let Some(holds) = relation_holds(stage, frame, left, right, relation, domain) else { return Step::Unresolved };
    Step::Value(stage.cond_word(holds))
}

/// A relation of two operands in `domain`, defining its Boolean in `destination`.
#[inline(always)]
pub(super) fn relation_value<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand, relation: NumberRelation, domain: Domain) -> Step<S::Word, F::Control> {
    let Some(holds) = relation_holds(stage, frame, left, right, relation, domain) else { return Step::Unresolved };
    let (value, fact) = frame.boolean(stage, holds);
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}

/// Whether `relation` holds of two operands in `domain`; `None` where the frame proves neither.
#[inline(always)]
pub fn relation_holds<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand, relation: NumberRelation, domain: Domain) -> Option<S::Cond> {
    match domain {
        Domain::Int32 => {
            let (left, right) = operands(stage, frame, left, right, domain)?;
            Some(stage.relation(relation, domain, left, right))
        }
        Domain::Float64 => relation_number(stage, frame, (left, right), relation),
    }
}

/// `relation` of two Number operands: where both are int32 at run time it is one int32 compare, and every
/// other pair takes the binary64 compare (`relation_float`). A choice yields a predicate word, so the two
/// arms each keep their own compare, and the word is read back as the condition.
#[inline(always)]
fn relation_number<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    (left, right): (Operand, Operand), relation: NumberRelation) -> Option<S::Cond> {
    // A pair match rather than `Option::zip`, whose instance stays out of line across crates.
    let (Some((left_int, left_test)), Some((right_int, right_test))) =
        (fast_int(stage, frame, left), fast_int(stage, frame, right))
    else {
        return relation_float(stage, frame, (left, right), relation);
    };
    let Some(tests) = both_tests(stage, [left_test, right_test]) else {
        return Some(stage.relation(relation, Domain::Int32, left_int, right_int));
    };
    let word = stage.choose_leaving(tests, #[inline(always)] |stage| {
        let holds = stage.relation(relation, Domain::Int32, left_int, right_int);
        Some(stage.cond_word(holds))
    }, #[inline(always)] |stage| {
        let holds = relation_float(stage, frame, (left, right), relation)?;
        Some(stage.cond_word(holds))
    })?;
    Some(stage.ne_imm(word, 0u64))
}

/// `relation` of two Number operands as binary64 (`float_word`); `None` where an operand is no Number.
#[inline(always)]
fn relation_float<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    (left, right): (Operand, Operand), relation: NumberRelation) -> Option<S::Cond> {
    let (left, right) = operands(stage, frame, left, right, Domain::Float64)?;
    Some(stage.relation(relation, Domain::Float64, left, right))
}
