//! Exception identity and payload use the shared heap and typed error transport.
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn wasm_exception_new(
        &mut self,
        tag: Value,
        payload: Vec<Value>,
    ) -> Result<Value, JsError> {
        let Some(Cell::WasmTag { declarations, ty }) = self.heap.get(tag) else {
            return Err(JsError::validation("invalid Wasm exception tag".into()));
        };
        let shape = declarations
            .function("wasm-exception", *ty as usize)
            .map_err(|error| JsError::validation(error.to_string()))?;
        if shape.params().len() != payload.len() {
            return Err(JsError::validation(
                "Wasm exception payload arity mismatch".into(),
            ));
        }
        for (&value, &ty) in payload.iter().zip(shape.params()) {
            let ty = declarations.callable_value_type(ty).ok_or_else(|| {
                JsError::validation("unsupported Wasm exception payload type".into())
            })?;
            self.decode_wasm_value_in(value, ty, Some(declarations))?;
        }
        Ok(self.heap.alloc(Cell::WasmException { tag, payload }))
    }

    pub(super) fn wasm_exception_matches(
        &self,
        exception: Value,
        expected: Value,
    ) -> Result<bool, JsError> {
        if !matches!(self.heap.get(expected), Some(Cell::WasmTag { .. })) {
            return Err(JsError::validation("invalid Wasm catch tag".into()));
        }
        let Some(Cell::WasmException { tag, .. }) = self.heap.get(exception) else {
            return Err(JsError::validation(
                "invalid Wasm exception reference".into(),
            ));
        };
        Ok(*tag == expected)
    }

    pub(super) fn wasm_exception_payload(
        &self,
        exception: Value,
        index: u32,
    ) -> Result<Value, JsError> {
        let Some(Cell::WasmException { payload, .. }) = self.heap.get(exception) else {
            return Err(JsError::validation(
                "invalid Wasm exception reference".into(),
            ));
        };
        payload
            .get(index as usize)
            .copied()
            .ok_or_else(|| JsError::validation("Wasm exception payload index out of bounds".into()))
    }

    pub(super) fn wasm_throw_ref(&self, exception: Value) -> JsError {
        if exception.is_null() {
            JsError::wasm_trap_error(crate::WasmTrap::NullExceptionReference)
        } else if matches!(self.heap.get(exception), Some(Cell::WasmException { .. })) {
            JsError::thrown_wasm_exception(exception)
        } else {
            JsError::validation("invalid Wasm exception reference".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_transport_keeps_identity_classification_and_owned_payload_through_gc() {
        let mut vm = Vm::new(crate::SystemHost);
        let declarations = crate::WasmTypes::from_functions([wasmparser::FuncType::new(
            [wasmparser::ValType::I64],
            [],
        )]);
        let tag = vm.heap.alloc(Cell::WasmTag {
            declarations,
            ty: 0,
        });
        let bits = vm.heap.alloc(Cell::WasmBits64(0x7ffc_1234_5678_9abc));
        let exception = vm.wasm_exception_new(tag, vec![bits]).unwrap();
        let handle = vm.root(exception);
        vm.heap.collect([exception]);
        assert!(vm.heap.get(tag).is_some());
        assert!(vm.heap.get(bits).is_some());
        let thrown = vm.wasm_throw_ref(exception);
        assert_eq!(thrown.wasm_exception(), Some(exception));
        assert_eq!(thrown.thrown_value(), Some(exception));
        assert_eq!(thrown.wasm_trap(), None);
        assert_eq!(
            vm.wasm_throw_ref(exception).wasm_exception(),
            Some(exception)
        );
        // Transport category follows the throwing operation, not payload shape.
        assert_eq!(
            JsError::thrown(exception, "JS throw".into()).wasm_exception(),
            None
        );
        assert_eq!(
            vm.wasm_throw_ref(Value::NULL).wasm_trap(),
            Some(crate::WasmTrap::NullExceptionReference)
        );
        for (heap, admitted) in [
            (wasmparser::AbstractHeapType::Exn, true),
            (wasmparser::AbstractHeapType::Any, false),
            (wasmparser::AbstractHeapType::Eq, false),
        ] {
            let ty = crate::WasmType::Reference {
                kind: crate::WasmReferenceKind::Internal(heap),
                nullable: false,
            };
            assert_eq!(vm.decode_wasm_value(exception, ty).is_ok(), admitted);
        }
        vm.release_root(handle);
        vm.heap.collect([]);
        assert!(vm.heap.get(exception).is_none());
        assert!(vm.heap.get(tag).is_none());
        assert!(vm.heap.get(bits).is_none());
    }
}
