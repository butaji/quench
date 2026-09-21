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
        let Some(current_proto) = self.object_data(target).map(|object| object.proto) else {
            return Err(JsError(
                "Object.setPrototypeOf target is not an object".into(),
            ));
        };
        if self.non_extensible.contains(&target) && current_proto != proto {
            return Err(JsError(
                "cannot change prototype of non-extensible object".into(),
            ));
        }
        self.object_data_mut(target)
            .expect("object validated")
            .proto = proto;
        Ok(target)
    }

    pub(super) fn integrity_bool(value: bool) -> Value {
        if value { Value::TRUE } else { Value::FALSE }
    }

    pub(super) fn check_property_write(
        &self,
        object: Value,
        atom: Atom,
        exists: bool,
    ) -> Result<(), JsError> {
        if !exists && self.non_extensible.contains(&object) {
            return Err(JsError(
                "cannot add property to non-extensible object".into(),
            ));
        }
        if exists
            && self
                .descriptors
                .get(&(object, atom))
                .is_some_and(|attributes| !attributes.writable)
        {
            return Err(JsError("cannot write non-writable property".into()));
        }
        Ok(())
    }

    pub(super) fn object_prevent_extensions(&mut self, args: &[Value]) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.object_data(target).is_some() {
            self.non_extensible.insert(target);
        }
        Ok(target)
    }

    pub(super) fn object_is_extensible(&self, args: &[Value]) -> bool {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        self.object_data(target).is_some() && !self.non_extensible.contains(&target)
    }

    pub(super) fn object_set_integrity(
        &mut self,
        args: &[Value],
        freeze: bool,
    ) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.object_data(target).is_none() {
            return Ok(target);
        }
        self.non_extensible.insert(target);
        let keys = self
            .object_data(target)
            .map(|data| {
                self.ordered_shape(data)
                    .into_iter()
                    .map(|(atom, _)| atom)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for atom in keys {
            let attributes = self
                .descriptors
                .entry((target, atom))
                .or_insert(DEFAULT_PROPERTY_ATTRIBUTES);
            attributes.configurable = false;
            if freeze {
                attributes.writable = false;
            }
        }
        Ok(target)
    }

    pub(super) fn object_is_integrity_level(&self, args: &[Value], freeze: bool) -> bool {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        let Some(data) = self.object_data(target) else {
            return true;
        };
        if !self.non_extensible.contains(&target) {
            return false;
        }
        self.ordered_shape(data).into_iter().all(|(atom, _)| {
            let attributes = self
                .descriptors
                .get(&(target, atom))
                .copied()
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            !attributes.configurable && (!freeze || !attributes.writable)
        })
    }
}
