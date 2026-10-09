use super::typed_array_install::TYPED_ARRAY_INSTALLS;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn function_realm(
        &mut self,
        p: &ResidualProgram,
        function: Value,
    ) -> Result<Value, JsError> {
        match self.heap.get(function).cloned() {
            Some(Cell::Function {
                kind: FunctionKind::Native(Native::FunctionBoundCall),
                env,
                ..
            }) => {
                let target_atom = self.intern_atom("\0quench:bound-target");
                let target = self
                    .own_property(env, target_atom)
                    .unwrap_or(Value::UNDEFINED);
                self.function_realm(p, target)
            }
            Some(Cell::Function { realm, .. }) => Ok(realm),
            Some(Cell::Proxy { handler, .. }) if handler.is_null() => {
                Err(self.type_error(p, "cannot access a revoked proxy".into()))
            }
            Some(Cell::Proxy { target, .. }) => self.function_realm(p, target),
            _ => Ok(self.realm.globals),
        }
    }

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
        let inferred =
            matches!(self.heap.get(current), Some(Cell::String(value)) if value.units().is_empty());
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
            Cell::Proxy { kind, .. } => *kind == ProxyKind::Constructor,
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
                    self.lookup_atom("\0quench:bound-target")
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
                        | Native::Iterator
                        | Native::Object
                        | Native::Proxy
                        | Native::Array
                        | Native::TypedArray
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
                        | Native::Float16Array
                        | Native::Float32Array
                        | Native::Float64Array
                        | Native::DataView
                        | Native::Map
                        | Native::Set
                        | Native::ShadowRealm
                        | Native::WeakMap
                        | Native::WeakSet
                        | Native::WeakRef
                        | Native::FinalizationRegistry
                        | Native::DisposableStack
                        | Native::AsyncDisposableStack
                        | Native::Date
                        | Native::BigInt
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
                        | Native::IntlNumberFormat
                        | Native::IntlCollator
                        | Native::IntlPluralRules
                        | Native::IntlDateTimeFormat
                        | Native::IntlDisplayNames
                        | Native::IntlDurationFormat
                        | Native::IntlListFormat
                        | Native::IntlLocale
                        | Native::IntlRelativeTimeFormat
                        | Native::IntlSegmenter
                        | Native::Promise
                        | Native::Symbol
                        | Native::TemporalDuration
                        | Native::TemporalPlainTime
                        | Native::TemporalInstant
                        | Native::TemporalPlainDate
                        | Native::TemporalPlainDateTime
                        | Native::TemporalZonedDateTime
                        | Native::TemporalPlainMonthDay
                        | Native::TemporalPlainYearMonth
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
        let with_objects = if env.is_null() {
            Vec::new()
        } else {
            let inherited = self.captured_with_objects(env);
            let active = self.frames.last().map_or(&[][..], |frame| {
                &self.with_stack[frame.with_base.min(self.with_stack.len())..]
            });
            active
                .strip_prefix(inherited.as_slice())
                .unwrap_or(active)
                .to_vec()
        };
        if !with_objects.is_empty() {
            env = self.heap.alloc(Cell::Environment {
                parent: env,
                program: None,
                root_eval_scope: false,
                binding_site_pc: None,
                function: u32::MAX,
                slots: Vec::<Value>::new().into_boxed_slice().into(),
                dynamic_bindings: Box::new(Vec::new().into()),
                with_objects: with_objects.into_boxed_slice(),
            });
        }
        let is_arrow = p.functions[id as usize].is_arrow;
        if is_arrow {
            env = self.heap.alloc(Cell::Environment {
                parent: env,
                program: None,
                root_eval_scope: false,
                binding_site_pc: None,
                function: u32::MAX,
                slots: Vec::<Value>::new().into_boxed_slice().into(),
                dynamic_bindings: Box::new(Vec::new().into()),
                with_objects: Box::default(),
            });
        }
        let generator_prototype_parent =
            if p.functions[id as usize].is_async && p.functions[id as usize].is_generator {
                let constructor_atom = self.intern_atom("AsyncGeneratorFunction");
                let prototype_atom = self.intern_atom("prototype");
                self.own_property(realm, constructor_atom)
                    .and_then(|constructor| self.own_property(constructor, prototype_atom))
                    .and_then(|function_prototype| {
                        self.own_property(function_prototype, prototype_atom)
                    })
                    .filter(|prototype| self.object_data(*prototype).is_some())
                    .unwrap_or(self.async_generator_proto)
            } else if p.functions[id as usize].is_generator {
                let constructor_atom = self.intern_atom("GeneratorFunction");
                let prototype_atom = self.intern_atom("prototype");
                self.own_property(realm, constructor_atom)
                    .and_then(|constructor| self.own_property(constructor, prototype_atom))
                    .and_then(|function_prototype| {
                        self.own_property(function_prototype, prototype_atom)
                    })
                    .filter(|prototype| self.object_data(*prototype).is_some())
                    .unwrap_or(self.generator_proto)
            } else {
                self.realm_object_prototype(realm)
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
        let function_object_prototype = if let Some(intrinsic) = intrinsic {
            let name = match intrinsic {
                Native::AsyncFunction => "AsyncFunction",
                Native::GeneratorFunction => "GeneratorFunction",
                Native::AsyncGeneratorFunction => "AsyncGeneratorFunction",
                _ => unreachable!(),
            };
            let name = self.intern_atom(name);
            self.own_property(realm, name)
                .and_then(|constructor| self.own_property(constructor, function_prototype_atom))
                .filter(|prototype| self.object_data(*prototype).is_some())
                .or_else(|| {
                    self.own_property(self.native_value(intrinsic), function_prototype_atom)
                })
                .unwrap_or(self.function_proto)
        } else {
            self.realm_function_prototype(realm)
        };
        let function = self.heap.alloc(Cell::Function {
            object: Box::new(Self::empty_object(function_object_prototype)),
            kind: match p.functions[id as usize].dispatch {
                DispatchClass::General => FunctionKind::User(self.active_program, id),
                DispatchClass::Numeric => FunctionKind::NumericUser(self.active_program, id),
            },
            env,
            realm,
        });
        if let Some(source) = p.functions[id as usize].source_text.as_deref() {
            self.set_function_source(function, source)?;
        }
        let program = self.active_program;
        self.function_values.entry((program, id)).or_default().push(
            self.heap
                .weak_handle(function)
                .expect("new closure is live"),
        );
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
        let constructor = match (
            p.functions[id as usize].is_async,
            p.functions[id as usize].is_generator,
        ) {
            (true, true) => self
                .realm_constructor(realm, "AsyncGeneratorFunction")
                .unwrap_or_else(|| self.native_value(Native::AsyncGeneratorFunction)),
            (true, false) => self
                .realm_constructor(realm, "AsyncFunction")
                .unwrap_or_else(|| self.native_value(Native::AsyncFunction)),
            (false, true) => self
                .realm_constructor(realm, "GeneratorFunction")
                .unwrap_or_else(|| self.native_value(Native::GeneratorFunction)),
            (false, false) => function,
        };
        if !p.functions[id as usize].is_generator {
            self.set_builtin_value_named(prototype, "constructor", constructor)?;
        }
        Ok(function)
    }

    fn realm_constructor(&mut self, realm: Value, name: &str) -> Option<Value> {
        let atom = self.intern_atom(name);
        self.own_property(realm, atom)
    }

    pub(super) fn realm_object_prototype(&self, realm: Value) -> Value {
        self.realm
            .intrinsics
            .builtin_prototypes
            .get(&(realm, Native::Object))
            .copied()
            .unwrap_or(self.object_proto)
    }

    pub(super) fn realm_function_prototype(&self, realm: Value) -> Value {
        self.realm
            .intrinsics
            .builtin_prototypes
            .get(&(realm, Native::Function))
            .copied()
            .unwrap_or(self.function_proto)
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
        self.with_call_roots(
            std::iter::once(callee)
                .chain(std::iter::once(new_target))
                .chain(args.iter().copied()),
            |vm| {
                let _stack = vm.enter_stack()?;
                if matches!(vm.heap.get(callee), Some(Cell::Proxy { .. })) {
                    return vm.proxy_construct(p, callee, new_target, args);
                }
                if !vm.is_constructable(p, callee) {
                    return Err(vm.type_error(p, "value is not a constructor".into()));
                }
                if !vm.is_constructable(p, new_target) {
                    return Err(vm.type_error(p, "newTarget is not a constructor".into()));
                }
                let (kind, derived_constructor) = match vm.heap.get(callee) {
                    Some(Cell::Function { kind, .. }) => {
                        let derived = match kind {
                            FunctionKind::User(program_id, id)
                            | FunctionKind::NumericUser(program_id, id) => {
                                vm.programs.get(*program_id).is_some_and(|program| {
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
                if let FunctionKind::User(program_id, id) | FunctionKind::NumericUser(program_id, id) =
                    kind
                {
                    let Some(program) = vm.programs.get(program_id) else {
                        return Err(
                            vm.type_error(p, "function belongs to an unavailable program".into())
                        );
                    };
                    let Some(function) = program.functions.get(id as usize) else {
                        return Err(vm.type_error(p, "function index is outside its program".into()));
                    };
                    if !function.constructible {
                        return Err(vm.type_error(p, "value is not a constructor".into()));
                    }
                    if function.is_async {
                        return Err(JsError("async function is not a constructor".into()));
                    }
                    if function.is_generator {
                        return Err(JsError("generator function is not a constructor".into()));
                    }
                    if vm
                        .lookup_atom("prototype")
                        .is_some_and(|atom| vm.own_property(callee, atom).is_none())
                    {
                        return Err(JsError("arrow function is not a constructor".into()));
                    }
                }
                if let FunctionKind::Native(Native::FunctionBoundCall) = kind {
                    let env = match vm.heap.get(callee) {
                        Some(Cell::Function { env, .. }) => *env,
                        _ => return Err(vm.type_error(p, "invalid bound function".into())),
                    };
                    let target_atom = vm.intern_atom("\0quench:bound-target");
                    let args_atom = vm.intern_atom("\0quench:bound-args");
                    let target = vm
                        .own_property(env, target_atom)
                        .ok_or_else(|| vm.type_error(p, "invalid bound function".into()))?;
                    let bound_args = vm
                        .own_property(env, args_atom)
                        .and_then(|value| match vm.heap.get(value) {
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
                    return vm.construct_value_with_new_target(p, target, new_target, &arguments);
                }
                if let FunctionKind::Native(native) = kind {
                    let realm = vm.function_realm(p, callee)?;
                    let previous_global = vm.switch_realm_global(realm);
                    let result = vm.construct_native_with_new_target(p, native, args, new_target);
                    vm.switch_realm_global(previous_global);
                    let result = result?;
                    let result = if vm.intl_constructor_prototypes(native).is_none()
                        && !native.is_typed_array_constructor()
                        && !native.is_error_constructor()
                        && !matches!(
                            native,
                            Native::Proxy
                                | Native::Array
                                | Native::ArrayBuffer
                                | Native::SharedArrayBuffer
                                | Native::RegExp
                        )
                        && !(native == Native::Object
                            && new_target == vm.native_value(Native::Object)
                            && args
                                .first()
                                .is_some_and(|value| !value.is_null() && !value.is_undefined()))
                    {
                        vm.set_constructed_prototype(p, result, new_target, native)?
                    } else {
                        result
                    };
                    return Ok(result);
                }
                let object = if derived_constructor {
                    Value::UNDEFINED
                } else {
                    let proto = vm.prototype_from_constructor(p, new_target)?;
                    vm.heap.alloc(Cell::Object(Self::empty_object(proto)))
                };
                let previous_target = vm.construct_target;
                vm.construct_target = Some(new_target);
                let result = if derived_constructor {
                    vm.call_user_for_construct(p, callee, args)
                        .map(|(value, this)| (value, Some(this)))
                } else {
                    vm.call_value(p, callee, object, args)
                        .map(|value| (value, None))
                };
                vm.construct_target = previous_target;
                let (result, this) = result?;
                if vm.is_object_like(result) {
                    return Ok(result);
                }
                if derived_constructor {
                    if result.is_undefined() {
                        let this = this.unwrap_or(Value::DELETED);
                        return if this.is_deleted() {
                            Err(vm.reference_error(
                                p,
                                "Must call super constructor before returning from derived constructor"
                                    .into(),
                            ))
                        } else {
                            Ok(this)
                        };
                    }
                    return Err(vm.type_error(
                        p,
                        "derived constructor may only return an object or undefined".into(),
                    ));
                }
                Ok(object)
            },
        )
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
        let has_function_realm = self.is_function(constructor)
            || matches!(self.heap.get(constructor), Some(Cell::Proxy { .. }));
        let realm = if has_function_realm {
            self.function_realm(p, constructor)?
        } else {
            return Ok(self.object_proto);
        };
        Ok(self.realm_object_prototype(realm))
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
        let realm = self.function_realm(p, new_target)?;
        Ok(self.array_prototype_for_realm(realm))
    }

    pub(super) fn array_prototype_for_realm(&self, realm: Value) -> Value {
        self.realm
            .intrinsics
            .builtin_prototypes
            .get(&(realm, Native::Array))
            .copied()
            .unwrap_or(self.array_proto)
    }

    pub(super) fn regexp_prototype_from_new_target(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        if self.object_data(prototype).is_some() {
            return Ok(prototype);
        }
        let realm = self.function_realm(p, new_target)?;
        Ok(self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&realm)
            .map(|intrinsics| intrinsics.prototype)
            .unwrap_or(self.regexp_proto))
    }

    pub(super) fn set_constructed_prototype(
        &mut self,
        p: &ResidualProgram,
        result: Value,
        new_target: Value,
        native: Native,
    ) -> Result<Value, JsError> {
        if self.object_data(result).is_none() {
            return Ok(result);
        }
        let result_root = self.heap.root(result);
        let target_root = self.heap.root(new_target);
        let outcome = self
            .set_constructed_prototype_rooted(p, result_root, target_root, native)
            .map(|()| {
                self.heap
                    .root_value(result_root)
                    .expect("constructed object root remains live")
            });
        self.heap.release_root(target_root);
        self.heap.release_root(result_root);
        outcome
    }

    fn set_constructed_prototype_rooted(
        &mut self,
        p: &ResidualProgram,
        result_root: crate::heap::RootId,
        target_root: crate::heap::RootId,
        native: Native,
    ) -> Result<(), JsError> {
        let new_target = self.heap.root_value(target_root).unwrap();
        let Some(prototype) = self.native_constructor_prototype(p, new_target, native)? else {
            return Ok(());
        };
        let result = self
            .heap
            .root_value(result_root)
            .expect("constructed object root remains live");
        self.object_set_prototype_of(p, result, prototype)?;
        if native == Native::DataView
            && let Some((buffer, offset, length)) = self.data_view_view(result)
        {
            if self.array_buffer_detached(buffer) {
                return Err(self.type_error(p, "Cannot use a detached ArrayBuffer".into()));
            }
            if self.array_buffer_out_of_bounds(buffer, offset, length) {
                return Err(self.range_error(p, "Invalid DataView byte length".into()));
            }
        }
        Ok(())
    }

    pub(super) fn native_constructor_prototype(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
        native: Native,
    ) -> Result<Option<Value>, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if prototype.is_null() || self.object_data(prototype).is_none() {
            let Some(intrinsic) = TYPED_ARRAY_INSTALLS
                .iter()
                .find_map(|(_, candidate, name)| (*candidate == native).then_some(*name))
                .or_else(|| match native {
                    Native::Object => Some("Object"),
                    Native::Function => Some("Function"),
                    Native::AsyncFunction => Some("AsyncFunction"),
                    Native::GeneratorFunction => Some("GeneratorFunction"),
                    Native::AsyncGeneratorFunction => Some("AsyncGeneratorFunction"),
                    Native::RegExp => Some("RegExp"),
                    Native::Number => Some("Number"),
                    Native::String => Some("String"),
                    Native::Iterator => Some("Iterator"),
                    Native::Boolean => Some("Boolean"),
                    Native::DataView => Some("DataView"),
                    Native::Date => Some("Date"),
                    Native::Promise => Some("Promise"),
                    Native::Error => Some("Error"),
                    Native::AggregateError => Some("AggregateError"),
                    Native::SuppressedError => Some("SuppressedError"),
                    Native::EvalError => Some("EvalError"),
                    Native::RangeError => Some("RangeError"),
                    Native::ReferenceError => Some("ReferenceError"),
                    Native::SyntaxError => Some("SyntaxError"),
                    Native::TypeError | Native::RealmTypeError => Some("TypeError"),
                    Native::URIError => Some("URIError"),
                    _ => None,
                })
            else {
                return Ok(None);
            };
            let realm = self.function_realm(p, new_target)?;
            let prototype = if native == Native::RegExp {
                self.realm
                    .intrinsics
                    .regexp_intrinsics
                    .get(&realm)
                    .map(|intrinsics| intrinsics.prototype)
                    .unwrap_or(self.regexp_proto)
            } else if let Some(prototype) = self
                .realm
                .intrinsics
                .builtin_prototypes
                .get(&(realm, native))
                .copied()
            {
                prototype
            } else {
                let constructor_atom = self.intern_atom(intrinsic);
                let constructor = self.get_property(p, realm, constructor_atom)?;
                self.get_property(p, constructor, prototype_atom)?
            };
            if self.object_data(prototype).is_none() {
                return Ok(None);
            }
            prototype
        } else {
            prototype
        };
        Ok(Some(prototype))
    }

    pub(super) fn construct_super_value(
        &mut self,
        p: &ResidualProgram,
        superclass: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_constructable(p, superclass) {
            return Err(self.type_error(p, "superclass is not a constructor".into()));
        }
        let new_target_atom = self.runtime_atoms.new_target;
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
        if let Some(&(kind, _, name)) = TYPED_ARRAY_INSTALLS
            .iter()
            .find(|(_, candidate, _)| *candidate == native)
        {
            return self.construct_typed_array_with_new_target(
                p,
                args,
                kind,
                name,
                Some(new_target),
            );
        }
        match native {
            Native::AbstractModuleSource => {
                Err(self.type_error(p, "AbstractModuleSource cannot be constructed".into()))
            }
            Native::Function
            | Native::AsyncFunction
            | Native::GeneratorFunction
            | Native::AsyncGeneratorFunction => self.function_native(p, native, args),
            Native::Iterator if new_target == self.native_value(Native::Iterator) => Err(self
                .type_error(
                    p,
                    "Iterator constructor cannot be called or constructed".into(),
                )),
            Native::Iterator => Ok(self.object()),
            Native::Object => {
                if new_target != self.native_value(Native::Object) {
                    return Ok(self.object());
                }
                if let Some(value) = args.first().copied() {
                    if self.object_data(value).is_some() {
                        return Ok(value);
                    }
                    if !value.is_null() && !value.is_undefined() {
                        return self.box_object(value);
                    }
                }
                Ok(self.object())
            }
            Native::Proxy => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                let handler = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                if !self.is_object_like(target) {
                    return Err(self.type_error(p, "Proxy target must be an object".into()));
                }
                if !self.is_object_like(handler) {
                    return Err(self.type_error(p, "Proxy handler must be an object".into()));
                }
                let kind = if self.is_constructable(p, target) {
                    ProxyKind::Constructor
                } else if self.is_function(target) {
                    ProxyKind::Callable
                } else {
                    ProxyKind::Object
                };
                Ok(self.heap.alloc(Cell::Proxy {
                    object: Self::empty_object(self.object_proto),
                    kind,
                    target,
                    handler,
                }))
            }
            Native::Array => self.construct_array_native(p, args, new_target),
            Native::TypedArray => {
                Err(self.type_error(p, "TypedArray is an abstract constructor".into()))
            }
            Native::ArrayBuffer | Native::SharedArrayBuffer => {
                self.construct_buffer_native(p, native, args, new_target)
            }
            Native::DataView => self.construct_data_view_native(p, args),
            Native::BigInt => Err(self.type_error(p, "BigInt cannot be called with new".into())),
            Native::Map | Native::Set => {
                self.construct_collection_native(p, native, args, new_target)
            }
            Native::ShadowRealm => self.construct_shadow_realm(p, new_target),
            Native::WeakMap | Native::WeakSet => {
                self.construct_weak_collection_native(p, native, args, new_target)
            }
            Native::WeakRef => self.construct_weak_ref_native(p, args, new_target),
            Native::FinalizationRegistry => {
                self.construct_finalization_registry_native(p, args, new_target)
            }
            Native::DisposableStack | Native::AsyncDisposableStack => {
                self.construct_disposable_stack_native(p, native, new_target)
            }
            Native::Promise => self.construct_promise(p, args),
            Native::RegExp => self.construct_regexp_native(p, args, Some(new_target)),
            Native::Date => self.date_construct_native(p, args),
            Native::IntlNumberFormat => self.intl_number_format_construct(p, args, new_target),
            Native::IntlCollator => self.intl_collator_construct(p, args, new_target),
            Native::IntlPluralRules => self.intl_plural_rules_construct(p, args, new_target),
            Native::IntlDateTimeFormat => self.intl_date_time_format_construct(p, args, new_target),
            Native::IntlDisplayNames => self.intl_display_names_construct(p, args, new_target),
            Native::IntlDurationFormat => self.intl_duration_format_construct(p, args, new_target),
            Native::IntlListFormat => self.intl_list_format_construct(p, args, new_target),
            Native::IntlLocale => self.intl_namespace_construct(p, native, args, new_target),
            Native::IntlRelativeTimeFormat => {
                self.intl_relative_time_format_construct(p, args, new_target)
            }
            Native::IntlSegmenter => self.intl_segmenter_construct(p, args, new_target),
            Native::TemporalDuration => self.temporal_duration_construct(p, args),
            Native::TemporalPlainTime => self.temporal_plain_time_construct(p, args),
            Native::TemporalPlainDate => self.temporal_plain_date_construct(p, args, new_target),
            Native::TemporalPlainDateTime => {
                self.temporal_plain_date_time_construct(p, args, new_target)
            }
            Native::TemporalPlainMonthDay | Native::TemporalPlainYearMonth => {
                self.temporal_calendar_projection_construct(p, native, args, new_target)
            }
            Native::TemporalZonedDateTime => {
                self.temporal_zoned_date_time_construct(p, args, new_target)
            }
            Native::TemporalInstant => self.temporal_instant_construct(p, args, new_target),
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
            | Native::RealmTypeError => {
                self.construct_error_with_new_target(p, native, args, new_target)
            }
            Native::String => {
                let value = self.string_constructor_value(p, args)?;
                self.box_primitive_object(value)
            }
            Native::Number => {
                let argument = args.first().copied().unwrap_or(Value::number(0.0));
                let primitive = if self.is_object_like(argument) {
                    self.to_primitive(p, argument, "number")?
                } else {
                    argument
                };
                let number = match self.heap.get(primitive) {
                    Some(Cell::BigInt(value)) => value.parse::<f64>().map_err(|_| {
                        self.type_error(p, "invalid BigInt numeric representation".into())
                    })?,
                    _ => self.to_number(p, primitive)?,
                };
                let value = Value::number(number);
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
