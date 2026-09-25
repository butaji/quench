use super::promise::PromiseState;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_atomics(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let atomics = self.object();
        for (name, native) in [
            ("load", Native::AtomicsLoad),
            ("store", Native::AtomicsStore),
            ("add", Native::AtomicsAdd),
            ("sub", Native::AtomicsSub),
            ("and", Native::AtomicsAnd),
            ("or", Native::AtomicsOr),
            ("xor", Native::AtomicsXor),
            ("exchange", Native::AtomicsExchange),
            ("compareExchange", Native::AtomicsCompareExchange),
            ("isLockFree", Native::AtomicsIsLockFree),
            ("notify", Native::AtomicsNotify),
            ("wait", Native::AtomicsWait),
            ("waitAsync", Native::AtomicsWaitAsync),
            ("pause", Native::AtomicsPause),
        ] {
            self.set_builtin_named(program, atomics, name, native)?;
        }
        self.global(program, "Atomics", atomics)
    }

    pub(super) fn atomics_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::AtomicsPause {
            return Ok(Value::UNDEFINED);
        }
        if native == Native::AtomicsIsLockFree {
            let size = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
            return Ok(if matches!(size, 1.0 | 2.0 | 4.0 | 8.0) {
                Value::TRUE
            } else {
                Value::FALSE
            });
        }
        if matches!(native, Native::AtomicsWait | Native::AtomicsWaitAsync) {
            return self.atomic_wait(p, native, args);
        }
        if native == Native::AtomicsNotify {
            return self.atomics_notify(p, args);
        }
        let view = args.first().copied().unwrap_or(Value::UNDEFINED);
        let kind = self.atomic_array_kind(p, view)?;
        if native != Native::AtomicsLoad {
            self.atomic_require_writable(p, view)?;
        }
        let length = self.typed_array_length(view).unwrap_or(0);
        let index = self.atomic_index(p, args.get(1).copied())?;
        self.atomic_validate_index(p, length, index)?;
        let current = self
            .typed_array_get(view, index)
            .unwrap_or(Value::UNDEFINED);
        match native {
            Native::AtomicsLoad => Ok(current),
            Native::AtomicsStore
            | Native::AtomicsAdd
            | Native::AtomicsSub
            | Native::AtomicsAnd
            | Native::AtomicsOr
            | Native::AtomicsXor
            | Native::AtomicsExchange
            | Native::AtomicsCompareExchange => {
                if matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64) {
                    self.atomic_bigint(p, native, view, index, current, args)
                } else {
                    self.atomic_number(p, native, kind, view, index, current, args)
                }
            }
            _ => unreachable!(),
        }
    }

    fn atomic_array_kind(
        &mut self,
        p: &ResidualProgram,
        view: Value,
    ) -> Result<TypedArrayKind, JsError> {
        let Some(kind) = self.typed_array_kind(view) else {
            return Err(self.type_error(p, "Atomics requires an integer typed array".into()));
        };
        if !matches!(
            kind,
            TypedArrayKind::Int8
                | TypedArrayKind::Uint8
                | TypedArrayKind::Int16
                | TypedArrayKind::Uint16
                | TypedArrayKind::Int32
                | TypedArrayKind::Uint32
                | TypedArrayKind::BigInt64
                | TypedArrayKind::BigUint64
        ) || self.typed_array_out_of_bounds(view)
        {
            return Err(
                self.type_error(p, "Atomics requires an attached integer typed array".into())
            );
        }
        Ok(kind)
    }

    fn atomic_index(
        &mut self,
        p: &ResidualProgram,
        value: Option<Value>,
    ) -> Result<usize, JsError> {
        let number = self.to_number(p, value.unwrap_or(Value::UNDEFINED))?;
        if number.is_nan() || number == 0.0 {
            return Ok(0);
        }
        let integer = number.trunc();
        if !integer.is_finite() || integer < 0.0 || integer >= usize::MAX as f64 {
            return Err(self.range_error(p, "Atomics index is out of range".into()));
        }
        Ok(integer as usize)
    }

    fn atomic_validate_index(
        &mut self,
        p: &ResidualProgram,
        length: usize,
        index: usize,
    ) -> Result<(), JsError> {
        if index >= length {
            return Err(self.range_error(p, "Atomics index is out of range".into()));
        }
        Ok(())
    }

    fn atomic_number(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        kind: TypedArrayKind,
        view: Value,
        index: usize,
        old: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let old_number = old.as_number().unwrap_or(0.0);
        let (next, store_result) = if native == Native::AtomicsCompareExchange {
            let expected = self.to_number(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
            let replacement =
                self.to_number(p, args.get(3).copied().unwrap_or(Value::UNDEFINED))?;
            let expected = self.atomic_number_value(kind, expected);
            let matches = self.atomic_number_value(kind, old_number) == expected;
            (matches.then_some(replacement), None)
        } else {
            let raw_input = self.to_number(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
            let input = self.atomic_number_value(kind, raw_input);
            let result = match native {
                Native::AtomicsStore => Some(if raw_input.is_nan() {
                    0.0
                } else if raw_input == 0.0 {
                    0.0
                } else {
                    raw_input.trunc()
                }),
                _ => None,
            };
            (
                Some(match native {
                    Native::AtomicsStore | Native::AtomicsExchange => input,
                    Native::AtomicsAdd => old_number + input,
                    Native::AtomicsSub => old_number - input,
                    Native::AtomicsAnd => self.atomic_number_value(
                        kind,
                        (Self::atomic_number_bits(kind, old_number)
                            & Self::atomic_number_bits(kind, input)) as f64,
                    ),
                    Native::AtomicsOr => self.atomic_number_value(
                        kind,
                        (Self::atomic_number_bits(kind, old_number)
                            | Self::atomic_number_bits(kind, input)) as f64,
                    ),
                    Native::AtomicsXor => self.atomic_number_value(
                        kind,
                        (Self::atomic_number_bits(kind, old_number)
                            ^ Self::atomic_number_bits(kind, input)) as f64,
                    ),
                    _ => unreachable!(),
                }),
                result,
            )
        };
        if let Some(next) = next {
            self.typed_array_set(p, view, index, Value::number(next))?;
        }
        if native == Native::AtomicsStore {
            Ok(Value::number(store_result.unwrap_or(0.0)))
        } else {
            Ok(old)
        }
    }

    fn atomic_number_value(&self, kind: TypedArrayKind, value: f64) -> f64 {
        match kind {
            TypedArrayKind::Int8 => Self::uint8_from_value(value) as i8 as f64,
            TypedArrayKind::Uint8 => Self::uint8_from_value(value) as f64,
            TypedArrayKind::Int16 => Self::uint16_from_value(value) as i16 as f64,
            TypedArrayKind::Uint16 => Self::uint16_from_value(value) as f64,
            TypedArrayKind::Int32 => Self::uint32_from_value(value) as i32 as f64,
            TypedArrayKind::Uint32 => Self::uint32_from_value(value) as f64,
            _ => value,
        }
    }

    fn atomic_number_bits(kind: TypedArrayKind, value: f64) -> u32 {
        if kind == TypedArrayKind::Uint32 {
            value as u32
        } else {
            value as i32 as u32
        }
    }

    fn atomic_bigint(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        view: Value,
        index: usize,
        old: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::BigInt(old_text)) = self.heap.get(old).cloned() else {
            return Err(self.type_error(p, "Atomics BigInt element is invalid".into()));
        };
        let old_number = old_text
            .parse::<num_bigint::BigInt>()
            .map_err(|_| self.type_error(p, "Atomics BigInt element is invalid".into()))?;
        let first = self.to_bigint(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
        let replacement = if native == Native::AtomicsCompareExchange {
            self.to_bigint(p, args.get(3).copied().unwrap_or(Value::UNDEFINED))?
        } else {
            first.clone()
        };
        let compare = native == Native::AtomicsCompareExchange
            && self.normalize_atomic_bigint(first.clone())
                == self.normalize_atomic_bigint(old_number.clone());
        if native != Native::AtomicsCompareExchange || compare {
            let next = match native {
                Native::AtomicsStore | Native::AtomicsExchange => replacement,
                Native::AtomicsAdd => old_number.clone() + first.clone(),
                Native::AtomicsSub => old_number.clone() - first.clone(),
                Native::AtomicsAnd => old_number.clone() & first.clone(),
                Native::AtomicsOr => old_number.clone() | first.clone(),
                Native::AtomicsXor => old_number.clone() ^ first.clone(),
                Native::AtomicsCompareExchange => replacement,
                _ => unreachable!(),
            };
            let value = self.heap.alloc(Cell::BigInt(next.to_string()));
            self.typed_array_set(p, view, index, value)?;
        }
        if native == Native::AtomicsStore {
            Ok(self.heap.alloc(Cell::BigInt(first.to_string())))
        } else {
            Ok(old)
        }
    }

    fn normalize_atomic_bigint(&self, value: num_bigint::BigInt) -> num_bigint::BigInt {
        let modulus = num_bigint::BigInt::from(1_u8) << 64;
        ((value % &modulus) + &modulus) % modulus
    }

    fn atomic_require_writable(&mut self, p: &ResidualProgram, view: Value) -> Result<(), JsError> {
        if let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(view)
            && matches!(
                self.heap.get(*buffer),
                Some(Cell::ArrayBuffer {
                    immutable: true,
                    ..
                })
            )
        {
            return Err(self.type_error(p, "Atomics operation requires a writable buffer".into()));
        }
        Ok(())
    }

    fn atomic_wait(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let view = args.first().copied().unwrap_or(Value::UNDEFINED);
        let kind = self.atomic_array_kind(p, view)?;
        if !matches!(kind, TypedArrayKind::Int32 | TypedArrayKind::BigInt64)
            || self.typed_array_shared(view) != Some(true)
        {
            return Err(self.type_error(
                p,
                "Atomics wait requires shared Int32Array or BigInt64Array".into(),
            ));
        }
        let length = self.typed_array_length(view).unwrap_or(0);
        let index = self.atomic_index(p, args.get(1).copied())?;
        self.atomic_validate_index(p, length, index)?;
        let expected = if kind == TypedArrayKind::Int32 {
            let number = self.to_number(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
            Value::number(self.atomic_number_value(kind, number))
        } else {
            let value = self.to_bigint(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
            self.heap.alloc(Cell::BigInt(
                self.normalize_atomic_bigint(value).to_string(),
            ))
        };
        let current = self
            .typed_array_get(view, index)
            .unwrap_or(Value::UNDEFINED);
        let equal = if kind == TypedArrayKind::Int32 {
            current.as_number() == expected.as_number()
        } else {
            let current = self.to_bigint(p, current)?;
            let expected = self.to_bigint(p, expected)?;
            self.normalize_atomic_bigint(current) == self.normalize_atomic_bigint(expected)
        };
        let timeout = self.to_number(p, args.get(3).copied().unwrap_or(Value::UNDEFINED))?;
        let state = if !equal { "not-equal" } else { "timed-out" };
        if native == Native::AtomicsWait {
            if equal && !self.host.can_block() {
                return Err(self.type_error(p, "Atomics.wait cannot block in this agent".into()));
            }
            return Ok(self.heap.alloc(Cell::String(state.into())));
        }
        let is_async = equal && (timeout.is_nan() || timeout > 0.0);
        let value = if is_async {
            let promise = self.promise_object();
            let timeout_result = self.heap.alloc(Cell::String("timed-out".into()));
            self.promise_settle(p, promise, PromiseState::Fulfilled, timeout_result)?;
            promise
        } else {
            self.heap.alloc(Cell::String(state.into()))
        };
        let result = self.object();
        self.set_named(
            p,
            result,
            "async",
            if is_async { Value::TRUE } else { Value::FALSE },
        )?;
        self.set_named(p, result, "value", value)?;
        Ok(result)
    }

    fn atomics_notify(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let view = args.first().copied().unwrap_or(Value::UNDEFINED);
        let kind = self.atomic_array_kind(p, view)?;
        if !matches!(kind, TypedArrayKind::Int32 | TypedArrayKind::BigInt64) {
            return Err(self.type_error(
                p,
                "Atomics.notify requires Int32Array or BigInt64Array".into(),
            ));
        }
        let length = self.typed_array_length(view).unwrap_or(0);
        let index = self.atomic_index(p, args.get(1).copied())?;
        self.atomic_validate_index(p, length, index)?;
        if let Some(count) = args.get(2).copied() {
            let _ = self.to_number(p, count)?;
        }
        if self.typed_array_shared(view) != Some(true) {
            return Ok(Value::number(0.0));
        }
        Ok(Value::number(0.0))
    }
}
