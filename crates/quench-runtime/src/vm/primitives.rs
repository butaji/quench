use super::*;
use crate::staged::string_text;

mod in_place;
mod whitespace;
pub(super) use whitespace::is_ecma_whitespace_character;

/// The longest `repeat` result made by one copy of ASCII text (`Native::StringRepeat`).
const REPEAT_COPY_MAX_UNITS: usize = 4096;

impl Vm {
    #[quench_macros::test_visible]
    pub(super) fn call_target(&self, callee: Value) -> Result<CallTarget, JsError> {
        match self.heap.function(callee) {
            Some(FunctionRecord {
                kind: FunctionKind::User(program, id),
                env,
                ..
            }) => Ok(CallTarget::User(*program, *id, *env)),
            Some(FunctionRecord {
                kind: FunctionKind::Native(native),
                ..
            }) => Ok(CallTarget::Native(*native)),
            // A call-only closure enters its function of the running program, whose code alone
            // names its binding (`Value::known_closure`).
            None => match callee.as_known_closure() {
                Some((id, env)) => Ok(CallTarget::User(self.active_program, id, env)),
                None => self.non_callable_target(callee, self.heap.get(callee)),
            },
        }
    }

    /// The user function `callee` names when it belongs to the current realm: the one
    /// target an in-loop frame push can enter without switching realm or program state.
    #[inline(always)]
    pub fn same_realm_user_target(
        &self,
        callee: Value,
    ) -> Option<(super::ProgramId, u32, Value)> {
        match self.heap.function(callee) {
            Some(record) if self.record_realm(record) == self.realm.globals => match record.kind {
                FunctionKind::User(program, id) => Some((program, id, record.env)),
                FunctionKind::Native(_) => None,
            },
            Some(_) => None,
            // A call-only closure of the running program (`Value::known_closure`).
            None => callee.as_known_closure().map(|(id, env)| (self.active_program, id, env)),
        }
    }

    /// `same_realm_user_target`, and whether its body is numeric (`DispatchClass::Numeric`).
    #[inline(always)] pub fn same_realm_script_target(
        &self,
        callee: Value,
    ) -> Option<(super::ProgramId, u32, Value, bool)> {
        self.same_realm_user_target(callee).map(|(program, id, env)| (program, id, env, self.numeric_body(program, id)))
    }

    /// Whether function `id` of `program` has a numeric body (`DispatchClass::Numeric`).
    #[inline(always)] pub fn numeric_body(&self, program: super::ProgramId, id: u32) -> bool {
        self.programs.residual(program).and_then(|code| code.function_get(id as usize).map(|body| body.dispatch))
            == Some(crate::bytecode::DispatchClass::Numeric)
    }

    pub(super) fn non_callable_target(
        &self,
        _callee: Value,
        _cell: Option<&Cell>,
    ) -> Result<CallTarget, JsError> {
        Err(JsError(super::call_printer::NOT_CALLABLE_MESSAGE.into()))
    }

    /// A call of `callee` that `Native::is_pure_string_method` admits for `receiver` and
    /// `args`, answered without the general call path. `None` leaves the call to it: a call
    /// outside the method's admission, or one that fails, which the general path repeats
    /// to raise the same error, since the attempt had no observable effect.
    ///
    /// Inlined into each caller, so a caller with a constant native folds the admission and
    /// the body choice to that one body.
    #[inline(always)]
    pub fn pure_string_method_call(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Option<Value> {
        if !self.pure_string_admitted(native, receiver, args) {
            return None;
        }
        if let Some(value) = self.pure_string_in_place(native, receiver, args) {
            return Some(value);
        }
        // The general body, which tries the same in-place body first, skips it this once.
        self.declined_fast = Some(native);
        self.pure_string_general(p, native, receiver, args)
    }

    /// The admission of `pure_string_method_call`: a pure String method on a string receiver,
    /// with no object-like argument.
    #[inline(always)]
    fn pure_string_admitted(&self, native: Native, receiver: Value, args: &[Value]) -> bool {
        native.is_pure_string_method()
            && self.heap.is_string(receiver)
            && !args.iter().any(|argument| self.is_object_like(*argument))
    }

    /// The one in-place body that serves `native` among the pure String methods, or `None`
    /// where that body leaves the call to `pure_string_general`. Each native belongs to one
    /// body only, so no other body is tried.
    #[inline(always)]
    fn pure_string_in_place(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        match native {
            Native::StringIndexOf | Native::StringLastIndexOf | Native::StringIncludes | Native::StringStartsWith
            | Native::StringEndsWith =>
                self.string_search_in_place(native, receiver, args),
            Native::StringSlice | Native::StringSubstring | Native::StringSubstr =>
                self.substring_in_place(native, receiver, args),
            Native::StringSplit => self.split_in_place(native, receiver, args),
            Native::StringToUpperCase | Native::StringToLowerCase | Native::StringTrim | Native::StringTrimStart
            | Native::StringTrimEnd | Native::StringPadStart | Native::StringPadEnd | Native::StringRepeat =>
                self.ascii_text_method(native, receiver, args),
            _ => None,
        }
    }

    /// A pure String method's general call: `call_primitive_native`, whose result is the
    /// call's value. A string method reads its receiver as text, which it may make on demand.
    #[inline(never)]
    fn pure_string_general(&mut self, p: &ResidualProgram, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        let result = self.call_primitive_native(p, native, receiver, args).ok();
        self.heap.charge_materialized_text();
        result
    }

    pub fn call_primitive_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::StringRaw => self.string_raw(p, args),
            Native::StringIsWellFormed => {
                let text = self.string_receiver_text(this)?;
                let wide = text.units().wide();
                let units: &[u16] = &wide;
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
                let text = self.string_receiver_text(this)?;
                let wide = text.units().wide();
                let units: &[u16] = &wide;
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
            Native::BigIntValueOf => self.bigint_value_of(p, this),
            Native::StringToString | Native::StringValueOf => {
                if self.heap.is_string(this) {
                    Ok(this)
                } else {
                    self.boxed_primitive_value(this).filter(|value| self.heap.is_string(*value)).ok_or_else(|| {
                        self.type_error(p, "String method called on incompatible receiver".into())
                    })
                }
            }
            Native::NumberValueOf => {
                if this.as_number().is_some() {
                    Ok(this)
                } else {
                    self.boxed_primitive_value(this)
                        .filter(|value| value.as_number().is_some())
                        .ok_or_else(|| {
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
                Ok(self.heap.alloc_string(text.into()))
            }
            Native::StringCharCodeAt => {
                let index = self.argument_integer(p, args, 0, 0)?;
                self.string_char_code_at_core(this, Value::number(index as f64))
                    .ok_or_else(|| JsError("string method receiver is not a string".into()))
            }
            Native::StringCharAt => {
                let index = self.argument_integer(p, args, 0, 0)?;
                self.string_char_at_core(this, Value::number(index as f64))
                    .ok_or_else(|| JsError("string method receiver is not a string".into()))
            }
            Native::StringSlice | Native::StringSubstring => {
                let text = self.string_receiver_text(this)?;
                let length = text.units().len() as i64;
                let start = self.argument_integer(p, args, 0, 0)?;
                let end = self.argument_integer(p, args, 1, length)?;
                self.string_substring(&text, crate::staged::string_search::cut(native, length, start, end))
            }
            Native::StringSubstr => {
                let text = self.string_receiver_text(this)?;
                let length = text.units().len() as i64;
                let start = self.argument_integer(p, args, 0, 0)?;
                let count = self.argument_integer(p, args, 1, crate::staged::string_search::SUBSTR_REST)?;
                self.string_substring(&text, crate::staged::string_search::cut(Native::StringSubstr, length, start, count))
            }
            Native::StringIndexOf | Native::StringLastIndexOf | Native::StringIncludes | Native::StringStartsWith
            | Native::StringEndsWith => self.string_search_general(p, native, this, args),
            // `string_replace_native` tries `replace_string_pattern` first.
            Native::StringReplace => self.string_replace_native(p, this, args, false),
            // `string_replace_native` tries `replace_string_pattern` first.
            Native::StringReplaceAll => self.string_replace_native(p, this, args, true),
            Native::StringSplit => self.string_split_native(p, this, args),
            Native::StringTrim | Native::StringTrimStart | Native::StringTrimEnd => {
                let Some(receiver) = self.heap.string(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let from_start = native != Native::StringTrimEnd;
                let from_end = native != Native::StringTrimStart;
                let range = match receiver.units() {
                    super::wtf16::Units::Ascii(units) | super::wtf16::Units::Latin1(units) => {
                        string_text::trim_range(units, from_start, from_end)
                    }
                    super::wtf16::Units::Wide(units) => string_text::trim_range(units, from_start, from_end),
                };
                self.string_substring(&receiver, range)
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
                let Some(receiver) = self.heap.string(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let count = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                if count.is_infinite() || count.trunc() < 0.0 {
                    let count = self.to_string(p, Value::number(count))?;
                    return Err(self.range_error(p, format!("Invalid count value: {count}")));
                }
                let count = if count.is_nan() {
                    0
                } else {
                    count.trunc() as usize
                };
                if receiver.len() == 0 {
                    return Ok(if self.heap.is_string(this) {
                        this
                    } else {
                        self.heap.alloc_string(receiver)
                    });
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
                    let mut text = smallvec::SmallVec::<[u8; REPEAT_COPY_MAX_UNITS]>::from_elem(0, size);
                    string_text::repeat_into(&mut text, bytes);
                    return Ok(self.heap.alloc_ascii_run(&text));
                }
                Ok(self.heap.alloc_string(receiver.repeat(count)))
            }
            Native::StringPadStart | Native::StringPadEnd => {
                let Some(receiver) = self.heap.string(this).cloned() else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let target =
                    self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                // A string that needs no padding is the receiver itself.
                if !target.is_finite() || target <= 0.0 {
                    return Ok(this);
                }
                let target = target.trunc().min(64.0 * 1024.0 * 1024.0) as usize;
                let Some(length) = string_text::pad_length(receiver.len(), target) else {
                    return Ok(this);
                };
                let fill = match args.get(1).copied() {
                    None | Some(Value::UNDEFINED) => super::wtf16::JsString::from_ascii_bytes(string_text::PAD_FILL),
                    Some(value) => self.coerce_js_string(p, value)?,
                };
                let fill_units = fill.units();
                if fill_units.is_empty() {
                    return Ok(this);
                }
                let at_end = native == Native::StringPadEnd;
                // ASCII text and fill: the bytes written once, in order, into the result.
                if let (super::wtf16::Units::Ascii(text), super::wtf16::Units::Ascii(filler)) = (receiver.units(), fill_units) {
                    let mut bytes = smallvec::SmallVec::<[u8; 32]>::from_elem(0, length);
                    string_text::pad_into(&mut bytes, text, filler, at_end);
                    return Ok(self.heap.alloc_ascii_run(&bytes));
                }
                let mut units = vec![0u16; length];
                string_text::pad_into(&mut units, &receiver.units().to_vec(), &fill_units.to_vec(), at_end);
                self.string_from_units(&units)
            }
            Native::EncodeUri | Native::EncodeUriComponent => {
                let value =
                    self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let encoded =
                    super::string_extra::encode_uri(&value, native == Native::EncodeUriComponent)
                        .map_err(|message| self.uri_error(p, message.into()))?;
                Ok(self.heap.alloc_string(encoded.into()))
            }
            Native::GlobalEscape | Native::GlobalUnescape => {
                let value =
                    self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let result = if native == Native::GlobalEscape {
                    super::string_extra::escape(&value).into()
                } else {
                    super::string_extra::unescape(&value)
                };
                Ok(self.heap.alloc_string(result))
            }
            Native::DecodeUri | Native::DecodeUriComponent => {
                let value =
                    self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let decoded =
                    super::string_extra::decode_uri(&value, native == Native::DecodeUriComponent)
                        .map_err(|message| self.uri_error(p, message.into()))?;
                Ok(self.heap.alloc_string(decoded))
            }
            Native::StringFromCharCode => {
                if let Some(text) = self.from_char_code_numbers(args) {
                    return Ok(text);
                }
                let mut units = Vec::with_capacity(args.len());
                for value in args {
                    units.push(crate::staged::strings::from_char_code_unit(self.to_number(p, *value)?));
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
                // A string argument is read in place; any other is converted by ToString first.
                let text = if self.heap.is_string(value) { None } else { Some(self.to_string(p, value)?) };
                let radix = match args.get(1).copied() {
                    Some(value) => crate::value::number_to_u32(self.to_number(p, value)?) as i32,
                    None => 0,
                };
                // Radix 10 (or absent): the shared int32 form of the text (`staged::parse_int`), which the b-jit's row answers too.
                let decimal = if radix == 0 || radix == 10 {
                    match (&text, self.heap.string(value)) {
                        (None, Some(string)) => string.read_units(|units| crate::staged::parse_int::decimal_int32(units.iter())),
                        _ => text.as_deref().and_then(|text| crate::staged::parse_int::decimal_int32(text.encode_utf16())),
                    }
                } else {
                    None
                };
                if let Some(int) = decimal {
                    return Ok(Value::integer(int));
                }
                Ok(Value::number(match (&text, self.heap.string(value)) {
                    (None, Some(string)) => string.read_units(|units| super::string_extra::parse_integer_units(units, radix)),
                    _ => super::string_extra::parse_integer(text.as_deref().unwrap_or_default(), radix),
                }))
            }
            Native::NumberParseFloat => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                if let Some(string) = self.heap.string(value) {
                    return Ok(Value::number(string.read_units(super::number::parse_float_units)));
                }
                let text = self.to_string(p, value)?;
                Ok(Value::number(super::number::parse_float(&text)))
            }
            native if super::number::is_math_unary(native) => {
                let value = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                Ok(Value::number(
                    super::number::math_unary(native, value).expect("a unary Math native"),
                ))
            }
            Native::MathAtan2 => {
                let y = args.first().copied().unwrap_or(Value::UNDEFINED);
                let x = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let numbers = [self.to_number(p, y)?, self.to_number(p, x)?];
                Ok(Value::number(super::number::math_numbers(native, &numbers).expect("Math.atan2")))
            }
            Native::MathMin | Native::MathMax => {
                let mut numbers = smallvec::SmallVec::<[f64; super::number::MATH_INLINE_ARGUMENTS]>::new();
                for value in args {
                    numbers.push(self.to_number(p, *value)?);
                }
                Ok(Value::number(super::number::math_numbers(native, &numbers).expect("Math.min or Math.max")))
            }
            Native::MathHypot => self.math_hypot(p, args),
            Native::MathImul => {
                let left = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let right = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                Ok(Value::number(super::number::math_numbers(native, &[left, right]).expect("Math.imul")))
            }
            Native::MathSumPrecise => self.math_sum_precise(p, args.first().copied()),
            Native::MathRandom => {
                crate::clock::note_input("Math.random");
                let mut state = self.random_state;
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                self.random_state = state;
                Ok(Value::number((state >> 11) as f64 / (1u64 << 53) as f64))
            }
            Native::NumberToLocaleString => {
                let number = self.number_receiver_value(p, this)?;
                layer_hooks::intl_format_primitive(self, p, Value::number(number), args)
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
                    return Ok(self.heap.alloc_string(super::wtf16::JsString::from_number(number)));
                }
                Ok(self.heap.alloc_string(super::string_extra::number_to_radix_string(number, radix)))
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
                Ok(if number.is_nan() {
                    0
                } else {
                    number.trunc() as i64
                })
            }
            None => Ok(default),
        }
    }

    /// The receiver string, sharing its units rather than copying them.
    fn string_receiver_text(&self, value: Value) -> Result<super::wtf16::JsString, JsError> {
        match self.heap.get(value) {
            _ if let Some(text) = self.heap.string(value)  => Ok(text.clone()),
            _ => Err(JsError("string method receiver is not a string".into())),
        }
    }

    /// The string value of `text`'s units `range`, sharing long ASCII storage.
    pub(super) fn string_substring(
        &mut self,
        text: &super::wtf16::JsString,
        range: std::ops::Range<usize>,
    ) -> Result<Value, JsError> {
        Ok(self.heap.alloc_string(text.substring(range)))
    }

    /// The general body of `indexOf`, `lastIndexOf`, `includes`, `startsWith` and `endsWith`: the
    /// receiver's text, the RegExp check of `includes`, `startsWith` and `endsWith`, the search's
    /// ToString and the position's ToNumber run in that order, each possibly running guest code.
    /// The search itself is `staged::string_search::search` from the clamped position, as the in-place
    /// body makes it.
    fn string_search_general(&mut self, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
        let Some(receiver) = self.heap.string(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let search_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let is_regexp_checked = matches!(native, Native::StringIncludes | Native::StringStartsWith | Native::StringEndsWith);
        if is_regexp_checked && self.regexp_is_regexp(p, search_value)? {
            return Err(self.type_error(p, "search string cannot be a RegExp".into()));
        }
        let search = self.coerce_js_string(p, search_value)?;
        let length = receiver.units().len();
        let number = match args.get(1).copied() {
            Some(value) if !value.is_undefined() => self.to_number(p, value)?,
            _ => crate::staged::string_search::search_default(native, length),
        };
        let position = crate::staged::string_search::clamped_position(native, number, length);
        Ok(crate::staged::string_search::search(native, receiver.units(), search.units(), position))
    }

    /// `String.fromCharCode` of Numbers, whose ToNumber runs no guest code; `None` when any
    /// argument is another value.
    #[expect(clippy::wrong_self_convention, reason = "named for `String.fromCharCode`; allocates its result on the VM heap")]
    pub fn from_char_code_numbers(&mut self, args: &[Value]) -> Option<Value> {
        // One code unit, as `fromCharCode(byte)` in a decode loop: its string with no list.
        if let [value] = args {
            let unit = crate::staged::strings::from_char_code_unit(value.as_number()?);
            if let Some(value) = self.heap.latin1_unit_string(unit) {
                return Some(value);
            }
            return self.string_from_units(&[unit]).ok();
        }
        let units = args
            .iter()
            .map(|value| value.as_number().map(crate::staged::strings::from_char_code_unit))
            .collect::<Option<smallvec::SmallVec<[u16; 32]>>>()?;
        self.string_from_units(&units).ok()
    }

    pub(super) fn string_from_units(&mut self, units: &[u16]) -> Result<Value, JsError> {
        if let &[unit] = units
            && let Some(value) = self.ascii_unit_string(unit)
        {
            return Ok(value);
        }
        Ok(self
            .heap
            .alloc_string(super::wtf16::JsString::from_units(units)))
    }

    /// The string of the one code unit `unit`, when it is ASCII (`Heap::ascii_unit_strings`).
    pub(super) fn ascii_unit_string(&mut self, unit: u16) -> Option<Value> {
        let byte = u8::try_from(unit).ok().filter(u8::is_ascii)?;
        Some(self.ascii_byte_string(byte))
    }

    /// The string of the one ASCII byte `byte`.
    pub(super) fn ascii_byte_string(&mut self, byte: u8) -> Value {
        // The byte's shared string once made (`Heap::alloc_unit_run`), with no string built
        // to find it.
        self.heap.alloc_unit_run(super::wtf16::Units::Ascii(&[byte]))
    }
}

