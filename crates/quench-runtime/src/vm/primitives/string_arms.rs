//! The string natives of `Vm::call_primitive_native` whose bodies are long enough to keep
//! apart from its dispatch.

use super::*;
use crate::unicode::{is_high_surrogate, is_low_surrogate};

/// The replacement a lone surrogate takes in `toWellFormed`.
const REPLACEMENT_UNIT: u16 = 0xFFFD;
/// The longest string `padStart` and `padEnd` make, in units: a larger target is clamped to it.
const PAD_TARGET_LIMIT: f64 = 64.0 * 1024.0 * 1024.0;

impl<H: Host> Vm<H> {
    /// The `StringIsWellFormed` native of `call_primitive_native`.
    pub(super) fn string_is_well_formed(&mut self, this: Value) -> Result<Value, JsError> {
        let text = self.string_receiver_text(this)?;
        let wide = text.units().wide();
        let units: &[u16] = &wide;
        let mut index = 0;
        while index < units.len() {
            let unit = units[index];
            if is_high_surrogate(unit) {
                if !units.get(index + 1).is_some_and(|low| is_low_surrogate(*low)) {
                    return Ok(Value::FALSE);
                }
                index += 1;
            } else if is_low_surrogate(unit) {
                return Ok(Value::FALSE);
            }
            index += 1;
        }
        Ok(Value::TRUE)
    }

    /// The `StringToWellFormed` native of `call_primitive_native`.
    pub(super) fn string_to_well_formed(&mut self, this: Value) -> Result<Value, JsError> {
        let text = self.string_receiver_text(this)?;
        let wide = text.units().wide();
        let units: &[u16] = &wide;
        let mut well_formed = Vec::with_capacity(units.len());
        let mut index = 0;
        while index < units.len() {
            let unit = units[index];
            if is_high_surrogate(unit) {
                if let Some(low) = units.get(index + 1).filter(|low| is_low_surrogate(**low)) {
                    well_formed.extend([unit, *low]);
                    index += 1;
                } else {
                    well_formed.push(REPLACEMENT_UNIT);
                }
            } else if is_low_surrogate(unit) {
                well_formed.push(REPLACEMENT_UNIT);
            } else {
                well_formed.push(unit);
            }
            index += 1;
        }
        self.string_from_units(&well_formed)
    }

    /// The `StringCharAt` native of `call_primitive_native`.
    pub(super) fn string_char_at(&mut self, p: &ResidualProgram, this: Value, args: &[Value]) -> Result<Value, JsError> {
        let index = self.argument_integer(p, args, 0, 0)?;
        let Some(receiver) = self.heap.string(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let Some(index) = usize::try_from(index).ok() else {
            return Ok(self
                .heap
                .alloc_string(super::wtf16::JsString::from_units(&[])));
        };
        let units = receiver.units();
        if let Some(value) = units.get(index).and_then(|unit| self.ascii_unit_string(unit)) {
            return Ok(value);
        }
        let value = units.get_range(index..index + 1).map_or_else(
            || super::wtf16::JsString::from_units(&[]),
            super::wtf16::JsString::from_unit_run,
        );
        Ok(self.heap.alloc_string(value))
    }

    /// The `StringIncludes` native of `call_primitive_native`.
    pub(super) fn string_search_predicate(&mut self, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
        let Some(receiver) = self.heap.string(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let search_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.regexp_is_regexp(p, search_value)? {
            return Err(self.type_error(p, "search string cannot be a RegExp".into()));
        }
        let search = self.coerce_js_string(p, search_value)?;
        let position = self.search_position(p, native, receiver.units().len(), args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        Ok(Value::boolean(search_matches(native, &receiver, search.units(), position)))
    }

    /// The position a search starts at (clamped to the receiver's length): the end for an
    /// `endsWith` without a position, and otherwise the position argument, NaN as 0.
    fn search_position(&mut self, p: &ResidualProgram, native: Native, length: usize, position_value: Value) -> Result<usize, JsError> {
        if native == Native::StringEndsWith && position_value.is_undefined() {
            return Ok(length);
        }
        let number = self.to_number(p, position_value)?;
        Ok(if number.is_nan() || number <= 0.0 {
            0
        } else if number.is_infinite() || number >= length as f64 {
            length
        } else {
            number.trunc() as usize
        })
    }

    /// The `StringIndexOf` native of `call_primitive_native`.
    pub(super) fn string_index_of(&mut self, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
        let Some(receiver) = self.heap.string(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let search =
            self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let text = receiver.units();
        let result = if native == Native::StringIndexOf {
            let start = self
                .to_number(p, args.get(1).copied().unwrap_or_else(|| Value::number(0.0)))?
                .max(0.0)
                .trunc() as usize;
            receiver.find_units(search.units(), start)
        } else {
            let position = self.to_number(
                p,
                args.get(1)
                    .copied()
                    .unwrap_or_else(|| Value::number(text.len() as f64)),
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

    /// The `StringTrim` native of `call_primitive_native`.
    pub(super) fn string_trim(&mut self, native: Native, this: Value) -> Result<Value, JsError> {
        let Some(receiver) = self.heap.string(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let units = receiver.units();
        let start = if native == Native::StringTrimEnd {
            0
        } else {
            units
                .iter()
                .position(|unit| !is_ecma_whitespace(unit))
                .unwrap_or(units.len())
        };
        let end = if native == Native::StringTrimStart {
            units.len()
        } else {
            units
                .iter()
                .rposition(|unit| !is_ecma_whitespace(unit))
                .map_or(start, |i| i + 1)
        };
        self.string_substring(&receiver, start..end)
    }

    /// The `StringRepeat` native of `call_primitive_native`.
    pub(super) fn string_repeat(&mut self, p: &ResidualProgram, this: Value, args: &[Value]) -> Result<Value, JsError> {
        let Some(receiver) = self.heap.string(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let count = self.repeat_count(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if receiver.len() == 0 {
            return Ok(if self.heap.is_string(this) { this } else { self.heap.alloc_string(receiver) });
        }
        let Some(size) = receiver.len().checked_mul(count) else {
            return Err(self.range_error(p, "Invalid string length".into()));
        };
        if size > super::wtf16::MAX_STRING_UNITS {
            return Err(self.range_error(p, "Invalid string length".into()));
        }
        if count == 1 && self.heap.is_string(this) {
            return Ok(this);
        }
        // Short ASCII text, as a pad or an indent: its bytes written once, in place
        // of the rope doubling a long result takes.
        if size <= REPEAT_COPY_MAX_UNITS
            && let Some(super::wtf16::Units::Ascii(bytes)) = receiver.existing_units()
        {
            let text: smallvec::SmallVec<[u8; 64]> = bytes.iter().copied().cycle().take(size).collect();
            return Ok(self.heap.alloc_string(super::wtf16::JsString::from_ascii_bytes(&text)));
        }
        Ok(self.heap.alloc_string(receiver.repeat(count)))
    }

    /// The count `repeat` takes from its argument: NaN is 0, and an infinite or negative count
    /// is a RangeError.
    fn repeat_count(&mut self, p: &ResidualProgram, argument: Value) -> Result<usize, JsError> {
        let count = self.to_number(p, argument)?;
        if count.is_infinite() || count.trunc() < 0.0 {
            let count = self.to_string(p, Value::number(count))?;
            return Err(self.range_error(p, format!("Invalid count value: {count}")));
        }
        Ok(if count.is_nan() { 0 } else { count.trunc() as usize })
    }

    /// The `StringPadStart` and `StringPadEnd` natives of `call_primitive_native`.
    pub(super) fn string_pad(&mut self, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
        let Some(receiver) = self.heap.string(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let target = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        // A string that needs no padding is the receiver itself.
        if !target.is_finite() || target <= 0.0 {
            return Ok(this);
        }
        let target = target.trunc().min(PAD_TARGET_LIMIT) as usize;
        if receiver.len() >= target {
            return Ok(this);
        }
        let fill = match args.get(1).copied() {
            None | Some(Value::UNDEFINED) => super::wtf16::JsString::from_str(" "),
            Some(value) => self.coerce_js_string(p, value)?,
        };
        let fill_units = fill.units();
        if fill_units.is_empty() {
            return Ok(this);
        }
        // ASCII text and fill: the bytes written once, in order, into the result.
        if let (super::wtf16::Units::Ascii(text), super::wtf16::Units::Ascii(fill)) = (receiver.units(), fill_units) {
            return Ok(self.pad_ascii(native, text, fill, target));
        }
        let units = pad_units(native, receiver.units().to_vec(), fill_units, target);
        self.string_from_units(&units)
    }

    /// The ASCII padding of `text` to `target` units, with `fill` repeated: a string of the
    /// bytes written once.
    fn pad_ascii(&mut self, native: Native, text: &[u8], fill: &[u8], target: usize) -> Value {
        let mut bytes = smallvec::SmallVec::<[u8; 32]>::with_capacity(target);
        let padding = fill.iter().copied().cycle().take(target - text.len());
        if native == Native::StringPadEnd {
            bytes.extend_from_slice(text);
            bytes.extend(padding);
        } else {
            bytes.extend(padding);
            bytes.extend_from_slice(text);
        }
        self.heap.alloc_string(super::wtf16::JsString::from_ascii_bytes(&bytes))
    }

    /// The `StringFromCodePoint` native of `call_primitive_native`.
    pub(super) fn string_from_code_point(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
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
}

/// Whether `receiver` contains, starts with or ends with `search`, by `native`, at `position`.
fn search_matches(native: Native, receiver: &super::wtf16::JsString, search: super::wtf16::Units<'_>, position: usize) -> bool {
    match native {
        Native::StringIncludes => receiver.find_units(search, position).is_some(),
        Native::StringStartsWith => receiver.units().slice(position..receiver.units().len()).starts_with(search),
        Native::StringEndsWith => {
            let start = position.saturating_sub(search.len());
            receiver.units().slice(start..position) == search
        }
        _ => unreachable!(),
    }
}

/// `receiver` padded to `target` units with `fill` repeated: at its end for `StringPadEnd`, at
/// its start otherwise.
fn pad_units(native: Native, receiver: Vec<u16>, fill: super::wtf16::Units<'_>, target: usize) -> Vec<u16> {
    let fill_len = target - receiver.len();
    let mut padding = Vec::with_capacity(fill_len);
    while padding.len() < fill_len {
        let remaining = fill_len - padding.len();
        padding.extend(fill.iter().take(remaining));
    }
    let mut units = Vec::with_capacity(target);
    if native == Native::StringPadEnd {
        units.extend(receiver);
        units.extend(padding);
    } else {
        units.extend(padding);
        units.extend(receiver);
    }
    units
}
