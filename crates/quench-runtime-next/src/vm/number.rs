use super::*;
use num_bigint::{BigInt, Sign};

const MINIMUM_SUBNORMAL_BIT_PATTERN: u64 = 1;
const MINIMUM_POSITIVE_SUBNORMAL: f64 = f64::from_bits(MINIMUM_SUBNORMAL_BIT_PATTERN);
const NUMBER_RADIX_PREFIX_LENGTH: usize = 2;
const BINARY_RADIX: u32 = 2;
const OCTAL_RADIX: u32 = 8;
const HEXADECIMAL_RADIX: u32 = 16;
const MAX_NUMBER_FORMAT_DIGITS: usize = 100;

pub(super) fn parse_number_string(text: &str) -> f64 {
    let text =
        text.trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}');
    if text.is_empty() {
        return 0.0;
    }
    if matches!(text, "INFINITY" | "infinity" | "+infinity" | "-infinity") {
        return f64::NAN;
    }
    let Some((prefix, digits)) = text
        .get(..NUMBER_RADIX_PREFIX_LENGTH)
        .map(|prefix| (prefix, &text[NUMBER_RADIX_PREFIX_LENGTH..]))
    else {
        return text.parse().unwrap_or(f64::NAN);
    };
    let radix = match prefix {
        "0b" | "0B" => Some(BINARY_RADIX),
        "0o" | "0O" => Some(OCTAL_RADIX),
        "0x" | "0X" => Some(HEXADECIMAL_RADIX),
        _ => None,
    };
    radix.map_or_else(
        || text.parse().unwrap_or(f64::NAN),
        |radix| i64::from_str_radix(digits, radix).map_or(f64::NAN, |value| value as f64),
    )
}

impl<H: Host> Vm<H> {
    pub(super) fn install_number(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let number = self.native_value(Native::Number);
        let prototype = self.object();
        self.set_builtin_value_named(number, "prototype", prototype)?;
        self.set_builtin_named(program, prototype, "constructor", Native::Number)?;
        self.set_builtin_named(program, prototype, "toString", Native::NumberString)?;
        self.set_builtin_named(program, prototype, "valueOf", Native::NumberValueOf)?;
        self.set_builtin_named(program, prototype, "toFixed", Native::NumberFixed)?;
        self.set_builtin_named(
            program,
            prototype,
            "toExponential",
            Native::NumberExponential,
        )?;
        self.set_builtin_named(program, prototype, "toPrecision", Native::NumberPrecision)?;
        for (name, native) in [
            ("isNaN", Native::NumberIsNaN),
            ("isFinite", Native::NumberIsFinite),
            ("isInteger", Native::NumberIsInteger),
            ("isSafeInteger", Native::NumberIsSafeInteger),
            ("parseInt", Native::ParseInt),
            ("parseFloat", Native::NumberParseFloat),
        ] {
            self.set_builtin_value_named(number, name, self.native_value(native))?;
        }
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
        self.global(program, "Number", number)
    }

    pub(super) fn number_exponential(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let number = self.to_number(p, receiver)?;
        let digits = match args.first().copied() {
            None | Some(Value::UNDEFINED) => None,
            Some(value) => {
                let value = self.to_number(p, value)?;
                let digits = if value.is_nan() || value == 0.0 {
                    0
                } else if !value.is_finite() || value.trunc() < 0.0 {
                    return Err(self.range_error(p, "toExponential() argument out of range".into()));
                } else {
                    value.trunc() as usize
                };
                if digits > MAX_NUMBER_FORMAT_DIGITS {
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
            let scientific = match digits {
                Some(digits) => format!("{number:.digits$e}"),
                None => format!("{number:e}"),
            };
            normalize_exponent_sign(&scientific)
        };
        Ok(self.heap.alloc(Cell::String(text.into())))
    }

    pub(super) fn call_number_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::number(0.0));
        Ok(match native {
            Native::Number => {
                let primitive = if self.is_object_like(value) {
                    self.to_primitive(p, value, "number")?
                } else {
                    value
                };
                let number = match self.heap.get(primitive) {
                    Some(Cell::BigInt(value)) => value.parse::<f64>().map_err(|_| {
                        self.type_error(p, "invalid BigInt numeric representation".into())
                    })?,
                    _ => self.to_number(p, primitive)?,
                };
                Value::number(number)
            }
            Native::NumberIsNaN => {
                if value.as_number().is_some_and(f64::is_nan) {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::NumberIsFinite => {
                if value.as_number().is_some_and(f64::is_finite) {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::NumberIsInteger => {
                if value
                    .as_number()
                    .is_some_and(|value| value.is_finite() && value.fract() == 0.0)
                {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::NumberIsSafeInteger => {
                if value.as_number().is_some_and(|value| {
                    value.is_finite() && value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER
                }) {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            _ => return Err(JsError("invalid Number native".into())),
        })
    }
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

pub(super) fn parse_float(text: &str) -> f64 {
    let text = text.trim_start();
    if text.starts_with('+') || text.starts_with('-') {
        if text[1..].starts_with("Infinity") {
            return if text.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
        }
    } else if text.starts_with("Infinity") {
        return f64::INFINITY;
    }
    let bytes = text.as_bytes();
    let mut end = usize::from(
        bytes
            .first()
            .is_some_and(|byte| *byte == b'+' || *byte == b'-'),
    );
    let mut digits = 0;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
        digits += 1;
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if bytes
        .get(end)
        .is_some_and(|byte| *byte == b'e' || *byte == b'E')
    {
        let exponent_start = end;
        end += 1;
        if bytes
            .get(end)
            .is_some_and(|byte| *byte == b'+' || *byte == b'-')
        {
            end += 1;
        }
        let exponent_digits = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if exponent_digits == end {
            end = exponent_start;
        }
    }
    text[..end].parse().unwrap_or(f64::NAN)
}

pub(super) fn math_unary(native: Native, value: f64) -> f64 {
    match native {
        Native::MathAbs => value.abs(),
        Native::MathCeil => value.ceil(),
        Native::MathRound => {
            if value.is_nan() || value == 0.0 || value.is_infinite() {
                value
            } else if (-0.5..0.0).contains(&value) {
                -0.0
            } else {
                let floor = value.floor();
                if value - floor < 0.5 {
                    floor
                } else {
                    floor + 1.0
                }
            }
        }
        Native::MathTrunc => value.trunc(),
        Native::MathSqrt => value.sqrt(),
        Native::MathAcos => value.acos(),
        Native::MathAsin => value.asin(),
        Native::MathAtan => value.atan(),
        Native::MathCos => value.cos(),
        Native::MathExp => value.exp(),
        Native::MathSin => value.sin(),
        Native::MathTan => value.tan(),
        Native::MathAcosh => value.acosh(),
        Native::MathAsinh => value.asinh(),
        Native::MathAtanh => value.atanh(),
        Native::MathCbrt => value.cbrt(),
        Native::MathCosh => value.cosh(),
        Native::MathExpm1 => value.exp_m1(),
        Native::MathFround => f64::from(value as f32),
        Native::MathLog10 => value.log10(),
        Native::MathLog1p => value.ln_1p(),
        Native::MathLog2 => value.log2(),
        Native::MathSinh => value.sinh(),
        Native::MathTanh => value.tanh(),
        Native::MathClz32 => f64::from(crate::value::number_to_u32(value).leading_zeros()),
        Native::MathF16Round => f16_round(value),
        Native::MathSign => {
            if value.is_nan() || value == 0.0 {
                value
            } else if value.is_sign_negative() {
                -1.0
            } else {
                1.0
            }
        }
        _ => unreachable!("non-unary Math native"),
    }
}

impl<H: Host> Vm<H> {
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
        if numbers.iter().any(|value| value.is_infinite()) {
            return Ok(Value::number(f64::INFINITY));
        }
        if numbers.iter().any(|value| value.is_nan()) {
            return Ok(Value::number(f64::NAN));
        }
        let largest = numbers.iter().copied().map(f64::abs).fold(0.0, f64::max);
        if largest == 0.0 {
            return Ok(Value::number(0.0));
        }
        let squares = numbers
            .iter()
            .map(|value| (value / largest).powi(2))
            .sum::<f64>();
        Ok(Value::number(largest * squares.sqrt()))
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
            let done_atom = self.intern_atom("done");
            let done = match self.get_property(p, step, done_atom) {
                Ok(done) => self.truthy(done),
                Err(error) => {
                    let _ = self.iterator_close(p, iterator);
                    return Err(error);
                }
            };
            if done {
                return Ok(Value::number(state.finish(exact_sum)));
            }
            let value_atom = self.intern_atom("value");
            let value = match self.get_property(p, step, value_atom) {
                Ok(value) => value,
                Err(error) => {
                    let _ = self.iterator_close(p, iterator);
                    return Err(error);
                }
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
            self.infinity.get_or_insert(value.is_sign_positive());
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
    let rounded = if bits <= 52 {
        magnitude.to_string().parse::<f64>().unwrap_or(f64::NAN) * 2_f64.powi(-1074)
    } else {
        let shift = bits - 53;
        let mut significand = &magnitude >> shift;
        let remainder = &magnitude - (&significand << shift);
        let halfway = BigInt::from(1_u8) << (shift - 1);
        if remainder > halfway
            || remainder == halfway && (&significand & BigInt::from(1_u8)) != BigInt::from(0_u8)
        {
            significand += 1_u8;
        }
        significand.to_string().parse::<f64>().unwrap_or(f64::NAN) * 2_f64.powi(shift as i32 - 1074)
    };
    if negative { -rounded } else { rounded }
}

fn f16_round(value: f64) -> f64 {
    half_to_f64(f64_to_half(value))
}

fn f64_to_half(value: f64) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    let exponent = ((bits >> 52) & 0x7ff) as u16;
    let fraction = bits & 0x000f_ffff_ffff_ffff;
    if exponent == 0x7ff {
        return sign
            | if fraction == 0 {
                0x7c00
            } else {
                0x7c00 | ((fraction >> 42) as u16).max(1)
            };
    }
    let absolute = f64::from_bits(bits & 0x7fff_ffff_ffff_ffff);
    if absolute < 2f64.powi(-14) {
        let rounded = round_half(absolute * 2f64.powi(24));
        return sign | if rounded >= 0x0400 { 0x0400 } else { rounded };
    }
    let unbiased = exponent as i32 - 1023;
    if unbiased > 15 {
        return sign | 0x7c00;
    }
    let mut significand = (fraction >> 42) as u16;
    let remainder = fraction & ((1_u64 << 42) - 1);
    if remainder > (1_u64 << 41) || (remainder == (1_u64 << 41) && significand & 1 != 0) {
        significand += 1;
    }
    let half_exponent = (unbiased + 15) as u16;
    if significand == 0x0400 {
        let half_exponent = half_exponent + 1;
        if half_exponent >= 0x1f {
            return sign | 0x7c00;
        }
        return sign | (half_exponent << 10);
    }
    sign | (half_exponent << 10) | significand
}

fn round_half(value: f64) -> u16 {
    let lower = value.floor() as u64;
    let fraction = value - lower as f64;
    (lower + u64::from(fraction > 0.5 || (fraction == 0.5 && lower & 1 != 0))) as u16
}

fn half_to_f64(bits: u16) -> f64 {
    let sign_bits = (u64::from(bits & 0x8000)) << 48;
    let exponent = (bits >> 10) & 0x1f;
    let fraction = bits & 0x03ff;
    match (exponent, fraction) {
        (0, 0) => f64::from_bits(sign_bits),
        (0, fraction) => f64::from(fraction) * 2_f64.powi(-24) * sign_factor(sign_bits),
        (0x1f, 0) => f64::from_bits(sign_bits | 0x7ff0_0000_0000_0000),
        (0x1f, fraction) => {
            f64::from_bits(sign_bits | 0x7ff0_0000_0000_0000 | (u64::from(fraction) << 42))
        }
        (exponent, fraction) => {
            let value = (1.0 + f64::from(fraction) / 1024.0) * 2_f64.powi(i32::from(exponent) - 15);
            value * sign_factor(sign_bits)
        }
    }
}

fn sign_factor(sign: u64) -> f64 {
    if sign == 0 { 1.0 } else { -1.0 }
}
