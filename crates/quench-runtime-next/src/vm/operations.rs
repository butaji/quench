use super::*;
use crate::host::{CapabilityId, HostContext};
impl<H: Host> Vm<H> {
    pub(super) fn construct_value(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let kind = match self.heap.get(callee) {
            Some(Cell::Function { kind, .. }) => *kind,
            _ => return Err(JsError("not a constructor".into())),
        };
        if let FunctionKind::Native(native) = kind {
            return self.construct_native(p, native, args);
        }
        let proto = self
            .lookup_atom("prototype")
            .and_then(|a| self.own_property(callee, a))
            .unwrap_or(Value::NULL);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(proto)));
        let result = self.call_value(p, callee, object, args)?;
        Ok(if result.is_heap() { result } else { object })
    }
    pub(super) fn construct_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::Object => {
                if let Some(value) = args.first().copied()
                    && self.object_data(value).is_some()
                {
                    return Ok(value);
                }
                Ok(self.object())
            }
            Native::Array => {
                let len = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
                Ok(self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(vec![Value::UNDEFINED; len]),
                }))
            }
            Native::ArrayBuffer => self.construct_array_buffer_native(p, args),
            Native::Map | Native::Set => self.construct_collection_native(native, args),
            Native::WeakMap | Native::WeakSet => self.construct_weak_collection_native(native),
            Native::WeakRef => self.construct_weak_ref_native(args),
            Native::Date => Ok(self.heap.alloc(Cell::Date(
                HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None),
            ))),
            Native::Error => {
                let v = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, v)?;
                Ok(self.heap.alloc(Cell::Error(text)))
            }
            Native::String => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, value)?;
                Ok(self.heap.alloc(Cell::String(text)))
            }
            Native::Number => Ok(Value::number(
                self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?,
            )),
            _ => Err(JsError("native is not constructible".into())),
        }
    }
    pub(super) fn call_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if Self::is_collection_native(native) {
            return self.call_collection_native(native, this, args);
        }
        match native {
            Native::Print => {
                let v = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, v)?;
                HostContext::new(&mut self.host).invoke(CapabilityId::WriteLine, Some(&text));
                Ok(Value::UNDEFINED)
            }
            Native::DateNow => Ok(Value::number(
                HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None),
            )),
            Native::ObjectKeys
            | Native::ObjectCreate
            | Native::ObjectAssign
            | Native::ObjectGetPrototypeOf
            | Native::ObjectSetPrototypeOf => self.call_object_native(native, args),
            Native::ReflectGet
            | Native::ReflectSet
            | Native::ReflectOwnKeys
            | Native::ReflectGetPrototypeOf
            | Native::ReflectSetPrototypeOf
            | Native::ReflectConstruct => self.call_reflect_native(p, native, args),
            Native::JsonParse => self.json_parse(p, args),
            Native::JsonStringify => self.json_stringify(p, args),
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
            Native::ArrayFrom | Native::ArrayOf => self.array_modern_native(p, native, this, args),
            Native::FunctionCall => {
                let receiver = args.first().copied().unwrap_or(Value::UNDEFINED);
                let receiver = if receiver.is_null() || receiver.is_undefined() {
                    self.globals
                } else {
                    receiver
                };
                self.call_value(p, this, receiver, args.get(1..).unwrap_or_default())
            }
            Native::FunctionApply => {
                let receiver = args.first().copied().unwrap_or(Value::UNDEFINED);
                let receiver = if receiver.is_null() || receiver.is_undefined() {
                    self.globals
                } else {
                    receiver
                };
                let argument_array = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let arguments = if argument_array.is_undefined() {
                    vec![]
                } else {
                    match self.heap.get(argument_array) {
                        Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
                        _ => return Err(JsError("apply arguments must be an array".into())),
                    }
                };
                self.call_value(p, this, receiver, &arguments)
            }
            Native::Number
            | Native::NumberIsNaN
            | Native::NumberIsFinite
            | Native::NumberIsInteger => self.call_number_native(p, native, args),
            Native::NumberFixed => {
                let number = self.to_number(p, this)?;
                let digits = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
                Ok(self.heap.alloc(Cell::String(format!("{number:.digits$}"))))
            }
            Native::NumberPrecision => {
                let number = self.to_number(p, this)?;
                let digits = args.first().and_then(|v| v.as_number()).unwrap_or(3.0) as usize;
                Ok(self.heap.alloc(Cell::String(format!("{number:.digits$}"))))
            }
            Native::String => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, value)?;
                Ok(self.heap.alloc(Cell::String(text)))
            }
            Native::Symbol => {
                let description = match args.first().copied() {
                    None | Some(Value::UNDEFINED) => None,
                    Some(value) => Some(self.to_string(p, value)?),
                };
                Ok(self.heap.alloc(Cell::Symbol(description)))
            }
            Native::SymbolFor | Native::SymbolKeyFor => self.call_symbol_native(p, native, args),
            Native::Date => Ok(self.heap.alloc(Cell::Date(
                HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None),
            ))),
            Native::Object
            | Native::Array
            | Native::Error
            | Native::Map
            | Native::Set
            | Native::WeakMap
            | Native::WeakSet
            | Native::WeakRef => self.construct_native(p, native, args),
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
        self.binary_slow(p, op, left, right)
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
            let text = self.to_string(p, left)? + &self.to_string(p, right)?;
            return Ok(self.intern_dynamic_string(text));
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
    pub(super) fn equal(
        &mut self,
        p: &ResidualProgram,
        a: Value,
        b: Value,
    ) -> Result<bool, JsError> {
        if a == b {
            return Ok(true);
        }
        if let (Some(Cell::String(a)), Some(Cell::String(b))) = (self.heap.get(a), self.heap.get(b))
        {
            return Ok(a == b);
        }
        if a.is_null() && b.is_undefined() || a.is_undefined() && b.is_null() {
            return Ok(true);
        }
        if a.is_null() || a.is_undefined() || b.is_null() || b.is_undefined() {
            return Ok(false);
        }
        let a_number = a.as_number().is_some();
        let b_number = b.as_number().is_some();
        if a_number || b_number {
            return Ok(self.to_number(p, a)? == self.to_number(p, b)?);
        }
        if a.as_bool().is_some() || b.as_bool().is_some() {
            return Ok(self.to_number(p, a)? == self.to_number(p, b)?);
        }
        Ok(false)
    }
    fn strict_equal(&self, a: Value, b: Value) -> bool {
        if a == b {
            return true;
        }
        matches!(
            (self.heap.get(a), self.heap.get(b)),
            (Some(Cell::String(a)), Some(Cell::String(b))) if a == b
        )
    }
    #[inline(always)]
    pub(super) fn truthy(&self, v: Value) -> bool {
        !(v.is_null()
            || v.is_undefined()
            || v == Value::FALSE
            || v.as_number().is_some_and(|n| n == 0.0 || n.is_nan()))
    }
}
