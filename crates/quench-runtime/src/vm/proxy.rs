use super::*;

/// The internal methods a Proxy handler's trap replaces, each named by its trap (ECMA-262
/// 10.5): the one list the trap atoms (`Vm::trap_atom`) derive from.
#[derive(Clone, Copy)]
pub(super) enum ProxyTrap {
    Get,
    Set,
    GetPrototypeOf,
    SetPrototypeOf,
    IsExtensible,
    PreventExtensions,
    GetOwnPropertyDescriptor,
    DefineProperty,
    Has,
    DeleteProperty,
    OwnKeys,
    Apply,
    Construct,
}

/// `ProxyTrap`'s names, in its order.
pub(super) const PROXY_TRAP_NAMES: [&str; 13] = [
    "get",
    "set",
    "getPrototypeOf",
    "setPrototypeOf",
    "isExtensible",
    "preventExtensions",
    "getOwnPropertyDescriptor",
    "defineProperty",
    "has",
    "deleteProperty",
    "ownKeys",
    "apply",
    "construct",
];

impl ProxyTrap {
    pub(super) fn name(self) -> &'static str {
        PROXY_TRAP_NAMES[self as usize]
    }
}

pub(super) const PROXY_TRAP_COUNT: usize = PROXY_TRAP_NAMES.len();

impl<H: Host> Vm<H> {
    /// The atom naming `trap`, interned once.
    pub(super) fn trap_atom(&self, trap: ProxyTrap) -> Atom {
        self.runtime_atoms.proxy_traps[trap as usize]
    }

    /// `receiver` seen through the proxies whose handlers have neither a `getOwnPropertyDescriptor`
    /// nor a `defineProperty` trap: the receiver's descriptor read and its data definition
    /// (OrdinarySetWithOwnDescriptor, steps 2.c to 2.e) forward to the target unchanged.
    pub(super) fn descriptor_forwarding_target(&self, mut receiver: Value) -> Value {
        while let Some((target, handler)) = self.proxy_parts(receiver)
            && self.handler_lacks_trap(handler, ProxyTrap::GetOwnPropertyDescriptor)
            && self.handler_lacks_trap(handler, ProxyTrap::DefineProperty)
        {
            receiver = target;
        }
        receiver
    }

    /// Whether `handler` has no `trap`, read without running guest code: a handler of plain
    /// data properties that names none. `false` leaves a lookup that may run guest code, or
    /// finds a trap, to the general path.
    pub(super) fn handler_lacks_trap(&self, handler: Value, trap: ProxyTrap) -> bool {
        !handler.is_null()
            && self.plain_property_lookup(handler, super::property_key::PropertyKey::string(self.trap_atom(trap))) == Some(None)
    }

    pub(super) fn proxy_call(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        let Some((target, handler)) = self.proxy_parts(proxy) else {
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
                    return Err(vm.type_error(p, super::call_printer::NOT_CALLABLE_MESSAGE.into()));
                }
                let trap_atom = vm.trap_atom(ProxyTrap::Apply);
                let trap = vm.get_property(p, handler, trap_atom)?;
                if !trap.is_undefined() && !trap.is_null() {
                    if !vm.is_function(trap) {
                        return Err(vm.type_error(p, "proxy apply trap is not callable".into()));
                    }
                    let arguments = vm.heap.alloc(Cell::Array {
                        object: crate::heap::ArrayObject::new(
                            Self::empty_object(vm.array_proto),
                            args.to_vec(),
                        ),
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
        let Some((target, handler)) = self.proxy_parts(proxy) else {
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
                if !vm.is_constructable(target) {
                    return Err(vm.type_error(p, "proxy target is not a constructor".into()));
                }
                if !vm.is_constructable(new_target) {
                    return Err(vm.type_error(p, "newTarget is not a constructor".into()));
                }
                let trap_atom = vm.trap_atom(ProxyTrap::Construct);
                let trap = vm.get_property(p, handler, trap_atom)?;
                if !trap.is_undefined() && !trap.is_null() {
                    if !vm.is_function(trap) {
                        return Err(vm.type_error(p, "proxy construct trap is not callable".into()));
                    }
                    let arguments = vm.heap.alloc(Cell::Array {
                        object: crate::heap::ArrayObject::new(
                            Self::empty_object(vm.array_proto),
                            args.to_vec(),
                        ),
                    });
                    let result =
                        vm.call_value(p, trap, handler, &[target, arguments, new_target])?;
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

    #[cfg(test)]
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
        self.with_call_roots([proxy], |vm| {
            let revoke = vm.native_with_env(Native::ProxyRevoke, proxy);
            vm.with_call_roots([revoke], |vm| {
                let result = vm.object();
                let proxy_atom = vm.intern_atom("proxy");
                let revoke_atom = vm.intern_atom("revoke");
                vm.set_property(result, proxy_atom, proxy)?;
                vm.set_property(result, revoke_atom, revoke)?;
                Ok(result)
            })
        })
    }

    pub(super) fn proxy_revoke(&mut self, revoke: Value) -> Result<Value, JsError> {
        let proxy = match self.heap.function_mut(revoke) {
            Some(FunctionRecord {
                kind: FunctionKind::Native(Native::ProxyRevoke),
                env,
                ..
            }) => std::mem::replace(env, Value::NULL),
            _ => return Err(JsError("invalid proxy revoke function".into())),
        };
        if proxy.is_null() {
            return Ok(Value::UNDEFINED);
        }
        let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get_mut(proxy)
        else {
            return Err(JsError("invalid proxy revoke target".into()));
        };
        *target = Value::NULL;
        *handler = Value::NULL;
        Ok(Value::UNDEFINED)
    }
}
