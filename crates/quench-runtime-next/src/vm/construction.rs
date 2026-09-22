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
                        | Native::AsyncFunction
                        | Native::GeneratorFunction
                        | Native::AsyncGeneratorFunction
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
                        | Native::BigInt64Array
                        | Native::BigUint64Array
                        | Native::DynamicDerivedClass
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
        self.function_values[id as usize].push((env, function));
        let length = self.intern_atom("length");
        self.set_property(
            function,
            length,
            Value::number(p.functions[id as usize].params as f64),
        )?;
        self.set_property_attributes(
            function,
            property_key::PropertyKey::string(length),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let name = self.intern_atom("name");
        let name_value = p.functions[id as usize]
            .name
            .map(|atom| {
                self.heap
                    .alloc(Cell::String(JsString::from_str(self.atom_name(atom))))
            })
            .unwrap_or_else(|| self.heap.alloc(Cell::String(JsString::from_str(""))));
        self.set_property(function, name, name_value)?;
        self.set_property_attributes(
            function,
            property_key::PropertyKey::string(name),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let arrow = p.functions[id as usize]
            .name
            .is_some_and(|name| p.atoms[name as usize].as_bytes() == b"\0rqj:arrow");
        if !arrow && let Some(atom) = self.lookup_atom("prototype") {
            self.set_property(function, atom, prototype)?;
            self.set_property_attributes(
                function,
                property_key::PropertyKey::string(atom),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        let constructor_atom = self.intern_atom("constructor");
        let constructor = match (
            p.functions[id as usize].is_async,
            p.functions[id as usize].is_generator,
        ) {
            (true, true) => self.native_value(Native::AsyncGeneratorFunction),
            (true, false) => self.native_value(Native::AsyncFunction),
            (false, true) => self.native_value(Native::GeneratorFunction),
            (false, false) => function,
        };
        self.set_property(prototype, constructor_atom, constructor)?;
        if p.functions[id as usize].is_async || p.functions[id as usize].is_generator {
            self.set_property(function, constructor_atom, constructor)?;
        }
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
        if let FunctionKind::Native(Native::DynamicDerivedClass) = kind {
            let base = match self.heap.get(callee) {
                Some(Cell::Function { env, .. }) => *env,
                _ => return Err(JsError("not a constructor".into())),
            };
            return self.construct_value(p, base, args);
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
            Native::Function
            | Native::AsyncFunction
            | Native::GeneratorFunction
            | Native::AsyncGeneratorFunction => self.function_native(p, args),
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
            Native::BigInt64Array => self.construct_bigint64_array_native(p, args),
            Native::BigUint64Array => self.construct_biguint64_array_native(p, args),
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
                let value = self.heap.alloc(Cell::String(text.into()));
                self.box_primitive_object(value)
            }
            Native::Number => {
                let value = Value::number(
                    self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?,
                );
                self.box_primitive_object(value)
            }
            Native::Boolean => {
                let value = if args
                    .first()
                    .copied()
                    .is_some_and(|value| self.truthy(value))
                {
                    Value::TRUE
                } else {
                    Value::FALSE
                };
                self.box_primitive_object(value)
            }
            _ => Err(JsError("native is not constructible".into())),
        }
    }
}
