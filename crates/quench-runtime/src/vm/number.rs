use super::native_rows::Holder;
use super::property_key::PropertyKey;
use super::*;
use crate::staged::math::{FloatUnary, float_extreme};
use crate::staged::call_native::{MathCall, concrete_math_call};
use num_bigint::{BigInt, Sign};
use smallvec::SmallVec;

const MINIMUM_SUBNORMAL_BIT_PATTERN: u64 = 1;
const MINIMUM_POSITIVE_SUBNORMAL: f64 = f64::from_bits(MINIMUM_SUBNORMAL_BIT_PATTERN);
const NUMBER_RADIX_PREFIX_LENGTH: usize = 2;
use crate::float_layout::{BINARY64, power_of_two};
use crate::units::{BINARY_RADIX, HEXADECIMAL_RADIX, OCTAL_RADIX};
use super::number_parse::{
    INFINITY_LITERAL, decimal_value, radix_digit_run, radix_digits_value, scan_decimal, split_sign, trim_ascii_space,
    trim_leading_space, with_ascii_units,
};
use super::wtf16::Units;
const MAX_NUMBER_FORMAT_DIGITS: usize = 100;

/// StringToNumber of wide or ASCII `units` (the ToNumber of a string), read in place.
pub(super) fn string_to_number_units(units: Units<'_>) -> f64 {
    with_ascii_units(units, string_to_number_bytes)
}

/// StringToNumber of ASCII `bytes`: the text with its whitespace trimmed, as a
/// StringNumericLiteral. Empty text is 0; `0x`, `0o` and `0b` take no sign; Infinity takes one.
pub(super) fn string_to_number_bytes(bytes: &[u8]) -> f64 {
    let text = trim_ascii_space(bytes);
    if text.is_empty() {
        return 0.0;
    }
    if let Some(radix) = radix_prefix(text) {
        return radix_literal_value(&text[NUMBER_RADIX_PREFIX_LENGTH..], radix);
    }
    match text {
        b"Infinity" | b"+Infinity" => return f64::INFINITY,
        b"-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    match scan_decimal(text) {
        Some(decimal) if decimal.len == text.len() => decimal_value(&decimal),
        _ => f64::NAN,
    }
}

/// The radix a `0b`, `0o` or `0x` prefix (either case) selects, `None` for other text.
fn radix_prefix(text: &[u8]) -> Option<u32> {
    match text.get(..NUMBER_RADIX_PREFIX_LENGTH)? {
        b"0b" | b"0B" => Some(BINARY_RADIX),
        b"0o" | b"0O" => Some(OCTAL_RADIX),
        b"0x" | b"0X" => Some(HEXADECIMAL_RADIX),
        _ => None,
    }
}

/// The value of the digits after a radix prefix: NaN when there are none or one is invalid.
fn radix_literal_value(digits: &[u8], radix: u32) -> f64 {
    if digits.is_empty() || radix_digit_run(digits, radix) != digits.len() {
        f64::NAN
    } else {
        radix_digits_value(digits, radix)
    }
}

/// parseFloat of `text`: the longest StrDecimalLiteral prefix after its leading whitespace.
pub(super) fn parse_float(text: &str) -> f64 {
    let text = text.trim_start_matches(super::primitives::is_ecma_whitespace_character);
    parse_float_bytes(text.as_bytes())
}

/// parseFloat of wide or ASCII `units`, read in place.
pub(super) fn parse_float_units(units: Units<'_>) -> f64 {
    with_ascii_units(units, parse_float_bytes)
}

/// parseFloat of ASCII `bytes`: the longest StrDecimalLiteral prefix after leading whitespace,
/// or an Infinity literal, with its sign.
pub(super) fn parse_float_bytes(bytes: &[u8]) -> f64 {
    let text = trim_leading_space(bytes);
    let (negative, unsigned) = split_sign(text);
    if unsigned.starts_with(INFINITY_LITERAL) {
        return if negative { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    scan_decimal(text).map_or(f64::NAN, |decimal| decimal_value(&decimal))
}

/// The magnitude from which `Number.prototype.toFixed` writes exponential form (`number_to_decimal`).
pub(super) const FIXED_EXPONENTIAL_FROM: f64 = 1e21;
/// Inline capacity of a `toFixed` text: a sign, 21 integer digits, a point and 20 digits.
const FIXED_INLINE_LEN: usize = 64;
/// 10^0 through 10^20, each exact in a u128: the scales `fixed_rounded` multiplies by.
const POWERS_OF_TEN: [u128; 21] = {
    let mut powers = [1u128; 21];
    let mut index = 1;
    while index < powers.len() {
        powers[index] = powers[index - 1] * 10;
        index += 1;
    }
    powers
};

/// Calls `use_text` with the `toFixed(digits)` text of a finite `number` below 1e21, with `digits`
/// at most 100: a sign for a negative number (a negative that rounds to zero keeps it, as the spec's
/// step 6 gives it), the magnitude rounded half up at `digits` places, and its point. A zero is
/// written unsigned. A rounded magnitude that fits 64 bits with `digits` at most 20 is written into a
/// stack buffer (`write_fixed_u64`); any other text is built by `fixed_text_wide`.
pub(super) fn with_fixed_text<T>(number: f64, digits: usize, use_text: impl FnOnce(&[u8]) -> T) -> T {
    // A zero rounds to 0 at any `digits`, but its text is `digits + 1` places: only `digits` at most 20 fits the buffer.
    if let Some(rounded) = fixed_rounded(number, digits).filter(|_| digits <= 20).and_then(|value| u64::try_from(value).ok()) {
        let mut buffer = [0u8; FIXED_INLINE_LEN];
        let length = write_fixed_u64(number < 0.0, rounded, digits, &mut buffer);
        return use_text(&buffer[..length]);
    }
    use_text(&fixed_text_wide(number, digits))
}

/// Writes the `toFixed` text of `rounded`, the magnitude already scaled by 10^`digits` (`digits` at
/// most 20), into `out`: a sign when `negative`, the decimal digits zero-padded to `digits + 1`
/// places, and the point before the last `digits` of them. Returns the text's length.
fn write_fixed_u64(negative: bool, rounded: u64, digits: usize, out: &mut [u8; FIXED_INLINE_LEN]) -> usize {
    /// One more than the 20 digits of a u64, the most a padded digit run takes.
    const PADDED_DIGITS: usize = 21;
    let mut padded = [b'0'; PADDED_DIGITS];
    let mut itoa_buffer = itoa::Buffer::new();
    let integer = itoa_buffer.format(rounded).as_bytes();
    padded[PADDED_DIGITS - integer.len()..].copy_from_slice(integer);
    let width = integer.len().max(digits + 1);
    let digit_text = &padded[PADDED_DIGITS - width..];
    let mut length = 0;
    if negative {
        out[0] = b'-';
        length = 1;
    }
    if digits == 0 {
        out[length..length + width].copy_from_slice(digit_text);
        return length + width;
    }
    let head = width - digits;
    out[length..length + head].copy_from_slice(&digit_text[..head]);
    out[length + head] = b'.';
    out[length + head + 1..length + width + 1].copy_from_slice(&digit_text[head..]);
    length + width + 1
}

/// The `toFixed(digits)` text of `number` as `with_fixed_text` writes it, built in a SmallVec for
/// the rounded magnitudes that do not fit 64 bits or the digit counts `fixed_rounded` does not hold.
fn fixed_text_wide(number: f64, digits: usize) -> SmallVec<[u8; FIXED_INLINE_LEN]> {
    let mut buffer = itoa::Buffer::new();
    let exact;
    let integer: &[u8] = match fixed_rounded(number, digits) {
        Some(rounded) => buffer.format(rounded).as_bytes(),
        None => {
            exact = fixed_rounded_exact(number, digits).to_string();
            exact.as_bytes()
        }
    };
    let mut text: SmallVec<[u8; FIXED_INLINE_LEN]> = SmallVec::new();
    if number < 0.0 {
        text.push(b'-');
    }
    if digits == 0 {
        text.extend_from_slice(integer);
        return text;
    }
    // At least one integer digit: leading zeros pad a short text before the point.
    let start = text.len();
    let width = integer.len().max(digits + 1);
    text.resize(start + width - integer.len(), b'0');
    text.extend_from_slice(integer);
    text.insert(start + width - digits, b'.');
    text
}

/// `round(|number| * 10^digits)` as an integer, a tie taking the larger, from the exact significand
/// and binary exponent in 128 bits. `None` when the product or its shift does not fit, which
/// `fixed_rounded_exact` then computes.
fn fixed_rounded(number: f64, digits: usize) -> Option<u128> {
    /// Right shifts at or past this bit count are left to the exact path.
    const MAX_RIGHT_SHIFT: u32 = 127;
    let (significand, exponent) = crate::float_layout::binary64_parts(number);
    if significand == 0 {
        return Some(0);
    }
    let scaled = u128::from(significand).checked_mul(*POWERS_OF_TEN.get(digits)?)?;
    if exponent >= 0 {
        let shift = exponent as u32;
        return (shift < scaled.leading_zeros()).then(|| scaled << shift);
    }
    let shift = exponent.unsigned_abs();
    if shift >= MAX_RIGHT_SHIFT {
        return None;
    }
    Some((scaled + (1u128 << (shift - 1))) >> shift)
}

/// `round(|number| * 10^digits)` in exact big-integer arithmetic, for the values and digit counts
/// `fixed_rounded` does not hold.
fn fixed_rounded_exact(number: f64, digits: usize) -> BigInt {
    let (significand, exponent) = crate::float_layout::binary64_parts(number);
    let scaled = BigInt::from(significand) * BigInt::from(10_u8).pow(digits as u32);
    if exponent >= 0 {
        return scaled << exponent as usize;
    }
    let shift = (-exponent) as usize;
    let quotient = &scaled >> shift;
    let remainder = &scaled - (&quotient << shift);
    if (&remainder << 1_usize) >= (BigInt::from(1_u8) << shift) {
        quotient + 1_u8
    } else {
        quotient
    }
}

impl Vm {
    /// Installs the `Number` constructor, its prototype and their constants in the main realm.
    pub(super) fn install_number(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let number = self.native_value(Native::Number);
        self.set_builtin_function_name(number, "Number")?;
        let prototype = self.object();
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((self.realm.globals, Native::Number), prototype);
        self.number_proto = prototype;
        self.set_builtin_value_named(number, "prototype", prototype)?;
        let prototype_key = self.prototype_atom();
        self.set_property_attributes(
            number,
            PropertyKey::string(prototype_key),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.object_data_mut(prototype)
            .expect("primitive prototype")
            .set_boxed_primitive(Value::number(0.0));
        self.set_builtin_value_named(prototype, "constructor", number)?;
        self.install_native_rows(program, prototype, Holder::NumberPrototype)?;
        self.install_native_rows(program, number, Holder::NumberConstructor)?;
        for (name, value) in [
            ("EPSILON", f64::EPSILON),
            ("MAX_SAFE_INTEGER", MAX_SAFE_INTEGER),
            ("MIN_SAFE_INTEGER", -MAX_SAFE_INTEGER),
            ("MAX_VALUE", f64::MAX),
            ("MIN_VALUE", MINIMUM_POSITIVE_SUBNORMAL),
            ("NaN", f64::NAN),
            ("POSITIVE_INFINITY", f64::INFINITY),
            ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ] {
            self.set_named_constant(program, number, name, Value::number(value))?;
        }
        self.global(program, "Number", number)?;
        Ok(())
    }

    pub(super) fn install_number_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::Number, global, global);
        self.set_builtin_function_name(constructor, "Number")?;
        let prototype = self
            .heap
            .alloc_object(Self::empty_object(object_prototype));
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Number), prototype);
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_key = self.prototype_atom();
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype_key),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.object_data_mut(prototype)
            .expect("primitive prototype")
            .set_boxed_primitive(Value::number(0.0));
        self.install_realm_native_rows(prototype, Holder::NumberPrototype, global)?;
        self.install_realm_native_rows(constructor, Holder::NumberConstructor, global)?;
        for (name, value) in [
            ("EPSILON", f64::EPSILON),
            ("MAX_SAFE_INTEGER", MAX_SAFE_INTEGER),
            ("MIN_SAFE_INTEGER", -MAX_SAFE_INTEGER),
            ("MAX_VALUE", f64::MAX),
            ("MIN_VALUE", MINIMUM_POSITIVE_SUBNORMAL),
            ("NaN", f64::NAN),
            ("POSITIVE_INFINITY", f64::INFINITY),
            ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ] {
            self.set_named_constant(program, constructor, name, Value::number(value))?;
        }
        self.set_builtin_value_named(global, "Number", constructor)?;
        Ok(())
    }

    pub(super) fn number_receiver_value(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<f64, JsError> {
        if let Some(number) = receiver.as_number() {
            return Ok(number);
        }
        self.boxed_primitive_value(receiver)
            .and_then(Value::as_number)
            .ok_or_else(|| {
                self.type_error(p, "Number method called on incompatible receiver".into())
            })
    }

    pub(super) fn number_format(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
        kind: Native,
    ) -> Result<Value, JsError> {
        let number = self.number_receiver_value(p, receiver)?;
        let number = if number == 0.0 { 0.0 } else { number };
        let digits_value = args.first().copied().filter(|value| !value.is_undefined());
        if !number.is_finite() {
            if let Some(value) = digits_value {
                if kind == Native::NumberFixed {
                    let _ = self.number_format_digits(p, value)?;
                } else {
                    let _ = self.to_number(p, value)?;
                }
            }
            return Ok(self
                .heap
                .alloc_string(number_to_decimal(number).into()));
        }
        let digits = digits_value
            .map(|value| self.number_format_digits(p, value))
            .transpose()?;
        let text = match kind {
            Native::NumberFixed if number.abs() < FIXED_EXPONENTIAL_FROM => {
                return Ok(with_fixed_text(number, digits.unwrap_or(0), |text| self.heap.alloc_ascii_run(text)));
            }
            Native::NumberFixed => number_to_decimal(number),
            Native::NumberPrecision => match digits {
                None => number_to_decimal(number),
                Some(0) => return Err(self.range_error(p, "Invalid precision".into())),
                Some(digits) => {
                    let magnitude = if number == 0.0 {
                        0
                    } else {
                        number.abs().log10().floor() as i32
                    };
                    if magnitude >= digits as i32 || magnitude < -6 {
                        format_scientific(number, digits - 1)
                    } else {
                        format!(
                            "{number:.precision$}",
                            precision = (digits as i32 - magnitude - 1) as usize
                        )
                    }
                }
            },
            _ => unreachable!(),
        };
        Ok(self.heap.alloc_string(text.into()))
    }

    fn number_format_digits(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<usize, JsError> {
        let value = self.to_number(p, value)?;
        if value.is_nan() {
            return Ok(0);
        }
        if !value.is_finite() || !(0.0..=MAX_NUMBER_FORMAT_DIGITS as f64).contains(&value.trunc()) {
            return Err(self.range_error(p, "Invalid precision".into()));
        }
        Ok(value.trunc() as usize)
    }

    pub(super) fn number_exponential(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let number = self.number_receiver_value(p, receiver)?;
        let number = if number == 0.0 { 0.0 } else { number };
        let digits = match args.first().copied() {
            None | Some(Value::UNDEFINED) => None,
            Some(value) => {
                let value = self.to_number(p, value)?;
                let digits = if value.is_nan() || value == 0.0 || !number.is_finite() {
                    0
                } else if !value.is_finite() || value.trunc() < 0.0 {
                    return Err(self.range_error(p, "toExponential() argument out of range".into()));
                } else {
                    value.trunc() as usize
                };
                if number.is_finite() && digits > MAX_NUMBER_FORMAT_DIGITS {
                    return Err(self.range_error(p, "toExponential() argument out of range".into()));
                }
                Some(digits)
            }
        };
        let text = if number.is_nan() {
            "NaN".to_owned()
        } else if number.is_infinite() {
            if number.is_sign_negative() {
                "-Infinity".to_owned()
            } else {
                "Infinity".to_owned()
            }
        } else {
            match digits {
                Some(digits) => format_scientific_exponential(number, digits),
                None => normalize_exponent_sign(&format!("{number:e}")),
            }
        };
        Ok(self.heap.alloc_string(text.into()))
    }

    pub(super) fn call_number_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        Ok(match native {
            Native::Number => {
                let value = args.first().copied().unwrap_or_else(|| Value::number(0.0));
                let primitive = if self.is_object_like(value) {
                    self.to_primitive(p, value, "number")?
                } else {
                    value
                };
                let number = match self.heap.get(primitive) {
                    Some(Cell::BigInt(value)) => value.to_f64(),
                    _ => self.to_number(p, primitive)?,
                };
                Value::number(number)
            }
            Native::NumberIsNaN
            | Native::NumberIsFinite
            | Native::NumberIsInteger
            | Native::NumberIsSafeInteger => {
                Value::boolean(number_predicate(native, value).unwrap_or_default())
            }
            _ => return Err(JsError("invalid Number native".into())),
        })
    }
}

/// What the global `isNaN` or `isFinite` returns for the Number its argument converted to;
/// `None` for any other native.
pub fn global_number_predicate(native: Native, number: f64) -> Option<bool> {
    Some(match native {
        Native::GlobalIsNaN => number.is_nan(),
        Native::GlobalIsFinite => number.is_finite(),
        _ => return None,
    })
}

/// What `Number.isNaN`, `isFinite`, `isInteger` or `isSafeInteger` returns for `value`: none
/// coerces its argument, so a non-Number answers `false`. `None` for any other native.
pub fn number_predicate(native: Native, value: Value) -> Option<bool> {
    let number = value.as_number();
    let integral = || crate::staged::builtins::concrete_is_integral(value);
    Some(match native {
        Native::NumberIsNaN => number.is_some_and(f64::is_nan),
        Native::NumberIsFinite => number.is_some_and(f64::is_finite),
        Native::NumberIsInteger => integral(),
        Native::NumberIsSafeInteger => integral() && number.is_some_and(|number| number.abs() <= MAX_SAFE_INTEGER),
        _ => return None,
    })
}

/// The coefficient text and exponent of a `{:e}` formatted number.
fn scientific_parts(value: &str) -> (&str, i32) {
    let (coefficient, exponent) = value.split_once('e').unwrap_or((value, "0"));
    (coefficient, exponent.parse::<i32>().unwrap_or_default())
}

fn format_scientific(number: f64, digits: usize) -> String {
    let value = format!("{number:.digits$e}");
    let (coefficient, exponent) = scientific_parts(&value);
    format!("{coefficient}e{exponent:+}")
}

pub(super) fn number_to_decimal(number: f64) -> String {
    crate::number_to_string::format(number)
}

fn format_scientific_exponential(number: f64, digits: usize) -> String {
    let value = format!("{number:.digits$e}");
    let (coefficient, exponent) = scientific_parts(&value);
    let scaled = number.abs() / 10_f64.powi(exponent - digits as i32);
    if scaled.fract() == 0.5 {
        let mut rounded = scaled.floor() + 1.0;
        let mut exponent = exponent;
        if rounded == 10_f64.powi(digits as i32 + 1) {
            rounded /= 10.0;
            exponent += 1;
        }
        let coefficient = rounded / 10_f64.powi(digits as i32);
        let sign = if number.is_sign_negative() { "-" } else { "" };
        return format!("{sign}{coefficient:.digits$}e{exponent:+}");
    }
    format!("{coefficient}e{exponent:+}")
}

fn normalize_exponent_sign(text: &str) -> String {
    let Some((mantissa, exponent)) = text.split_once('e') else {
        return text.to_owned();
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return text.to_owned();
    };
    format!("{mantissa}e{exponent:+}")
}

/// Arguments a Math call of Numbers keeps inline (`math_numbers`); longer calls allocate.
pub const MATH_INLINE_ARGUMENTS: usize = 4;

/// The Math function `native` computes on Numbers, the one table every tier reads: each tier derives
/// its dispatch from `MathCall`, and each body is written once, in `staged::math` or in the libm it
/// names. `None` for any other native.
pub const fn math_call(native: Native) -> Option<MathCall> {
    use crate::staged::math::{atan, atanh, cbrt, f16_round};
    use MathCall::{Binary, Extreme, Imul, LeadingZeros, Libm, Unary};
    Some(match native {
        Native::MathFloor => Unary(FloatUnary::Floor),
        Native::MathAbs => Unary(FloatUnary::Abs),
        Native::MathCeil => Unary(FloatUnary::Ceil),
        Native::MathRound => Unary(FloatUnary::Round),
        Native::MathTrunc => Unary(FloatUnary::Trunc),
        Native::MathSqrt => Unary(FloatUnary::Sqrt),
        Native::MathFround => Unary(FloatUnary::Fround),
        Native::MathSign => Unary(FloatUnary::Sign),
        Native::MathClz32 => LeadingZeros,
        Native::MathMax => Extreme(true),
        Native::MathMin => Extreme(false),
        Native::MathImul => Imul,
        Native::MathPow => Binary(crate::staged::math::power),
        Native::MathAtan2 => Binary(crate::staged::math::atan2),
        Native::MathF16Round => Libm(f16_round),
        Native::MathAtanh => Libm(atanh),
        Native::MathAtan => Libm(atan),
        Native::MathCbrt => Libm(cbrt),
        Native::MathAcos => Libm(f64::acos),
        Native::MathAsin => Libm(f64::asin),
        Native::MathCos => Libm(f64::cos),
        Native::MathExp => Libm(f64::exp),
        Native::MathSin => Libm(f64::sin),
        Native::MathTan => Libm(f64::tan),
        Native::MathAcosh => Libm(f64::acosh),
        Native::MathAsinh => Libm(f64::asinh),
        Native::MathCosh => Libm(f64::cosh),
        Native::MathExpm1 => Libm(f64::exp_m1),
        Native::MathLog10 => Libm(f64::log10),
        Native::MathLog1p => Libm(f64::ln_1p),
        Native::MathLog => Libm(f64::ln),
        Native::MathLog2 => Libm(f64::log2),
        Native::MathSinh => Libm(f64::sinh),
        Native::MathTanh => Libm(f64::tanh),
        _ => return None,
    })
}

/// The value of a call of the Math function `native` (`math_call`) with Number arguments, as
/// `staged::call_native::math_call` computes it, int32 fast paths included. `None` where an argument
/// is no Number (the native converts it, and may run guest code) or the count is one the function
/// does not take.
#[inline(always)]
pub fn math_inline_call(native: Native, args: &[Value]) -> Option<Value> {
    concrete_math_call(math_call(native)?, args)
}

/// The value of a Math function of Number values that reads two (`Math.pow`, `Math.atan2`,
/// `Math.imul`, a missing one NaN or `undefined`, as its conversion makes it), or of any count
/// for `Math.max` and `Math.min`; `None` for a function of one Number. Each runs no guest code.
#[inline(always)]
pub fn math_numbers(native: Native, values: &[f64]) -> Option<f64> {
    match math_call(native)? {
        MathCall::Extreme(maximum) => Some(values.iter().fold(MathCall::extreme_start(maximum), |result, &number| {
            float_extreme(maximum, result, number)
        })),
        call @ (MathCall::Binary(_) | MathCall::Imul) => {
            let args: SmallVec<[Value; 2]> = values.iter().take(2).map(|&number| Value::number(number)).collect();
            concrete_math_call(call, &args)?.as_number()
        }
        MathCall::Unary(_) | MathCall::LeadingZeros | MathCall::Libm(_) => None,
    }
}

/// Whether a call of `native` with Number arguments is a Math function's value that
/// `math_inline_call` computes (`math_call`).
pub fn math_numbers_inline(native: Native) -> bool {
    math_call(native).is_some()
}

/// The value of a Math function of one Number argument (`Math.sqrt` and its kin); `None`
/// for any other native. Such a function reads only its first argument.
pub(super) fn math_unary_call(native: Native, args: &[Value]) -> Option<Value> {
    math_unary(native, args.first()?.as_number()?).map(Value::number)
}

/// The value of a Math function of one Number (`math_call`, `MathCall::reads_one_number`), as
/// `staged::call_native::math_call` computes it. `None` for any other function.
#[inline(always)]
pub(super) fn math_unary(native: Native, value: f64) -> Option<f64> {
    let call = math_call(native).filter(|call| call.reads_one_number())?;
    concrete_math_call(call, &[Value::number(value)])?.as_number()
}

/// Whether `native` is a Math function of one Number, which `math_unary` computes.
pub(super) fn is_math_unary(native: Native) -> bool {
    math_call(native).is_some_and(MathCall::reads_one_number)
}

impl Vm {
    pub(super) fn math_hypot(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let numbers = args
            .iter()
            .copied()
            .map(|value| self.to_number(p, value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Value::number(crate::staged::math::hypot(&numbers)))
    }

    pub(super) fn math_sum_precise(
        &mut self,
        p: &ResidualProgram,
        iterable: Option<Value>,
    ) -> Result<Value, JsError> {
        let iterable = iterable.unwrap_or(Value::UNDEFINED);
        let iterator = self.get_iterator(p, iterable)?;
        let mut exact_sum = BigInt::from(0_u8);
        let mut state = PreciseSumState::default();
        loop {
            let step = match self.iterator_next(p, iterator) {
                Ok(step) => step,
                Err(error) => {
                    let _ = self.iterator_close(p, iterator);
                    return Err(error);
                }
            };
            let item = match self.iterator_result_value(p, step) {
                Ok(item) => item,
                Err(error) => {
                    let _ = self.iterator_close(p, iterator);
                    return Err(error);
                }
            };
            let Some(value) = item else {
                return Ok(Value::number(state.finish(exact_sum)));
            };
            let Some(number) = value.as_number() else {
                let error = self.type_error(p, "Math.sumPrecise requires Number values".into());
                let _ = self.iterator_close(p, iterator);
                return Err(error);
            };
            state.add(number, &mut exact_sum);
        }
    }
}

#[derive(Default)]
struct PreciseSumState {
    infinity: Option<bool>,
    invalid: bool,
    positive_zero: bool,
    nonzero: bool,
}

impl PreciseSumState {
    fn add(&mut self, value: f64, sum: &mut BigInt) {
        if value.is_nan() {
            self.invalid = true;
        } else if value.is_infinite() {
            self.invalid |= self
                .infinity
                .is_some_and(|sign| sign != value.is_sign_positive());
            self.infinity.get_or_insert_with(|| value.is_sign_positive());
        } else if value == 0.0 {
            self.positive_zero |= value.is_sign_positive();
        } else {
            self.nonzero = true;
            let bits = value.to_bits();
            let fraction = bits & ((1_u64 << 52) - 1);
            let exponent = ((bits >> 52) & 0x7ff) as usize;
            let significand = if exponent == 0 {
                fraction
            } else {
                fraction | (1_u64 << 52)
            };
            let units = BigInt::from(significand) << exponent.saturating_sub(1);
            *sum += if value.is_sign_negative() {
                -units
            } else {
                units
            };
        }
    }

    fn finish(self, sum: BigInt) -> f64 {
        if self.invalid {
            f64::NAN
        } else if let Some(positive) = self.infinity {
            if positive {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            }
        } else if sum == BigInt::from(0_u8) {
            if !self.nonzero && !self.positive_zero {
                -0.0
            } else {
                0.0
            }
        } else {
            scaled_binary_sum(sum)
        }
    }
}

fn scaled_binary_sum(sum: BigInt) -> f64 {
    let negative = sum.sign() == Sign::Minus;
    let magnitude = if negative { -sum } else { sum };
    let bits = magnitude.magnitude().bits() as usize;
    let least = BINARY64.subnormal_exponent();
    let rounded = if bits < f64::MANTISSA_DIGITS as usize {
        magnitude.to_string().parse::<f64>().unwrap_or(f64::NAN) * power_of_two(least)
    } else {
        let shift = bits - f64::MANTISSA_DIGITS as usize;
        let mut significand = &magnitude >> shift;
        let remainder = &magnitude - (&significand << shift);
        let halfway = BigInt::from(1_u8) << (shift - 1);
        if remainder > halfway
            || remainder == halfway && (&significand & BigInt::from(1_u8)) != BigInt::from(0_u8)
        {
            significand += 1_u8;
        }
        significand.to_string().parse::<f64>().unwrap_or(f64::NAN) * power_of_two(shift as i32 + least)
    };
    if negative { -rounded } else { rounded }
}
