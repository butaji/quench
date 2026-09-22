use super::*;

impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn capture_env(&self, frame: usize, depth: u16) -> Option<Value> {
        let frame = &self.frames[frame];
        let mut env = if frame.captured {
            match self.heap.get(frame.env)? {
                Cell::Environment { parent, .. } => *parent,
                _ => return None,
            }
        } else {
            frame.env
        };
        for _ in 0..depth {
            env = match self.heap.get(env)? {
                Cell::Environment { parent, .. } => *parent,
                _ => return None,
            };
        }
        Some(env)
    }

    pub(super) fn capture(&self, frame: usize, address: u32) -> Result<Value, JsError> {
        let env = self
            .capture_env(frame, (address >> 16) as u16)
            .ok_or_else(|| JsError("invalid capture environment".into()))?;
        let Cell::Environment { slots, .. } = self
            .heap
            .get(env)
            .ok_or_else(|| JsError("invalid capture".into()))?
        else {
            return Err(JsError("invalid capture".into()));
        };
        slots
            .get(address as u16 as usize)
            .copied()
            .ok_or_else(|| JsError("invalid capture slot".into()))
    }

    pub(super) fn store_capture(
        &mut self,
        frame: usize,
        address: u32,
        value: Value,
    ) -> Result<(), JsError> {
        let env = self
            .capture_env(frame, (address >> 16) as u16)
            .ok_or_else(|| JsError("invalid capture environment".into()))?;
        let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env) else {
            return Err(JsError("invalid capture".into()));
        };
        let slot = slots
            .get_mut(address as u16 as usize)
            .ok_or_else(|| JsError("invalid capture slot".into()))?;
        *slot = value;
        Ok(())
    }

    pub(super) fn load_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: u16,
    ) -> Result<Value, JsError> {
        let name = self.atom_name(atom);
        if !name.starts_with('\0') {
            let key = self.heap.alloc(Cell::String(name.into()));
            for object in self.with_stack.clone().into_iter().rev() {
                if self.has_property(p, object, key)? {
                    return self.get_property(p, object, atom);
                }
            }
        }
        let value = self.get_field_cached(p, self.globals, atom, cache)?;
        if value.is_undefined() && self.own_property(self.globals, atom).is_none() {
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        Ok(value)
    }

    pub(super) fn load_name_typeof(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: u16,
    ) -> Result<Value, JsError> {
        let name = self.atom_name(atom);
        if !name.starts_with('\0') {
            let key = self.heap.alloc(Cell::String(name.into()));
            for object in self.with_stack.clone().into_iter().rev() {
                if self.has_property(p, object, key)? {
                    return self.get_property(p, object, atom);
                }
            }
        }
        self.get_field_cached(p, self.globals, atom, cache)
    }

    pub(super) fn store_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
        cache: u16,
    ) -> Result<(), JsError> {
        let name = self.atom_name(atom);
        if !name.starts_with('\0') {
            let key = self.heap.alloc(Cell::String(name.into()));
            for object in self.with_stack.clone().into_iter().rev() {
                if self.has_property(p, object, key)? {
                    return self.set_property_with_program(p, object, atom, value);
                }
            }
        }
        self.set_field_cached(p, self.globals, atom, value, cache)
    }
}
