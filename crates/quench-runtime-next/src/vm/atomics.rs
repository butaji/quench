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
        if native == Native::AtomicsIsLockFree {
            let size = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
            return Ok(if matches!(size, 1.0 | 2.0 | 4.0 | 8.0) {
                Value::TRUE
            } else {
                Value::FALSE
            });
        }
        let view = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.typed_array_shared(view) != Some(true)
            || self.typed_array_kind(view) != Some(TypedArrayKind::Uint8)
        {
            return Err(JsError(
                "Atomics operation requires a shared Uint8Array".into(),
            ));
        }
        let index_number = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        if index_number.is_nan() || index_number.is_sign_negative() {
            return Err(JsError("Atomics index is invalid".into()));
        }
        let index = index_number.trunc() as usize;
        if self
            .typed_array_length(view)
            .is_none_or(|length| index >= length)
        {
            return Err(JsError("Atomics index is out of range".into()));
        }
        let Some(current) = self.typed_array_get(view, index) else {
            return Err(JsError("Atomics receiver is invalid".into()));
        };
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
                let old = current.as_number().unwrap_or(0.0);
                let (next, should_store) = if native == Native::AtomicsCompareExchange {
                    let expected =
                        self.to_number(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
                    let replacement =
                        self.to_number(p, args.get(3).copied().unwrap_or(Value::UNDEFINED))?;
                    (Self::uint8_from_value(replacement), old == expected)
                } else {
                    let input =
                        self.to_number(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
                    let next = match native {
                        Native::AtomicsStore => Self::uint8_from_value(input),
                        Native::AtomicsAdd => Self::uint8_from_value(old + input),
                        Native::AtomicsSub => Self::uint8_from_value(old - input),
                        Native::AtomicsAnd => (old as u8) & Self::uint8_from_value(input),
                        Native::AtomicsOr => (old as u8) | Self::uint8_from_value(input),
                        Native::AtomicsXor => (old as u8) ^ Self::uint8_from_value(input),
                        Native::AtomicsExchange => Self::uint8_from_value(input),
                        _ => unreachable!(),
                    };
                    (next, true)
                };
                if should_store {
                    self.typed_array_set(p, view, index, Value::number(next as f64))?;
                }
                if native == Native::AtomicsStore {
                    Ok(Value::number(next as f64))
                } else {
                    Ok(current)
                }
            }
            _ => unreachable!(),
        }
    }
}
