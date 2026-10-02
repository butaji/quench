use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn proxy_call(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(proxy).cloned()
        else {
            return Err(JsError("proxy call target is invalid".into()));
        };
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        self.with_call_roots(
            [proxy, target, handler, this]
                .into_iter()
                .chain(args.iter().copied()),
            |vm| {
                if !vm.is_function(target) {
                    return Err(vm.type_error(p, "value is not callable".into()));
                }
                let trap_atom = vm.intern_atom("apply");
                let trap = vm.get_property(p, handler, trap_atom)?;
                if !trap.is_undefined() && !trap.is_null() {
                    if !vm.is_function(trap) {
                        return Err(vm.type_error(p, "proxy apply trap is not callable".into()));
                    }
                    let arguments = vm.heap.alloc(Cell::Array {
                        object: Self::empty_object(vm.array_proto),
                        elements: Rc::new(args.to_vec()),
                    });
                    return vm.call_value(p, trap, handler, &[target, this, arguments]);
                }
                vm.call_value(p, target, this, args)
            },
        )
    }

    pub(super) fn proxy_construct(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
        new_target: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(proxy).cloned()
        else {
            return Err(JsError("proxy construct target is invalid".into()));
        };
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        self.with_call_roots(
            [proxy, target, handler, new_target]
                .into_iter()
                .chain(args.iter().copied()),
            |vm| {
                if !vm.is_constructable(p, target) {
                    return Err(vm.type_error(p, "proxy target is not a constructor".into()));
                }
                let trap_atom = vm.intern_atom("construct");
                let trap = vm.get_property(p, handler, trap_atom)?;
                if !trap.is_undefined() && !trap.is_null() {
                    if !vm.is_function(trap) {
                        return Err(vm.type_error(p, "proxy construct trap is not callable".into()));
                    }
                    if !vm.is_constructable(p, new_target) {
                        return Err(vm.type_error(p, "newTarget is not a constructor".into()));
                    }
                    let arguments = vm.heap.alloc(Cell::Array {
                        object: Self::empty_object(vm.array_proto),
                        elements: Rc::new(args.to_vec()),
                    });
                    let result = vm.call_value(p, trap, handler, &[target, arguments, new_target])?;
                    if !vm.is_object_like(result) {
                        return Err(
                            vm.type_error(p, "proxy construct trap must return an object".into())
                        );
                    }
                    return Ok(result);
                }
                vm.construct_value_with_new_target(p, target, new_target, args)
            },
        )
    }

    pub(super) fn proxy_target(&self, mut value: Value) -> Value {
        while let Some(Cell::Proxy { target, .. }) = self.heap.get(value) {
            value = *target;
        }
        value
    }

    pub(super) fn proxy_revocable(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let proxy = self.construct_native(p, Native::Proxy, args)?;
        let revoke = self.native_with_env(Native::ProxyRevoke, proxy);
        let result = self.object();
        let proxy_atom = self.intern_atom("proxy");
        let revoke_atom = self.intern_atom("revoke");
        let state_atom = self.intern_atom("\0rqj:proxy-revoke-target");
        self.set_property(result, proxy_atom, proxy)?;
        self.set_property(result, revoke_atom, revoke)?;
        self.set_property(result, state_atom, proxy)?;
        self.set_property_attributes(
            result,
            PropertyKey::string(state_atom),
            PropertyAttributes {
                enumerable: false,
                configurable: false,
                ..DEFAULT_PROPERTY_ATTRIBUTES
            },
        );
        Ok(result)
    }

    pub(super) fn proxy_revoke(&mut self, revoke: Value) -> Result<Value, JsError> {
        let proxy = match self.heap.get(revoke) {
            Some(Cell::Function { env, .. }) => *env,
            _ => return Err(JsError("invalid proxy revoke function".into())),
        };
        let Some(Cell::Proxy { handler, .. }) = self.heap.get_mut(proxy) else {
            return Err(JsError("invalid proxy revoke target".into()));
        };
        *handler = Value::NULL;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn proxy_revoke_receiver(&mut self, receiver: Value) -> Result<Value, JsError> {
        let atom = self.intern_atom("\0rqj:proxy-revoke-target");
        let proxy = self
            .own_property(receiver, atom)
            .ok_or_else(|| JsError("invalid proxy revoke function".into()))?;
        let Some(Cell::Proxy { handler, .. }) = self.heap.get_mut(proxy) else {
            return Err(JsError("invalid proxy revoke target".into()));
        };
        *handler = Value::NULL;
        Ok(Value::UNDEFINED)
    }
}
