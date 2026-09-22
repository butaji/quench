use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn object_get_own_property_descriptor(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = self.proxy_target(args.first().copied().unwrap_or(Value::UNDEFINED));
        let target = self.box_object(target)?;
        let key = self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let atom = self.intern_atom(&key);
        let Some(value) = self.own_property(target, atom) else {
            return Ok(Value::UNDEFINED);
        };
        let attributes = self
            .descriptors
            .get(&(target, atom))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let descriptor = self.object();
        if attributes.accessor {
            for (name, value) in [
                ("get", attributes.getter.unwrap_or(Value::UNDEFINED)),
                ("set", attributes.setter.unwrap_or(Value::UNDEFINED)),
                ("enumerable", Self::integrity_bool(attributes.enumerable)),
                (
                    "configurable",
                    Self::integrity_bool(attributes.configurable),
                ),
            ] {
                let atom = self.intern_atom(name);
                self.set_property(descriptor, atom, value)?;
            }
            return Ok(descriptor);
        }
        for (name, value) in [
            ("value", value),
            ("writable", Self::integrity_bool(attributes.writable)),
            ("enumerable", Self::integrity_bool(attributes.enumerable)),
            (
                "configurable",
                Self::integrity_bool(attributes.configurable),
            ),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, value)?;
        }
        Ok(descriptor)
    }

    pub(super) fn object_get_own_property_descriptors(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = self.proxy_target(args.first().copied().unwrap_or(Value::UNDEFINED));
        let target = self.box_object(target)?;
        let data = self.object_data(target).expect("boxed target is object");
        let keys = self.ordered_shape(data);
        let result = self.object();
        for (atom, _) in keys {
            let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
            let descriptor = self.object_get_own_property_descriptor(p, &[target, key])?;
            self.set_property(result, atom, descriptor)?;
        }
        Ok(result)
    }
}
