use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_atomics(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let atomics = self.object();
        for (name, native) in [
            ("load", Native::AtomicsLoad),
            ("store", Native::AtomicsStore),
            ("add", Native::AtomicsAdd),
            ("isLockFree", Native::AtomicsIsLockFree),
        ] {
            self.set_named(program, atomics, name, self.native_value(native))?;
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
        if self.typed_array_shared(view) != Some(true) {
            return Err(JsError(
                "Atomics operation requires a shared Uint8Array".into(),
            ));
        }
        let index_number = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        if index_number.is_nan() || index_number.is_sign_negative() {
            return Err(JsError("Atomics index is invalid".into()));
        }
        let index = index_number.trunc() as usize;
        let Some(current) = self.typed_array_get(view, index) else {
            return Err(JsError("Atomics receiver is invalid".into()));
        };
        match native {
            Native::AtomicsLoad => Ok(current),
            Native::AtomicsStore | Native::AtomicsAdd => {
                let input = self.to_number(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
                let old = current.as_number().unwrap_or(0.0);
                let next = if native == Native::AtomicsAdd {
                    Self::uint8_from_value(old + input)
                } else {
                    Self::uint8_from_value(input)
                };
                self.typed_array_set(p, view, index, Value::number(next as f64))?;
                if native == Native::AtomicsAdd {
                    Ok(current)
                } else {
                    Ok(Value::number(next as f64))
                }
            }
            _ => unreachable!(),
        }
    }
}
