use super::*;

mod in_place;
mod string_arms;
mod whitespace;
pub(super) use whitespace::is_ecma_whitespace_character;
use whitespace::is_ecma_whitespace;

/// The longest `repeat` result made by one copy of ASCII text (`Native::StringRepeat`).
const REPEAT_COPY_MAX_UNITS: usize = 4096;

impl<H: Host> Vm<H> {
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
    pub(super) fn same_realm_user_target(
        &self,
        callee: Value,
    ) -> Option<(super::ProgramId, u32, Value)> {
        match self.heap.function(callee) {
            Some(FunctionRecord {
                kind: FunctionKind::User(program, id),
                env,
                realm,
                ..
            }) if *realm == self.realm.globals => Some((*program, *id, *env)),
            Some(_) => None,
            // A call-only closure of the running program (`Value::known_closure`).
            None => callee.as_known_closure().map(|(id, env)| (self.active_program, id, env)),
        }
    }

    /// `same_realm_user_target`, and whether its body is numeric (`DispatchClass::Numeric`).
    #[inline(always)] pub(super) fn same_realm_script_target(
        &self,
        callee: Value,
    ) -> Option<(super::ProgramId, u32, Value, bool)> {
        self.same_realm_user_target(callee).map(|(program, id, env)| (program, id, env, self.numeric_body(program, id)))
    }

    /// Whether function `id` of `program` has a numeric body (`DispatchClass::Numeric`).
    #[inline(always)] pub(super) fn numeric_body(&self, program: super::ProgramId, id: u32) -> bool {
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
    #[inline(never)]
    pub(super) fn pure_string_method_call(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Option<Value> {
        if !native.is_pure_string_method()
            || !self.heap.is_string(receiver)
            || args.iter().any(|argument| self.is_object_like(*argument))
        {
            return None;
        }
        if let Some(value) = self.string_search_in_place(native, receiver, args) {
            return Some(value);
        }
        if let Some(value) = self.substring_in_place(native, receiver, args) {
            return Some(value);
        }
        if let Some(value) = self.split_in_place(native, receiver, args) {
            return Some(value);
        }
        let result = self.call_primitive_native(p, native, receiver, args).ok();
        // A string method reads its receiver as text, which it may make on demand.
        self.heap.charge_materialized_text();
        result
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
            Native::StringIsWellFormed => self.string_is_well_formed(this),
            Native::StringToWellFormed => self.string_to_well_formed(this),
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
                let Some(text) = self.heap.string(this) else {
                    return Err(JsError("string method receiver is not a string".into()));
                };
                let unit = index
                    .try_into()
                    .ok()
                    .and_then(|index: usize| text.units().get(index));
                Ok(unit.map_or_else(
                    || Value::number(f64::NAN),
                    |unit| Value::number(unit.into()),
                ))
            }
            Native::StringCharAt => self.string_char_at(p, this, args),
            Native::StringSlice | Native::StringSubstring => {
                let text = self.string_receiver_text(this)?;
                let length = text.units().len() as i64;
                let start = self.argument_integer(p, args, 0, 0)?;
                let end = self.argument_integer(p, args, 1, length)?;
                self.string_substring(&text, in_place::substring_range(native, length, start, end))
            }
            Native::StringSubstr => {
                let text = self.string_receiver_text(this)?;
                let units = text.units();
                let length = units.len() as i64;
                let raw_start = self.argument_integer(p, args, 0, 0)?;
                let start = if raw_start < 0 {
                    (length + raw_start).max(0)
                } else {
                    raw_start.min(length)
                };
                let count = self.argument_integer(p, args, 1, length - start)?.max(0);
                let end = start.saturating_add(count).min(length);
                self.string_substring(&text, start as usize..end as usize)
            }
            Native::StringIncludes | Native::StringStartsWith | Native::StringEndsWith => self.string_search_predicate(p, native, this, args),
            Native::StringIndexOf | Native::StringLastIndexOf => self.string_index_of(p, native, this, args),
            Native::StringReplace => match self.replace_in_place(native, this, args) {
                Some(value) => Ok(value),
                None => self.string_replace_native(p, this, args, false),
            },
            Native::StringReplaceAll => match self.replace_in_place(native, this, args) {
                Some(value) => Ok(value),
                None => self.string_replace_native(p, this, args, true),
            },
            Native::StringSplit => self.string_split_native(p, this, args),
            Native::StringTrim | Native::StringTrimStart | Native::StringTrimEnd => self.string_trim(native, this),
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
            Native::StringRepeat => self.string_repeat(p, this, args),
            Native::StringPadStart | Native::StringPadEnd => self.string_pad(p, native, this, args),
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
                    units.push(Self::uint16_from_value(self.to_number(p, *value)?));
                }
                self.string_from_units(&units)
            }
            Native::StringFromCodePoint => self.string_from_code_point(p, args),
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
            Native::MathFloor
            | Native::MathAbs
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
                Ok(Value::number(
                    super::number::math_unary(native, value).expect("a unary Math native"),
                ))
            }
            Native::MathAtan2 => {
                let y = args.first().copied().unwrap_or(Value::UNDEFINED);
                let x = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let y = self.to_number(p, y)?;
                let x = self.to_number(p, x)?;
                Ok(Value::number(super::fdlibm_math::atan2(y, x)))
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

    /// `String.fromCharCode` of Numbers, whose ToNumber runs no guest code; `None` when any
    /// argument is another value.
    #[expect(clippy::wrong_self_convention, reason = "named for `String.fromCharCode`; allocates its result on the VM heap")]
    pub(super) fn from_char_code_numbers(&mut self, args: &[Value]) -> Option<Value> {
        // One code unit, as `fromCharCode(byte)` in a decode loop: its string with no list.
        if let [value] = args {
            let unit = Self::uint16_from_value(value.as_number()?);
            if let Some(value) = self.heap.latin1_unit_string(unit) {
                return Some(value);
            }
            return self.string_from_units(&[unit]).ok();
        }
        let units = args
            .iter()
            .map(|value| value.as_number().map(Self::uint16_from_value))
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

