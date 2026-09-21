use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_number(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let number = self.native_value(Native::Number);
        for (name, native) in [
            ("isNaN", Native::NumberIsNaN),
            ("isFinite", Native::NumberIsFinite),
            ("isInteger", Native::NumberIsInteger),
            ("isSafeInteger", Native::NumberIsSafeInteger),
            ("parseInt", Native::ParseInt),
            ("parseFloat", Native::NumberParseFloat),
        ] {
            self.set_named(program, number, name, self.native_value(native))?;
        }
        for (name, value) in [
            ("EPSILON", f64::EPSILON),
            ("MAX_SAFE_INTEGER", 9_007_199_254_740_991.0),
            ("MIN_SAFE_INTEGER", -9_007_199_254_740_991.0),
            ("MAX_VALUE", f64::MAX),
            ("MIN_VALUE", f64::MIN_POSITIVE),
            ("NaN", f64::NAN),
            ("POSITIVE_INFINITY", f64::INFINITY),
            ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ] {
            self.set_named(program, number, name, Value::number(value))?;
        }
        self.global(program, "Number", number)
    }

    pub(super) fn call_number_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
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
                    value.is_finite()
                        && value.fract() == 0.0
                        && value.abs() <= 9_007_199_254_740_991.0
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
