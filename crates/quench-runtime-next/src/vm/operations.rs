use super::*;
impl<H: Host> Vm<H> {
    pub(super) fn call_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if Self::is_finalization_native(native) {
            return self.call_finalization_registry_native(native, this, args);
        }
        if Self::is_disposal_native(native) {
            return self.call_disposal_native(p, native, this, args);
        }
        if Self::is_collection_native(native) {
            return self.call_collection_native(p, native, this, args);
        }
        if let Some(result) = self.maybe_call_typed_array_native(p, native, this, args) {
            return result;
        }
        if native.is_atomics_native() {
            return self.atomics_native(p, native, args);
        }
        if native.is_promise_native() {
            return self.call_promise_native(p, native, this, args);
        }
        if native.is_object_static() {
            return self.call_object_native(p, native, args);
        }
        if native == Native::ProxyRevoke {
            return self.proxy_revoke_receiver(this);
        }
        if native.is_data_view_native() {
            return self.data_view_native(p, native, this, args);
        }
        match native {
            Native::DynamicDerivedClass => Err(JsError(
                "class constructor cannot be called without new".into(),
            )),
            Native::WithEnter => {
                self.with_stack
                    .push(args.first().copied().unwrap_or(Value::UNDEFINED));
                Ok(Value::UNDEFINED)
            }
            Native::WithExit => {
                self.with_stack.pop();
                Ok(Value::UNDEFINED)
            }
            Native::Eval => self.eval_native(p, args),
            Native::ProxyRevocable => self.proxy_revocable(p, args),
            native if native.is_host_control_native() => self.call_host(p, native, args),
            Native::Print => {
                let v = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, v)?;
                HostContext::new(&mut self.host).invoke(CapabilityId::WriteLine, Some(&text));
                Ok(Value::UNDEFINED)
            }
            Native::DateNow => Ok(Value::number(
                HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None),
            )),
            Native::DateGetTime
            | Native::DateValueOf
            | Native::DateGetTimezoneOffset
            | Native::DateToISOString
            | Native::DateToJSON => self.date_native(native, this),
            Native::DateParse | Native::DateUTC => self.date_static_native(p, native, args),
            Native::RegExpExec | Native::RegExpTest => self.regexp_native(p, native, this, args),
            Native::ObjectPrototypeHasOwnProperty | Native::ObjectPrototypePropertyIsEnumerable => {
                let key_value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let descriptor = self.object_get_own_property_descriptor(p, &[this, key_value])?;
                if descriptor.is_undefined() {
                    return Ok(Value::FALSE);
                }
                if native == Native::ObjectPrototypeHasOwnProperty {
                    return Ok(Value::TRUE);
                }
                let enumerable = self.intern_atom("enumerable");
                let enumerable_value = self.get_property(p, descriptor, enumerable)?;
                Ok(if self.truthy(enumerable_value) {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::ObjectPrototypeIsPrototypeOf => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                let prototype = self.box_object(this)?;
                let mut current = target;
                let mut found = false;
                while let Some(object) = self.object_data(current) {
                    current = object.proto;
                    if current == prototype {
                        found = true;
                        break;
                    }
                    if current.is_null() {
                        break;
                    }
                }
                Ok(if found { Value::TRUE } else { Value::FALSE })
            }
            Native::ReflectGet
            | Native::ReflectGetOwnPropertyDescriptor
            | Native::ReflectDefineProperty
            | Native::ReflectDeleteProperty
            | Native::ReflectPreventExtensions
            | Native::ReflectIsExtensible
            | Native::ReflectSet
            | Native::ReflectOwnKeys
            | Native::ReflectGetPrototypeOf
            | Native::ReflectSetPrototypeOf
            | Native::ReflectConstruct => self.call_reflect_native(p, native, args),
            Native::JsonParse => self.json_parse(p, args),
            Native::JsonStringify => self.json_stringify(p, args),
            Native::GlobalIsNaN => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(if self.to_number(p, value)?.is_nan() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::MathLog => {
                let v = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(Value::number(self.to_number(p, v)?.ln()))
            }
            Native::MathPow => {
                let a = args.first().copied().unwrap_or(Value::UNDEFINED);
                let b = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                Ok(Value::number(
                    self.to_number(p, a)?.powf(self.to_number(p, b)?),
                ))
            }
            Native::ArrayPush => self.array_push_native(this, args),
            Native::ArrayIsArray => Ok(
                if matches!(
                    args.first().and_then(|value| self.heap.get(*value)),
                    Some(Cell::Array { .. })
                ) {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            Native::ArrayPop => self.array_pop_native(this),
            Native::ArraySlice => self.array_slice_native(p, this, args),
            Native::ArrayIncludes => self.array_includes_native(p, this, args),
            Native::ArrayJoin => self.array_join_native(p, this, args),
            Native::ArrayConcat => self.array_concat_native(this, args),
            Native::ArrayFlat => self.array_flat_native(p, this, args),
            Native::ArrayReverse => self.array_reverse_native(this),
            Native::ArrayShift => self.array_shift_native(this),
            Native::ArrayUnshift => self.array_unshift_native(this, args),
            Native::ArraySplice => self.array_splice_native(p, this, args),
            Native::ArrayFill => self.array_fill_native(p, this, args),
            Native::ArrayAt
            | Native::ArrayLastIndexOf
            | Native::ArrayIndexOf
            | Native::ArrayCopyWithin
            | Native::ArrayWith
            | Native::ArrayForEach
            | Native::ArrayMap
            | Native::ArrayFilter
            | Native::ArraySome
            | Native::ArrayEvery
            | Native::ArrayFind
            | Native::ArrayFindIndex
            | Native::ArrayFindLast
            | Native::ArrayFindLastIndex
            | Native::ArrayGroup
            | Native::ArrayGroupToMap
            | Native::ArrayFlatMap
            | Native::ArrayReduce
            | Native::ArrayReduceRight => self.array_indexed_native(p, native, this, args),
            Native::ArrayToReversed
            | Native::ArrayToSpliced
            | Native::ArraySort
            | Native::ArrayToSorted
            | Native::ArrayToString => self.array_modern_native(p, native, this, args),
            Native::ArrayKeys | Native::ArrayValues | Native::ArrayEntries => {
                self.array_iterator_native(native, this)
            }
            Native::ArrayBufferSlice => self.array_buffer_slice_native(p, this, args),
            Native::ArrayBufferTransfer => self.array_buffer_transfer_native(this),
            Native::ArrayBufferResize => self.array_buffer_resize_native(p, this, args),
            Native::ArrayBufferTransferToFixedLength => {
                self.array_buffer_transfer_fixed_native(this)
            }
            Native::SharedArrayBufferGrow => self.shared_array_buffer_grow_native(p, this, args),
            Native::ArrayFrom | Native::ArrayOf => self.array_modern_native(p, native, this, args),
            Native::FunctionCall => {
                let receiver = args.first().copied().unwrap_or(Value::UNDEFINED);
                self.call_value(p, this, receiver, args.get(1..).unwrap_or_default())
            }
            Native::FunctionApply => {
                let receiver = args.first().copied().unwrap_or(Value::UNDEFINED);
                let argument_array = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let arguments = if argument_array.is_undefined() {
                    vec![]
                } else {
                    match self.heap.get(argument_array) {
                        Some(Cell::Array { elements, .. }) => {
                            super::array::normalized_array_values(elements)
                        }
                        _ => return Err(JsError("apply arguments must be an array".into())),
                    }
                };
                self.call_value(p, this, receiver, &arguments)
            }
            Native::FunctionBind => self.bind_function(this, args),
            Native::FunctionBoundCall => self.call_bound_function(p, args),
            Native::Number
            | Native::NumberIsNaN
            | Native::NumberIsFinite
            | Native::NumberIsInteger
            | Native::NumberIsSafeInteger => self.call_number_native(p, native, args),
            Native::NumberFixed => {
                let number = self.to_number(p, this)?;
                let digits = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
                Ok(self
                    .heap
                    .alloc(Cell::String(format!("{number:.digits$}").into())))
            }
            Native::NumberPrecision => {
                let number = self.to_number(p, this)?;
                let digits = args.first().and_then(|v| v.as_number()).unwrap_or(3.0) as usize;
                Ok(self
                    .heap
                    .alloc(Cell::String(format!("{number:.digits$}").into())))
            }
            Native::String => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, value)?;
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::Symbol => self.call_symbol_constructor(p, args),
            Native::SymbolToString | Native::SymbolValueOf => {
                self.call_symbol_value_native(p, native, this)
            }
            Native::SymbolFor | Native::SymbolKeyFor => self.call_symbol_native(p, native, args),
            Native::Date => {
                let milliseconds =
                    HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None);
                let prototype_atom = self.intern_atom("prototype");
                let prototype = self
                    .own_property(self.native_value(Native::Date), prototype_atom)
                    .unwrap_or(self.object_proto);
                Ok(self.heap.alloc(Cell::Date {
                    milliseconds,
                    object: Box::new(Self::empty_object(prototype)),
                }))
            }
            Native::Object
            | Native::Array
            | Native::Map
            | Native::Set
            | Native::WeakMap
            | Native::WeakSet
            | Native::WeakRef
            | Native::FinalizationRegistry
            | Native::DisposableStack
            | Native::RegExp => self.construct_native(p, native, args),
            Native::ThrowTypeError => {
                Err(self.type_error(p, "restricted arguments property".into()))
            }
            native if native.is_error_constructor() => self.construct_native(p, native, args),
            _ => self.call_primitive_native(p, native, this, args),
        }
    }
    #[inline]
    pub(super) fn binary(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        let primitive_operands = (8..=19).contains(&op);
        let left = if primitive_operands && self.object_data(left).is_some() {
            self.to_primitive(p, left, "default")?
        } else {
            left
        };
        let right = if primitive_operands && self.object_data(right).is_some() {
            self.to_primitive(p, right, "default")?
        } else {
            right
        };
        if let Some((a, b)) = Value::int_pair(left, right) {
            return Ok(match op {
                0 | 2 => {
                    if a == b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                1 | 3 => {
                    if a != b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                4 => {
                    if a < b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                5 => {
                    if a <= b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                6 => {
                    if a > b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                7 => {
                    if a >= b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                8 => a
                    .checked_add(b)
                    .map(Value::integer)
                    .unwrap_or_else(|| Value::number(a as f64 + b as f64)),
                9 => a
                    .checked_sub(b)
                    .map(Value::integer)
                    .unwrap_or_else(|| Value::number(a as f64 - b as f64)),
                10 => a
                    .checked_mul(b)
                    .map(Value::integer)
                    .unwrap_or_else(|| Value::number(a as f64 * b as f64)),
                11 => Value::number(a as f64 / b as f64),
                12 => a
                    .checked_rem(b)
                    .map(Value::integer)
                    .unwrap_or_else(|| Value::number(f64::NAN)),
                13 => Value::number((a as f64).powf(b as f64)),
                14 => Value::integer(a << (b as u32 & 31)),
                15 => Value::integer(a >> (b as u32 & 31)),
                16 => Value::number(((a as u32) >> (b as u32 & 31)) as f64),
                17 => Value::integer(a | b),
                18 => Value::integer(a ^ b),
                19 => Value::integer(a & b),
                _ => return Err(JsError(format!("unsupported binary operator {op}").into())),
            });
        }
        if op >= 8
            && (matches!(self.heap.get(left), Some(Cell::BigInt(_)))
                || matches!(self.heap.get(right), Some(Cell::BigInt(_))))
        {
            return self.binary_bigint(p, op, left, right);
        }
        self.binary_slow(p, op, left, right)
    }

    fn binary_bigint(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        let Some(Cell::BigInt(left)) = self.heap.get(left) else {
            return Err(self.type_error(p, "Cannot mix BigInt and other types".into()));
        };
        let Some(Cell::BigInt(right)) = self.heap.get(right) else {
            return Err(self.type_error(p, "Cannot mix BigInt and other types".into()));
        };
        let result = match op {
            8 => crate::bigint::binary(left, right, |a, b| Ok(a + b)),
            9 => crate::bigint::binary(left, right, |a, b| Ok(a - b)),
            10 => crate::bigint::binary(left, right, |a, b| Ok(a * b)),
            11 => crate::bigint::binary(left, right, |a, b| {
                if b == 0.into() {
                    Err(crate::bigint::Error::DivisionByZero)
                } else {
                    Ok(a / b)
                }
            }),
            12 => crate::bigint::binary(left, right, |a, b| {
                if b == 0.into() {
                    Err(crate::bigint::Error::DivisionByZero)
                } else {
                    Ok(a % b)
                }
            }),
            13 => crate::bigint::binary(left, right, |a, b| {
                if b.sign() == num_bigint::Sign::Minus {
                    return Err(crate::bigint::Error::NegativeExponent);
                }
                let exponent = b
                    .to_str_radix(10)
                    .parse::<u32>()
                    .map_err(|_| crate::bigint::Error::ExponentTooLarge)?;
                Ok(a.pow(exponent))
            }),
            14 => crate::bigint::shift(left, right, true),
            15 => crate::bigint::shift(left, right, false),
            17 => crate::bigint::binary(left, right, |a, b| Ok(a | b)),
            18 => crate::bigint::binary(left, right, |a, b| Ok(a ^ b)),
            19 => crate::bigint::binary(left, right, |a, b| Ok(a & b)),
            _ => return Err(self.type_error(p, "BigInt operation is not supported".into())),
        };
        match result {
            Ok(value) => Ok(self.heap.alloc(Cell::BigInt(value))),
            Err(crate::bigint::Error::DivisionByZero) => {
                Err(self.type_error(p, "Division by zero".into()))
            }
            Err(crate::bigint::Error::NegativeExponent) => {
                Err(self.type_error(p, "Negative exponent".into()))
            }
            Err(crate::bigint::Error::ExponentTooLarge | crate::bigint::Error::InvalidDecimal) => {
                Err(self.type_error(p, "Invalid BigInt operation".into()))
            }
        }
    }
    #[cold]
    #[inline(never)]
    pub(super) fn binary_slow(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        if let (Some(a), Some(b)) = (left.as_number(), right.as_number()) {
            return Ok(match op {
                0 | 2 => {
                    if a == b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                1 | 3 => {
                    if a != b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                4 => {
                    if a < b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                5 => {
                    if a <= b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                6 => {
                    if a > b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                7 => {
                    if a >= b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                8 => Value::number(a + b),
                9 => Value::number(a - b),
                10 => Value::number(a * b),
                11 => Value::number(a / b),
                12 => Value::number(a % b),
                13 => Value::number(a.powf(b)),
                14 => Value::number(((number_to_u32(a) as i32) << (number_to_u32(b) & 31)) as f64),
                15 => Value::number(((number_to_u32(a) as i32) >> (number_to_u32(b) & 31)) as f64),
                16 => Value::number((number_to_u32(a) >> (number_to_u32(b) & 31)) as f64),
                17 => Value::number(((number_to_u32(a) as i32) | (number_to_u32(b) as i32)) as f64),
                18 => Value::number(((number_to_u32(a) as i32) ^ (number_to_u32(b) as i32)) as f64),
                19 => Value::number(((number_to_u32(a) as i32) & (number_to_u32(b) as i32)) as f64),
                _ => return Err(JsError(format!("unsupported binary operator {op}").into())),
            });
        }
        if op == 8 && (self.is_string(left) || self.is_string(right)) {
            if let Some(value) = self.intern_dynamic_concat(left, right) {
                #[cfg(feature = "profile-aggregate")]
                self.profile.string_concat(true);
                return Ok(value);
            }
            #[cfg(feature = "profile-aggregate")]
            self.profile.string_concat(false);
            let mut text = self.coerce_js_string(p, left)?;
            text.push_js_string(&self.coerce_js_string(p, right)?);
            return Ok(self.intern_dynamic_value(text));
        }
        let answer = match op {
            0 => self.equal(p, left, right)?,
            1 => !self.equal(p, left, right)?,
            2 => self.strict_equal(left, right),
            3 => !self.strict_equal(left, right),
            4 => self.to_number(p, left)? < self.to_number(p, right)?,
            5 => self.to_number(p, left)? <= self.to_number(p, right)?,
            6 => self.to_number(p, left)? > self.to_number(p, right)?,
            7 => self.to_number(p, left)? >= self.to_number(p, right)?,
            20 => self.has_property(p, right, left)?,
            21 => self.instanceof(p, left, right)?,
            _ => return Ok(Value::number(self.numeric(p, op, left, right)?)),
        };
        Ok(if answer { Value::TRUE } else { Value::FALSE })
    }
    #[inline(always)]
    pub(super) fn binary_truthy(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<bool, JsError> {
        if op <= 7
            && let Some((a, b)) = Value::int_pair(left, right)
        {
            return Ok(match op {
                0 | 2 => a == b,
                1 | 3 => a != b,
                4 => a < b,
                5 => a <= b,
                6 => a > b,
                7 => a >= b,
                _ => unreachable!(),
            });
        }
        let value = self.binary(p, op, left, right)?;
        Ok(self.truthy(value))
    }
    pub(super) fn numeric(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<f64, JsError> {
        let a = self.to_number(p, left)?;
        let b = self.to_number(p, right)?;
        Ok(match op {
            8 => a + b,
            9 => a - b,
            10 => a * b,
            11 => a / b,
            12 => a % b,
            13 => a.powf(b),
            14 => ((number_to_u32(a) as i32) << (number_to_u32(b) & 31)) as f64,
            15 => ((number_to_u32(a) as i32) >> (number_to_u32(b) & 31)) as f64,
            16 => (number_to_u32(a) >> (number_to_u32(b) & 31)) as f64,
            17 => ((number_to_u32(a) as i32) | (number_to_u32(b) as i32)) as f64,
            18 => ((number_to_u32(a) as i32) ^ (number_to_u32(b) as i32)) as f64,
            19 => ((number_to_u32(a) as i32) & (number_to_u32(b) as i32)) as f64,
            _ => return Err(JsError(format!("unsupported binary operator {op}").into())),
        })
    }
}
