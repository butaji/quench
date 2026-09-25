use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn root_global_var_atom(
        &self,
        program: &ResidualProgram,
        function: u32,
        slot: usize,
    ) -> Option<Atom> {
        if program.module || function != super::ROOT_FUNCTION_ID {
            return None;
        }
        let root = program.functions.first()?;
        let atom = *root.local_atoms.get(slot)?;
        root.global_var_atoms.contains(&atom).then_some(atom)
    }

    fn root_declares_binding(&self, program: &ResidualProgram, atom: Atom) -> bool {
        program.functions.first().is_some_and(|root| {
            root.global_var_atoms.contains(&atom) || root.global_lexical_atoms.contains(&atom)
        })
    }

    fn module_root_var_binding(&self, p: &ResidualProgram, function: u32, atom: Atom) -> bool {
        p.module
            && function == super::ROOT_FUNCTION_ID
            && p.functions[super::ROOT_FUNCTION_ID as usize]
                .global_var_atoms
                .contains(&atom)
    }

    pub(super) fn initialize_this_binding(&mut self, frame: usize, value: Value) {
        let atom = self.intern_atom("\0rqj:lexical-this");
        let arrow = self.intern_atom("\0rqj:arrow");
        let constructor = (0..=frame)
            .rev()
            .find(|index| {
                let frame = &self.frames[*index];
                self.programs
                    .get(frame.program)
                    .and_then(|program| {
                        program
                            .functions
                            .get(frame.function as usize)
                            .map(|function| {
                                super::dispatch_frame::is_derived_constructor(function)
                                    && function.name != Some(arrow)
                            })
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(frame);
        for index in constructor..=frame {
            let owns_lexical_this = index == constructor
                || self.frames.get(index).is_some_and(|frame| {
                    self.programs
                        .get(frame.program)
                        .and_then(|program| {
                            program
                                .functions
                                .get(frame.function as usize)
                                .map(|function| function.name == Some(arrow))
                        })
                        .unwrap_or(false)
                });
            if !owns_lexical_this {
                continue;
            }
            self.frames[index].this = value;
            if let Some((_, current)) = self.frames[index]
                .dynamic_bindings
                .iter_mut()
                .rev()
                .find(|(candidate, _)| *candidate == atom)
            {
                *current = value;
            }
            if !self.frames[index].captured {
                continue;
            }
            let env = self.frames[index].env;
            if let Some(Cell::Environment {
                dynamic_bindings, ..
            }) = self.heap.get_mut(env)
                && let Some((_, current)) = dynamic_bindings
                    .iter_mut()
                    .rev()
                    .find(|(candidate, _)| *candidate == atom)
            {
                *current = value;
            }
        }
    }

    pub(super) fn local_binding_slot(
        &self,
        p: &ResidualProgram,
        function: u32,
        atom: Atom,
    ) -> Option<usize> {
        let name = self.atom_name(atom);
        p.functions
            .get(function as usize)?
            .local_atoms
            .iter()
            .position(|candidate| {
                *candidate == atom
                    || self
                        .atom_name(*candidate)
                        .strip_prefix(name)
                        .is_some_and(|suffix| suffix.starts_with("\0rqj:self-binding:"))
            })
    }

    fn is_self_binding(&self, atom: Atom) -> bool {
        self.atom_name(atom).contains("\0rqj:self-binding:")
    }

    pub(super) fn captured_lexical_this(&self, mut env: Value) -> Option<Value> {
        while let Some(Cell::Environment {
            parent,
            dynamic_bindings,
            ..
        }) = self.heap.get(env)
        {
            if let Some((_, value)) = dynamic_bindings
                .iter()
                .rev()
                .find(|(atom, _)| self.atom_name(*atom) == "\0rqj:lexical-this")
            {
                return Some(*value);
            }
            env = *parent;
        }
        None
    }

    pub(super) fn check_super_call(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let atom = self.intern_atom("\0rqj:super-called");
        let frame_index = self
            .frames
            .len()
            .checked_sub(1)
            .ok_or_else(|| JsError("super call outside an activation".into()))?;
        if let Some((_, called)) = self.frames[frame_index]
            .dynamic_bindings
            .iter_mut()
            .rev()
            .find(|(candidate, _)| *candidate == atom)
        {
            if called.as_bool() == Some(true) {
                return Err(
                    self.reference_error(p, "super constructor may only be called once".into())
                );
            }
            *called = Value::TRUE;
            return Ok(());
        }
        let mut env = self.frames[frame_index].env;
        loop {
            let Some(Cell::Environment {
                parent,
                dynamic_bindings,
                ..
            }) = self.heap.get(env)
            else {
                break;
            };
            let parent = *parent;
            let called = dynamic_bindings
                .iter()
                .rev()
                .find(|(candidate, _)| *candidate == atom)
                .map(|(_, value)| *value);
            if let Some(called) = called {
                if called.as_bool() == Some(true) {
                    return Err(
                        self.reference_error(p, "super constructor may only be called once".into())
                    );
                }
                if let Some(Cell::Environment {
                    dynamic_bindings, ..
                }) = self.heap.get_mut(env)
                    && let Some((_, value)) = dynamic_bindings
                        .iter_mut()
                        .rev()
                        .find(|(candidate, _)| *candidate == atom)
                {
                    *value = Value::TRUE;
                    return Ok(());
                }
            }
            env = parent;
        }
        Err(self.reference_error(p, "super constructor binding is unavailable".into()))
    }

    pub(super) fn with_binding(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        atom: Atom,
    ) -> Result<bool, JsError> {
        if !self.has_property(p, object, key)? {
            return Ok(false);
        }
        let Some(unscopables) = self.well_known_symbols.get("unscopables").copied() else {
            return Ok(true);
        };
        let exclusions = self.get_index(p, object, unscopables)?;
        if self.is_object_like(exclusions) {
            let excluded = self.get_property(p, exclusions, atom)?;
            if self.truthy(excluded) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn get_with_binding_value(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if !self.has_property(p, object, key)? {
            let strict = self
                .frames
                .last()
                .is_some_and(|frame| p.functions[frame.function as usize].strict);
            if strict {
                return Err(
                    self.reference_error(p, format!("{} is not defined", self.atom_name(atom)))
                );
            }
            return Ok(Value::UNDEFINED);
        }
        self.get_property(p, object, atom)
    }

    pub(super) fn outer_environment_binding(
        &mut self,
        mut env: Value,
        atom: Atom,
    ) -> Option<(Value, usize)> {
        let mut root_eval_scope = false;
        while let Some(Cell::Environment {
            parent,
            program,
            root_eval_scope: environment_root_eval_scope,
            function,
            slots,
            ..
        }) = self.heap.get(env)
        {
            root_eval_scope |= *environment_root_eval_scope;
            if *function != u32::MAX
                && let Some(environment_program) = program.and_then(|program| {
                    self.programs
                        .get(super::program_store::ProgramId::from_raw(program))
                })
                && let Some(metadata) = environment_program.functions.get(*function as usize)
                && (*function != super::ROOT_FUNCTION_ID
                    || metadata.global_lexical_atoms.contains(&atom)
                    || self.module_root_var_binding(&environment_program, *function, atom)
                    || (root_eval_scope
                        && program.is_some_and(|program| program != self.active_program.raw())))
                && let Some(slot) = self.local_binding_slot(&environment_program, *function, atom)
                && slot < slots.len()
            {
                return Some((env, slot));
            }
            env = *parent;
        }
        None
    }

    fn environment_binding_atom(&self, env: Value, slot: usize) -> Option<Atom> {
        let Cell::Environment {
            program: Some(program),
            function,
            ..
        } = self.heap.get(env)?
        else {
            return None;
        };
        self.programs
            .get(super::program_store::ProgramId::from_raw(*program))?
            .functions
            .get(*function as usize)?
            .local_atoms
            .get(slot)
            .copied()
    }

    pub(super) fn captured_parent_environment(&self, frame: usize) -> Value {
        let frame = &self.frames[frame];
        if frame.captured {
            match self.heap.get(frame.env) {
                Some(Cell::Environment { parent, .. }) => *parent,
                _ => Value::NULL,
            }
        } else {
            frame.env
        }
    }

    pub(super) fn store_resolved_name(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        strict: bool,
    ) -> Result<(), JsError> {
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        // ResolveName has already selected a non-global object environment.
        // Preserve that reference even if evaluating the RHS removed the
        // binding before PutValue (the specification's captured Reference).
        if object != self.realm.globals {
            if strict && !self.has_property(p, object, key)? {
                return Err(
                    self.reference_error(p, format!("{} is not defined", self.atom_name(atom)))
                );
            }
            return self.set_property_with_program(p, object, atom, value);
        }
        let with_base = self
            .frames
            .last()
            .map_or(self.with_stack.len(), |frame| frame.with_base)
            .min(self.with_stack.len());
        let with_objects = self.with_stack[with_base..].to_vec();
        let mut is_with_binding = false;
        for candidate in with_objects.into_iter().rev() {
            if candidate == object && self.with_binding(p, candidate, key, atom)? {
                is_with_binding = true;
                break;
            }
        }
        if is_with_binding {
            return self.set_property_with_program(p, object, atom, value);
        }

        if let Some(frame_index) = self.frames.len().checked_sub(1)
            && (self.frames[frame_index].function != 0
                || p.functions[self.frames[frame_index].function as usize]
                    .global_lexical_atoms
                    .contains(&atom)
                || p.functions[self.frames[frame_index].function as usize]
                    .global_var_atoms
                    .contains(&atom)
                || self.module_root_var_binding(p, self.frames[frame_index].function, atom))
            && let Some(slot) = self.local_binding_slot(p, self.frames[frame_index].function, atom)
        {
            let mirrors_global_var = !p.module
                && self.frames[frame_index].function == 0
                && p.functions[0].global_var_atoms.contains(&atom);
            if self.frames[frame_index].captured {
                let env = self.frames[frame_index].env;
                if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env) {
                    slots[slot] = value;
                }
            } else {
                self.frames[frame_index].locals[slot] = value;
            }
            self.mapped_argument_store(p, frame_index, slot, value);
            if mirrors_global_var {
                self.set_property_with_program(p, self.realm.globals, atom, value)?;
            }
            return Ok(());
        }
        if let Some(frame_index) = self.frames.len().checked_sub(1)
            && let Some((env, slot)) =
                self.outer_environment_binding(self.captured_parent_environment(frame_index), atom)
            && let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env)
        {
            slots[slot] = value;
            return Ok(());
        }
        if self.store_global_var_binding(p, atom, value)? {
            return Ok(());
        }
        if strict && self.own_property(self.realm.globals, atom).is_none() {
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        let _ = object;
        self.set_property_with_program(p, self.realm.globals, atom, value)
    }

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
        strict: bool,
    ) -> Result<Value, JsError> {
        if strict && !self.atom_name(atom).starts_with('\0') && !self.has_name_binding(p, atom)? {
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        Ok(self.resolve_name_reference(p, atom)?.0)
    }

    fn has_name_binding(&mut self, p: &ResidualProgram, atom: Atom) -> Result<bool, JsError> {
        let name = self.atom_name(atom);
        if name.starts_with('\0') {
            return Ok(true);
        }
        if !name.starts_with('\0') {
            let key = self.heap.alloc(Cell::String(name.into()));
            let with_base = self
                .frames
                .last()
                .map_or(self.with_stack.len(), |frame| frame.with_base)
                .min(self.with_stack.len());
            let with_objects = self.with_stack[with_base..].to_vec();
            for object in with_objects.into_iter().rev() {
                if self.with_binding(p, object, key, atom)? {
                    return Ok(true);
                }
            }
        }
        if self
            .dynamic_binding(self.frames.len().saturating_sub(1), atom)
            .is_some()
            || self.realm.global_lexical_declarations.contains(&atom)
            || self.realm.global_lexical_bindings.contains_key(&atom)
            || self.own_property(self.realm.globals, atom).is_some()
        {
            return Ok(true);
        }
        if self
            .frames
            .last()
            .is_some_and(|frame| self.local_binding_slot(p, frame.function, atom).is_some())
        {
            return Ok(true);
        }
        Ok(self
            .outer_environment_binding(
                self.captured_parent_environment(self.frames.len().saturating_sub(1)),
                atom,
            )
            .is_some())
    }

    fn resolve_name_reference(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
    ) -> Result<(Value, bool), JsError> {
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
                let has_binding = self.with_binding(p, object, key, atom)?;
                if has_binding {
                    return Ok((object, true));
                }
            }
        }
        Ok((self.realm.globals, false))
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
        env = self.skip_with_environment_layers(env)?;
        for _ in 0..depth {
            env = match self.heap.get(env)? {
                Cell::Environment { parent, .. } => *parent,
                _ => return None,
            };
            env = self.skip_with_environment_layers(env)?;
        }
        Some(env)
    }

    fn skip_with_environment_layers(&self, mut env: Value) -> Option<Value> {
        while let Some(Cell::Environment {
            parent,
            function,
            slots,
            dynamic_bindings,
            with_objects,
            root_eval_scope,
            ..
        }) = self.heap.get(env)
        {
            let lexical_this_wrapper = !*root_eval_scope
                && *function == u32::MAX
                && slots.is_empty()
                && (dynamic_bindings.is_empty()
                    || (dynamic_bindings.len() == 1
                        && self.atom_name(dynamic_bindings[0].0) == "\0rqj:lexical-this"));
            if with_objects.is_empty() && !lexical_this_wrapper {
                break;
            }
            env = *parent;
        }
        Some(env)
    }

    pub(super) fn capture(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        address: u32,
    ) -> Result<Value, JsError> {
        let env = self
            .capture_env(frame, (address >> 16) as u16)
            .ok_or_else(|| JsError("invalid capture environment".into()))?;
        let Cell::Environment {
            function,
            program,
            slots,
            ..
        } = self
            .heap
            .get(env)
            .ok_or_else(|| JsError("invalid capture".into()))?
        else {
            return Err(JsError("invalid capture".into()));
        };
        let slot = address as u16 as usize;
        let atom = program
            .and_then(|program| {
                self.programs
                    .get(super::program_store::ProgramId::from_raw(program))
            })
            .as_deref()
            .and_then(|program| self.root_global_var_atom(program, *function, slot))
            .or_else(|| self.root_global_var_atom(p, *function, slot));
        if let Some(atom) = atom {
            return self.get_property(p, self.realm.globals, atom);
        }
        let value = program
            .and_then(|program| {
                self.module_import_value(
                    super::program_store::ProgramId::from_raw(program),
                    *function,
                    slot,
                )
            })
            .or_else(|| slots.get(slot).copied())
            .ok_or_else(|| JsError("invalid capture slot".into()))?;
        if value.is_deleted() {
            let atom = program
                .and_then(|program| {
                    self.programs
                        .get(super::program_store::ProgramId::from_raw(program))
                })
                .and_then(|program| {
                    program
                        .functions
                        .get(*function as usize)
                        .and_then(|metadata| metadata.local_atoms.get(slot))
                        .copied()
                })
                .or_else(|| {
                    p.functions
                        .get(*function as usize)
                        .and_then(|metadata| metadata.local_atoms.get(slot))
                        .copied()
                })
                .unwrap_or_default();
            return Err(self.reference_error(
                p,
                format!(
                    "Cannot access '{}' before initialization",
                    self.atom_name(atom)
                ),
            ));
        }
        Ok(value)
    }

    pub(super) fn store_capture(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        address: u32,
        value: Value,
    ) -> Result<(), JsError> {
        let env = self
            .capture_env(frame, (address >> 16) as u16)
            .ok_or_else(|| JsError("invalid capture environment".into()))?;
        let Some(Cell::Environment {
            function, slots, ..
        }) = self.heap.get(env)
        else {
            return Err(JsError("invalid capture".into()));
        };
        let function = *function;
        if slots
            .get(address as u16 as usize)
            .is_some_and(|current| current.is_deleted())
        {
            let atom = p
                .functions
                .get(function as usize)
                .and_then(|metadata| metadata.local_atoms.get(address as u16 as usize))
                .copied()
                .unwrap_or_default();
            return Err(self.reference_error(
                p,
                format!(
                    "Cannot access '{}' before initialization",
                    self.atom_name(atom)
                ),
            ));
        }
        let atom = p
            .functions
            .get(function as usize)
            .and_then(|metadata| metadata.local_atoms.get(address as u16 as usize))
            .copied();
        if atom.is_some_and(|atom| self.is_self_binding(atom)) {
            if p.functions[self.frames[frame].function as usize].strict {
                return Err(self.type_error(p, "assignment to function name binding".into()));
            }
            return Ok(());
        }
        if atom.is_some_and(|atom| {
            p.functions[function as usize]
                .global_immutable_atoms
                .contains(&atom)
        }) {
            return Err(self.type_error(p, "assignment to constant binding".into()));
        }
        if let Some(atom) = self
            .programs
            .get(self.frames[frame].program)
            .as_deref()
            .and_then(|program| {
                self.root_global_var_atom(program, function, address as u16 as usize)
            })
            .or_else(|| self.root_global_var_atom(p, function, address as u16 as usize))
        {
            if !self.store_global_var_binding(p, atom, value)? {
                self.set_property_with_program(p, self.realm.globals, atom, value)?;
            }
            return Ok(());
        }
        let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env) else {
            return Err(JsError("invalid capture".into()));
        };
        let slot = slots
            .get_mut(address as u16 as usize)
            .ok_or_else(|| JsError("invalid capture slot".into()))?;
        *slot = value;
        if function == 0
            && let Some(atom) = atom
            && !p.functions[0].global_lexical_atoms.contains(&atom)
            && !self.module_root_var_binding(p, function, atom)
        {
            self.set_property_with_program(p, self.realm.globals, atom, value)?;
        }
        Ok(())
    }

    pub(super) fn load_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: u16,
    ) -> Result<Value, JsError> {
        self.load_name_call(p, atom, cache).map(|(value, _)| value)
    }

    fn load_name_without_with(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: u16,
    ) -> Result<Value, JsError> {
        let name = self.atom_name(atom);
        if name == "\0rqj:dynamic-import" {
            return Ok(self.native_value(Native::DynamicImport));
        }
        if name == "\0rqj:intrinsic-promise" {
            return Ok(self.native_value(Native::Promise));
        }
        if name == "\0rqj:super-get" {
            return Ok(self.native_value(Native::ReflectGet));
        }
        if name == "\0rqj:super-set" {
            return Ok(self.native_value(Native::SuperSet));
        }
        if name == "\0rqj:super-base" {
            return Ok(self.native_value(Native::ReflectGetPrototypeOf));
        }
        if name == "\0rqj:object-literal-prototype" {
            return Ok(self.native_value(Native::ObjectLiteralPrototype));
        }
        if name == "\0rqj:object-define-property" {
            return Ok(self.native_value(Native::ObjectDefineProperty));
        }
        if name == "\0rqj:object-freeze" {
            return Ok(self.native_value(Native::ObjectFreeze));
        }
        if let Some(value) = self.dynamic_binding(self.frames.len().saturating_sub(1), atom) {
            return Ok(value);
        }
        if !name.starts_with('\0') {
            if let Some(frame) = self.frames.last()
                && (frame.function != 0
                    || p.functions[frame.function as usize]
                        .global_lexical_atoms
                        .contains(&atom)
                    || self.module_root_var_binding(p, frame.function, atom))
                && let Some(slot) = self.local_binding_slot(p, frame.function, atom)
            {
                return Ok(if frame.captured {
                    match self.heap.get(frame.env) {
                        Some(Cell::Environment { slots, .. }) => slots[slot],
                        _ => Value::UNDEFINED,
                    }
                } else {
                    frame.locals[slot]
                });
            }
            if let Some(value) = self.realm.global_lexical_bindings.get(&atom).copied() {
                return self.checked_binding_read(p, atom, value);
            }
            if let Some((env, slot)) = self.outer_environment_binding(
                self.captured_parent_environment(self.frames.len().saturating_sub(1)),
                atom,
            ) && let Some(Cell::Environment { slots, .. }) = self.heap.get(env)
            {
                return self.checked_binding_read(p, atom, slots[slot]);
            }
            if let Some(value) = self.load_eval_frame_local(p, atom) {
                return Ok(value);
            }
        }
        let value = self.get_field_cached(p, self.realm.globals, atom, cache)?;
        if value.is_undefined() && self.own_property(self.realm.globals, atom).is_none() {
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        Ok(value)
    }

    pub(super) fn load_name_call(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: u16,
    ) -> Result<(Value, Value), JsError> {
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
                if !self.with_binding(p, object, key, atom)? {
                    continue;
                }
                return Ok((self.get_with_binding_value(p, object, key, atom)?, object));
            }
        }
        Ok((
            self.load_name_without_with(p, atom, cache)?,
            Value::UNDEFINED,
        ))
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
                if self.with_binding(p, object, key, atom)? {
                    return self.get_property(p, object, atom);
                }
            }
            if let Some(frame) = self.frames.last()
                && (frame.function != 0
                    || p.functions[frame.function as usize]
                        .global_lexical_atoms
                        .contains(&atom)
                    || self.module_root_var_binding(p, frame.function, atom))
                && let Some(slot) = self.local_binding_slot(p, frame.function, atom)
            {
                let value = if frame.captured {
                    match self.heap.get(frame.env) {
                        Some(Cell::Environment { slots, .. }) => slots[slot],
                        _ => Value::UNDEFINED,
                    }
                } else {
                    frame.locals[slot]
                };
                return self.checked_binding_read(p, atom, value);
            }
            if let Some((env, slot)) = self.outer_environment_binding(
                self.captured_parent_environment(self.frames.len().saturating_sub(1)),
                atom,
            ) && let Some(Cell::Environment { slots, .. }) = self.heap.get(env)
            {
                return self.checked_binding_read(p, atom, slots[slot]);
            }
            if let Some(value) = self.dynamic_binding(self.frames.len().saturating_sub(1), atom) {
                return self.checked_binding_read(p, atom, value);
            }
            if let Some(value) = self.load_eval_frame_local(p, atom) {
                return self.checked_binding_read(p, atom, value);
            }
        }
        self.get_field_cached(p, self.realm.globals, atom, cache)
    }

    pub(super) fn checked_binding_read(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
    ) -> Result<Value, JsError> {
        if value.is_deleted() {
            return Err(self.reference_error(
                p,
                format!(
                    "Cannot access '{}' before initialization",
                    self.atom_name(atom)
                ),
            ));
        }
        Ok(value)
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
                if self.with_binding(p, object, key, atom)? {
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
            if let Some(frame_index) = self.frames.len().checked_sub(1)
                && (self.frames[frame_index].function != 0
                    || p.functions[self.frames[frame_index].function as usize]
                        .global_lexical_atoms
                        .contains(&atom)
                    || self.module_root_var_binding(p, self.frames[frame_index].function, atom))
                && let Some(slot) =
                    self.local_binding_slot(p, self.frames[frame_index].function, atom)
            {
                let binding =
                    p.functions[self.frames[frame_index].function as usize].local_atoms[slot];
                if self.is_self_binding(binding) {
                    if p.functions[self.frames[frame_index].function as usize].strict {
                        return Err(
                            self.type_error(p, "assignment to function name binding".into())
                        );
                    }
                    return Ok(());
                }
                if self.frames[frame_index].captured {
                    let env = self.frames[frame_index].env;
                    if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env) {
                        slots[slot] = value;
                    }
                } else {
                    self.frames[frame_index].locals[slot] = value;
                }
                self.mapped_argument_store(p, frame_index, slot, value);
                return Ok(());
            }
            if self.realm.global_lexical_bindings.contains_key(&atom) {
                if self.realm.immutable_global_lexical_bindings.contains(&atom) {
                    return Err(self.type_error(p, "assignment to constant binding".into()));
                }
                self.realm.global_lexical_bindings.insert(atom, value);
                return Ok(());
            }
            if let Some(frame_index) = self.frames.len().checked_sub(1)
                && let Some((env, slot)) = self
                    .outer_environment_binding(self.captured_parent_environment(frame_index), atom)
            {
                let binding = self.environment_binding_atom(env, slot);
                if binding.is_some_and(|atom| self.is_self_binding(atom)) {
                    if p.functions[self.frames[frame_index].function as usize].strict {
                        return Err(
                            self.type_error(p, "assignment to function name binding".into())
                        );
                    }
                    return Ok(());
                }
                let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env) else {
                    return Err(JsError("invalid outer environment".into()));
                };
                slots[slot] = value;
                return Ok(());
            }
        }
        if self.store_global_var_binding(p, atom, value)? {
            return Ok(());
        }
        let root_declared = self
            .frames
            .last()
            .is_some_and(|frame| frame.function == super::ROOT_FUNCTION_ID)
            && self.root_declares_binding(p, atom);
        if root_declared
            && self.own_property(self.realm.globals, atom).is_none()
            && self
                .object_data(self.realm.globals)
                .is_some_and(|object| !object.is_extensible())
        {
            return Ok(());
        }
        let strict = self
            .frames
            .last()
            .and_then(|frame| p.functions.get(frame.function as usize))
            .is_some_and(|function| function.strict);
        let result = self.set_field_cached(p, self.realm.globals, atom, value, cache, strict);
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

    fn store_global_var_binding(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
    ) -> Result<bool, JsError> {
        let Some(root_index) = self
            .frames
            .iter()
            .rposition(|frame| frame.program == self.active_program && frame.function == 0)
        else {
            return Ok(false);
        };
        let root_frame = &self.frames[root_index];
        let Some(root_program) = self.programs.get(root_frame.program) else {
            return Ok(false);
        };
        let Some(root_function) = root_program.functions.first() else {
            return Ok(false);
        };
        if p.module || !root_function.global_var_atoms.contains(&atom) {
            return Ok(false);
        }
        let Some(slot) = root_function
            .local_atoms
            .iter()
            .position(|candidate| *candidate == atom)
        else {
            return Ok(false);
        };
        if self.frames[root_index].captured {
            if let Some(Cell::Environment { slots, .. }) =
                self.heap.get_mut(self.frames[root_index].env)
            {
                slots[slot] = value;
            }
        } else {
            self.frames[root_index].locals[slot] = value;
        }
        self.set_property_with_program(p, self.realm.globals, atom, value)?;
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
        Ok(true)
    }
}
