//! Exception identity and payload use the shared heap and typed error transport.
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn wasm_exception_new(
        &mut self,
        tag: Value,
        payload: Vec<Value>,
    ) -> Result<Value, JsError> {
        let Some(Cell::WasmTag { declarations, ty, .. }) = self.heap.get(tag) else {
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
        Ok(self.heap.alloc(Cell::WasmException {
            tag,
            payload: Box::new(payload),
        }))
    }

    pub(super) fn wasm_exception_matches(
        &self,
        exception: Value,
        expected: Value,
    ) -> Result<bool, JsError> {
        if !matches!(self.heap.get(expected), Some(Cell::WasmTag { .. })) {
            return Err(JsError::validation("invalid Wasm catch tag".into()));
        }
        match self.heap.get(exception) {
            Some(Cell::WasmException { tag, .. }) => Ok(*tag == expected),
            // A JavaScript exception is caught by `WebAssembly.JSTag` alone.
            _ => Ok(expected == self.wasm_js.js_tag),
        }
    }

    pub(super) fn wasm_exception_payload(
        &self,
        exception: Value,
        index: u32,
    ) -> Result<Value, JsError> {
        let Some(Cell::WasmException { payload, .. }) = self.heap.get(exception) else {
            // `WebAssembly.JSTag`'s one `externref` parameter is the thrown value itself.
            return (index == 0)
                .then_some(exception)
                .ok_or_else(|| JsError::validation("Wasm exception payload index out of bounds".into()));
        };
        payload
            .get(index as usize)
            .copied()
            .ok_or_else(|| JsError::validation("Wasm exception payload index out of bounds".into()))
    }

    pub(super) fn wasm_throw_ref(&self, exception: Value) -> JsError {
        match self.heap.get(exception) {
            _ if exception.is_null() => {
                JsError::wasm_trap_error(crate::WasmTrap::NullExceptionReference)
            }
            // An exception of `WebAssembly.JSTag` reaches JavaScript as its payload.
            Some(Cell::WasmException { tag, payload }) if *tag == self.wasm_js.js_tag => {
                let value = payload.first().copied().unwrap_or(Value::UNDEFINED);
                JsError::thrown(value, "uncaught exception".into())
            }
            Some(Cell::WasmException { .. }) => JsError::thrown_wasm_exception(exception),
            // A JavaScript exception a Wasm handler caught by reference is rethrown as itself.
            _ => JsError::thrown(exception, "uncaught exception".into()),
        }
    }
}

impl<H: Host> Vm<H> {
    /// `new WebAssembly.Tag({ parameters })`: a fresh tag whose payload has those types.
    pub(super) fn construct_wasm_tag(
        &mut self,
        p: &ResidualProgram,
        descriptor: Value,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(descriptor) {
            return Err(self.type_error(p, "WebAssembly.Tag(): descriptor must be an object".into()));
        }
        let atom = self.intern_atom("parameters");
        let parameters = self.get_property(p, descriptor, atom)?;
        let parameters = self.create_list_from_array_like(p, parameters, super::operations::ArrayLikeElementKind::Any)?;
        let mut types = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            let name = self.to_string(p, parameter)?;
            let Some(ty) = tag_parameter_type(&name) else {
                return Err(self.type_error(p, "WebAssembly.Tag(): invalid parameter type".into()));
            };
            types.push(ty);
        }
        let declarations = crate::WasmTypes::from_functions([wasmparser::FuncType::new(types, [])]);
        Ok(self.heap.alloc(Cell::WasmTag { object: Box::new(Self::empty_object(self.wasm_js.tag)), declarations, ty: 0 }))
    }
}

/// The value types a `WebAssembly.Tag` descriptor's parameters name.
fn tag_parameter_type(name: &str) -> Option<wasmparser::ValType> {
    Some(match name {
        "i32" => wasmparser::ValType::I32,
        "i64" => wasmparser::ValType::I64,
        "f32" => wasmparser::ValType::F32,
        "f64" => wasmparser::ValType::F64,
        "v128" => wasmparser::ValType::V128,
        _ => return super::wasm_js::descriptor_reference_type(name).map(wasmparser::ValType::Ref),
    })
}

/// `WebAssembly.JSTag`'s type: a tag whose one `externref` parameter carries a JavaScript
/// exception through Wasm. Every realm makes the same one.
pub(crate) fn js_tag_type() -> (crate::WasmTypes, u32) {
    let declarations = crate::WasmTypes::from_functions([wasmparser::FuncType::new(
        [wasmparser::ValType::EXTERNREF],
        [],
    )]);
    (declarations, 0)
}

/// `WebAssembly.JSTag`'s cell (`js_tag_type`), with its object part.
pub(crate) fn js_tag_cell(object: Box<crate::heap::Object>) -> Cell {
    let (declarations, ty) = js_tag_type();
    Cell::WasmTag { object, declarations, ty }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `new WebAssembly.Tag` reads its descriptor's parameter types; an unknown one, or a
    /// descriptor that is no object, is a TypeError.
    #[test]
    fn constructed_tags_read_their_parameter_types() {
        let source = r#"
            new WebAssembly.Tag({ parameters: ['i32', 'i64', 'f32', 'f64', 'v128', 'externref', 'anyfunc'] });
            for (const descriptor of [{ parameters: ['none'] }, 1]) {
                let typed = true;
                try { new WebAssembly.Tag(descriptor); } catch (error) { typed = !(error instanceof TypeError); }
                if (typed) throw 'an invalid descriptor was accepted';
            }
        "#;
        let program = crate::Engine::specialize(source, "tag.js").unwrap();
        crate::Runtime::new(crate::test_host::TestHost::default()).execute(&program).unwrap();
    }

    #[test]
    fn exception_transport_keeps_identity_classification_and_owned_payload_through_gc() {
        let mut vm = Vm::new(crate::SystemHost);
        let declarations = crate::WasmTypes::from_functions([wasmparser::FuncType::new(
            [wasmparser::ValType::I64],
            [],
        )]);
        let tag = vm.heap.alloc(Cell::WasmTag { object: Box::new(crate::heap::Object::new(Value::NULL)), declarations, ty: 0 });
        let bits = vm.heap.alloc(Cell::WasmBits64(0x7ffc_1234_5678_9abc));
        let exception = vm.wasm_exception_new(tag, vec![bits]).unwrap();
        let handle = vm.root(exception);
        vm.heap.collect([exception]);
        assert!(vm.heap.is_live(tag));
        assert!(vm.heap.is_live(bits));
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
        assert!(!vm.heap.is_live(exception));
        assert!(!vm.heap.is_live(tag));
        assert!(!vm.heap.is_live(bits));
    }
}
