use super::*;

fn is_ecma_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x0009..=0x000D
            | 0x0020
            | 0x00A0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028..=0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

pub(super) fn is_ecma_whitespace_character(character: char) -> bool {
    u16::try_from(u32::from(character)).is_ok_and(is_ecma_whitespace)
}

impl<H: Host> Vm<H> {
    pub(super) fn call_target(&self, callee: Value) -> Result<CallTarget, JsError> {
        match self.heap.get(callee) {
            Some(Cell::Function {
                kind: FunctionKind::User(program, id),
                env,
                ..
            }) => Ok(CallTarget::User(*program, *id, *env)),
            Some(Cell::Function {
                kind: FunctionKind::NumericUser(program, id),
                env,
                ..
            }) => Ok(CallTarget::NumericUser(*program, *id, *env)),
            Some(Cell::Function {
                kind: FunctionKind::Native(native),
                ..
            }) => Ok(CallTarget::Native(*native)),
            other => self.non_callable_target(callee, other),
        }
    }

    pub(super) fn non_callable_target(
        &self,
        _callee: Value,
        _cell: Option<&Cell>,
    ) -> Result<CallTarget, JsError> {
        Err(JsError("value is not callable".into()))
    }

    pub(super) fn call_primitive_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::StringRaw => self.string_raw(p, args),
            Native::StringIsWellFormed => {
                let units = self.string_units(this)?;
                let mut index = 0;
                while index < units.len() {
                    let unit = units[index];
                    if (0xD800..=0xDBFF).contains(&unit) {
                        if !units
                            .get(index + 1)
                            .is_some_and(|low| (0xDC00..=0xDFFF).contains(low))
                        {
                            return Ok(Value::FALSE);
                        }
                        index += 1;
                    } else if (0xDC00..=0xDFFF).contains(&unit) {
                        return Ok(Value::FALSE);
                    }
                    index += 1;
                }
                Ok(Value::TRUE)
            }
            Native::StringToWellFormed => {
                let units = self.string_units(this)?;
                let mut well_formed = Vec::with_capacity(units.len());
                let mut index = 0;
                while index < units.len() {
                    let unit = units[index];
                    if (0xD800..=0xDBFF).contains(&unit) {
                        if let Some(low) = units
                            .get(index + 1)
                            .filter(|low| (0xDC00..=0xDFFF).contains(*low))
                        {
                            well_formed.extend([unit, *low]);
                            index += 1;
                        } else {
                            well_formed.push(0xFFFD);
                        }
                    } else if (0xDC00..=0xDFFF).contains(&unit) {
                        well_formed.push(0xFFFD);
                    } else {
                        well_formed.push(unit);
                    }
                    index += 1;
                }
                self.string_from_units(&well_formed)
            }
            Native::BigIntValueOf => {
                if matches!(self.heap.get(this), Some(Cell::BigInt(_))) {
                    return Ok(this);
                }
                let value_atom = self.intern_atom("\0rqj:bigint-value");
                self.own_property(this, value_atom).ok_or_else(|| {
                    JsError("BigInt.prototype.valueOf called on incompatible receiver".into())
                })
            }
            Native::StringToString | Native::StringValueOf => {
                if matches!(self.heap.get(this), Some(Cell::String(_))) {
                    Ok(this)
                } else {
                    let value_atom = self.intern_atom("\0rqj:string-value");
                    self.own_property(this, value_atom).ok_or_else(|| {
                        self.type_error(
                            p,
                            "String method called on incompatible receiver".into(),
                        )
                    })
                }
            }
            Native::NumberValueOf => {
                if this.as_number().is_some() {
                    Ok(this)
                } else {
                    let value_atom = self.intern_atom("\0rqj:number-value");
                    self.own_property(this, value_atom).ok_or_else(|| {
                        self.type_error(
                            p,
                            "Number.prototype.valueOf called on incompatible receiver".into(),
                        )
                    })
                }
            }
            Native::BooleanValueOf => self.boolean_prototype_value(this).ok_or_else(|| {
                self.type_error(
                    p,
                    "Boolean.prototype.valueOf called on incompatible receiver".into(),
                )
            }),
            Native::BooleanToString => {
                let value = self.boolean_prototype_value(this).ok_or_else(|| {
                    self.type_error(
                        p,
                        "Boolean.prototype.toString called on incompatible receiver".into(),
                    )
                })?;
                let text = if value == Value::TRUE {
                    "true"
                } else {
                    "false"
                };
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::StringCharCodeAt => {
                let index = self.argument_integer(p, args, 0, 0)?;
                let Some(Cell::String(text)) = self.heap.get(this) else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let unit = index
                    .try_into()
                    .ok()
                    .and_then(|index: usize| text.units().get(index).copied());
                Ok(unit.map_or_else(
                    || Value::number(f64::NAN),
                    |unit| Value::number(unit.into()),
                ))
            }
            Native::StringCharAt => {
                let index = self.argument_integer(p, args, 0, 0)?;
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let Some(index) = usize::try_from(index).ok() else {
                    return Ok(self
                        .heap
                        .alloc(Cell::String(super::wtf16::JsString::from_units(&[]))));
                };
                let units = receiver.units();
                let value = units.get(index).map_or_else(
                    || super::wtf16::JsString::from_units(&[]),
                    |unit| super::wtf16::JsString::from_units(std::slice::from_ref(unit)),
                );
                Ok(self.heap.alloc(Cell::String(value)))
            }
            Native::StringSlice => {
                let units = self.string_units(this)?;
                let length = units.len() as i64;
                let normalize = |value: i64| {
                    if value < 0 {
                        (length + value).max(0)
                    } else {
                        value.min(length)
                    }
                };
                let start = normalize(self.argument_integer(p, args, 0, 0)?);
                let end = normalize(self.argument_integer(p, args, 1, length)?);
                if start >= end {
                    return self.string_from_units(&[]);
                }
                self.string_from_units(&units[start as usize..end as usize])
            }
            Native::StringSubstring => {
                if let Some(length) = self.ascii_string_len(this) {
                    let length = length as i64;
                    let mut start = self.argument_integer(p, args, 0, 0)?.clamp(0, length);
                    let mut end = self.argument_integer(p, args, 1, length)?.clamp(0, length);
                    if start > end {
                        std::mem::swap(&mut start, &mut end);
                    }
                    return self.ascii_string_slice(this, start as usize, end as usize);
                }
                let units = self.string_units(this)?;
                let length = units.len() as i64;
                let mut start = self.argument_integer(p, args, 0, 0)?.clamp(0, length);
                let mut end = self.argument_integer(p, args, 1, length)?.clamp(0, length);
                if start > end {
                    std::mem::swap(&mut start, &mut end);
                }
                self.string_from_units(&units[start as usize..end as usize])
            }
            Native::StringSubstr => {
                if let Some(length) = self.ascii_string_len(this) {
                    let length = length as i64;
                    let raw_start = self.argument_integer(p, args, 0, 0)?;
                    let start = if raw_start < 0 {
                        (length + raw_start).max(0)
                    } else {
                        raw_start.min(length)
                    };
                    let count = self.argument_integer(p, args, 1, length - start)?.max(0);
                    let end = start.saturating_add(count).min(length);
                    return self.ascii_string_slice(this, start as usize, end as usize);
                }
                let units = self.string_units(this)?;
                let length = units.len() as i64;
                let raw_start = self.argument_integer(p, args, 0, 0)?;
                let start = if raw_start < 0 {
                    (length + raw_start).max(0)
                } else {
                    raw_start.min(length)
                };
                let count = self.argument_integer(p, args, 1, length - start)?.max(0);
                let end = start.saturating_add(count).min(length);
                self.string_from_units(&units[start as usize..end as usize])
            }
            Native::StringIncludes | Native::StringStartsWith | Native::StringEndsWith => {
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let search_value = args.first().copied().unwrap_or(Value::UNDEFINED);
                if self.regexp_is_regexp(p, search_value)? {
                    return Err(self.type_error(p, "search string cannot be a RegExp".into()));
                }
                let search = self.coerce_js_string(p, search_value)?;
                let length = receiver.units().len();
                let position_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let position = if native == Native::StringEndsWith && position_value.is_undefined()
                {
                    length
                } else {
                    let number = self.to_number(p, position_value)?;
                    if number.is_nan() || number <= 0.0 {
                        0
                    } else if number.is_infinite() || number >= length as f64 {
                        length
                    } else {
                        number.trunc() as usize
                    }
                };
                let matched = match native {
                    Native::StringIncludes => receiver.find_units(search.units(), position).is_some(),
                    Native::StringStartsWith => receiver.units()[position..].starts_with(search.units()),
                    Native::StringEndsWith => {
                        let start = position.saturating_sub(search.units().len());
                        receiver.units()[start..position] == *search.units()
                    }
                    _ => unreachable!(),
                };
                Ok(if matched { Value::TRUE } else { Value::FALSE })
            }
            Native::StringIndexOf | Native::StringLastIndexOf => {
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let search =
                    self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let text = receiver.units();
                let result = if native == Native::StringIndexOf {
                    let start = self
                        .to_number(p, args.get(1).copied().unwrap_or(Value::number(0.0)))?
                        .max(0.0)
                        .trunc() as usize;
                    receiver.find_units(search.units(), start)
                } else {
                    let position = self.to_number(
                        p,
                        args.get(1)
                            .copied()
                            .unwrap_or(Value::number(text.len() as f64)),
                    )?;
                    let position = if position.is_nan() {
                        text.len()
                    } else {
                        position.max(0.0).min(text.len() as f64).trunc() as usize
                    };
                    super::string::rfind_utf16(text, search.units(), position)
                };
                Ok(Value::number(result.map_or(-1.0, |index| index as f64)))
            }
            Native::StringReplace => self.string_replace_native(p, this, args, false),
            Native::StringReplaceAll => self.string_replace_native(p, this, args, true),
            Native::StringSplit => self.string_split_native(p, this, args),
            Native::StringTrim | Native::StringTrimStart | Native::StringTrimEnd => {
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let units = receiver.units();
                let start = if native == Native::StringTrimEnd {
                    0
                } else {
                    units.iter().position(|unit| !is_ecma_whitespace(*unit)).unwrap_or(units.len())
                };
                let end = if native == Native::StringTrimStart {
                    units.len()
                } else {
                    units.iter().rposition(|unit| !is_ecma_whitespace(*unit)).map_or(start, |i| i + 1)
                };
                self.string_from_units(&units[start..end])
            }
            Native::StringMatch | Native::StringSearch => {
                self.string_match_or_search_native(p, native, this, args)
            }
            Native::StringMatchAll => self.string_match_all_native(p, this, args),
            Native::StringAt
            | Native::StringCodePointAt
            | Native::StringToUpperCase
            | Native::StringToLowerCase
            | Native::StringToLocaleUpperCase
            | Native::StringToLocaleLowerCase
            | Native::StringLocaleCompare
            | Native::StringConcat
            | Native::StringNormalize => self.string_basic_native(p, native, this, args),
            Native::StringRepeat => {
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let count = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                if count.is_infinite() || count < 0.0 {
                    return Err(self.range_error(p, "invalid string repeat count".into()));
                }
                let count = if count.is_nan() { 0 } else { count.trunc() as usize };
                let Some(size) = receiver.units().len().checked_mul(count) else {
                    return Err(self.range_error(p, "string repeat count is too large".into()));
                };
                if size > 64 * 1024 * 1024 {
                    return Err(self.range_error(p, "string repeat count is too large".into()));
                }
                Ok(self.heap.alloc(Cell::String(receiver.repeat(count))))
            }
            Native::StringPadStart | Native::StringPadEnd => {
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let target =
                    self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                if !target.is_finite() || target <= 0.0 {
                    return Ok(self.heap.alloc(Cell::String(receiver)));
                }
                let target = target.trunc().min(64.0 * 1024.0 * 1024.0) as usize;
                let receiver_units = receiver.units().to_vec();
                if receiver_units.len() >= target {
                    return Ok(self.heap.alloc(Cell::String(receiver)));
                }
                let fill = match args.get(1).copied() {
                    None | Some(Value::UNDEFINED) => {
                        super::wtf16::JsString::from_str(" ")
                    }
                    Some(value) => self.coerce_js_string(p, value)?,
                };
                let fill_units = fill.units();
                if fill_units.is_empty() {
                    return Ok(self.heap.alloc(Cell::String(receiver)));
                }
                let fill_len = target - receiver_units.len();
                let mut padding = Vec::with_capacity(fill_len);
                while padding.len() < fill_len {
                    let remaining = fill_len - padding.len();
                    padding.extend(fill_units.iter().copied().take(remaining));
                }
                let mut units = Vec::with_capacity(target);
                if native == Native::StringPadEnd {
                    units.extend(receiver_units);
                    units.extend(padding);
                } else {
                    units.extend(padding);
                    units.extend(receiver_units);
                }
                self.string_from_units(&units)
            }
            Native::EncodeUri | Native::EncodeUriComponent => {
                let value = self.coerce_js_string(
                    p,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                )?;
                let encoded = super::string_extra::encode_uri(
                    &value,
                    native == Native::EncodeUriComponent,
                )
                .map_err(|message| self.uri_error(p, message.into()))?;
                Ok(self.heap.alloc(Cell::String(encoded.into())))
            }
            Native::GlobalEscape | Native::GlobalUnescape => {
                let value = self.coerce_js_string(
                    p,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                )?;
                let result = if native == Native::GlobalEscape {
                    super::string_extra::escape(&value).into()
                } else {
                    super::string_extra::unescape(&value)
                };
                Ok(self.heap.alloc(Cell::String(result)))
            }
            Native::DecodeUri | Native::DecodeUriComponent => {
                let value = self.coerce_js_string(
                    p,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                )?;
                let decoded = super::string_extra::decode_uri(
                    &value,
                    native == Native::DecodeUriComponent,
                )
                .map_err(|message| self.uri_error(p, message.into()))?;
                Ok(self.heap.alloc(Cell::String(decoded)))
            }
            Native::StringFromCharCode => {
                let mut units = Vec::with_capacity(args.len());
                for value in args {
                    units.push(Self::uint16_from_value(self.to_number(p, *value)?));
                }
                self.string_from_units(&units)
            }
            Native::StringFromCodePoint => {
                let mut units = Vec::with_capacity(args.len());
                for value in args {
                    let number = self.to_number(p, *value)?;
                    if !number.is_finite()
                        || number.fract() != 0.0
                        || !(0.0..=crate::unicode::UNICODE_MAX_CODE_POINT as f64).contains(&number)
                    {
                        return Err(self.range_error(p, "invalid code point".into()));
                    }
                    let code_point = number as u32;
                    if crate::unicode::is_surrogate(code_point)
                        || code_point <= crate::unicode::UTF16_MAX_CODE_UNIT
                    {
                        units.push(code_point as u16);
                    } else {
                        let payload = code_point - crate::unicode::SUPPLEMENTARY_CODE_POINT_START;
                        units.push(
                            (crate::unicode::SURROGATE_START
                                + (payload >> crate::unicode::SURROGATE_PAYLOAD_SHIFT))
                                as u16,
                        );
                        units.push(
                            (crate::unicode::LOW_SURROGATE_START as u32
                                + (payload & crate::unicode::SURROGATE_PAYLOAD_MASK))
                                as u16,
                        );
                    }
                }
                self.string_from_units(&units)
            }
            Native::ParseInt => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, value)?;
                let radix = match args.get(1).copied() {
                    Some(value) => crate::value::number_to_u32(self.to_number(p, value)?) as i32,
                    None => 0,
                };
                Ok(Value::number(super::string_extra::parse_integer(
                    &text, radix,
                )))
            }
            Native::NumberParseFloat => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, value)?;
                Ok(Value::number(super::number::parse_float(&text)))
            }
            Native::MathFloor => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(Value::number(self.to_number(p, value)?.floor()))
            }
            Native::MathAbs
            | Native::MathCeil
            | Native::MathRound
            | Native::MathTrunc
            | Native::MathSqrt
            | Native::MathSign
            | Native::MathAcos
            | Native::MathAsin
            | Native::MathAtan
            | Native::MathCos
            | Native::MathExp
            | Native::MathSin
            | Native::MathTan
            | Native::MathAcosh
            | Native::MathAsinh
            | Native::MathAtanh
            | Native::MathCbrt
            | Native::MathCosh
            | Native::MathExpm1
            | Native::MathFround
            | Native::MathLog10
            | Native::MathLog1p
            | Native::MathLog2
            | Native::MathSinh
            | Native::MathTanh
            | Native::MathClz32
            | Native::MathF16Round => {
                let value = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                Ok(Value::number(super::number::math_unary(native, value)))
            }
            Native::MathAtan2 => {
                let y = args.first().copied().unwrap_or(Value::UNDEFINED);
                let x = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                Ok(Value::number(
                    self.to_number(p, y)?.atan2(self.to_number(p, x)?),
                ))
            }
            Native::MathMin | Native::MathMax => {
                let mut result = if native == Native::MathMin {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                };
                let mut saw_nan = false;
                for value in args {
                    let number = self.to_number(p, *value)?;
                    if number.is_nan() {
                        saw_nan = true;
                        continue;
                    }
                    result = if native == Native::MathMin {
                        if number == 0.0 && result == 0.0 {
                            if number.is_sign_negative() || result.is_sign_negative() {
                                -0.0
                            } else {
                                0.0
                            }
                        } else {
                            result.min(number)
                        }
                    } else {
                        if number == 0.0 && result == 0.0 {
                            if number.is_sign_positive() || result.is_sign_positive() {
                                0.0
                            } else {
                                -0.0
                            }
                        } else {
                            result.max(number)
                        }
                    };
                }
                Ok(Value::number(if saw_nan { f64::NAN } else { result }))
            }
            Native::MathHypot => self.math_hypot(p, args),
            Native::MathImul => {
                let left = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let right = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let product = (crate::value::number_to_u32(left) as i32)
                    .wrapping_mul(crate::value::number_to_u32(right) as i32);
                Ok(Value::number(product as f64))
            }
            Native::MathSumPrecise => self.math_sum_precise(p, args.first().copied()),
            Native::MathRandom => {
                let mut state = self.random_state;
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                self.random_state = state;
                Ok(Value::number((state >> 11) as f64 / (1u64 << 53) as f64))
            }
            Native::NumberToLocaleString => {
                let number = self.number_receiver_value(p, this)?;
                self.intl_format_primitive(p, Value::number(number), args)
            }
            Native::NumberString => {
                let number = self.number_receiver_value(p, this)?;
                let radix = match args.first().copied() {
                    Some(value) if !value.is_undefined() => self.to_number(p, value)? as u32,
                    _ => 10,
                };
                if !(2..=36).contains(&radix) {
                    return Err(self.range_error(p, "Invalid radix".into()));
                }
                if radix == 10 || !number.is_finite() {
                    return Ok(self.heap.alloc(Cell::String(
                        super::number::number_to_decimal(number).into(),
                    )));
                }
                Ok(self.heap.alloc(Cell::String(
                    super::string_extra::number_to_radix(number, radix).into(),
                )))
            }
            _ => unreachable!("non-primitive native routed to primitive library"),
        }
    }

    fn argument_integer(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        index: usize,
        default: i64,
    ) -> Result<i64, JsError> {
        match args.get(index).copied() {
            Some(value) if value.is_undefined() => Ok(default),
            Some(value) => {
                let number = self.to_number(p, value)?;
                Ok(if number.is_nan() { 0 } else { number.trunc() as i64 })
            }
            None => Ok(default),
        }
    }

    fn string_units(&self, value: Value) -> Result<Vec<u16>, JsError> {
        match self.heap.get(value) {
            Some(Cell::String(text)) => Ok(text.units().to_vec()),
            _ => Err(JsError("string method receiver is not a string".into())),
        }
    }

    fn ascii_string_len(&self, value: Value) -> Option<usize> {
        match self.heap.get(value) {
            Some(Cell::String(text))
                if text
                    .units()
                    .iter()
                    .all(|unit| *unit < crate::unicode::ASCII_CODE_UNIT_LIMIT) =>
            {
                Some(text.units().len())
            }
            _ => None,
        }
    }

    fn ascii_string_slice(
        &mut self,
        value: Value,
        start: usize,
        end: usize,
    ) -> Result<Value, JsError> {
        let Some(Cell::String(text)) = self.heap.get(value) else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let result = text.host_string()[start..end].to_owned();
        Ok(self.heap.alloc(Cell::String(result.into())))
    }

    pub(super) fn string_from_units(&mut self, units: &[u16]) -> Result<Value, JsError> {
        Ok(self
            .heap
            .alloc(Cell::String(super::wtf16::JsString::from_units(units))))
    }
}
