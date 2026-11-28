//! The `switch` case slot of a dense int label run.

use super::*;

/// The case slot of `source` in a `switch` over the dense int labels `minimum..=maximum` (`count` slots). A value
/// is its own label's slot where it is an int32 or an integral double in the run; every other value (a string,
/// an object, a fraction) takes `count`, the default. The int32 word is ToInt32 of the Number, which is exact
/// for the integral ones, so the classes come from the staged tag tests and float steps only.
#[inline(always)]
pub(super) fn int_case<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    source: Operand, (minimum, maximum): (i32, i32), count: u16) -> Step<S::Word, F::Control> {
    // An int32 the frame holds is its own integral value: only the label range decides its slot.
    if let Some(value) = frame.held_int(stage, source).map(super::super::Int32::word) {
        let slot = int_slot(stage, value, (minimum, maximum), count, None);
        let slot = frame.int_result(stage, slot);
        let fact = frame.int_fact();
        frame.write(stage, destination, slot, fact);
        return Step::Value(slot);
    }
    let Some(tagged) = frame.operand_word(stage, source) else { return Step::Unresolved };
    // An int32 is its own value: the int32 tag alone decides, and no float work runs for it. Any other word
    // is read as the double it holds (a Number's value, or the bits of a non-Number, which `integral` rejects).
    let int_tag = super::super::value::is_int(stage, tagged);
    let miss = frame.miss();
    let Some(slot) = stage.choose_leaving(int_tag, #[inline(always)] |stage| {
        let value = stage.narrow(tagged);
        Some(int_slot(stage, value, (minimum, maximum), count, None))
    }, #[inline(always)] |stage| {
        let double_tag = super::super::value::is_double(stage, tagged);
        let number = stage.f64_from_bits(tagged);
        let truncated = stage.float_unary(super::super::math::FloatUnary::Trunc, number);
        let exact = stage.relation(NumberRelation::Equal, Domain::Float64, truncated, number);
        let lowest = stage.f64_constant(f64::from(i32::MIN));
        let highest = stage.f64_constant(f64::from(i32::MAX));
        let above = stage.relation(NumberRelation::GreaterEqual, Domain::Float64, truncated, lowest);
        let below = stage.relation(NumberRelation::LessEqual, Domain::Float64, truncated, highest);
        let integral = stage.both(exact, above);
        let integral = stage.both(integral, below);
        let integral = stage.both(integral, double_tag);
        let value = stage.to_int32(number, miss)?;
        Some(int_slot(stage, value, (minimum, maximum), count, Some(integral)))
    }) else { return Step::Unresolved };
    // The int's word as the frame holds an int (`int_result`: the interpreter's tagged int), as `typeof_index` defines its.
    let slot = frame.int_result(stage, slot);
    let fact = frame.int_fact();
    frame.write(stage, destination, slot, fact);
    Step::Value(slot)
}

/// The case slot of the int32 `value` among the labels from `minimum` to `maximum`: its offset
/// from `minimum` inside them (and where `integral` holds, the operand being that int), else `count`.
#[inline(always)]
fn int_slot<S: NumericStage>(stage: &mut S, value: S::Word, (minimum, maximum): (i32, i32), count: u16,
    integral: Option<S::Cond>) -> S::Word {
    let low = stage.i32_constant(minimum);
    let high = stage.i32_constant(maximum);
    let from_low = stage.relation(NumberRelation::GreaterEqual, Domain::Int32, value, low);
    let to_high = stage.relation(NumberRelation::LessEqual, Domain::Int32, value, high);
    let inside = match integral { Some(integral) => stage.both(integral, from_low), None => from_low };
    let inside = stage.both(inside, to_high);
    let (offset, _) = stage.int_arithmetic(NumberArithmetic::Subtract, value, low);
    let fallback = stage.i32_constant(i32::from(count));
    stage.select(inside, offset, fallback)
}
