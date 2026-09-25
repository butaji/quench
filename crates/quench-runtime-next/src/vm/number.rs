use super::*;

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
            Native::Number => Value::number(self.to_number(p, value)?),
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
                (value + 0.5).floor()
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
