use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn set_function_source(
        &mut self,
        function: Value,
        source: &str,
    ) -> Result<(), JsError> {
        let source_atom = self.intern_atom("\0rqj:function-source");
        let source_value = self.heap.alloc(Cell::String(source.into()));
        self.set_property(function, source_atom, source_value)
    }

    pub(super) fn function_caller_is_restricted(&self, function: Value) -> bool {
        match self.heap.get(function) {
            Some(Cell::Function {
                kind: FunctionKind::Native(Native::FunctionBoundCall),
                ..
            }) => true,
            Some(Cell::Function {
                kind: FunctionKind::User(program_id, id) | FunctionKind::NumericUser(program_id, id),
                ..
            }) => self.programs.get(*program_id).is_some_and(|program| {
                program.functions.get(*id as usize).is_some_and(|function| {
                    function.strict
                        || function.is_async
                        || function.is_generator
                        || function.name.is_some_and(|name| {
                            (name as usize) < program.atoms.len()
                                && program.atoms[name as usize].as_bytes() == b"\0rqj:arrow"
                        })
                })
            }),
            _ => false,
        }
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
        let target_atom = self.intern_atom("\0rqj:bound-target");
        let this_atom = self.intern_atom("\0rqj:bound-this");
        let args_atom = self.intern_atom("\0rqj:bound-args");
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
        let length_atom = self.intern_atom("length");
        let bound_count = args.len().saturating_sub(1) as f64;
        let bound_length = if self
            .property_attributes(target, PropertyKey::string(length_atom))
            .is_some()
        {
            let target_length = self.get_property(p, target, length_atom)?;
            match target_length.as_number() {
                None => 0.0,
                Some(target_length) if target_length.is_infinite() => target_length.max(0.0),
                Some(target_length) => (target_length.trunc() - bound_count).max(0.0),
            }
        } else {
            0.0
        };
        self.set_builtin_value_named(function, "length", Value::number(bound_length))?;
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
        let target_name = self.get_property(p, target, name_atom)?;
        let target_name = match self.heap.get(target_name) {
            Some(Cell::String(name)) => name.to_string(),
            _ => String::new(),
        };
        self.set_builtin_function_name(function, &format!("bound {target_name}"))?;
        Ok(function)
    }

    pub(super) fn call_bound_function(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let env = self
            .active_native_env()
            .ok_or_else(|| JsError("invalid bound function".into()))?;
        let target_atom = self.intern_atom("\0rqj:bound-target");
        let this_atom = self.intern_atom("\0rqj:bound-this");
        let args_atom = self.intern_atom("\0rqj:bound-args");
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
