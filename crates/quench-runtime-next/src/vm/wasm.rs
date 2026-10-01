use super::*;

impl<H: Host> Vm<H> {
    pub(crate) fn execute_wasm_i32(
        &mut self,
        function: &crate::WasmI32Function,
        args: &[i32],
    ) -> Result<Option<i32>, JsError> {
        if function
            .signature
            .params
            .iter()
            .any(|ty| *ty != crate::WasmType::I32)
            || function
                .signature
                .result
                .is_some_and(|ty| ty != crate::WasmType::I32)
        {
            return Err(JsError::validation(
                "function requires the typed Wasm boundary".into(),
            ));
        }
        let args: Vec<_> = args.iter().copied().map(crate::WasmValue::I32).collect();
        self.execute_wasm(function, &args).map(|result| {
            result.map(|value| {
                let crate::WasmValue::I32(value) = value else {
                    unreachable!("checked i32 signature")
                };
                value
            })
        })
    }

    pub(crate) fn execute_wasm(
        &mut self,
        function: &crate::WasmFunction,
        args: &[crate::WasmValue],
    ) -> Result<Option<crate::WasmValue>, JsError> {
        let program = &function.program;
        program.validate().map_err(JsError::validation)?;
        if args.len() != function.signature.params.len() {
            return Err(JsError::validation("Wasm argument count mismatch".into()));
        }
        if args
            .iter()
            .zip(&function.signature.params)
            .any(|(value, ty)| value.ty() != *ty)
        {
            return Err(JsError::validation("Wasm argument type mismatch".into()));
        }
        self.initialize(program)?;
        let args = args
            .iter()
            .copied()
            .map(|value| self.encode_wasm_scalar(value))
            .collect::<Vec<_>>();
        let result = self.call_user(
            program,
            function.entry,
            Value::NULL,
            Value::UNDEFINED,
            &args,
        )?;
        function
            .signature
            .result
            .map(|ty| self.decode_wasm_scalar(result, ty))
            .transpose()
    }

    pub(super) fn wasm_i64_operand(&self, value: Value) -> Result<i64, JsError> {
        let crate::WasmValue::I64(value) = self.decode_wasm_scalar(value, crate::WasmType::I64)?
        else {
            unreachable!("decoded i64 operand")
        };
        Ok(value)
    }

    pub(super) fn wasm_f32_operand(&self, value: Value) -> Result<f32, JsError> {
        let crate::WasmValue::F32(bits) = self.decode_wasm_scalar(value, crate::WasmType::F32)?
        else {
            unreachable!("decoded f32 operand")
        };
        Ok(f32::from_bits(bits))
    }

    pub(super) fn wasm_f64_operand(&self, value: Value) -> Result<f64, JsError> {
        let crate::WasmValue::F64(bits) = self.decode_wasm_scalar(value, crate::WasmType::F64)?
        else {
            unreachable!("decoded f64 operand")
        };
        Ok(f64::from_bits(bits))
    }

    pub(super) fn encode_wasm_scalar(&mut self, value: crate::WasmValue) -> Value {
        match value.bits() {
            crate::wasm::ScalarBits::Bits32(bits) => Value::integer(bits as i32),
            crate::wasm::ScalarBits::Bits64(bits) => self.heap.alloc(Cell::WasmBits64(bits)),
        }
    }

    pub(super) fn decode_wasm_scalar(
        &self,
        value: Value,
        ty: crate::WasmType,
    ) -> Result<crate::WasmValue, JsError> {
        let bits = if let Some(bits) = value.as_int() {
            Some(crate::wasm::ScalarBits::Bits32(bits as u32))
        } else if let Some(Cell::WasmBits64(bits)) = self.heap.get(value) {
            Some(crate::wasm::ScalarBits::Bits64(*bits))
        } else {
            None
        };
        bits.and_then(|bits| ty.decode(bits))
            .ok_or_else(|| JsError::validation("invalid Wasm scalar representation".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmparser::Operator;

    #[test]
    fn unreachable_trap_unwinds_the_shared_activation() {
        let function = crate::Engine::lower_wasm_i32_function(
            "unreachable-unwind",
            0,
            0,
            true,
            [Operator::Unreachable, Operator::End].into_iter().map(Ok),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(
            vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        assert!(vm.frames.is_empty());
        vm.collect_now(function.residual());
    }

    #[test]
    fn nested_call_trap_unwinds_every_shared_activation() {
        let function = crate::Engine::lower_wasm_i32_module(
            "nested-unwind",
            1,
            [
                (0, 0, true, vec![Operator::Unreachable, Operator::End]),
                (
                    0,
                    0,
                    true,
                    vec![Operator::Call { function_index: 0 }, Operator::End],
                ),
            ]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(
            vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        assert!(vm.frames.is_empty());
        vm.collect_now(function.residual());
    }

    #[test]
    fn integer_traps_unwind_the_shared_activation() {
        let operators = [
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32DivS,
            Operator::End,
        ];
        let function = crate::Engine::lower_wasm_i32_function(
            "trap-unwind",
            2,
            0,
            true,
            operators.into_iter().map(Ok),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        for args in [[1, 0], [i32::MIN, -1]] {
            assert!(
                vm.execute_wasm_i32(&function, &args)
                    .unwrap_err()
                    .wasm_trap()
                    .is_some()
            );
            assert!(vm.frames.is_empty());
            vm.collect_now(function.residual());
        }
        assert_eq!(vm.execute_wasm_i32(&function, &[7, 2]).unwrap(), Some(3));
        assert!(vm.frames.is_empty());
    }
}
