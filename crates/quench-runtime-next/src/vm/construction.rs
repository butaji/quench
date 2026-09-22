use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn is_constructable(&self, p: &ResidualProgram, value: Value) -> bool {
        let Some(cell) = self.heap.get(value) else {
            return false;
        };
        match cell {
            Cell::Proxy {
                target, handler, ..
            } => !handler.is_null() && self.is_constructable(p, *target),
            Cell::Function { kind, .. } => match kind {
                FunctionKind::User(id) | FunctionKind::NumericUser(id) => {
                    let function = &p.functions[*id as usize];
                    !function.is_async
                        && !function.is_generator
                        && self
                            .lookup_atom("prototype")
                            .is_some_and(|atom| self.own_property(value, atom).is_some())
                }
                FunctionKind::Native(native) => matches!(
                    native,
                    Native::Function
                        | Native::Object
                        | Native::Proxy
                        | Native::Array
                        | Native::ArrayBuffer
                        | Native::SharedArrayBuffer
                        | Native::Uint8Array
                        | Native::Uint8ClampedArray
                        | Native::Uint16Array
                        | Native::Uint32Array
                        | Native::Int8Array
                        | Native::Int16Array
                        | Native::Int32Array
                        | Native::Float32Array
                        | Native::Float64Array
                        | Native::DataView
                        | Native::Map
                        | Native::Set
                        | Native::WeakMap
                        | Native::WeakSet
                        | Native::WeakRef
                        | Native::FinalizationRegistry
                        | Native::DisposableStack
                        | Native::Date
                        | Native::Error
                        | Native::EvalError
                        | Native::RangeError
                        | Native::ReferenceError
                        | Native::SyntaxError
                        | Native::TypeError
                        | Native::URIError
                        | Native::RegExp
                        | Native::String
                        | Native::Boolean
                        | Native::Number
                        | Native::Promise
                ),
            },
            _ => false,
        }
    }

    pub(super) fn closure(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
    ) -> Result<Value, JsError> {
        let prototype = self.object();
        let function = self.heap.alloc(Cell::Function {
            object: Box::new(Self::empty_object(self.function_proto)),
            kind: match p.functions[id as usize].dispatch {
                DispatchClass::General => FunctionKind::User(id),
                DispatchClass::Numeric => FunctionKind::NumericUser(id),
            },
            env,
        });
        let arrow = p.functions[id as usize]
            .name
            .is_some_and(|name| p.atoms[name as usize].as_bytes() == b"\0rqj:arrow");
        if !arrow {
            if let Some(atom) = self.lookup_atom("prototype") {
                self.set_property(function, atom, prototype)?;
            }
        }
        let constructor_atom = self.intern_atom("constructor");
        self.set_property(prototype, constructor_atom, function)?;
        Ok(function)
    }

    pub(super) fn construct_value(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if matches!(self.heap.get(callee), Some(Cell::Proxy { .. })) {
            return self.proxy_construct(p, callee, args);
        }
        let kind = match self.heap.get(callee) {
            Some(Cell::Function { kind, .. }) => *kind,
            _ => return Err(JsError("not a constructor".into())),
        };
        if let FunctionKind::User(id) = kind {
            if p.functions[id as usize]
                .name
                .is_some_and(|name| p.atoms[name as usize].as_bytes() == b"\0rqj:arrow")
            {
                return Err(JsError("arrow function is not a constructor".into()));
            }
            if p.functions[id as usize].is_async {
                return Err(JsError("async function is not a constructor".into()));
            }
            if p.functions[id as usize].is_generator {
                return Err(JsError("generator function is not a constructor".into()));
            }
            if self
                .lookup_atom("prototype")
                .is_some_and(|atom| self.own_property(callee, atom).is_none())
            {
                return Err(JsError("arrow function is not a constructor".into()));
            }
        }
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
            Native::Function => self.function_native(p, args),
            Native::Object => {
                if let Some(value) = args.first().copied()
                    && self.object_data(value).is_some()
                {
                    return Ok(value);
                }
                Ok(self.object())
            }
            Native::Proxy => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                let handler = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                if self.object_data(target).is_none() || self.object_data(handler).is_none() {
                    return Err(JsError("Proxy target and handler must be objects".into()));
                }
                Ok(self.heap.alloc(Cell::Proxy {
                    object: Self::empty_object(self.object_proto),
                    target,
                    handler,
                }))
            }
            Native::Array => {
                let len = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
                Ok(self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(vec![Value::DELETED; len]),
                }))
            }
            Native::ArrayBuffer | Native::SharedArrayBuffer => {
                self.construct_buffer_native(p, native, args)
            }
            Native::Uint8Array => self.construct_uint8_array_native(p, args),
            Native::Uint8ClampedArray => self.construct_uint8_clamped_array_native(p, args),
            Native::Uint16Array => self.construct_uint16_array_native(p, args),
            Native::Uint32Array => self.construct_uint32_array_native(p, args),
            Native::Int8Array => self.construct_int8_array_native(p, args),
            Native::Int16Array => self.construct_int16_array_native(p, args),
            Native::Int32Array => self.construct_int32_array_native(p, args),
            Native::Float32Array => self.construct_float32_array_native(p, args),
            Native::Float64Array => self.construct_float64_array_native(p, args),
            Native::DataView => self.construct_data_view_native(p, args),
            Native::Map | Native::Set => self.construct_collection_native(native, args),
            Native::WeakMap | Native::WeakSet => self.construct_weak_collection_native(native),
            Native::WeakRef => self.construct_weak_ref_native(args),
            Native::FinalizationRegistry => self.construct_finalization_registry_native(args),
            Native::DisposableStack => self.construct_disposable_stack_native(p),
            Native::Promise => self.construct_promise(p, args),
            Native::RegExp => self.construct_regexp_native(p, args),
            Native::Date => self.date_construct_native(p, args),
            Native::Error
            | Native::EvalError
            | Native::RangeError
            | Native::ReferenceError
            | Native::SyntaxError
            | Native::TypeError
            | Native::URIError
            | Native::RealmTypeError => self.construct_error_native(p, native, args),
            Native::String => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, value)?;
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::Number => Ok(Value::number(
                self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?,
            )),
            Native::Boolean => Err(JsError("Boolean object construction is unsupported".into())),
            _ => Err(JsError("native is not constructible".into())),
        }
    }
}
