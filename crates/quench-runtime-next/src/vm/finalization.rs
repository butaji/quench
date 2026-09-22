use super::*;
use crate::heap::{FinalizationEntries, FinalizationEntry};

impl<H: Host> Vm<H> {
    pub(super) fn is_finalization_native(native: Native) -> bool {
        matches!(
            native,
            Native::FinalizationRegistryRegister | Native::FinalizationRegistryUnregister
        )
    }

    pub(super) fn install_finalization_registry(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let registry = self.native_value(Native::FinalizationRegistry);
        let prototype = self.object();
        self.finalization_registry_proto = prototype;
        for (name, native) in [
            ("register", Native::FinalizationRegistryRegister),
            ("unregister", Native::FinalizationRegistryUnregister),
        ] {
            self.set_named(program, prototype, name, self.native_value(native))?;
        }
        self.set_named(program, registry, "prototype", prototype)?;
        self.global(program, "FinalizationRegistry", registry)
    }

    pub(super) fn construct_finalization_registry_native(
        &mut self,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !matches!(
            self.heap.get(callback),
            Some(Cell::Function { .. } | Cell::Proxy { .. })
        ) {
            return Err(JsError(
                "FinalizationRegistry callback is not callable".into(),
            ));
        }
        Ok(self.heap.alloc(Cell::FinalizationRegistry {
            object: Self::empty_object(self.finalization_registry_proto),
            callback,
            entries: Box::new(FinalizationEntries::default()),
        }))
    }

    pub(super) fn call_finalization_registry_native(
        &mut self,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::FinalizationRegistryRegister => {
                let target = self.weak_key(args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let target = self
                    .heap
                    .weak_handle(target)
                    .ok_or_else(|| JsError("FinalizationRegistry target is not live".into()))?;
                let held = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let token = match args.get(2).copied().filter(|value| !value.is_undefined()) {
                    None => None,
                    Some(value) => {
                        let value = self.weak_key(value)?;
                        Some(self.heap.weak_handle(value).ok_or_else(|| {
                            JsError("FinalizationRegistry unregister token is not live".into())
                        })?)
                    }
                };
                let Some(Cell::FinalizationRegistry { entries, .. }) = self.heap.get_mut(this)
                else {
                    return Err(JsError("invalid FinalizationRegistry receiver".into()));
                };
                entries.push(FinalizationEntry {
                    target,
                    held,
                    token,
                });
                Ok(this)
            }
            Native::FinalizationRegistryUnregister => {
                let token = self.weak_key(args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let Some(token) = self.heap.weak_handle(token) else {
                    return Ok(Value::FALSE);
                };
                let Some(Cell::FinalizationRegistry { entries, .. }) = self.heap.get_mut(this)
                else {
                    return Err(JsError("invalid FinalizationRegistry receiver".into()));
                };
                let before = entries.len();
                entries.retain(|entry| entry.token != Some(token));
                Ok(if entries.len() != before {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            _ => Err(JsError("invalid finalization native".into())),
        }
    }
}

include!("disposal.rs");
