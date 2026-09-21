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
            Native::StringCharCodeAt => {
                let index = self.argument_integer(p, args, 0, 0)?;
                let Some(Cell::String(text)) = self.heap.get(this) else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let unit = index.try_into().ok().and_then(|index: usize| {
                    if text.is_ascii() {
                        text.as_bytes().get(index).copied().map(u16::from)
                    } else {
                        text.encode_utf16().nth(index)
                    }
                });
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
                let unit = index.try_into().ok().and_then(|index: usize| {
                    if receiver.is_ascii() {
                        receiver.as_bytes().get(index).copied().map(u16::from)
                    } else {
                        receiver.encode_utf16().nth(index)
                    }
                });
                let text = unit.map_or_else(String::new, |unit| String::from_utf16_lossy(&[unit]));
                Ok(self.heap.alloc(Cell::String(text)))
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
                    self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let matched = match native {
                    Native::StringIncludes => receiver.contains(&search),
                    Native::StringStartsWith => receiver.starts_with(&search),
                    Native::StringEndsWith => receiver.ends_with(&search),
                    _ => unreachable!(),
                };
                Ok(if matched { Value::TRUE } else { Value::FALSE })
            }
            Native::StringReplace => self.string_replace_native(p, this, args),
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
                let parts = if separator.is_undefined() {
                    vec![receiver]
                } else {
                    let separator = self.to_string(p, separator)?;
                    if separator.is_empty() {
                        receiver
                            .encode_utf16()
                            .map(|unit| String::from_utf16_lossy(&[unit]))
                            .collect()
                    } else {
                        receiver.split(&separator).map(str::to_owned).collect()
                    }
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
                    Native::StringTrim => receiver.trim(),
                    Native::StringTrimStart => receiver.trim_start(),
                    Native::StringTrimEnd => receiver.trim_end(),
                    _ => unreachable!(),
                };
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::StringRepeat => {
                let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let count = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                if !count.is_finite() || count < 0.0 {
                    return Err(JsError("invalid string repeat count".into()));
                }
                let count = count.trunc() as usize;
                let Some(size) = receiver.len().checked_mul(count) else {
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
                let receiver_units: Vec<u16> = receiver.encode_utf16().collect();
                if receiver_units.len() >= target {
                    return Ok(self.heap.alloc(Cell::String(receiver)));
                }
                let fill = self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let fill_units: Vec<u16> = fill.encode_utf16().collect();
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
                Ok(self.heap.alloc(Cell::String(encode_uri(
                    &value,
                    native == Native::EncodeUriComponent,
                ))))
            }
            Native::DecodeUri | Native::DecodeUriComponent => {
                let value = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let decoded = decode_uri(&value, native == Native::DecodeUriComponent)
                    .map_err(|message| JsError(message.into()))?;
                Ok(self.heap.alloc(Cell::String(decoded)))
            }
            Native::StringFromCharCode => {
                let mut units = Vec::with_capacity(args.len());
                for value in args {
                    units.push(self.to_number(p, *value)? as i64 as u16);
                }
                self.string_from_units(&units)
            }
            Native::ParseInt => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, value)?;
                let radix = match args.get(1).copied() {
                    Some(value) => self.to_number(p, value)? as i32,
                    None => 0,
                };
                Ok(Value::number(parse_integer(&text, radix)))
            }
            Native::MathFloor => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(Value::number(self.to_number(p, value)?.floor()))
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
                Ok(self
                    .heap
                    .alloc(Cell::String(number_to_radix(number, radix))))
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
            Some(Cell::String(text)) => Ok(text.encode_utf16().collect()),
            _ => Err(JsError("string method receiver is not a string".into())),
        }
    }

    fn ascii_string_len(&self, value: Value) -> Option<usize> {
        match self.heap.get(value) {
            Some(Cell::String(text)) if text.is_ascii() => Some(text.len()),
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
        let result = text[start..end].to_owned();
        Ok(self.heap.alloc(Cell::String(result)))
    }

    fn string_from_units(&mut self, units: &[u16]) -> Result<Value, JsError> {
        Ok(self
            .heap
            .alloc(Cell::String(String::from_utf16_lossy(units))))
    }
}

fn encode_uri(value: &str, component: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let unescaped = byte.is_ascii_alphanumeric()
            || b"-_.!~*'()".contains(byte)
            || (!component && b";/?:@&=+$,#".contains(byte));
        if unescaped {
            output.push(*byte as char);
        } else {
            output.push('%');
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0xf) as usize] as char);
        }
    }
    output
}

fn decode_uri(value: &str, component: bool) -> Result<String, &'static str> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let reserved = b";/?:@&=+$,#";
    let hex = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err("malformed URI escape");
        }
        let value = (hex(bytes[index + 1]).ok_or("malformed URI escape")? << 4)
            | hex(bytes[index + 2]).ok_or("malformed URI escape")?;
        if !component && reserved.contains(&value) {
            output.extend_from_slice(&bytes[index..index + 3]);
        } else {
            output.push(value);
        }
        index += 3;
    }
    String::from_utf8(output).map_err(|_| "malformed URI sequence")
}

fn parse_integer(text: &str, mut radix: i32) -> f64 {
    let mut input = text.trim_start();
    let sign = if let Some(rest) = input.strip_prefix('-') {
        input = rest;
        -1.0
    } else {
        input = input.strip_prefix('+').unwrap_or(input);
        1.0
    };
    if radix == 0 {
        radix = if input.starts_with("0x") || input.starts_with("0X") {
            16
        } else {
            10
        };
    }
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    if radix == 16 {
        input = input
            .strip_prefix("0x")
            .or_else(|| input.strip_prefix("0X"))
            .unwrap_or(input);
    }
    let mut result = 0.0;
    let mut digits = 0;
    for digit in input
        .chars()
        .map_while(|character| character.to_digit(radix as u32))
    {
        result = result * f64::from(radix) + f64::from(digit);
        digits += 1;
    }
    if digits == 0 { f64::NAN } else { sign * result }
}

fn number_to_radix(number: f64, radix: u32) -> String {
    if radix == 10 || !number.is_finite() || number.fract() != 0.0 {
        return if number.fract() == 0.0 {
            format!("{number:.0}")
        } else {
            number.to_string()
        };
    }
    if number == 0.0 {
        return "0".into();
    }
    let negative = number < 0.0;
    let mut value = number.abs();
    let alphabet = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut digits = Vec::new();
    while value >= 1.0 {
        let digit = (value % f64::from(radix)) as usize;
        digits.push(alphabet[digit] as char);
        value = (value / f64::from(radix)).floor();
    }
    if negative {
        digits.push('-');
    }
    digits.iter().rev().collect()
}
