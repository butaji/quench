use super::*;

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
        let mut caller = self.function_caller(function);
        if caller.as_known_closure().is_some() {
            caller = self.known_closure_function(caller)?;
        }
        let Some(&FunctionRecord { kind: FunctionKind::User(program, id), realm, .. }) = self.heap.function(caller) else {
            return Ok(caller);
        };
        let deferred = self
            .programs
            .get(program)
            .is_some_and(|code| code.functions.get(id as usize).is_some_and(|slot| slot.header().prototype_unobservable));
        let atom = self.intern_atom("prototype");
        if deferred && self.own_property(caller, atom).is_none() {
            let parent = self.realm_object_prototype(realm);
            let prototype = self.heap.alloc(Cell::Object(Self::empty_object(parent)));
            self.define_instance_prototype(caller, atom, prototype, false, false)?;
        }
        Ok(caller)
    }

    /// The function object a call-only closure (`Value::known_closure`) stands for, made when
    /// legacy reflection hands it out: each frame running it names the object from then on,
    /// so further reads in those calls return the same one. A closure of another program than
    /// the running one is not handed out.
    fn known_closure_function(&mut self, known: Value) -> Result<Value, JsError> {
        let Some((id, env)) = known.as_known_closure() else {
            return Ok(known);
        };
        let program = self.active_program;
        let Some(code) = self.programs.get(program) else {
            return Ok(Value::NULL);
        };
        if !self.frames.iter().any(|frame| frame.context.callable() == Some(known) && frame.program == program) {
            return Ok(Value::NULL);
        }
        let function = self.closure(&code, id, env)?;
        for frame in self.frames.iter_mut().filter(|frame| frame.context.callable() == Some(known)) {
            frame.context = CallContext::Function(function);
        }
        Ok(function)
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
            if self.crosses_opaque_native(index) {
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
                    return self.visible_caller(caller, index);
                }
            }
        }
    }

    /// Whether an opaque native call's boundary sits at frame `index`, which hides the frame's
    /// caller.
    fn crosses_opaque_native(&self, index: usize) -> bool {
        self.realm.promise.active_native.iter().any(|activation| {
            activation.frame_depth == index
                && activation.boundary == super::activation::NativeCallBoundary::Opaque
        })
    }

    /// The caller `caller` of frame `frame`, when legacy reflection shows it: a plain sloppy
    /// function, else null.
    fn visible_caller(&self, caller: Value, frame: usize) -> Value {
        // A call-only closure's function is a plain sloppy one of its frame's program
        // (`Function::prototype_unobservable`).
        if let Some((id, _)) = caller.as_known_closure() {
            let visible = self.programs.get(self.frames[frame].program).is_some_and(|program| {
                program.function_get(id as usize).is_some_and(|metadata| !metadata.strict)
            });
            return if visible { caller } else { Value::NULL };
        }
        let Some(FunctionRecord {
            kind: FunctionKind::User(program_id, id),
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
        if visible { caller } else { Value::NULL }
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
            vm.initialize_arguments_object(arguments, Some(function), &args.values_cow(), expose_callee)?;
            Ok(arguments)
        })
    }

    /// The own data property `atom` of an ordinary object whose shape holds every own
    /// property's attributes and no getter (`Vm::plain_function_length`'s test): its value,
    /// read observing nothing. `None` for an accessor, an absent name or any other object.
    pub(super) fn plain_own_data(&self, object: Value, atom: Atom) -> Option<Value> {
        let data = self.object_data(object)?;
        if data.has_side_descriptors() || self.shapes[data.shape() as usize].facts.slot_getters() {
            return None;
        }
        let slot = self.shape_slot(data.shape(), atom)?;
        self.heap.property_get(data, slot).filter(|value| !self.is_accessor_pair(data.shape(), *value))
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

/// The call a forwarding call makes (`Vm::forwarded_call`), with the facts the walk read on
/// the way, so no caller reads them again.
pub(super) struct Forwarded<A> {
    pub(super) callee: Value,
    pub(super) receiver: Value,
    pub(super) args: A,
    /// The callee's kind; `None` for a callee that is no function.
    pub(super) kind: Option<FunctionKind>,
    /// Whether the outermost layer was `Function.prototype.call`.
    pub(super) through_call: bool,
}

