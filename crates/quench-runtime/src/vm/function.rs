use super::*;

/// The forwarding layers `Vm::forwarded_call` peels from one call: an uncurried builtin is
/// two (its bound `call`, then `call`); more is left to the general call path, which needs
/// no bound since each layer there is a native call of its own.
const CALL_FORWARDING_DEPTH: usize = 4;

impl<H: Host> Vm<H> {
    pub(super) fn set_function_source(
        &mut self,
        function: Value,
        source: &str,
    ) -> Result<(), JsError> {
        let source_value = self.heap.alloc_string(source.into());
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

    /// The source text `Function.prototype.toString` reports for `function`: one recorded for
    /// it (a dynamic function's synthesized source), otherwise its definition's range of the
    /// program source, made a string on first request and shared by every closure of it.
    pub(super) fn function_source_value(&mut self, function: Value) -> Option<Value> {
        let source_atom = self.intern_atom("\0quench:function-source");
        if let Some(source) = self.own_property(function, source_atom) {
            return Some(source);
        }
        if let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = self.heap.function(function)
            && *native != Native::FunctionBoundCall
            && let Some(name) = self.original_function_name(function)
        {
            let source = native_function_source(self.atom_name(name));
            return Some(self.heap.alloc_string(source.as_str().into()));
        }
        let Some(FunctionRecord {
            kind: FunctionKind::User(program, id),
            ..
        }) = self.heap.function(function)
        else {
            return None;
        };
        let (program, id) = (*program, *id);
        if let Some(source) = self.programs.function_source(program, id) {
            return Some(source);
        }
        let residual = self.programs.residual(program)?;
        let span = residual.functions.get(id as usize)?.header().source_span?;
        let text = residual.source.as_ref()?.slice(span.start, span.end)?;
        let source = self.heap.alloc_string(text.into());
        self.programs
            .cache_function_source(program, id, source)
            .or(Some(source))
    }

    pub(super) fn function_caller_is_restricted(&self, function: Value) -> bool {
        match self.heap.function(function) {
            Some(FunctionRecord {
                kind: FunctionKind::User(program_id, id),
                ..
            }) => self.programs.get(*program_id).is_some_and(|program| {
                program
                    .function_get(*id as usize)
                    .is_some_and(|function| function.has_restricted_legacy_caller_access())
            }),
            Some(FunctionRecord {
                kind: FunctionKind::Native(_),
                ..
            }) => true,
            _ => false,
        }
    }

    /// `function_caller`, handed out: a caller whose `prototype` was deferred
    /// (`Function::prototype_unobservable`) gets it first, as its creation would have made
    /// it, so the value is the ordinary function it stands for.
    pub(super) fn function_caller_value(&mut self, function: Value) -> Result<Value, JsError> {
        // Legacy reflection reads the frame stack (`Vm::materialize_deferred_lane`).
        self.materialize_deferred_lane();
        let caller = self.function_caller(function);
        let Some(&FunctionRecord { kind: FunctionKind::User(program, id), realm, .. }) = self.heap.function(caller) else {
            return Ok(caller);
        };
        let deferred = self
            .programs
            .get(program)
            .is_some_and(|code| code.function_get(id as usize).is_some_and(|body| body.prototype_unobservable));
        let atom = self.intern_atom("prototype");
        if deferred && self.own_property(caller, atom).is_none() {
            let parent = self.realm_object_prototype(realm);
            let prototype = self.heap.alloc(Cell::Object(Self::empty_object(parent)));
            self.define_instance_prototype(caller, atom, prototype, false, false)?;
        }
        Ok(caller)
    }

    fn function_caller(&self, function: Value) -> Value {
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
                CallContext::Function(caller) | CallContext::Constructor(caller) => {
                    let Some(FunctionRecord {
                        kind:
                            FunctionKind::User(program_id, id),
                        realm,
                        ..
                    }) = self.heap.function(caller)
                    else {
                        return Value::NULL;
                    };
                    let visible = *realm == self.realm.globals
                        && self.programs.get(*program_id).is_some_and(|program| {
                            program.function_get(*id as usize).is_some_and(|metadata| {
                                !metadata.strict && !metadata.is_async && !metadata.is_generator
                            })
                        });
                    return if visible { caller } else { Value::NULL };
                }
            }
        }
    }

    pub(super) fn function_arguments(&mut self, function: Value) -> Result<Value, JsError> {
        self.materialize_deferred_lane();
        let Some(frame) = self
            .frames
            .iter()
            .rfind(|frame| frame.context.callable() == Some(function))
        else {
            return Ok(Value::NULL);
        };
        let program = self.programs.get(frame.program);
        let metadata = program
            .as_deref()
            .and_then(|program| program.function_get(frame.function as usize));
        let expose_callee = metadata.is_some_and(|metadata| metadata.simple_parameters);
        let reflected = (0..frame.reflected_parameters).map(|parameter| {
            let metadata = metadata.expect("reflected parameters belong to a live function");
            match metadata
                .local_registers
                .iter()
                .find(|entry| entry.local == parameter)
            {
                Some(entry) => frame.registers[usize::from(entry.register)],
                None => frame.locals[usize::from(parameter)],
            }
        });
        let args = crate::heap::ArrayElements::from(
            reflected
                .chain(frame.original_arguments.iter().copied())
                .collect::<Vec<_>>(),
        );
        let arguments = self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.realm_object_prototype(self.realm.globals)),
                args.clone(),
            ),
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
            return Err(JsError(super::call_printer::NOT_CALLABLE_MESSAGE.into()));
        }
        let env = self.object();
        let target_atom = self.runtime_atoms.bound_target;
        let this_atom = self.runtime_atoms.bound_this;
        let args_atom = self.runtime_atoms.bound_args;
        self.set_property(env, target_atom, target)?;
        self.set_property(
            env,
            this_atom,
            args.first().copied().unwrap_or(Value::UNDEFINED),
        )?;
        let bound_args = self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                args.get(1..).unwrap_or_default().to_vec(),
            ),
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
            .set_proto(prototype);
        let target = self
            .heap
            .root_value(target_root)
            .expect("bound target root remains live");
        let length_atom = self.length_atom;
        let length_key = self.heap.alloc_string(self.atom_value(length_atom));
        // HasOwnProperty(Target, "length"): [[GetOwnProperty]], a Proxy's through its trap.
        let bound_length = if self.own_property_enumerable(p, target, length_key)?.is_some() {
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
            _ if let Some(name) = self.heap.string(target_name)  => name.to_string(),
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
        let (target, receiver, bound_args) = self
            .bound_function_parts(env)
            .ok_or_else(|| JsError("invalid bound function".into()))?;
        if bound_args.is_empty() {
            // `Function.prototype.call` bound to a function, as Node's primordials uncurry every
            // method: the call is that function's, with the first argument as its receiver.
            if matches!(
                self.heap.function(target),
                Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionCall), .. })
            ) {
                let this = args.first().copied().unwrap_or(Value::UNDEFINED);
                return self.call_value(p, receiver, this, args.get(1..).unwrap_or_default());
            }
            return self.call_value(p, target, receiver, args);
        }
        let mut combined = bound_args.to_vec();
        combined.extend_from_slice(args);
        self.call_value(p, target, receiver, &combined)
    }

    /// The call a call of `callee` with `receiver` and `args` makes without an observable
    /// step of its own: a bound function without bound arguments calls its target with its
    /// bound `this` and the same arguments (ECMA-262 10.4.1.1), and `Function.prototype.call`
    /// calls its receiver with the first argument as `this` and the rest (20.2.3.3), so an
    /// uncurried builtin (`bind.bind(call)(f)`) calls `f`. `None` unless at least one such
    /// layer was peeled within `CALL_FORWARDING_DEPTH`; the target may still be anything,
    /// which the caller checks before making the call itself, and otherwise makes the
    /// original call.
    pub(super) fn forwarded_call<'a>(
        &self,
        mut callee: Value,
        mut receiver: Value,
        mut args: &'a [Value],
    ) -> Option<(Value, Value, &'a [Value])> {
        for depth in 0..CALL_FORWARDING_DEPTH {
            match self.heap.function(callee) {
                Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionBoundCall), env, .. }) => {
                    let (target, this, bound) = self.bound_function_parts(*env)?;
                    if !bound.is_empty() {
                        return None;
                    }
                    (callee, receiver) = (target, this);
                }
                Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionCall), .. }) => {
                    callee = receiver;
                    receiver = args.first().copied().unwrap_or(Value::UNDEFINED);
                    args = args.get(1..).unwrap_or_default();
                }
                _ => return (depth != 0).then_some((callee, receiver, args)),
            }
        }
        None
    }

    /// `forwarded_call` of a call whose `args` the caller owns, as an applied list's copied
    /// elements: a bound function's bound arguments then lead the values it passes on
    /// (ECMA-262 10.4.1.1), which `forwarded_call` leaves to the original call.
    pub(super) fn forwarded_list_call<const N: usize>(
        &self,
        mut callee: Value,
        mut receiver: Value,
        mut args: smallvec::SmallVec<[Value; N]>,
    ) -> Option<(Value, Value, smallvec::SmallVec<[Value; N]>)> {
        for depth in 0..CALL_FORWARDING_DEPTH {
            match self.heap.function(callee) {
                Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionBoundCall), env, .. }) => {
                    let (target, this, bound) = self.bound_function_parts(*env)?;
                    args.insert_many(0, bound.iter().copied());
                    (callee, receiver) = (target, this);
                }
                Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionCall), .. }) => {
                    callee = receiver;
                    receiver = if args.is_empty() { Value::UNDEFINED } else { args.remove(0) };
                }
                _ => return (depth != 0).then_some((callee, receiver, args)),
            }
        }
        None
    }

    /// A bound function's target, bound `this` and bound arguments, read from the
    /// environment object `Function.prototype.bind` made (`env`); `None` for any other.
    pub(super) fn bound_function_parts(&self, env: Value) -> Option<(Value, Value, &[Value])> {
        let target = self.own_property(env, self.runtime_atoms.bound_target)?;
        let receiver = self.own_property(env, self.runtime_atoms.bound_this).unwrap_or(Value::UNDEFINED);
        let arguments = self
            .own_property(env, self.runtime_atoms.bound_args)
            .and_then(|list| self.heap.array_elements(list))
            .map_or(&[][..], |elements| elements.as_slice());
        Some((target, receiver, arguments))
    }
}

/// `NativeFunction` source text (ECMA-262 20.2.3.5) for a builtin named `name`: the name
/// stands in the text only where it is an identifier, or a `get `/`set ` accessor's or a
/// well-known symbol's identifier member.
fn native_function_source(name: &str) -> String {
    let property_name = name
        .strip_prefix("get ")
        .or_else(|| name.strip_prefix("set "))
        .unwrap_or(name);
    let symbol_member = property_name
        .strip_prefix("[Symbol.")
        .and_then(|name| name.strip_suffix(']'));
    let source_name = if oxc_syntax::identifier::is_identifier_name(property_name)
        || symbol_member.is_some_and(oxc_syntax::identifier::is_identifier_name)
    {
        name
    } else {
        ""
    };
    format!("function {source_name}() {{ [native code] }}")
}
