use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn bind_function(
        &mut self,
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
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.object();
        self.set_property(function, prototype_atom, prototype)?;
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
