use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn dynamic_binding(&self, frame: usize, atom: Atom) -> Option<Value> {
        if let Some(value) = self.frames.get(frame).and_then(|frame| {
            frame
                .dynamic_bindings
                .iter()
                .rev()
                .find_map(|(candidate, value)| (*candidate == atom).then_some(*value))
        }) {
            return Some(value);
        }
        let mut env = self.frames.get(frame).map(|frame| frame.env)?;
        loop {
            let Cell::Environment {
                parent,
                dynamic_bindings,
                ..
            } = self.heap.get(env)?
            else {
                return None;
            };
            if let Some(value) = dynamic_bindings
                .iter()
                .rev()
                .find_map(|(candidate, value)| (*candidate == atom).then_some(*value))
            {
                return Some(value);
            }
            if parent.is_null() {
                return None;
            }
            env = *parent;
        }
    }

    pub(super) fn resolve_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        _cache: u16,
    ) -> Result<Value, JsError> {
        let name = self.atom_name(atom);
        if !name.starts_with('\0') {
            let key = self.heap.alloc(Cell::String(name.into()));
            let with_base = self
                .frames
                .last()
                .map_or(self.with_stack.len(), |frame| frame.with_base)
                .min(self.with_stack.len());
            let with_objects = self.with_stack[with_base..].to_vec();
            for object in with_objects.into_iter().rev() {
                if self.has_property(p, object, key)? {
                    return Ok(object);
                }
            }
        }
        Ok(self.realm.globals)
    }

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
            let with_base = self
                .frames
                .last()
                .map_or(self.with_stack.len(), |frame| frame.with_base)
                .min(self.with_stack.len());
            let with_objects = self.with_stack[with_base..].to_vec();
            for object in with_objects.into_iter().rev() {
                if self.has_property(p, object, key)? {
                    return self.get_property(p, object, atom);
                }
            }
            if let Some(value) = self.dynamic_binding(self.frames.len().saturating_sub(1), atom) {
                return Ok(value);
            }
        }
        let value = self.get_field_cached(p, self.realm.globals, atom, cache)?;
        if value.is_undefined() && self.own_property(self.realm.globals, atom).is_none() {
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
            let with_base = self
                .frames
                .last()
                .map_or(self.with_stack.len(), |frame| frame.with_base)
                .min(self.with_stack.len());
            let with_objects = self.with_stack[with_base..].to_vec();
            for object in with_objects.into_iter().rev() {
                if self.has_property(p, object, key)? {
                    return self.get_property(p, object, atom);
                }
            }
            if let Some(value) = self.dynamic_binding(self.frames.len().saturating_sub(1), atom) {
                return Ok(value);
            }
        }
        self.get_field_cached(p, self.realm.globals, atom, cache)
    }

    pub(super) fn store_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
        cache: u16,
    ) -> Result<(), JsError> {
        let name = self.atom_name(atom).to_owned();
        if !name.starts_with('\0') {
            let key = self.heap.alloc(Cell::String(name.as_str().into()));
            let with_base = self
                .frames
                .last()
                .map_or(self.with_stack.len(), |frame| frame.with_base)
                .min(self.with_stack.len());
            let with_objects = self.with_stack[with_base..].to_vec();
            for object in with_objects.into_iter().rev() {
                if self.has_property(p, object, key)? {
                    return self.set_property_with_program(p, object, atom, value);
                }
            }
            if let Some(frame) = self.frames.last_mut()
                && let Some((_, current)) = frame
                    .dynamic_bindings
                    .iter_mut()
                    .rev()
                    .find(|(candidate, _)| *candidate == atom)
            {
                *current = value;
                return Ok(());
            }
        }
        let strict_local = self
            .frames
            .last()
            .and_then(|frame| p.functions.get(frame.function as usize))
            .map_or(false, |function| {
                function.strict && !function.local_atoms.contains(&atom)
            });
        if strict_local && self.own_property(self.realm.globals, atom).is_none() {
            return Err(self.reference_error(p, format!("{} is not defined", name)));
        }
        let result = self.set_field_cached(p, self.realm.globals, atom, value, cache);
        let root_declared = self.frames.last().is_some_and(|frame| frame.function == 0)
            && p.functions
                .first()
                .is_some_and(|function| function.local_atoms.contains(&atom));
        if result.is_ok() && root_declared {
            self.set_property_attributes(
                self.realm.globals,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: true,
                    enumerable: true,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        result
    }
}
