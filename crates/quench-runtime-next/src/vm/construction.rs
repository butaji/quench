use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn string_constructor_argument(&mut self, args: &[Value]) -> Value {
        args.first().copied().unwrap_or_else(|| {
            self.heap
                .alloc(Cell::String(super::wtf16::JsString::from_str("")))
        })
    }

    pub(super) fn string_constructor_value(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let argument = self.string_constructor_argument(args);
        let text = self.to_string(p, argument)?;
        Ok(self.heap.alloc(Cell::String(text.into())))
    }

    pub(super) fn set_function_name(
        &mut self,
        p: &ResidualProgram,
        function: Value,
        name: Atom,
    ) -> Result<(), JsError> {
        let Some(text) = ((name as usize) < p.atoms.len()).then(|| &p.atoms[name as usize]) else {
            return Err(JsError("function name atom is invalid".into()));
        };
        self.set_function_name_text(function, text.to_string());
        Ok(())
    }

    pub(super) fn set_function_name_key(&mut self, function: Value, key: Value, prefix: u32) {
        let name = match self.heap.get(key) {
            Some(Cell::String(value)) => value.to_string(),
            Some(Cell::Symbol(description)) => description
                .as_ref()
                .map_or_else(String::new, |description| format!("[{}]", description)),
            _ => return,
        };
        let name = match prefix {
            crate::bytecode::FUNCTION_NAME_PREFIX_GETTER => format!("get {name}"),
            crate::bytecode::FUNCTION_NAME_PREFIX_SETTER => format!("set {name}"),
            _ => name,
        };
        self.set_function_name_text(function, name);
    }

    fn set_function_name_text(&mut self, function: Value, text: String) {
        if !matches!(self.heap.get(function), Some(Cell::Function { .. })) {
            return;
        }
        let name_atom = self.intern_atom("name");
        let current = self
            .own_property(function, name_atom)
            .unwrap_or(Value::UNDEFINED);
        let inferred = matches!(self.heap.get(current), Some(Cell::String(value)) if value.units().is_empty())
            || matches!(self.heap.get(current), Some(Cell::String(value)) if value.to_string() == "\0rqj:arrow");
        if !inferred {
            return;
        }
        let value = self.heap.alloc(Cell::String(text.into()));
        let Some(object) = self.object_data(function) else {
            return;
        };
        if let Some(slot) = self.shape_slot(object.shape(), name_atom) {
            self.heap.property_set(function, slot, value);
        }
    }

    pub(super) fn is_constructable(&self, p: &ResidualProgram, value: Value) -> bool {
        let Some(cell) = self.heap.get(value) else {
            return false;
        };
        match cell {
            Cell::Proxy {
                target, handler, ..
            } => !handler.is_null() && self.is_constructable(p, *target),
            Cell::Function { kind, .. } => match kind {
                FunctionKind::User(program_id, id) | FunctionKind::NumericUser(program_id, id) => {
                    self.programs.get(*program_id).is_some_and(|program| {
                        program.functions.get(*id as usize).is_some_and(|function| {
                            function.constructible && !function.is_async && !function.is_generator
                        })
                    })
                }
                FunctionKind::Native(Native::FunctionBoundCall) => {
                    let Cell::Function { env, .. } = cell else {
                        return false;
                    };
                    self.lookup_atom("\0rqj:bound-target")
                        .and_then(|atom| self.own_property(*env, atom))
                        .is_some_and(|target| self.is_constructable(p, target))
                }
                FunctionKind::Native(native) => matches!(
                    native,
                    Native::Function
                        | Native::AbstractModuleSource
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
                        | Native::AggregateError
                        | Native::SuppressedError
                        | Native::EvalError
                        | Native::RangeError
                        | Native::ReferenceError
                        | Native::SyntaxError
                        | Native::TypeError
                        | Native::RealmTypeError
                        | Native::URIError
                        | Native::RegExp
                        | Native::String
                        | Native::Boolean
                        | Native::Number
                        | Native::Promise
                        | Native::Symbol
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
        self.closure_in_realm(p, id, env, self.realm.globals)
    }

    pub(super) fn closure_in_realm(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        mut env: Value,
        realm: Value,
    ) -> Result<Value, JsError> {
        let with_objects = self
            .frames
            .last()
            .map(|frame| self.with_stack[frame.with_base.min(self.with_stack.len())..].to_vec())
            .unwrap_or_default();
        if !with_objects.is_empty() {
            env = self.heap.alloc(Cell::Environment {
                parent: env,
                program: None,
                root_eval_scope: false,
                function: u32::MAX,
                slots: Box::new([]),
                dynamic_bindings: Vec::new(),
                with_objects,
            });
        }
        let is_arrow = p.functions[id as usize]
            .name
            .is_some_and(|atom| self.atom_name(atom) == "\0rqj:arrow");
        if is_arrow {
            env = self.heap.alloc(Cell::Environment {
                parent: env,
                program: None,
                root_eval_scope: false,
                function: u32::MAX,
                slots: Box::new([]),
                dynamic_bindings: Vec::new(),
                with_objects: Vec::new(),
            });
        }
        let generator_prototype_parent =
            if p.functions[id as usize].is_async && p.functions[id as usize].is_generator {
                self.async_generator_proto
            } else if p.functions[id as usize].is_generator {
                self.iterator_proto
            } else {
                self.object_proto
            };
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(generator_prototype_parent)));
        let function_prototype_atom = self.intern_atom("prototype");
        let intrinsic = match (
            p.functions[id as usize].is_async,
            p.functions[id as usize].is_generator,
        ) {
            (true, true) => Some(Native::AsyncGeneratorFunction),
            (false, true) => Some(Native::GeneratorFunction),
            (true, false) => Some(Native::AsyncFunction),
            (false, false) => None,
        };
        let function_object_prototype = intrinsic
            .and_then(|intrinsic| {
                self.own_property(self.native_value(intrinsic), function_prototype_atom)
            })
            .unwrap_or(self.function_proto);
        let function = self.heap.alloc(Cell::Function {
            object: Box::new(Self::empty_object(function_object_prototype)),
            kind: match p.functions[id as usize].dispatch {
                DispatchClass::General => FunctionKind::User(self.active_program, id),
                DispatchClass::Numeric => FunctionKind::NumericUser(self.active_program, id),
            },
            env,
            realm,
        });
        let program = self.active_program;
        self.function_values
            .entry((program, id))
            .or_default()
            .push((env, function));
        let length = self.intern_atom("length");
        self.set_property(
            function,
            length,
            Value::number(p.functions[id as usize].length as f64),
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
        let name_value = if is_arrow {
            self.heap.alloc(Cell::String(JsString::from_str("")))
        } else {
            p.functions[id as usize]
                .name
                .map(|atom| {
                    self.heap
                        .alloc(Cell::String(JsString::from_str(self.atom_name(atom))))
                })
                .unwrap_or_else(|| self.heap.alloc(Cell::String(JsString::from_str(""))))
        };
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
        if (p.functions[id as usize].constructible || p.functions[id as usize].is_generator)
            && let Some(atom) = self.lookup_atom("prototype")
        {
            self.set_property(function, atom, prototype)?;
            self.set_property_attributes(
                function,
                property_key::PropertyKey::string(atom),
                PropertyAttributes {
                    writable: !p.functions[id as usize].is_class_constructor,
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
        if !p.functions[id as usize].is_generator {
            self.set_builtin_value_named(prototype, "constructor", constructor)?;
        }
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
        self.construct_value_with_new_target(p, callee, callee, args)
    }

    pub(super) fn construct_value_with_new_target(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        new_target: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if matches!(self.heap.get(callee), Some(Cell::Proxy { .. })) {
            return self.proxy_construct(p, callee, new_target, args);
        }
        if !self.is_constructable(p, callee) {
            return Err(self.type_error(p, "value is not a constructor".into()));
        }
        if !self.is_constructable(p, new_target) {
            return Err(self.type_error(p, "newTarget is not a constructor".into()));
        }
        let (kind, derived_constructor) = match self.heap.get(callee) {
            Some(Cell::Function { kind, .. }) => {
                let derived = match kind {
                    FunctionKind::User(program_id, id)
                    | FunctionKind::NumericUser(program_id, id) => {
                        self.programs.get(*program_id).is_some_and(|program| {
                            program
                                .functions
                                .get(*id as usize)
                                .is_some_and(|function| function.derived_constructor)
                        })
                    }
                    _ => false,
                };
                (*kind, derived)
            }
            _ => unreachable!("IsConstructor accepted a non-function target"),
        };
        if let FunctionKind::User(program_id, id) | FunctionKind::NumericUser(program_id, id) = kind
        {
            let Some(program) = self.programs.get(program_id) else {
                return Err(self.type_error(p, "function belongs to an unavailable program".into()));
            };
            let Some(function) = program.functions.get(id as usize) else {
                return Err(self.type_error(p, "function index is outside its program".into()));
            };
            if !function.constructible {
                return Err(self.type_error(p, "value is not a constructor".into()));
            }
            if function.is_async {
                return Err(JsError("async function is not a constructor".into()));
            }
            if function.is_generator {
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
            return self.construct_value_with_new_target(p, base, new_target, args);
        }
        if let FunctionKind::Native(Native::FunctionBoundCall) = kind {
            let env = match self.heap.get(callee) {
                Some(Cell::Function { env, .. }) => *env,
                _ => return Err(self.type_error(p, "invalid bound function".into())),
            };
            let target_atom = self.intern_atom("\0rqj:bound-target");
            let args_atom = self.intern_atom("\0rqj:bound-args");
            let target = self
                .own_property(env, target_atom)
                .ok_or_else(|| self.type_error(p, "invalid bound function".into()))?;
            let bound_args = self
                .own_property(env, args_atom)
                .and_then(|value| match self.heap.get(value) {
                    Some(Cell::Array { elements, .. }) => Some(elements.as_ref().clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let mut arguments = bound_args;
            arguments.extend_from_slice(args);
            let new_target = if new_target == callee {
                target
            } else {
                new_target
            };
            return self.construct_value_with_new_target(p, target, new_target, &arguments);
        }
        if let FunctionKind::Native(native) = kind {
            let realm = match self.heap.get(callee) {
                Some(Cell::Function { realm, .. }) => *realm,
                _ => self.realm.globals,
            };
            let previous_global = std::mem::replace(&mut self.realm.globals, realm);
            let result = self.construct_native_with_new_target(p, native, args, new_target);
            self.realm.globals = previous_global;
            let result = result?;
            if !matches!(
                native,
                Native::Proxy | Native::Array | Native::ArrayBuffer | Native::SharedArrayBuffer
            ) {
                self.set_constructed_prototype(p, result, new_target)?;
            }
            return Ok(result);
        }
        let object = if derived_constructor {
            Value::UNDEFINED
        } else {
            let proto = self.prototype_from_constructor(p, new_target)?;
            self.heap.alloc(Cell::Object(Self::empty_object(proto)))
        };
        let previous_target = self.construct_target;
        self.construct_target = Some(new_target);
        let result = if derived_constructor {
            self.call_user_for_construct(p, callee, args)
                .map(|(value, this)| (value, Some(this)))
        } else {
            self.call_value(p, callee, object, args)
                .map(|value| (value, None))
        };
        self.construct_target = previous_target;
        let (result, this) = result?;
        if self.is_object_like(result) {
            return Ok(result);
        }
        if derived_constructor {
            if result.is_undefined() {
                let this = this.unwrap_or(Value::DELETED);
                return if this.is_deleted() {
                    Err(self.reference_error(
                        p,
                        "Must call super constructor before returning from derived constructor"
                            .into(),
                    ))
                } else {
                    Ok(this)
                };
            }
            return Err(self.type_error(
                p,
                "derived constructor may only return an object or undefined".into(),
            ));
        }
        Ok(object)
    }

    fn prototype_from_constructor(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, constructor, prototype_atom)?;
        if self.object_data(prototype).is_some() {
            return Ok(prototype);
        }
        let realm = match self.heap.get(constructor) {
            Some(Cell::Function { realm, .. }) => *realm,
            _ => return Ok(self.object_proto),
        };
        let object_atom = self.intern_atom("Object");
        let object_constructor = self.get_property(p, realm, object_atom)?;
        let object_prototype = self.get_property(p, object_constructor, prototype_atom)?;
        Ok(if self.object_data(object_prototype).is_some() {
            object_prototype
        } else {
            self.object_proto
        })
    }

    fn array_prototype_from_new_target(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        if self.object_data(prototype).is_some() {
            return Ok(prototype);
        }
        let realm = match self.heap.get(new_target) {
            Some(Cell::Function { realm, .. }) => *realm,
            _ => self.realm.globals,
        };
        let array_atom = self.intern_atom("Array");
        let array = self.get_property(p, realm, array_atom)?;
        let prototype = self.get_property(p, array, prototype_atom)?;
        Ok(if self.object_data(prototype).is_some() {
            prototype
        } else {
            self.array_proto
        })
    }

    fn set_constructed_prototype(
        &mut self,
        p: &ResidualProgram,
        result: Value,
        new_target: Value,
    ) -> Result<(), JsError> {
        if self.object_data(result).is_none() {
            return Ok(());
        }
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        if prototype.is_null() || self.object_data(prototype).is_none() {
            return Ok(());
        }
        self.object_set_prototype_of(p, result, prototype)?;
        Ok(())
    }

    pub(super) fn construct_super_value(
        &mut self,
        p: &ResidualProgram,
        active_constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let active_constructor = if active_constructor.is_undefined() {
            self.frames
                .last()
                .and_then(|frame| {
                    self.function_values
                        .get(&(frame.program, frame.function))
                        .and_then(|entries| {
                            entries.iter().rev().find_map(|(environment, function)| {
                                (*environment == frame.env).then_some(*function)
                            })
                        })
                })
                .unwrap_or(active_constructor)
        } else {
            active_constructor
        };
        let superclass = self.object_get_prototype_of(p, active_constructor)?;
        if !self.is_constructable(p, superclass) {
            return Err(self.type_error(p, "superclass is not a constructor".into()));
        }
        let new_target_atom = self.intern_atom("\0rqj:new-target");
        let new_target = self
            .frames
            .len()
            .checked_sub(1)
            .and_then(|frame| self.dynamic_binding(frame, new_target_atom))
            .filter(|value| !value.is_undefined())
            .ok_or_else(|| {
                self.reference_error(p, "new.target is unavailable for super()".into())
            })?;
        self.construct_value_with_new_target(p, superclass, new_target, args)
    }

    pub(super) fn construct_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let new_target = self.native_value(native);
        self.construct_native_with_new_target(p, native, args, new_target)
    }

    fn construct_native_with_new_target(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        match native {
            Native::AbstractModuleSource => {
                Err(self.type_error(p, "AbstractModuleSource cannot be constructed".into()))
            }
            Native::Function
            | Native::AsyncFunction
            | Native::GeneratorFunction
            | Native::AsyncGeneratorFunction => self.function_native(p, native, args),
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
            Native::Array => self.construct_array_native(p, args, new_target),
            Native::ArrayBuffer | Native::SharedArrayBuffer => {
                self.construct_buffer_native(p, native, args, new_target)
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
            Native::Map | Native::Set => self.construct_collection_native(p, native, args),
            Native::WeakMap | Native::WeakSet => self.construct_weak_collection_native(native),
            Native::WeakRef => self.construct_weak_ref_native(args),
            Native::FinalizationRegistry => self.construct_finalization_registry_native(args),
            Native::DisposableStack => self.construct_disposable_stack_native(p),
            Native::Promise => self.construct_promise(p, args),
            Native::RegExp => self.construct_regexp_native(p, args),
            Native::Date => self.date_construct_native(p, args),
            Native::AggregateError => self.construct_aggregate_error(p, args, new_target),
            Native::Symbol => Err(self.type_error(p, "Symbol is not a constructor".into())),
            Native::Error
            | Native::SuppressedError
            | Native::EvalError
            | Native::RangeError
            | Native::ReferenceError
            | Native::SyntaxError
            | Native::TypeError
            | Native::URIError
            | Native::RealmTypeError => self.construct_error_native(p, native, args),
            Native::String => {
                let value = self.string_constructor_value(p, args)?;
                self.box_primitive_object(value)
            }
            Native::Number => {
                let value = Value::number(
                    self.to_number(p, args.first().copied().unwrap_or(Value::number(0.0)))?,
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

    fn construct_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype = self.array_prototype_from_new_target(p, new_target)?;
        if let [length] = args
            && let Some(length) = length.as_number()
        {
            if !length.is_finite()
                || length < 0.0
                || length.fract() != 0.0
                || length > u32::MAX as f64
            {
                return Err(self.range_error(p, "Invalid array length".into()));
            }
            let array = self.heap.alloc(Cell::Array {
                object: Self::empty_object(prototype),
                elements: Rc::new(Vec::new()),
            });
            self.heap.sparse_set_length(array, length as usize);
            return Ok(array);
        }

        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(prototype),
            elements: Rc::new(args.to_vec()),
        }))
    }
}
