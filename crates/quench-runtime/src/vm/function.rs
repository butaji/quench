use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn set_function_source(
        &mut self,
        function: Value,
        source: &str,
    ) -> Result<(), JsError> {
        let source_value = self.heap.alloc(Cell::String(source.into()));
        self.set_function_source_value(function, source_value)
    }

    pub(super) fn set_function_source_value(
        &mut self,
        function: Value,
        source: Value,
    ) -> Result<(), JsError> {
        let source_atom = self.intern_atom("\0quench:function-source");
        self.set_property(function, source_atom, source)
    }

    pub(super) fn function_caller_is_restricted(&self, function: Value) -> bool {
        match self.heap.get(function) {
            Some(Cell::Function {
                kind: FunctionKind::User(program_id, id) | FunctionKind::NumericUser(program_id, id),
                ..
            }) => self.programs.get(*program_id).is_some_and(|program| {
                program.functions.get(*id as usize).is_some_and(|function| {
                    function.has_restricted_legacy_caller_access()
                })
            }),
            Some(Cell::Function {
                kind: FunctionKind::Native(_),
                ..
            }) => true,
            _ => false,
        }
    }

    pub(super) fn function_caller(&self, function: Value) -> Value {
        let Some(mut index) = self
            .frames
            .iter()
            .rposition(|frame| frame.context.callable() == Some(function))
        else {
            return Value::NULL;
        };
        loop {
            if self.realm.promise.active_native.iter().any(|activation| {
                activation.frame_depth == index
                    && activation.boundary == super::activation::NativeCallBoundary::Opaque
            }) {
                return Value::NULL;
            }
            let Some(parent) = index.checked_sub(1) else {
                return Value::NULL;
            };
            index = parent;
            match self.frames[index].context {
                CallContext::DirectEval(_) => continue,
                CallContext::Internal | CallContext::IndirectEval(_) => return Value::NULL,
                CallContext::Function(caller) => {
                    let Some(Cell::Function {
                        kind:
                            FunctionKind::User(program_id, id)
                            | FunctionKind::NumericUser(program_id, id),
                        realm,
                        ..
                    }) = self.heap.get(caller)
                    else {
                        return Value::NULL;
                    };
                    let visible = *realm == self.realm.globals
                        && self.programs.get(*program_id).is_some_and(|program| {
                            program.functions.get(*id as usize).is_some_and(|metadata| {
                                !metadata.strict && !metadata.is_async && !metadata.is_generator
                            })
                        });
                    return if visible { caller } else { Value::NULL };
                }
            }
        }
    }

    pub(super) fn function_arguments(&mut self, function: Value) -> Result<Value, JsError> {
        let Some(frame) = self
            .frames
            .iter()
            .rfind(|frame| frame.context.callable() == Some(function))
        else {
            return Ok(Value::NULL);
        };
        let expose_callee = self
            .programs
            .get(frame.program)
            .and_then(|program| {
                program
                    .functions
                    .get(frame.function as usize)
                    .map(|metadata| metadata.simple_parameters)
            })
            .unwrap_or(false);
        let args = Rc::new(frame.original_arguments.clone());
        let arguments = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.realm_object_prototype(self.realm.globals)),
            elements: args.clone(),
        });
        self.with_call_roots([arguments], |vm| {
            vm.initialize_arguments_object(arguments, Some(function), &args, expose_callee)?;
            Ok(arguments)
        })
    }

    pub(super) fn bind_function(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_function(target) {
            return Err(JsError("value is not callable".into()));
        }
        let env = self.object();
        let target_atom = self.intern_atom("\0quench:bound-target");
        let this_atom = self.intern_atom("\0quench:bound-this");
        let args_atom = self.intern_atom("\0quench:bound-args");
        self.set_property(env, target_atom, target)?;
        self.set_property(
            env,
            this_atom,
            args.first().copied().unwrap_or(Value::UNDEFINED),
        )?;
        let bound_args = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(args.get(1..).unwrap_or_default().to_vec()),
        });
        self.set_property(env, args_atom, bound_args)?;
        let function = self.native_with_env(Native::FunctionBoundCall, env);
        let function_root = self.heap.root(function);
        let target_root = self.heap.root(target);
        let outcome = self.initialize_bound_function_metadata(
            p,
            function_root,
            target_root,
            args.len().saturating_sub(1) as f64,
        );
        self.heap.release_root(target_root);
        self.heap.release_root(function_root);
        outcome
    }

    fn initialize_bound_function_metadata(
        &mut self,
        p: &ResidualProgram,
        function_root: RootId,
        target_root: RootId,
        bound_count: f64,
    ) -> Result<Value, JsError> {
        let target = self
            .heap
            .root_value(target_root)
            .expect("bound target root remains live");
        let prototype = self.object_get_prototype_of(p, target)?;
        let function = self
            .heap
            .root_value(function_root)
            .expect("bound function root remains live");
        self.object_data_mut(function)
            .expect("bound function has object storage")
            .proto = prototype;
        let target = self
            .heap
            .root_value(target_root)
            .expect("bound target root remains live");
        let length_atom = self.intern_atom("length");
        let length_key = self.heap.alloc(Cell::String(self.atom_value(length_atom)));
        let descriptor = self.object_get_own_property_descriptor(p, &[target, length_key])?;
        let bound_length = if !descriptor.is_undefined() {
            let target = self
                .heap
                .root_value(target_root)
                .expect("bound target root remains live");
            let target_length = self.get_property(p, target, length_atom)?;
            match target_length.as_number() {
                None => 0.0,
                Some(target_length) if target_length.is_infinite() => target_length.max(0.0),
                Some(target_length) => (target_length.trunc() - bound_count).max(0.0),
            }
        } else {
            0.0
        };
        let function = self
            .heap
            .root_value(function_root)
            .expect("bound function root remains live");
        self.set_builtin_value_named(function, "length", Value::number(bound_length))?;
        let function = self
            .heap
            .root_value(function_root)
            .expect("bound function root remains live");
        self.set_property_attributes(
            function,
            PropertyKey::string(length_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let name_atom = self.intern_atom("name");
        let target = self
            .heap
            .root_value(target_root)
            .expect("bound target root remains live");
        let target_name = self.get_property(p, target, name_atom)?;
        let target_name = match self.heap.get(target_name) {
            Some(Cell::String(name)) => name.to_string(),
            _ => String::new(),
        };
        let function = self
            .heap
            .root_value(function_root)
            .expect("bound function root remains live");
        self.set_builtin_function_name(function, &format!("bound {target_name}"))?;
        Ok(self
            .heap
            .root_value(function_root)
            .expect("bound function root remains live"))
    }

    pub(super) fn call_bound_function(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let env = self
            .active_native_env()
            .ok_or_else(|| JsError("invalid bound function".into()))?;
        let target_atom = self.intern_atom("\0quench:bound-target");
        let this_atom = self.intern_atom("\0quench:bound-this");
        let args_atom = self.intern_atom("\0quench:bound-args");
        let target = self
            .own_property(env, target_atom)
            .ok_or_else(|| JsError("invalid bound function".into()))?;
        let receiver = self
            .own_property(env, this_atom)
            .unwrap_or(Value::UNDEFINED);
        let bound_args = self
            .own_property(env, args_atom)
            .and_then(|value| match self.heap.get(value) {
                Some(Cell::Array { elements, .. }) => Some(elements.as_ref().clone()),
                _ => None,
            })
            .unwrap_or_default();
        let mut combined = bound_args;
        combined.extend_from_slice(args);
        self.call_value(p, target, receiver, &combined)
    }
}
