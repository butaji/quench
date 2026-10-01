use super::*;

impl<H: Host> Vm<H> {
    pub(crate) fn execute_wasm_i32(
        &mut self,
        function: &crate::WasmI32Function,
        args: &[i32],
    ) -> Result<Option<i32>, JsError> {
        let program = &function.program;
        program.validate().map_err(JsError::validation)?;
        if args.len() != usize::from(program.functions[0].params) {
            return Err(JsError::validation("Wasm argument count mismatch".into()));
        }
        self.initialize(program)?;
        let args = args.iter().copied().map(Value::integer).collect::<Vec<_>>();
        let result = self.call_user(program, 0, Value::NULL, Value::UNDEFINED, &args)?;
        if function.has_result {
            result
                .as_int()
                .map(Some)
                .ok_or_else(|| JsError::validation("invalid Wasm i32 result".into()))
        } else {
            Ok(None)
        }
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
