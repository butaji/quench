use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn object_get_prototype_of(&self, value: Value) -> Result<Value, JsError> {
        self.object_data(value)
            .map(|object| object.proto)
            .ok_or_else(|| JsError("Object.getPrototypeOf target is not an object".into()))
    }

    pub(super) fn object_set_prototype_of(
        &mut self,
        target: Value,
        proto: Value,
    ) -> Result<Value, JsError> {
        if !proto.is_null() && self.object_data(proto).is_none() {
            return Err(JsError("Object prototype is not an object".into()));
        }
        let Some(object) = self.object_data_mut(target) else {
            return Err(JsError(
                "Object.setPrototypeOf target is not an object".into(),
            ));
        };
        object.proto = proto;
        Ok(target)
    }

    pub(super) fn object_assign(&mut self, args: &[Value]) -> Result<Value, JsError> {
        let target = args
            .first()
            .copied()
            .filter(|value| self.object_data(*value).is_some())
            .ok_or_else(|| JsError("Object.assign target is not an object".into()))?;
        for source in args.iter().copied().skip(1) {
            let Some(data) = self.object_data(source) else {
                continue;
            };
            let shape = data.shape();
            let keys = self.shapes[shape as usize].clone();
            let values = keys
                .iter()
                .enumerate()
                .filter_map(|(slot, atom)| {
                    self.heap
                        .property_get(data, slot)
                        .map(|value| (*atom, value))
                })
                .collect::<Vec<_>>();
            for (atom, value) in values {
                self.set_property(target, atom, value)?;
            }
        }
        Ok(target)
    }

    pub(super) fn object_keys(&mut self, object: Value) -> Result<Value, JsError> {
        let keys = self
            .object_data(object)
            .map(|data| self.shapes[data.shape() as usize].clone())
            .ok_or_else(|| JsError("Object.keys target is not an object".into()))?;
        let values = keys
            .iter()
            .map(|atom| self.heap.alloc(Cell::String(self.atom_name(*atom).into())))
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    pub(super) fn call_object_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::ObjectKeys => {
                self.object_keys(args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGetOwnPropertyNames => {
                self.object_keys(args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectCreate => {
                let proto = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !proto.is_null() && self.object_data(proto).is_none() {
                    return Err(JsError("Object prototype is not an object".into()));
                }
                Ok(self.heap.alloc(Cell::Object(Self::empty_object(proto))))
            }
            Native::ObjectAssign => self.object_assign(args),
            Native::ObjectGetPrototypeOf => {
                self.object_get_prototype_of(args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectSetPrototypeOf => self.object_set_prototype_of(
                args.first().copied().unwrap_or(Value::UNDEFINED),
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::ObjectHasOwn => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                if target.is_null() || target.is_undefined() {
                    return Err(JsError("Object.hasOwn target is nullish".into()));
                }
                let text = self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let key = self.intern_atom(&text);
                Ok(if self.own_property(target, key).is_some() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            _ => Err(JsError("invalid object native".into())),
        }
    }
}
