impl<H: Host> Vm<H> {
    pub(super) fn has_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<bool, JsError> {
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(object).cloned()
        {
            if handler.is_null() {
                return Err(JsError("cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("has");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[target, key])?;
                return Ok(self.truthy(result));
            }
            return self.has_property(p, target, key);
        }
        if self.object_data(object).is_none() {
            return Err(JsError("right-hand side of 'in' is not an object".into()));
        }
        let key = self.to_string(p, key)?;
        let atom = self.intern_atom(&key);
        let mut current = object;
        loop {
            if self.own_property(current, atom).is_some() {
                return Ok(true);
            }
            let Some(data) = self.object_data(current) else {
                return Ok(false);
            };
            if data.proto.is_null() {
                return Ok(false);
            }
            current = data.proto;
        }
    }

    pub(super) fn instanceof(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        constructor: Value,
    ) -> Result<bool, JsError> {
        if !self.is_function(constructor) {
            return Err(JsError(
                "right-hand side of 'instanceof' is not callable".into(),
            ));
        }
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, constructor, prototype_atom)?;
        if self.object_data(prototype).is_none() {
            return Err(JsError("instanceof prototype is not an object".into()));
        }
        let Some(mut current) = self.object_data(value).map(|data| data.proto) else {
            return Ok(false);
        };
        loop {
            if current == prototype {
                return Ok(true);
            }
            if current.is_null() {
                return Ok(false);
            }
            let Some(data) = self.object_data(current) else {
                return Ok(false);
            };
            current = data.proto;
        }
    }
}
