use super::*;

impl<H: Host> Vm<H> {
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
