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
use super::region::Region;
use super::{Concrete, Stage, word_bytes, word_offset};
use crate::bytecode::Register;
use crate::vm::operations::{NumberArithmetic, NumberRelation};
use crate::Value;

/// Admits the call of the native a site's row runs: continuing where the callee word `callee` is
/// `expected`, the identity the lane proved for the row (one compare). The identity is a
/// dependency of the lane's generation: a collection, which may free the function and reuse its
/// id, or another realm starts a new generation, which forgets it.
#[inline(always)]
pub fn admit_identity<S: Stage>(stage: &mut S, callee: S::Word, expected: S::Word, miss: S::Miss) -> Option<()> {
    let difference = stage.sub(callee, expected);
    let same = stage.eq_imm(difference, 0u32);
    stage.guard(same, miss)
}

/// `admit_identity` on machine words, for the interpreter's native call handlers.
#[inline(always)]
pub fn concrete_admit_identity(callee: u64, expected: u64) -> bool {
    admit_identity(&mut Concrete::pure(), callee, expected, ()).is_some()
}

/// The registers of a call site's in-place native call: its callee, receiver, first argument (the
/// window the arguments lie in) and result.
#[derive(Clone, Copy, Debug)]
pub struct InPlaceCall {
    pub callee: Register,
    pub receiver: Register,
    pub arguments: Register,
    pub result: Register,
}

/// A tier's in-place body of the native a site's row runs: the value word for `receiver` and the
/// arguments at the address `arguments`, or `None` (leaving at `miss`, before any write) where the
/// native's own steps decide. The interpreter's body is `Vm::in_place_native_value`; the b-jit's
/// builtin rows prepare their operands and run the shared native steps (`step_string_native_of`,
/// `step_number_native_of`, `builtins::step_array_native_of`).
pub trait InPlaceBody<S: Stage> {
    fn in_place_value(&mut self, stage: &mut S, receiver: S::Word, arguments: S::Word, miss: S::Miss) -> Option<S::Word>;
}

/// The in-place call of a native at a site whose registers lie in the file at `registers`: the
/// callee admitted against the identity the tier proved for it (`admit_identity`), the body over
/// the receiver and the argument window where it lies, and its value stored to the result register.
/// The one step of an in-place native call: the interpreter's in-place handlers
/// (`lane_call_in_place`) and the b-jit's builtin rows run it. `None` (leaving at `miss`, before any
/// write) where the callee is another or the body declines.
#[inline(always)]
pub fn step_in_place_call_of<S: Stage, B: InPlaceBody<S>>(stage: &mut S, body: &mut B, registers: S::Word, identity: S::Word,
    call: InPlaceCall, miss: S::Miss) -> Option<S::Word> {
    let callee = stage.load_u64(Region::Frame.mutable(), registers, word_offset(call.callee));
    admit_identity(stage, callee, identity, miss)?;
    let receiver = stage.load_u64(Region::Frame.mutable(), registers, word_offset(call.receiver));
    let arguments = stage.add_imm(registers, word_bytes(call.arguments));
    let value = body.in_place_value(stage, receiver, arguments, miss)?;
    stage.store_u64(Region::Frame.mutable(), registers, word_offset(call.result), value);
    Some(value)
}

/// A Math function of Number arguments: the one table of them (`vm::number::math_call`), from which
/// each tier derives its dispatch.
// No equality: it holds function pointers, whose addresses name no function uniquely.
#[derive(Clone, Copy, Debug)]
pub enum MathCall {
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
    pub const fn extreme_start(maximum: bool) -> f64 {
        if maximum { f64::NEG_INFINITY } else { f64::INFINITY }
    }

    /// Whether a stage runs the function as a row (`math_call`), which native code lowers inline:
    /// every function but the libm and two-Number bodies.
    pub const fn is_row(self) -> bool {
        !matches!(self, Self::Libm(_) | Self::Binary(_))
    }

    /// Whether the function takes one Number (`Math.sqrt`, `Math.clz32`, `Math.sin`, ...).
    pub const fn reads_one_number(self) -> bool {
        matches!(self, Self::Unary(_) | Self::LeadingZeros | Self::Libm(_))
    }

    /// The number of arguments the function's operation reads: two for the binary functions, else one.
    pub const fn arity(self) -> u16 {
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
pub fn math_call<S: NumericStage>(stage: &mut S, call: MathCall, args: &[S::Word], miss: S::Miss) -> Option<S::Word> {
    match (call, args) {
        (_, &[only]) => math_call_of_one(stage, call, only, miss),
        (MathCall::Extreme(maximum), &[left, right]) => extreme_pair(stage, maximum, (left, right), miss),
        (MathCall::Imul, &[left, right]) => imul_pair(stage, (left, right), miss),
        (MathCall::Extreme(maximum), &[]) => {
            let start = super::Float64::constant(stage, MathCall::extreme_start(maximum));
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
    if matches!(call, MathCall::Imul) {
        // The missing argument is `undefined`, whose ToInt32 is 0: the product is 0, once the
        // given argument is a Number.
        let zero = stage.i32_constant(0);
        return Some(int_value(stage, zero));
    }
    // ToUint32 and ToInt32 share their low 32 bits, which `Math.clz32` reads.
    let operand = match math_operands(call) { Domain::Int32 => stage.to_int32(number, miss)?, _ => number };
    math_value(stage, call, &[operand])
}

/// The domain a Math row reads its operands in: int32 words for `Math.imul` and `Math.clz32`
/// (after ToInt32), binary64 words for the rest.
#[inline(always)]
pub const fn math_operands(call: MathCall) -> Domain {
    match call { MathCall::Imul | MathCall::LeadingZeros => Domain::Int32, _ => Domain::Float64 }
}

/// The Math row `call` of `operands`, words of its domain (`math_operands`): its result word and
/// the result's domain (an int32 for `Math.imul` and `Math.clz32`, else a binary64). The one
/// definition of each row's operation, which the value-word step (`math_call`) and the u-jit's
/// unboxed lowering both run. `None` for a call with no row, or another operand count.
#[inline(always)]
pub fn math_of_operands<S: NumericStage>(stage: &mut S, call: MathCall, operands: &[S::Word]) -> Option<(Domain, S::Word)> {
    Some(match (call, operands) {
        (MathCall::Imul, &[left, right]) => (Domain::Int32, super::math::multiply(stage, left, right)),
        (MathCall::LeadingZeros, &[only]) => (Domain::Int32, super::math::count_leading_zeros(stage, only)),
        (MathCall::Unary(operation), &[only]) => (Domain::Float64, super::math::unary(stage, operation, only)),
        (MathCall::Extreme(maximum), &[left, right]) => (Domain::Float64, super::math::extreme(stage, maximum, left, right)),
        (MathCall::Extreme(maximum), &[only]) => {
            let start = stage.f64_constant(MathCall::extreme_start(maximum));
            (Domain::Float64, super::math::extreme(stage, maximum, start, only))
        }
        _ => return None,
    })
}

/// `math_of_operands` as a value word: an int32 result tagged, a binary64 boxed.
#[inline(always)]
fn math_value<S: NumericStage>(stage: &mut S, call: MathCall, operands: &[S::Word]) -> Option<S::Word> {
    match math_of_operands(stage, call, operands)? {
        (Domain::Int32, word) => Some(int_value(stage, word)),
        (_, word) => Some(stage.box_number(super::Float64::of_row(word))),
    }
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
        Some(stage.box_number(super::Float64::of_row(result)))
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
        math_value(stage, MathCall::Extreme(maximum), &[x, y])
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
        math_value(stage, MathCall::Imul, &[x, y])
    })
}

/// A native whose value is a function of Number value words: a Math function (`math_call`) or
/// `Number.isInteger`. The interpreter (`concrete_math_call`, `builtins::concrete_is_integral`) and
/// the b-jit's builtin rows run it through the one step, `step_number_native_of`.
#[derive(Clone, Copy, Debug)]
pub enum NumberNative {
    /// A Math function of Numbers.
    Math(MathCall),
    /// `Number.isInteger` of its one argument.
    IsInteger,
    /// `Number.isSafeInteger` of its one argument.
    IsSafeInteger,
    /// `Number.isNaN` of its one argument.
    IsNaN,
    /// `Number.isFinite` of its one argument.
    IsFinite,
    /// SameValue of its two Number arguments (`Object.is`): NaN is itself, `+0` is not `-0`.
    SameValue,
}

/// The value word `native` answers for the value words `args`: the one step of a native of
/// Numbers, on any numeric stage. `None` (leaving at `miss`) for any case the native's own steps
/// decide (`math_call`; for `Number.isInteger`, an argument that is no Number).
#[inline(always)]
pub fn step_number_native_of<S: NumericStage>(stage: &mut S, native: NumberNative, args: &[S::Word], miss: S::Miss)
    -> Option<S::Word> {
    match native {
        NumberNative::Math(call) => math_call(stage, call, args, miss),
        NumberNative::SameValue => {
            let &[left, right] = args else { return None };
            let (a, b) = (number_word(stage, left, miss)?, number_word(stage, right, miss)?);
            let equal = stage.relation(NumberRelation::Equal, Domain::Float64, a, b);
            Some(stage.choose(equal, #[inline(always)] |stage| {
                // Equal zeros differ in sign exactly where their reciprocals are opposite infinities.
                let zero = stage.f64_constant(0.0);
                let zeros = stage.relation(NumberRelation::Equal, Domain::Float64, a, zero);
                stage.choose(zeros, #[inline(always)] |stage| {
                    let one = stage.f64_constant(1.0);
                    let (left, right) = (stage.float_arithmetic(NumberArithmetic::Divide, one, a), stage.float_arithmetic(NumberArithmetic::Divide, one, b));
                    let same = stage.relation(NumberRelation::Equal, Domain::Float64, left, right);
                    boolean_word(stage, same)
                }, #[inline(always)] |stage| stage.constant(Value::boolean(true).to_bits()))
            }, #[inline(always)] |stage| {
                let (ordered_a, ordered_b) = (stage.relation(NumberRelation::Equal, Domain::Float64, a, a), stage.relation(NumberRelation::Equal, Domain::Float64, b, b));
                let either = stage.any(ordered_a, ordered_b);
                let both_nan = stage.not(either);
                boolean_word(stage, both_nan)
            }))
        }
        NumberNative::IsInteger | NumberNative::IsSafeInteger | NumberNative::IsNaN | NumberNative::IsFinite => {
            let &[value] = args else { return None };
            let int = is_int(stage, value);
            stage.choose_leaving(int, #[inline(always)] |stage| {
                // Every int32 is a finite, safe integer and no NaN.
                let payload = stage.narrow(value);
                let integral = super::builtins::is_integral(stage, Domain::Int32, payload);
                let answer = if matches!(native, NumberNative::IsNaN) { stage.not(integral) } else { integral };
                Some(boolean_word(stage, answer))
            }, #[inline(always)] |stage| {
                let number = number_word(stage, value, miss)?;
                let answer = match native {
                    NumberNative::IsNaN => {
                        let ordered = stage.relation(NumberRelation::Equal, Domain::Float64, number, number);
                        stage.not(ordered)
                    }
                    NumberNative::IsFinite => {
                        // Only an infinity or a NaN differs from itself by something other than zero.
                        let spread = stage.float_arithmetic(NumberArithmetic::Subtract, number, number);
                        stage.relation(NumberRelation::Equal, Domain::Float64, spread, spread)
                    }
                    NumberNative::IsSafeInteger => {
                        let integral = super::builtins::is_integral(stage, Domain::Float64, number);
                        let magnitude = stage.float_unary(FloatUnary::Abs, number);
                        let limit = stage.f64_constant(MAX_SAFE_INTEGER);
                        let safe = stage.relation(NumberRelation::LessEqual, Domain::Float64, magnitude, limit);
                        stage.all(integral, safe)
                    }
                    _ => super::builtins::is_integral(stage, Domain::Float64, number),
                };
                Some(boolean_word(stage, answer))
            })
        }
    }
}

/// The largest integer a double holds exactly with all smaller ones, 2^53 - 1.
const MAX_SAFE_INTEGER: f64 = 9007199254740991.0;

/// The boolean value word of `cond`.
#[inline(always)]
fn boolean_word<S: Stage>(stage: &mut S, cond: S::Cond) -> S::Word {
    let (yes, no) = (stage.constant(Value::boolean(true).to_bits()), stage.constant(Value::boolean(false).to_bits()));
    stage.select(cond, yes, no)
}

/// `math_call` of `call` on the interpreter's values, and the bodies no stage runs: a libm function
/// of one Number, or a two-Number function, whose missing argument is `undefined`, a NaN.
/// `None` for any argument or count the row leaves at its miss.
#[inline(always)]
pub fn concrete_math_call(call: MathCall, args: &[Value]) -> Option<Value> {
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
    let stage = &mut Concrete::pure();
    match *args {
        [] => step_number_native_of(stage, NumberNative::Math(call), &[], ()),
        [only] => step_number_native_of(stage, NumberNative::Math(call), &[only.to_bits()], ()),
        [left, right] => step_number_native_of(stage, NumberNative::Math(call), &[left.to_bits(), right.to_bits()], ()),
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
        let pow = MathCall::Binary(crate::staged::power::power);
        assert_eq!(call(pow, &[Value::integer(2), Value::integer(3)]), Some(Value::integer(8)));
        assert!(call(pow, &[Value::integer(2)]).is_some_and(|value| value.as_number().is_some_and(f64::is_nan)));
        assert_eq!(call(pow, &[Value::UNDEFINED, Value::integer(1)]), None);
    }
}
