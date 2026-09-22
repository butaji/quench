use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn non_callable_target(
        &self,
        callee: Value,
        cell: Option<&Cell>,
    ) -> Result<CallTarget, JsError> {
        if std::env::var_os("RQJ_CALL_DIAGNOSTICS").is_some() {
            let location = self
                .frames
                .last()
                .map(|frame| (frame.function, frame.pc.saturating_sub(1)));
            eprintln!("rqj: non-callable location={location:?} value={callee:?} cell={cell:?}");
            if let Some(frame) = self.frames.last() {
                eprintln!(
                    "rqj: locals={:?} registers={:?}",
                    frame.locals, frame.registers
                );
            }
        }
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
                    Ok(self.own_property(this, value_atom).unwrap_or(this))
                }
            }
            Native::NumberValueOf => {
                if this.as_number().is_some() {
                    Ok(this)
                } else {
                    let value_atom = self.intern_atom("\0rqj:number-value");
                    self.own_property(this, value_atom).ok_or_else(|| {
                        JsError("Number.prototype.valueOf called on incompatible receiver".into())
                    })
                }
            }
            Native::BooleanValueOf => {
                if this.as_bool().is_some() {
                    Ok(this)
                } else {
                    let value_atom = self.intern_atom("\0rqj:boolean-value");
                    self.own_property(this, value_atom).ok_or_else(|| {
                        JsError("Boolean.prototype.valueOf called on incompatible receiver".into())
                    })
                }
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
                let search =
                    self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let matched = match native {
                    Native::StringIncludes => receiver.find_units(search.units(), 0).is_some(),
                    Native::StringStartsWith => receiver.units().starts_with(search.units()),
                    Native::StringEndsWith => receiver.units().ends_with(search.units()),
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
            Native::StringSplit => {
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let limit = match args.get(1).copied() {
                    None | Some(Value::UNDEFINED) => usize::MAX,
                    Some(value) => {
                        let number = self.to_number(p, value)?;
                        if !number.is_finite() || number <= 0.0 {
                            0
                        } else {
                            number.trunc().min(usize::MAX as f64) as usize
                        }
                    }
                };
                let separator = args.first().copied().unwrap_or(Value::UNDEFINED);
                if self.is_regexp(separator) {
                    return self.string_split_regexp_native(p, this, separator, limit);
                }
                let parts = if separator.is_undefined() {
                    vec![receiver]
                } else {
                    let separator = self.coerce_js_string(p, separator)?;
                    receiver.split_units(separator.units())
                };
                let values = parts
                    .into_iter()
                    .take(limit)
                    .map(|part| self.heap.alloc(Cell::String(part)))
                    .collect();
                Ok(self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(values),
                }))
            }
            Native::StringTrim | Native::StringTrimStart | Native::StringTrimEnd => {
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let text = match native {
                    Native::StringTrim => receiver.host_string().trim(),
                    Native::StringTrimStart => receiver.host_string().trim_start(),
                    Native::StringTrimEnd => receiver.host_string().trim_end(),
                    _ => unreachable!(),
                };
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::StringMatch | Native::StringSearch => {
                self.string_match_or_search_native(p, native, this, args)
            }
            Native::StringAt
            | Native::StringCodePointAt
            | Native::StringToUpperCase
            | Native::StringToLowerCase
            | Native::StringConcat
            | Native::StringNormalize => self.string_basic_native(p, native, this, args),
            Native::StringRepeat => {
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let count = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                if !count.is_finite() || count < 0.0 {
                    return Err(JsError("invalid string repeat count".into()));
                }
                let count = count.trunc() as usize;
                let Some(size) = receiver.units().len().checked_mul(count) else {
                    return Err(JsError("string repeat count is too large".into()));
                };
                if size > 64 * 1024 * 1024 {
                    return Err(JsError("string repeat count is too large".into()));
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
                let fill =
                    self.coerce_js_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
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
                let value = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                Ok(self.heap.alloc(Cell::String(
                    super::string_extra::encode_uri(&value, native == Native::EncodeUriComponent)
                        .into(),
                )))
            }
            Native::DecodeUri | Native::DecodeUriComponent => {
                let value = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let decoded =
                    super::string_extra::decode_uri(&value, native == Native::DecodeUriComponent)
                        .map_err(|message| JsError(message.into()))?;
                Ok(self.heap.alloc(Cell::String(decoded.into())))
            }
            Native::StringFromCharCode => {
                let mut units = Vec::with_capacity(args.len());
                for value in args {
                    units.push(self.to_number(p, *value)? as i64 as u16);
                }
                self.string_from_units(&units)
            }
            Native::StringFromCodePoint => {
                let mut text = String::new();
                for value in args {
                    let number = self.to_number(p, *value)?;
                    if !number.is_finite()
                        || number.fract() != 0.0
                        || !(0.0..=0x10ffff as f64).contains(&number)
                    {
                        return Err(JsError("invalid code point".into()));
                    }
                    let code_point = number as u32;
                    if (0xd800..=0xdfff).contains(&code_point) {
                        return Err(JsError("invalid code point".into()));
                    }
                    text.push(char::from_u32(code_point).expect("validated code point"));
                }
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::ParseInt => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, value)?;
                let radix = match args.get(1).copied() {
                    Some(value) => self.to_number(p, value)? as i32,
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
            | Native::MathSign => {
                let value = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                Ok(Value::number(super::number::math_unary(native, value)))
            }
            Native::MathMin | Native::MathMax => {
                let mut result = if native == Native::MathMin {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                };
                for value in args {
                    let number = self.to_number(p, *value)?;
                    if number.is_nan() {
                        return Ok(Value::number(f64::NAN));
                    }
                    result = if native == Native::MathMin {
                        result.min(number)
                    } else {
                        result.max(number)
                    };
                }
                Ok(Value::number(result))
            }
            Native::MathRandom => {
                let mut state = self.random_state;
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                self.random_state = state;
                Ok(Value::number((state >> 11) as f64 / (1u64 << 53) as f64))
            }
            Native::NumberString => {
                let number = self.to_number(p, this)?;
                let radix = match args.first().copied() {
                    Some(value) if !value.is_undefined() => self.to_number(p, value)? as u32,
                    _ => 10,
                };
                if !(2..=36).contains(&radix) {
                    return Err(JsError("invalid number radix".into()));
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
        Ok(match args.get(index).copied() {
            Some(value) => self.to_number(p, value)?.trunc() as i64,
            None => default,
        })
    }

    fn string_units(&self, value: Value) -> Result<Vec<u16>, JsError> {
        match self.heap.get(value) {
            Some(Cell::String(text)) => Ok(text.units().to_vec()),
            _ => Err(JsError("string method receiver is not a string".into())),
        }
    }

    fn ascii_string_len(&self, value: Value) -> Option<usize> {
        match self.heap.get(value) {
            Some(Cell::String(text)) if text.units().iter().all(|unit| *unit < 0x80) => {
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
