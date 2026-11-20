//! A call site's in-place call of a native, as staged rows: the admission of the native the lane
//! proved the site's callee to be (`admit_identity`), and the value of a Math function of Number
//! arguments (`math_call`), the int32 fast paths included. The interpreter runs them on machine
//! words (`Concrete`, the `concrete_*` wrappers) from the native call handlers
//! (`vm/dispatch_fast/native_calls.rs`) and from `vm::number::math_inline_call`; a tier that emits
//! them computes the same words. A row leaves at its `miss` for any case the native's own steps
//! decide: another callee, an argument that is no Number, an argument count the row does not take.

use super::math::FloatUnary;
use super::numeric::{Domain, NumericStage};
use super::value::{int_word, is_double, is_int};
use super::{Concrete, Stage};
use crate::vm::operations::{NumberArithmetic, NumberRelation};
use crate::Value;

/// Admits the call of the native a site's row runs: continuing where the callee word `callee` is
/// `expected`, the identity the lane proved for the row (one compare). The identity is a
/// dependency of the lane's generation: a collection, which may free the function and reuse its
/// id, or another realm starts a new generation, which forgets it.
#[inline(always)]
pub(crate) fn admit_identity<S: Stage>(stage: &mut S, callee: S::Word, expected: S::Word, miss: S::Miss) -> Option<()> {
    let difference = stage.sub(callee, expected);
    let same = stage.eq_imm(difference, 0u32);
    stage.guard(same, miss)
}

/// `admit_identity` on machine words, for the interpreter's native call handlers.
#[inline(always)]
pub(crate) fn concrete_admit_identity(callee: u64, expected: u64) -> bool {
    admit_identity(&mut Concrete::new(std::ptr::null()), callee, expected, ()).is_some()
}

/// A Math function of Number arguments: the one table of them (`vm::number::math_call`), from which
/// each tier derives its dispatch.
// No equality: it holds function pointers, whose addresses name no function uniquely.
#[derive(Clone, Copy, Debug)]
pub(crate) enum MathCall {
    /// A machine operation of one Number (`staged::math::float_unary`), `Math.sign` included.
    Unary(FloatUnary),
    /// `Math.max` (true) or `Math.min` (false) of any count of Numbers.
    Extreme(bool),
    /// `Math.imul`: the wrapping product of the arguments' ToInt32.
    Imul,
    /// `Math.clz32`: the leading zero bits of the argument's ToUint32, as an int32.
    LeadingZeros,
    /// A libm function of one Number, or a body the stages do not run (`Math.atanh`, `Math.cos`, ...).
    Libm(fn(f64) -> f64),
    /// `Math.pow` or `Math.atan2`: a function of two Numbers, which no stage runs.
    Binary(fn(f64, f64) -> f64),
}

impl MathCall {
    /// The value `Math.max` (`maximum`) or `Math.min` folds its arguments from, and returns for
    /// none: the identity of the extreme, which a NaN argument still overrides.
    pub(crate) const fn extreme_start(maximum: bool) -> f64 {
        if maximum { f64::NEG_INFINITY } else { f64::INFINITY }
    }

    /// Whether a stage runs the function as a row (`math_call`), which native code lowers inline:
    /// every function but the libm and two-Number bodies.
    pub(crate) const fn is_row(self) -> bool {
        !matches!(self, Self::Libm(_) | Self::Binary(_))
    }

    /// Whether the function takes one Number (`Math.sqrt`, `Math.clz32`, `Math.sin`, ...).
    pub(crate) const fn reads_one_number(self) -> bool {
        matches!(self, Self::Unary(_) | Self::LeadingZeros | Self::Libm(_))
    }

    /// The number of arguments the function's operation reads: two for the binary functions, else one.
    pub(crate) const fn arity(self) -> u16 {
        match self {
            Self::Extreme(_) | Self::Imul | Self::Binary(_) => 2,
            Self::Unary(_) | Self::LeadingZeros | Self::Libm(_) => 1,
        }
    }
}

/// The binary64 word of the Number value word `bits`: an int32 widened, a double as it is. `None`
/// (leaving at `miss`) for any other value, which the native converts.
#[inline(always)]
fn number_word<S: NumericStage>(stage: &mut S, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    let int = is_int(stage, bits);
    let double = is_double(stage, bits);
    let number = stage.any(int, double);
    stage.guard(number, miss)?;
    Some(stage.choose(int, #[inline(always)] |stage| {
        let payload = stage.narrow(bits);
        stage.i32_to_f64(payload)
    }, #[inline(always)] |stage| stage.f64_from_bits(bits)))
}

/// The value word of the int32 word `payload` (`Value::integer`).
#[inline(always)]
fn int_value<S: NumericStage>(stage: &mut S, payload: S::Word) -> S::Word {
    let payload = stage.uextend32(payload);
    int_word(stage, payload)
}

/// The value word of `call` of the Number value words `args`, as the native returns it: `None`
/// (leaving at `miss`) where an argument is no Number, and, decided by the site's argument count,
/// for a count the function's Number steps do not cover (a one-argument function passed none or
/// two, any function passed more than two), or a function no stage runs (`Libm`, `Binary`).
///
/// An int32 result is the int32 the double path would box to (`Value::number` stores an integral
/// double in int32 range other than -0 as that int32), so the int32 fast paths of `Math.abs`,
/// `Math.max`, `Math.min` and `Math.imul` change no result: they skip the conversions.
#[inline(always)]
pub(crate) fn math_call<S: NumericStage>(stage: &mut S, call: MathCall, args: &[S::Word], miss: S::Miss) -> Option<S::Word> {
    match (call, args) {
        (_, &[only]) => math_call_of_one(stage, call, only, miss),
        (MathCall::Extreme(maximum), &[left, right]) => extreme_pair(stage, maximum, (left, right), miss),
        (MathCall::Imul, &[left, right]) => imul_pair(stage, (left, right), miss),
        (MathCall::Extreme(maximum), &[]) => {
            let start = stage.f64_constant(MathCall::extreme_start(maximum));
            Some(stage.box_number(start))
        }
        (MathCall::Imul, &[]) => {
            // Both arguments are `undefined`, whose ToInt32 is 0.
            let zero = stage.i32_constant(0);
            Some(int_value(stage, zero))
        }
        _ => None,
    }
}

/// `math_call` of one argument, the value word `only`.
#[inline(always)]
fn math_call_of_one<S: NumericStage>(stage: &mut S, call: MathCall, only: S::Word, miss: S::Miss) -> Option<S::Word> {
    if matches!(call, MathCall::Unary(FloatUnary::Abs)) {
        return absolute(stage, only, miss);
    }
    let number = number_word(stage, only, miss)?;
    let result = match call {
        MathCall::Unary(operation) => stage.float_unary(operation, number),
        MathCall::Extreme(maximum) => {
            let start = stage.f64_constant(MathCall::extreme_start(maximum));
            stage.float_extreme(maximum, start, number)
        }
        MathCall::Imul => {
            // The missing argument is `undefined`, whose ToInt32 is 0: the product is 0, once
            // the given argument is a Number.
            let zero = stage.i32_constant(0);
            return Some(int_value(stage, zero));
        }
        MathCall::LeadingZeros => {
            // ToUint32 and ToInt32 share their low 32 bits, which the count reads.
            let word = stage.to_int32(number, miss)?;
            let count = stage.int_leading_zeros(word);
            return Some(int_value(stage, count));
        }
        MathCall::Libm(_) | MathCall::Binary(_) => return None,
    };
    Some(stage.box_number(result))
}

/// `Math.abs` of the value word `only`: an int32 other than `i32::MIN` by negation, whose result
/// is an int32 again; any other Number by the double's magnitude.
#[inline(always)]
fn absolute<S: NumericStage>(stage: &mut S, only: S::Word, miss: S::Miss) -> Option<S::Word> {
    let int = is_int(stage, only);
    let not_minimum = stage.ne_imm(only, Value::integer(i32::MIN).to_bits());
    let negatable = stage.all(int, not_minimum);
    stage.choose_leaving(negatable, #[inline(always)] |stage| {
        let payload = stage.narrow(only);
        let negative = stage.i32_is_negative(payload);
        let zero = stage.i32_constant(0);
        let (negated, _) = stage.int_arithmetic(NumberArithmetic::Subtract, zero, payload);
        let magnitude = stage.select(negative, negated, payload);
        Some(int_value(stage, magnitude))
    }, #[inline(always)] |stage| {
        let number = number_word(stage, only, miss)?;
        let result = stage.float_unary(FloatUnary::Abs, number);
        Some(stage.box_number(result))
    })
}

/// `Math.max` (`maximum`) or `Math.min` of two value words: two int32s by their int32 order, any
/// other two Numbers by `float_extreme`. The native folds from `MathCall::extreme_start`, which
/// `float_extreme` of any Number returns unchanged (a NaN as a NaN), so the pair alone decides.
#[inline(always)]
fn extreme_pair<S: NumericStage>(stage: &mut S, maximum: bool, (left, right): (S::Word, S::Word), miss: S::Miss)
    -> Option<S::Word> {
    let (left_int, right_int) = (is_int(stage, left), is_int(stage, right));
    let ints = stage.all(left_int, right_int);
    stage.choose_leaving(ints, #[inline(always)] |stage| {
        let (x, y) = (stage.narrow(left), stage.narrow(right));
        let greater = stage.relation(NumberRelation::GreaterThan, Domain::Int32, x, y);
        let chosen = if maximum { stage.select(greater, x, y) } else { stage.select(greater, y, x) };
        Some(int_value(stage, chosen))
    }, #[inline(always)] |stage| {
        let x = number_word(stage, left, miss)?;
        let y = number_word(stage, right, miss)?;
        let result = stage.float_extreme(maximum, x, y);
        Some(stage.box_number(result))
    })
}

/// `Math.imul` of two value words: the wrapping product of two int32s as they are (ToInt32 of an
/// int32 is itself), of any other two Numbers after their ToInt32.
#[inline(always)]
fn imul_pair<S: NumericStage>(stage: &mut S, (left, right): (S::Word, S::Word), miss: S::Miss) -> Option<S::Word> {
    let (left_int, right_int) = (is_int(stage, left), is_int(stage, right));
    let ints = stage.all(left_int, right_int);
    stage.choose_leaving(ints, #[inline(always)] |stage| {
        let (x, y) = (stage.narrow(left), stage.narrow(right));
        let product = stage.int_multiply(x, y);
        Some(int_value(stage, product))
    }, #[inline(always)] |stage| {
        let x = number_word(stage, left, miss)?;
        let y = number_word(stage, right, miss)?;
        let x = stage.to_int32(x, miss)?;
        let y = stage.to_int32(y, miss)?;
        let product = stage.int_multiply(x, y);
        Some(int_value(stage, product))
    })
}

/// `math_call` of `call` on the interpreter's values, and the bodies no stage runs: a libm function
/// of one Number, or a two-Number function, whose missing argument is `undefined`, a NaN.
/// `None` for any argument or count the row leaves at its miss.
#[inline(always)]
pub(crate) fn concrete_math_call(call: MathCall, args: &[Value]) -> Option<Value> {
    match (call, args) {
        (MathCall::Libm(operation), &[only]) => Some(Value::number(operation(only.as_number()?))),
        (MathCall::Binary(operation), _) if args.len() <= 2 => {
            let argument = |index: usize| args.get(index).map_or(Some(f64::NAN), |value| value.as_number());
            Some(Value::number(operation(argument(0)?, argument(1)?)))
        }
        _ => concrete_row(call, args),
    }
}

/// `math_call` of `call` on the interpreter's values, for the stage rows.
#[inline(always)]
fn concrete_row(call: MathCall, args: &[Value]) -> Option<Value> {
    let stage = &mut Concrete::new(std::ptr::null());
    match *args {
        [] => math_call(stage, call, &[], ()),
        [only] => math_call(stage, call, &[only.to_bits()], ()),
        [left, right] => math_call(stage, call, &[left.to_bits(), right.to_bits()], ()),
        _ => None,
    }.map(Value::from_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(call: MathCall, args: &[Value]) -> Option<Value> {
        concrete_math_call(call, args)
    }

    #[test]
    fn the_identity_admission_is_one_compare_of_the_callee_word() {
        assert!(concrete_admit_identity(7, 7));
        assert!(!concrete_admit_identity(7, 8));
    }

    #[test]
    fn abs_keeps_int32s_but_the_minimum_and_boxes_doubles() {
        assert_eq!(call(MathCall::Unary(FloatUnary::Abs), &[Value::integer(-5)]), Some(Value::integer(5)));
        assert_eq!(call(MathCall::Unary(FloatUnary::Abs), &[Value::integer(i32::MIN)]), Some(Value::number(2_147_483_648.0)));
        assert_eq!(call(MathCall::Unary(FloatUnary::Abs), &[Value::number(-0.0)]), Some(Value::integer(0)));
        assert_eq!(call(MathCall::Unary(FloatUnary::Abs), &[Value::number(-2.5)]), Some(Value::number(2.5)));
        assert_eq!(call(MathCall::Unary(FloatUnary::Abs), &[Value::UNDEFINED]), None);
    }

    #[test]
    fn unary_functions_box_as_value_number_and_take_exactly_one_argument() {
        assert_eq!(call(MathCall::Unary(FloatUnary::Sqrt), &[Value::integer(16)]), Some(Value::integer(4)));
        assert_eq!(call(MathCall::Unary(FloatUnary::Floor), &[Value::number(-0.5)]), Some(Value::integer(-1)));
        assert!(call(MathCall::Unary(FloatUnary::Sqrt), &[Value::integer(-1)]).is_some_and(|value| value.as_number().is_some_and(f64::is_nan)));
        assert_eq!(call(MathCall::Unary(FloatUnary::Sqrt), &[]), None);
        assert_eq!(call(MathCall::Unary(FloatUnary::Sqrt), &[Value::integer(1), Value::integer(2)]), None);
    }

    #[test]
    fn round_takes_halves_up_and_keeps_zeros_as_they_are() {
        assert_eq!(call(MathCall::Unary(FloatUnary::Round), &[Value::number(2.5)]), Some(Value::integer(3)));
        assert_eq!(call(MathCall::Unary(FloatUnary::Round), &[Value::number(-2.5)]), Some(Value::integer(-2)));
        assert_eq!(call(MathCall::Unary(FloatUnary::Round), &[Value::number(-0.2)]).map(Value::to_bits), Some(Value::number(-0.0).to_bits()));
        assert_eq!(call(MathCall::Unary(FloatUnary::Round), &[Value::number(-0.0)]).map(Value::to_bits), Some(Value::number(-0.0).to_bits()));
    }

    #[test]
    fn sign_keeps_nan_and_zeros_and_maps_the_rest_to_a_unit() {
        let sign = MathCall::Unary(FloatUnary::Sign);
        assert_eq!(call(sign, &[Value::number(-3.5)]), Some(Value::integer(-1)));
        assert_eq!(call(sign, &[Value::integer(9)]), Some(Value::integer(1)));
        assert_eq!(call(sign, &[Value::number(-0.0)]).map(Value::to_bits), Some(Value::number(-0.0).to_bits()));
        assert_eq!(call(sign, &[Value::NAN]), Some(Value::NAN));
    }

    #[test]
    fn clz32_counts_the_low_32_bits_of_the_toint32() {
        assert_eq!(call(MathCall::LeadingZeros, &[Value::integer(1)]), Some(Value::integer(31)));
        assert_eq!(call(MathCall::LeadingZeros, &[Value::integer(0)]), Some(Value::integer(32)));
        assert_eq!(call(MathCall::LeadingZeros, &[Value::number(4_294_967_296.0 + 1.0)]), Some(Value::integer(31)));
    }

    #[test]
    fn extremes_order_int32s_and_numbers_and_fold_from_their_identity() {
        assert_eq!(call(MathCall::Extreme(true), &[Value::integer(3), Value::integer(-7)]), Some(Value::integer(3)));
        assert_eq!(call(MathCall::Extreme(false), &[Value::integer(3), Value::integer(-7)]), Some(Value::integer(-7)));
        assert_eq!(call(MathCall::Extreme(false), &[Value::integer(0), Value::number(-0.0)]).map(Value::to_bits), Some(Value::number(-0.0).to_bits()));
        assert_eq!(call(MathCall::Extreme(true), &[Value::integer(1), Value::NAN]), Some(Value::NAN));
        assert_eq!(call(MathCall::Extreme(true), &[]), Some(Value::number(f64::NEG_INFINITY)));
        assert_eq!(call(MathCall::Extreme(false), &[Value::number(2.5)]), Some(Value::number(2.5)));
        assert_eq!(call(MathCall::Extreme(true), &[Value::integer(1), Value::UNDEFINED]), None);
    }

    #[test]
    fn imul_wraps_the_toint32_product_and_reads_a_missing_argument_as_zero() {
        assert_eq!(call(MathCall::Imul, &[Value::integer(0x7fff_ffff), Value::integer(2)]), Some(Value::integer(-2)));
        assert_eq!(call(MathCall::Imul, &[Value::number(4_294_967_297.0), Value::number(3.9)]), Some(Value::integer(3)));
        assert_eq!(call(MathCall::Imul, &[Value::integer(5)]), Some(Value::integer(0)));
        assert_eq!(call(MathCall::Imul, &[]), Some(Value::integer(0)));
        assert_eq!(call(MathCall::Imul, &[Value::UNDEFINED]), None);
    }

    #[test]
    fn binary_functions_read_a_missing_argument_as_nan() {
        let pow = MathCall::Binary(crate::staged::math::power);
        assert_eq!(call(pow, &[Value::integer(2), Value::integer(3)]), Some(Value::integer(8)));
        assert!(call(pow, &[Value::integer(2)]).is_some_and(|value| value.as_number().is_some_and(f64::is_nan)));
        assert_eq!(call(pow, &[Value::UNDEFINED, Value::integer(1)]), None);
    }
}
