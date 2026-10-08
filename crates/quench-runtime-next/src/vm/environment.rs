use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn own_dynamic_bindings(&self, frame: usize) -> Option<&Vec<(Atom, Value)>> {
        let activation = self.frames.get(frame)?;
        if activation.captured {
            self.heap.environment_bindings(activation.env)
        } else {
            Some(&activation.dynamic_bindings)
        }
    }

    pub(super) fn own_dynamic_bindings_mut(
        &mut self,
        frame: usize,
    ) -> Option<&mut Vec<(Atom, Value)>> {
        let activation = self.frames.get(frame)?;
        if activation.captured {
            self.heap.environment_bindings_mut(activation.env)
        } else {
            Some(&mut self.frames[frame].dynamic_bindings)
        }
    }

    pub(super) fn store_own_dynamic_binding(
        &mut self,
        frame: usize,
        atom: Atom,
        value: Value,
    ) -> bool {
        let Some((_, binding)) = self.own_dynamic_bindings_mut(frame).and_then(|bindings| {
            bindings
                .iter_mut()
                .rev()
                .find(|(candidate, _)| *candidate == atom)
        }) else {
            return false;
        };
        *binding = value;
        true
    }

    pub(super) fn root_global_var_atom(
        &self,
        program: &ResidualProgram,
        program_id: super::ProgramId,
        function: u32,
        slot: usize,
    ) -> Option<Atom> {
        if program.root_variables_are_local()
            || function != super::ROOT_FUNCTION_ID
            || self.direct_eval_var_program == Some(program_id)
        {
            return None;
        }
        let root = program.functions.first()?;
        let atom = *root.local_atoms.get(slot)?;
        root.global_var_atoms.contains(&atom).then_some(atom)
    }

    pub(super) fn global_object_var_atom(&self, frame: usize, slot: usize) -> Option<Atom> {
        let eval = self.frames.get(frame)?;
        if eval.function != super::ROOT_FUNCTION_ID {
            return None;
        }
        let program = self.programs.get(eval.program)?;
        self.root_global_var_atom(&program, eval.program, eval.function, slot)
    }

    pub(super) fn root_global_lexical_atom(
        &self,
        program: &ResidualProgram,
        function: u32,
        slot: usize,
    ) -> Option<Atom> {
        if program.kind != crate::bytecode::ProgramKind::Script
            || function != super::ROOT_FUNCTION_ID
        {
            return None;
        }
        let root = program.functions.first()?;
        let atom = *root.local_atoms.get(slot)?;
        root.global_lexical_atoms.contains(&atom).then_some(atom)
    }

    pub(super) fn root_global_lexical_value(
        &self,
        program: &ResidualProgram,
        function: u32,
        slot: usize,
    ) -> Option<Value> {
        if !self.eval_script_context {
            return None;
        }
        self.root_global_lexical_atom(program, function, slot)
            .and_then(|atom| self.realm.global_lexical_bindings.get(&atom).copied())
    }

    fn active_global_lexical_value(&self, atom: Atom) -> Option<Value> {
        self.frames.iter().rev().find_map(|frame| {
            if frame.function != super::ROOT_FUNCTION_ID || frame.this != self.realm.globals {
                return None;
            }
            let program = self.programs.get(frame.program)?;
            if program.kind != crate::bytecode::ProgramKind::Script {
                return None;
            }
            let root = program.functions.first()?;
            if !root.global_lexical_atoms.contains(&atom) {
                return None;
            }
            let slot = root
                .local_atoms
                .iter()
                .position(|candidate| *candidate == atom)?;
            if frame.captured {
                self.heap.environment_slot(frame.env, slot)
            } else {
                frame.locals.get(slot).copied()
            }
        })
    }

    fn root_declares_binding(&self, program: &ResidualProgram, atom: Atom) -> bool {
        program.functions.first().is_some_and(|root| {
            root.global_var_atoms.contains(&atom) || root.global_lexical_atoms.contains(&atom)
        })
    }

    fn root_local_var_binding(&self, p: &ResidualProgram, function: u32, atom: Atom) -> bool {
        p.root_variables_are_local()
            && function == super::ROOT_FUNCTION_ID
            && p.functions[super::ROOT_FUNCTION_ID as usize]
                .global_var_atoms
                .contains(&atom)
    }

    /// The activation owning lexical this also owns constructor initialization.
    pub(super) fn lexical_this_owner(
        &self,
        frame: usize,
        atom: Atom,
    ) -> Option<(super::ProgramId, u32, Value)> {
        if self
            .own_dynamic_bindings(frame)?
            .iter()
            .any(|(key, _)| *key == atom)
        {
            let activation = &self.frames[frame];
            return Some((
                activation.program,
                activation.function,
                self.captured_parent_environment(frame),
            ));
        }
        let (env, _) = self.outer_dynamic_binding(frame, atom)?;
        let Cell::Environment {
            program: Some(program),
            function,
            parent,
            ..
        } = self.heap.get(env)?
        else {
            return None;
        };
        Some((super::ProgramId::from_raw(*program), *function, *parent))
    }

    pub(super) fn initialize_this_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom("\0quench:lexical-this");
        let owner = self.lexical_this_owner(frame, atom);
        if !self.store_own_dynamic_binding(frame, atom, value)
            && !self.store_outer_dynamic_binding(frame, atom, value)
        {
            return Err(self.reference_error(p, "this binding is unavailable".into()));
        }
        for index in 0..=frame {
            if let Some(value) = self.dynamic_binding(index, atom) {
                self.frames[index].this = value;
            }
        }
        if let Some((program_id, function, parent)) = owner {
            let program = self
                .programs
                .get(program_id)
                .ok_or_else(|| JsError("constructor program is unavailable".into()))?;
            if let Some(initializer) = program.functions[function as usize].instance_initializer {
                let previous = std::mem::replace(&mut self.active_program, program_id);
                let result = self.call_user(
                    &program,
                    initializer,
                    parent,
                    value,
                    &[],
                    CallContext::Internal,
                );
                self.active_program = previous;
                result?;
            }
        }
        Ok(())
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
                        .is_some_and(|suffix| suffix.starts_with("\0quench:self-binding:"))
            })
    }

    fn function_environment_binding(&self, p: &ResidualProgram, function: u32, atom: Atom) -> bool {
        p.functions
            .get(function as usize)
            .is_some_and(|metadata| metadata.environment_atoms.contains(&atom))
    }

    pub(super) fn activation_binding_slot(&self, frame: usize, atom: Atom) -> Option<usize> {
        let frame = self.frames.get(frame)?;
        let program = self.programs.get(frame.program)?;
        let metadata = program.functions.get(frame.function as usize)?;
        // Local storage also reserves slots for source names whose actual
        // binding lives in a catch scope or an enclosing environment.
        if !metadata.environment_atoms.contains(&atom) {
            return None;
        }
        metadata
            .local_atoms
            .iter()
            .position(|candidate| *candidate == atom)
    }

    pub(super) fn name_binding(
        &self,
        frame: usize,
        atom: Atom,
    ) -> Option<crate::bytecode::EvalBinding> {
        let activation = self.frames.get(frame)?;
        let program = self.programs.get(activation.program)?;
        let metadata = program.functions.get(activation.function as usize)?;
        if let Some(site_pc) = activation.binding_site_pc
            && let Ok(index) = metadata
                .binding_sites
                .binary_search_by_key(&site_pc, |site| site.resume_pc)
            && let Some(binding) = metadata.binding_sites[index]
                .bindings
                .iter()
                .find(|binding| binding.atom == atom)
        {
            return Some(*binding);
        }
        let bindings = &metadata.name_bindings;
        if let Ok(index) = bindings.binary_search_by_key(&atom, |binding| binding.atom) {
            return bindings.get(index).copied();
        }
        if metadata.environment_atoms.contains(&atom) {
            return None;
        }
        // An eval-capable parent deliberately prevents static source captures.
        // Its lexical fallback remains authoritative after that parent returns.
        let mut depth = 0u16;
        let mut environment = self.capture_env(frame, depth)?;
        while let Some(Cell::Environment {
            parent,
            program,
            function,
            binding_site_pc,
            ..
        }) = self.heap.get(environment)
        {
            if let Some(program) = program.and_then(|id| {
                self.programs
                    .get(super::program_store::ProgramId::from_raw(id))
            }) && let Some(metadata) = program.functions.get(*function as usize)
            {
                let scoped_binding = binding_site_pc
                    .and_then(|pc| {
                        metadata
                            .binding_sites
                            .binary_search_by_key(&pc, |site| site.resume_pc)
                            .ok()
                    })
                    .and_then(|index| {
                        metadata.binding_sites[index]
                            .bindings
                            .iter()
                            .find(|binding| binding.atom == atom)
                    })
                    .copied();
                if scoped_binding.is_none() && metadata.environment_atoms.contains(&atom) {
                    return None;
                }
                let binding = scoped_binding.or_else(|| {
                    metadata
                        .name_bindings
                        .binary_search_by_key(&atom, |binding| binding.atom)
                        .ok()
                        .map(|index| metadata.name_bindings[index])
                });
                if let Some(mut binding) = binding {
                    binding.location = match binding.location {
                        crate::bytecode::EvalBindingLocation::Local(slot) => {
                            crate::bytecode::EvalBindingLocation::Capture { depth, slot }
                        }
                        crate::bytecode::EvalBindingLocation::Capture {
                            depth: inherited,
                            slot,
                        } => crate::bytecode::EvalBindingLocation::Capture {
                            depth: depth.checked_add(1)?.checked_add(inherited)?,
                            slot,
                        },
                    };
                    return Some(binding);
                }
            }
            environment = self.skip_with_environment_layers(*parent)?;
            depth = depth.checked_add(1)?;
        }
        None
    }

    pub(super) fn load_name_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        binding: crate::bytecode::EvalBinding,
    ) -> Result<Value, JsError> {
        match binding.location {
            crate::bytecode::EvalBindingLocation::Capture { depth, slot } => {
                self.capture(p, frame, depth, slot)
            }
            crate::bytecode::EvalBindingLocation::Local(slot) => {
                self.load_local_binding(p, frame, usize::from(slot), Some(binding.atom))
            }
        }
    }

    pub(super) fn store_name_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        binding: crate::bytecode::EvalBinding,
        value: Value,
        strict: bool,
    ) -> Result<(), JsError> {
        self.load_name_binding(p, frame, binding)?;
        if !self.check_named_binding_assignment(p, binding.kind, strict)? {
            return Ok(());
        }
        match binding.location {
            crate::bytecode::EvalBindingLocation::Capture { depth, slot } => {
                self.store_capture(p, frame, depth, slot, value)
            }
            crate::bytecode::EvalBindingLocation::Local(slot) => {
                if self.store_activation_binding(frame, usize::from(slot), value) {
                    Ok(())
                } else {
                    Err(JsError("invalid named binding".into()))
                }
            }
        }
    }

    fn binding_reference(
        &self,
        reference: Value,
    ) -> Option<(Value, u16, crate::bytecode::LexicalBindingKind)> {
        match self.heap.get(reference)? {
            Cell::BindingReference {
                environment,
                slot,
                kind,
            } => Some((*environment, *slot, *kind)),
            _ => None,
        }
    }

    pub(super) fn check_named_binding_assignment(
        &mut self,
        p: &ResidualProgram,
        kind: crate::bytecode::LexicalBindingKind,
        strict: bool,
    ) -> Result<bool, JsError> {
        match kind {
            crate::bytecode::LexicalBindingKind::Mutable => Ok(true),
            crate::bytecode::LexicalBindingKind::Immutable => {
                Err(self.type_error(p, "assignment to immutable binding".into()))
            }
            crate::bytecode::LexicalBindingKind::FunctionName if strict => {
                Err(self.type_error(p, "assignment to function name binding".into()))
            }
            crate::bytecode::LexicalBindingKind::FunctionName => Ok(false),
        }
    }

    pub(super) fn initialize_handler_binding(
        &mut self,
        frame: usize,
        slot: u16,
        value: Value,
    ) -> Result<(), JsError> {
        self.clone_frame_environment(frame, &[slot])?;
        if self.frames[frame].captured {
            *self
                .heap
                .environment_slot_mut(self.frames[frame].env, usize::from(slot))
                .ok_or_else(|| JsError("invalid handler binding".into()))? = value;
        } else {
            self.frames[frame].locals[usize::from(slot)] = value;
        }
        Ok(())
    }

    pub(super) fn activation_binding_value(&self, frame: usize, atom: Atom) -> Option<Value> {
        let slot = self.activation_binding_slot(frame, atom)?;
        let frame = self.frames.get(frame)?;
        if frame.captured {
            self.heap.environment_slot(frame.env, slot)
        } else {
            frame.locals.get(slot).copied()
        }
    }

    pub(super) fn store_activation_binding(
        &mut self,
        frame: usize,
        slot: usize,
        value: Value,
    ) -> bool {
        let activation = &self.frames[frame];
        let binding = if activation.captured {
            self.heap.environment_slot_mut(activation.env, slot)
        } else {
            self.frames[frame].locals.get_mut(slot)
        };
        let Some(binding) = binding else {
            return false;
        };
        *binding = value;
        if let Some(program) = self.programs.get(self.frames[frame].program) {
            self.mapped_argument_store(&program, frame, slot, value);
        }
        true
    }

    fn direct_eval_variable_caller(&self, frame: usize, atom: Atom) -> Option<usize> {
        let program_id = self.direct_eval_var_program?;
        let eval = self.frames.get(frame)?;
        if eval.program != program_id || eval.function != super::ROOT_FUNCTION_ID {
            return None;
        }
        let program = self.programs.get(program_id)?;
        program.functions[super::ROOT_FUNCTION_ID as usize]
            .global_var_atoms
            .contains(&atom)
            .then(|| frame.checked_sub(1))
            .flatten()
    }

    fn direct_eval_catch_binding(
        &self,
        frame: usize,
        atom: Atom,
    ) -> Option<(usize, crate::bytecode::EvalBinding)> {
        let caller = self.direct_eval_variable_caller(frame, atom)?;
        let binding = self.name_binding(caller, atom)?;
        (binding.declaration == crate::bytecode::EvalBindingDeclaration::CatchParameter)
            .then_some((caller, binding))
    }

    pub(super) fn direct_eval_var_binding(&self, frame: usize, slot: usize) -> Option<Value> {
        let eval = self.frames.get(frame)?;
        let program = self.programs.get(eval.program)?;
        let atom = *program
            .functions
            .get(eval.function as usize)?
            .local_atoms
            .get(slot)?;
        let caller = self.direct_eval_variable_caller(frame, atom)?;
        // Annex B leaves the catch binding visible to ordinary references,
        // while declaration instantiation creates the separate variable binding.
        if let Some((caller, binding)) = self.direct_eval_catch_binding(frame, atom) {
            let (environment, slot) = match binding.location {
                crate::bytecode::EvalBindingLocation::Local(slot) => {
                    let activation = &self.frames[caller];
                    if !activation.captured {
                        return activation.locals.get(usize::from(slot)).copied();
                    }
                    (activation.env, slot)
                }
                crate::bytecode::EvalBindingLocation::Capture { depth, slot } => {
                    (self.capture_env(caller, depth)?, slot)
                }
            };
            return self.heap.environment_slot(environment, usize::from(slot));
        }
        (!self.parameter_eval)
            .then(|| self.activation_binding_value(caller, atom))
            .flatten()
            .or_else(|| self.dynamic_binding(caller, atom))
    }

    pub(super) fn check_local_assignment_initialized(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        initializing: bool,
    ) -> Result<(), JsError> {
        if initializing {
            return Ok(());
        }
        let value = if self.frames[frame].captured {
            self.heap.environment_slot(self.frames[frame].env, slot)
        } else {
            self.frames[frame].locals.get(slot).copied()
        };
        if let Some(value) = value
            && value.is_deleted()
        {
            let atom = p.functions[self.frames[frame].function as usize]
                .local_atoms
                .get(slot)
                .copied()
                .ok_or_else(|| JsError("invalid local name".into()))?;
            self.checked_binding_read(p, atom, value)?;
        }
        Ok(())
    }

    /// All local execution views read the same binding owner before coercion.
    pub(super) fn load_local_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        source_atom: Option<Atom>,
    ) -> Result<Value, JsError> {
        let value = if let Some(value) = self.direct_eval_var_binding(frame, slot) {
            value
        } else if let Some(value) =
            self.root_global_lexical_value(p, self.frames[frame].function, slot)
        {
            value
        } else if let Some(atom) = self.global_object_var_atom(frame, slot) {
            self.get_property(p, self.realm.globals, atom)?
        } else if let Some(value) = self.module_import_value(
            self.frames[frame].program,
            self.frames[frame].function,
            slot,
        ) {
            value
        } else if self.frames[frame].captured {
            self.heap
                .environment_slot(self.frames[frame].env, slot)
                .ok_or_else(|| JsError("invalid local environment".into()))?
        } else {
            *self.frames[frame]
                .locals
                .get(slot)
                .ok_or_else(|| JsError("invalid local slot".into()))?
        };
        if value.is_deleted() {
            let atom = source_atom
                .or_else(|| {
                    p.functions[self.frames[frame].function as usize]
                        .local_atoms
                        .get(slot)
                        .copied()
                })
                .ok_or_else(|| JsError("invalid local name".into()))?;
            self.checked_binding_read(p, atom, value)?;
        }
        Ok(self.mapped_argument_load(p, frame, slot, value))
    }

    fn is_self_binding(&self, atom: Atom) -> bool {
        self.atom_name(atom).contains("\0quench:self-binding:")
    }

    pub(super) fn captured_lexical_this(&self, mut env: Value) -> Option<Value> {
        while let Some(Cell::Environment { parent, .. }) = self.heap.get(env) {
            let dynamic_bindings = self.heap.environment_bindings(env)?;
            if let Some((_, value)) = dynamic_bindings
                .iter()
                .rev()
                .find(|(atom, _)| self.atom_name(*atom) == "\0quench:lexical-this")
            {
                return Some(*value);
            }
            env = *parent;
        }
        None
    }

    pub(super) fn check_super_call(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let atom = self.intern_atom("\0quench:lexical-this");
        let frame = self
            .frames
            .len()
            .checked_sub(1)
            .ok_or_else(|| JsError("super call outside an activation".into()))?;
        match self.dynamic_binding(frame, atom) {
            Some(value) if value.is_deleted() => Ok(()),
            Some(_) => {
                Err(self.reference_error(p, "super constructor may only be called once".into()))
            }
            None => Err(self.reference_error(p, "super constructor binding is unavailable".into())),
        }
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

    /// Object environments participate only before the selected declarative
    /// binding. Captured scopes retain their position in the parent chain;
    /// scopes entered by the current activation are its innermost suffix.
    pub(super) fn with_objects_before_binding(&self, frame: usize, atom: Atom) -> Vec<Value> {
        let activation = &self.frames[frame];
        let parent = self.captured_parent_environment(frame);
        let own_dynamic = self
            .own_dynamic_bindings(frame)
            .is_some_and(|bindings| bindings.iter().any(|(candidate, _)| *candidate == atom));
        let named = self.name_binding(frame, atom);
        let own_static = self.activation_has_static_binding(frame, atom)
            && self
                .activation_binding_slot(frame, atom)
                .is_some_and(|slot| self.global_object_var_atom(frame, slot).is_none());
        let local = own_dynamic
            || own_static
            || named.is_some_and(|binding| {
                matches!(
                    binding.location,
                    crate::bytecode::EvalBindingLocation::Local(_)
                )
            });
        let named_owner = named.and_then(|binding| match binding.location {
            crate::bytecode::EvalBindingLocation::Capture { depth, .. } => {
                self.capture_env(frame, depth)
            }
            crate::bytecode::EvalBindingLocation::Local(_) => None,
        });
        let boundary = self
            .outer_dynamic_binding(frame, atom)
            .map(|(env, _)| env)
            .or(named_owner)
            .or_else(|| {
                self.outer_environment_binding(parent, atom)
                    .map(|(env, _)| env)
            });
        let mut layers = Vec::new();
        let mut inherited_count = 0;
        let mut before_binding = !local;
        let mut environment = parent;
        while let Some(Cell::Environment {
            parent,
            with_objects,
            ..
        }) = self.heap.get(environment)
        {
            inherited_count += with_objects.len();
            before_binding &= boundary != Some(environment);
            if before_binding && !with_objects.is_empty() {
                layers.push(with_objects.as_slice());
            }
            environment = *parent;
        }
        let active_start = (activation.with_base + inherited_count).min(self.with_stack.len());
        let mut objects = layers
            .into_iter()
            .rev()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        objects.extend_from_slice(&self.with_stack[active_start..]);
        // A lexical declaration inside an active with scope precedes that
        // object environment. Later entered with scopes still precede it.
        let declaration_depth = named.filter(|_| local || boundary == named_owner)
            .map_or(0, |binding| {
                let owner_depth = named_owner.and_then(|owner| {
                    let Cell::Environment { program, function, binding_site_pc: Some(pc), .. } = self.heap.get(owner)? else { return None; };
                    let program = self.programs.get(super::ProgramId::from_raw((*program)?))?;
                    let metadata = program.functions.get(*function as usize)?;
                    let site = &metadata.binding_sites[metadata.binding_sites.binary_search_by_key(pc, |site| site.resume_pc).ok()?];
                    let crate::bytecode::EvalBindingLocation::Capture { slot, .. } = binding.location else { return None; };
                    site.bindings.iter().find(|candidate| {
                        matches!(candidate.location, crate::bytecode::EvalBindingLocation::Local(candidate_slot) if candidate_slot == slot)
                    }).map(|candidate| candidate.with_depth)
                });
                usize::from(owner_depth.unwrap_or(binding.with_depth))
            });
        objects.drain(..declaration_depth.min(objects.len()));
        objects
    }

    fn find_with_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        atom: Atom,
    ) -> Result<Option<(Value, Value)>, JsError> {
        if self.atom_name(atom).starts_with('\0') {
            return Ok(None);
        }
        let objects = self.with_objects_before_binding(frame, atom);
        if objects.is_empty() {
            return Ok(None);
        }
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        self.with_call_roots(std::iter::once(key).chain(objects.iter().copied()), |vm| {
            for object in objects.iter().rev().copied() {
                if vm.with_binding(p, object, key, atom)? {
                    return Ok(Some((object, key)));
                }
            }
            Ok(None)
        })
    }

    pub(super) fn store_with_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        atom: Atom,
        value: Value,
        strict: bool,
    ) -> Result<bool, JsError> {
        let Some((object, _)) = self.find_with_binding(p, frame, atom)? else {
            return Ok(false);
        };
        self.set_property_with_program_mode(p, object, atom, value, strict)?;
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
        &self,
        mut env: Value,
        atom: Atom,
    ) -> Option<(Value, usize)> {
        while let Some(Cell::Environment {
            parent,
            program,
            function,
            slots,
            ..
        }) = self.heap.get(env)
        {
            if *function != u32::MAX
                && let Some(environment_program) = program.and_then(|program| {
                    self.programs
                        .get(super::program_store::ProgramId::from_raw(program))
                })
                && let Some(metadata) = environment_program.functions.get(*function as usize)
                && if *function == super::ROOT_FUNCTION_ID {
                    metadata.global_lexical_atoms.contains(&atom)
                        || self.root_local_var_binding(&environment_program, *function, atom)
                } else {
                    metadata.environment_atoms.contains(&atom)
                }
                && let Some(slot) = self.local_binding_slot(&environment_program, *function, atom)
                && slot < slots.len()
            {
                return Some((env, slot));
            }
            env = *parent;
        }
        None
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
        if let Some((environment, slot, kind)) = self.binding_reference(object) {
            self.load_environment_binding(p, environment, slot)?;
            if !self.check_named_binding_assignment(p, kind, strict)? {
                return Ok(());
            }
            return self.store_environment_binding(
                p,
                self.frames.len() - 1,
                environment,
                slot,
                value,
            );
        }
        if let Some(dynamic_bindings) = self.heap.environment_bindings_mut(object) {
            if let Some((_, binding)) = dynamic_bindings
                .iter_mut()
                .rev()
                .find(|(candidate, _)| *candidate == atom)
            {
                *binding = value;
                return Ok(());
            }
            if !strict {
                dynamic_bindings.push((atom, value));
                return Ok(());
            }
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        // ResolveName has already selected a non-global object environment.
        // Preserve that reference even if evaluating the RHS removed the
        // binding before PutValue (the specification's captured Reference).
        if object != self.realm.globals {
            let still_exists = self.has_property(p, object, key)?;
            if strict && !still_exists {
                return Err(
                    self.reference_error(p, format!("{} is not defined", self.atom_name(atom)))
                );
            }
            if let Some(success) = self.set_through_typed_array_prototype(p, object, atom, value)? {
                if !success && strict {
                    return Err(self.type_error(p, "cannot write typed array index".into()));
                }
                return Ok(());
            }
            return self.set_property_with_program_mode(p, object, atom, value, strict);
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
            return self.set_property_with_program_mode(p, object, atom, value, strict);
        }

        if let Some(frame_index) = self.frames.len().checked_sub(1)
            && (self.frames[frame_index].function != 0
                || p.functions[self.frames[frame_index].function as usize]
                    .global_lexical_atoms
                    .contains(&atom)
                || p.functions[self.frames[frame_index].function as usize]
                    .global_var_atoms
                    .contains(&atom)
                || self.root_local_var_binding(p, self.frames[frame_index].function, atom))
            && let Some(slot) = self.local_binding_slot(p, self.frames[frame_index].function, atom)
        {
            let mirrors_global_var = self
                .root_global_var_atom(
                    p,
                    self.frames[frame_index].program,
                    self.frames[frame_index].function,
                    slot,
                )
                .is_some();
            if p.functions[self.frames[frame_index].function as usize]
                .global_immutable_atoms
                .contains(&atom)
            {
                return Err(self.type_error(p, "assignment to constant binding".into()));
            }
            if mirrors_global_var
                && !self.set_property_with_receiver(
                    p,
                    self.realm.globals,
                    atom,
                    value,
                    self.realm.globals,
                )?
            {
                return if strict {
                    Err(self.type_error(p, "cannot assign to read-only global binding".into()))
                } else {
                    Ok(())
                };
            }
            if self.frames[frame_index].captured {
                let env = self.frames[frame_index].env;
                if let Some(binding) = self.heap.environment_slot_mut(env, slot) {
                    *binding = value;
                }
            } else {
                self.frames[frame_index].locals[slot] = value;
            }
            self.mapped_argument_store(p, frame_index, slot, value);
            return Ok(());
        }
        if let Some(frame_index) = self.frames.len().checked_sub(1)
            && let Some((env, slot)) =
                self.outer_environment_binding(self.captured_parent_environment(frame_index), atom)
        {
            return self.store_environment_binding(p, frame_index, env, slot as u16, value);
        }
        if self.store_global_var_binding(p, atom, value)? {
            return Ok(());
        }
        if strict && !self.has_global_object_binding(p, atom)? {
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        let _ = object;
        self.set_property_with_program_mode(p, self.realm.globals, atom, value, strict)
    }

    fn outer_dynamic_binding(&self, frame: usize, atom: Atom) -> Option<(Value, usize)> {
        self.frames.get(frame)?;
        let mut env = self.captured_parent_environment(frame);
        let named = self.name_binding(frame, atom);
        if named.is_some_and(|binding| {
            matches!(
                binding.location,
                crate::bytecode::EvalBindingLocation::Local(_)
            )
        }) {
            return None;
        }
        let named_owner = named.and_then(|binding| match binding.location {
            crate::bytecode::EvalBindingLocation::Capture { depth, .. } => {
                self.capture_env(frame, depth)
            }
            _ => None,
        });
        let lexical_capture = named.is_some_and(|binding| {
            binding.declaration != crate::bytecode::EvalBindingDeclaration::Variable
                && binding.kind != crate::bytecode::LexicalBindingKind::FunctionName
        });
        let static_owner = self
            .outer_environment_binding(env, atom)
            .map(|(env, _)| env);
        while let Some(Cell::Environment { parent, .. }) = self.heap.get(env) {
            if lexical_capture && named_owner == Some(env) {
                return None;
            }
            let dynamic_bindings = self.heap.environment_bindings(env)?;
            if let Some(index) = dynamic_bindings
                .iter()
                .rposition(|(candidate, _)| *candidate == atom)
            {
                return Some((self.heap.environment_binding_owner(env)?, index));
            }
            if static_owner == Some(env) || named_owner == Some(env) {
                return None;
            }
            env = *parent;
        }
        None
    }

    pub(super) fn store_outer_dynamic_binding(
        &mut self,
        frame: usize,
        atom: Atom,
        value: Value,
    ) -> bool {
        let Some((env, index)) = self.outer_dynamic_binding(frame, atom) else {
            return false;
        };
        let Some(dynamic_bindings) = self.heap.environment_bindings_mut(env) else {
            return false;
        };
        dynamic_bindings[index].1 = value;
        true
    }

    pub(super) fn dynamic_binding(&self, frame: usize, atom: Atom) -> Option<Value> {
        if let Some(value) = self.own_dynamic_bindings(frame).and_then(|bindings| {
            bindings
                .iter()
                .rev()
                .find_map(|(candidate, value)| (*candidate == atom).then_some(*value))
        }) {
            return Some(value);
        }
        if self.activation_has_static_binding(frame, atom) {
            return None;
        }
        let (env, index) = self.outer_dynamic_binding(frame, atom)?;
        self.heap
            .environment_bindings(env)?
            .get(index)
            .map(|(_, value)| *value)
    }

    fn activation_has_static_binding(&self, frame: usize, atom: Atom) -> bool {
        let Some(activation) = self.frames.get(frame) else {
            return false;
        };
        let Some(program) = self.programs.get(activation.program) else {
            return false;
        };
        let Some(metadata) = program.functions.get(activation.function as usize) else {
            return false;
        };
        metadata.environment_atoms.contains(&atom)
            && (!self.parameter_eval
                || activation.function == super::ROOT_FUNCTION_ID
                || metadata.parameter_atoms.contains(&atom))
    }

    pub(super) fn delete_environment_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        atom: Atom,
    ) -> Option<bool> {
        let activation = self.frames.get(frame)?;
        let metadata = p.functions.get(activation.function as usize)?;
        if (metadata.environment_atoms.contains(&atom)
            || metadata.global_lexical_atoms.contains(&atom)
            || self.root_local_var_binding(p, activation.function, atom))
            && self
                .local_binding_slot(p, activation.function, atom)
                .is_some()
        {
            return Some(false);
        }
        if let Some(index) = self
            .own_dynamic_bindings(frame)?
            .iter()
            .rposition(|(candidate, _)| *candidate == atom)
        {
            self.own_dynamic_bindings_mut(frame)?.remove(index);
            return Some(true);
        }
        if self.name_binding(frame, atom).is_some() && self.dynamic_binding(frame, atom).is_none() {
            return Some(false);
        }
        let mut env = self.frames[frame].env;
        let static_owner = self
            .outer_environment_binding(env, atom)
            .map(|(env, _)| env);
        while let Some(Cell::Environment { parent, .. }) = self.heap.get(env) {
            let dynamic_bindings = self.heap.environment_bindings(env)?;
            if static_owner == Some(env) {
                return Some(false);
            }
            let parent = *parent;
            if let Some(index) = dynamic_bindings
                .iter()
                .rposition(|(candidate, _)| *candidate == atom)
            {
                if let Some(dynamic_bindings) = self.heap.environment_bindings_mut(env) {
                    dynamic_bindings.remove(index);
                }
                return Some(true);
            }
            env = parent;
        }
        None
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

    pub(super) fn load_resolved_name(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        strict: bool,
    ) -> Result<Value, JsError> {
        if let Some((environment, slot, _)) = self.binding_reference(object) {
            return self.load_environment_binding(p, environment, slot);
        }
        if let Some(dynamic_bindings) = self.heap.environment_bindings(object) {
            let value = dynamic_bindings
                .iter()
                .rev()
                .find_map(|(candidate, value)| (*candidate == atom).then_some(*value));
            return match value {
                Some(value) => self.checked_binding_read(p, atom, value),
                None if !strict => Ok(Value::UNDEFINED),
                None => {
                    Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))))
                }
            };
        }
        if object == self.realm.globals {
            return self.load_name_without_with(p, atom, None, false);
        }
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        if !self.has_property(p, object, key)? {
            if strict {
                return Err(
                    self.reference_error(p, format!("{} is not defined", self.atom_name(atom)))
                );
            }
            return Ok(Value::UNDEFINED);
        }
        self.get_property(p, object, atom)
    }

    fn has_name_binding(&mut self, p: &ResidualProgram, atom: Atom) -> Result<bool, JsError> {
        let name = self.atom_name(atom);
        if name.starts_with('\0') {
            return Ok(true);
        }
        if let Some(frame) = self.frames.len().checked_sub(1)
            && self.find_with_binding(p, frame, atom)?.is_some()
        {
            return Ok(true);
        }
        if self
            .dynamic_binding(self.frames.len().saturating_sub(1), atom)
            .is_some()
            || self.realm.global_lexical_declarations.contains(&atom)
            || self.realm.global_lexical_bindings.contains_key(&atom)
        {
            return Ok(true);
        }
        if self
            .frames
            .last()
            .is_some_and(|frame| self.function_environment_binding(p, frame.function, atom))
        {
            return Ok(true);
        }
        if let Some(frame) = self.frames.len().checked_sub(1)
            && self.name_binding(frame, atom).is_some()
        {
            return Ok(true);
        }
        if self
            .outer_environment_binding(
                self.captured_parent_environment(self.frames.len().saturating_sub(1)),
                atom,
            )
            .is_some()
        {
            return Ok(true);
        }
        self.has_global_object_binding(p, atom)
    }

    pub(super) fn has_global_object_binding(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
    ) -> Result<bool, JsError> {
        if self.own_property(self.realm.globals, atom).is_some() {
            return Ok(true);
        }
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        self.has_property(p, self.realm.globals, key)
    }

    fn resolve_name_reference(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
    ) -> Result<(Value, bool), JsError> {
        if let Some(frame) = self.frames.len().checked_sub(1)
            && let Some((object, _)) = self.find_with_binding(p, frame, atom)?
        {
            return Ok((object, true));
        }
        if let Some(frame) = self.frames.len().checked_sub(1) {
            if self
                .own_dynamic_bindings(frame)
                .is_some_and(|bindings| bindings.iter().any(|(candidate, _)| *candidate == atom))
            {
                return Ok((self.promote_frame_environment(frame), true));
            }
            if !self.activation_has_static_binding(frame, atom)
                && let Some((env, _)) = self.outer_dynamic_binding(frame, atom)
            {
                return Ok((env, true));
            }
        }
        if let Some(frame) = self.frames.len().checked_sub(1)
            && let Some(binding) = self.name_binding(frame, atom)
        {
            let (environment, slot) = match binding.location {
                crate::bytecode::EvalBindingLocation::Local(slot) => {
                    (self.promote_frame_environment(frame), slot)
                }
                crate::bytecode::EvalBindingLocation::Capture { depth, slot } => (
                    self.capture_env(frame, depth)
                        .ok_or_else(|| JsError("invalid named reference".into()))?,
                    slot,
                ),
            };
            let environment = self
                .heap
                .environment_slot_owner(environment, usize::from(slot))
                .ok_or_else(|| JsError("invalid named reference slot".into()))?;
            let reference = self.heap.alloc(Cell::BindingReference {
                environment,
                slot,
                kind: binding.kind,
            });
            return Ok((reference, true));
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
            with_objects,
            root_eval_scope,
            ..
        }) = self.heap.get(env)
        {
            let dynamic_bindings = self.heap.environment_bindings(env)?;
            let lexical_this_wrapper = !*root_eval_scope
                && *function == u32::MAX
                && slots.is_empty()
                && (dynamic_bindings.is_empty()
                    || (dynamic_bindings.len() == 1
                        && self.atom_name(dynamic_bindings[0].0) == "\0quench:lexical-this"));
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
        depth: u16,
        slot: u16,
    ) -> Result<Value, JsError> {
        let env = self
            .capture_env(frame, depth)
            .ok_or_else(|| JsError("invalid capture environment".into()))?;
        self.load_environment_binding(p, env, slot)
    }

    fn load_environment_binding(
        &mut self,
        p: &ResidualProgram,
        env: Value,
        slot: u16,
    ) -> Result<Value, JsError> {
        let Cell::Environment {
            function, program, ..
        } = self
            .heap
            .get(env)
            .ok_or_else(|| JsError("invalid capture".into()))?
        else {
            return Err(JsError("invalid capture".into()));
        };
        let slot = usize::from(slot);
        let owner_id = program
            .map(super::ProgramId::from_raw)
            .unwrap_or(self.active_program);
        let owner_program = self.programs.get(owner_id);
        let owner = owner_program.as_deref().unwrap_or(p);
        let atom = self.root_global_var_atom(owner, owner_id, *function, slot);
        if let Some(atom) = atom {
            return self.get_property(p, self.realm.globals, atom);
        }
        let lexical_atom = self.root_global_lexical_atom(owner, *function, slot);
        if self.eval_script_context
            && let Some(atom) = lexical_atom
            && let Some(value) = self.realm.global_lexical_bindings.get(&atom).copied()
        {
            return self.checked_binding_read(p, atom, value);
        }
        let value = program
            .and_then(|program| {
                self.module_import_value(
                    super::program_store::ProgramId::from_raw(program),
                    *function,
                    slot,
                )
            })
            .or_else(|| self.heap.environment_slot(env, slot))
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
        depth: u16,
        slot: u16,
        value: Value,
    ) -> Result<(), JsError> {
        let env = self
            .capture_env(frame, depth)
            .ok_or_else(|| JsError("invalid capture environment".into()))?;
        self.store_environment_binding(p, frame, env, slot, value)
    }

    fn store_environment_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        env: Value,
        slot: u16,
        value: Value,
    ) -> Result<(), JsError> {
        let (function, program, deleted) = match self.heap.get(env) {
            Some(Cell::Environment {
                function, program, ..
            }) => (
                *function,
                *program,
                self.heap
                    .environment_slot(env, usize::from(slot))
                    .is_some_and(Value::is_deleted),
            ),
            _ => return Err(JsError("invalid capture".into())),
        };
        let owner_id = program
            .map(super::ProgramId::from_raw)
            .unwrap_or(self.frames[frame].program);
        let owner_program = self.programs.get(owner_id);
        let owner = owner_program.as_deref().unwrap_or(p);
        let atom = owner
            .functions
            .get(function as usize)
            .and_then(|metadata| metadata.local_atoms.get(usize::from(slot)))
            .copied();
        if deleted {
            let atom = owner
                .functions
                .get(function as usize)
                .and_then(|metadata| metadata.local_atoms.get(usize::from(slot)))
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
        if atom.is_some_and(|atom| self.is_self_binding(atom)) {
            if p.functions[self.frames[frame].function as usize].strict {
                return Err(self.type_error(p, "assignment to function name binding".into()));
            }
            return Ok(());
        }
        if atom.is_some_and(|atom| {
            owner.functions[function as usize]
                .global_immutable_atoms
                .contains(&atom)
        }) {
            return Err(self.type_error(p, "assignment to constant binding".into()));
        }
        if let Some(atom) = self.root_global_var_atom(owner, owner_id, function, usize::from(slot))
        {
            if !self.store_global_var_binding(p, atom, value)? {
                self.set_property_with_program_mode(
                    p,
                    self.realm.globals,
                    atom,
                    value,
                    p.functions[self.frames[frame].function as usize].strict,
                )?;
            }
            return Ok(());
        }
        let slot_index = usize::from(slot);
        let slot = self
            .heap
            .environment_slot_mut(env, slot_index)
            .ok_or_else(|| JsError("invalid capture slot".into()))?;
        *slot = value;
        self.store_environment_mapped_argument(owner, function, env, slot_index, value);
        if let Some(atom) = self.root_global_lexical_atom(owner, function, slot_index) {
            if self.eval_script_context {
                self.realm.global_lexical_bindings.insert(atom, value);
            }
            let owner = self.frames.iter().rposition(|active| {
                active.function == super::ROOT_FUNCTION_ID && active.program == owner_id
            });
            if let Some(owner) = owner {
                let (owner_env, captured) = (self.frames[owner].env, self.frames[owner].captured);
                if captured && owner_env != env {
                    if let Some(slot) = self.heap.environment_slot_mut(owner_env, slot_index) {
                        *slot = value;
                    }
                } else if !captured {
                    if let Some(slot) = self.frames[owner].locals.get_mut(slot_index) {
                        *slot = value;
                    }
                }
            }
        }
        if function == 0
            && let Some(atom) = atom
            && !owner.functions[0].global_lexical_atoms.contains(&atom)
            && !self.root_local_var_binding(owner, function, atom)
        {
            self.set_property_with_program_mode(
                p,
                self.realm.globals,
                atom,
                value,
                p.functions[self.frames[frame].function as usize].strict,
            )?;
        }
        Ok(())
    }

    pub(super) fn load_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: Option<u16>,
    ) -> Result<Value, JsError> {
        self.load_name_call(p, atom, cache, false)
            .map(|(value, _)| value)
    }

    fn load_name_without_with(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: Option<u16>,
        allow_unresolvable: bool,
    ) -> Result<Value, JsError> {
        let name = self.atom_name(atom);
        if name == "\0quench:dynamic-import" {
            return Ok(self.native_value(Native::DynamicImport));
        }
        if name == crate::bytecode::INTRINSIC_REGEXP_BINDING {
            return Ok(self.regexp_intrinsic_constructor());
        }
        if name == "\0quench:intrinsic-promise" {
            return Ok(self.native_value(Native::Promise));
        }
        if name == "\0quench:super-get" {
            return Ok(self.native_value(Native::ReflectGet));
        }
        if name == "\0quench:super-set" {
            return Ok(self.native_value(Native::SuperSet));
        }
        if name == "\0quench:super-base" {
            return Ok(self.native_value(Native::ReflectGetPrototypeOf));
        }
        if name == "\0quench:object-literal-prototype" {
            return Ok(self.native_value(Native::ObjectLiteralPrototype));
        }
        if name == "\0quench:object-define-property" {
            return Ok(self.native_value(Native::ObjectDefineProperty));
        }
        if name == "\0quench:object-freeze" {
            return Ok(self.native_value(Native::ObjectFreeze));
        }
        if let Some(value) = self.dynamic_binding(self.frames.len().saturating_sub(1), atom) {
            return self.checked_binding_read(p, atom, value);
        }
        if let Some(frame) = self.frames.len().checked_sub(1)
            && let Some(binding) = self.name_binding(frame, atom)
        {
            return self.load_name_binding(p, frame, binding);
        }
        if !name.starts_with('\0') {
            if let Some(frame) = self.frames.last()
                && (self.function_environment_binding(p, frame.function, atom)
                    || p.functions[frame.function as usize]
                        .global_lexical_atoms
                        .contains(&atom)
                    || self.root_local_var_binding(p, frame.function, atom))
                && let Some(slot) = self.local_binding_slot(p, frame.function, atom)
            {
                let value = if frame.captured {
                    self.heap
                        .environment_slot(frame.env, slot)
                        .unwrap_or(Value::UNDEFINED)
                } else {
                    frame.locals[slot]
                };
                return self.checked_binding_read(p, atom, value);
            }
            if let Some(value) = self.realm.global_lexical_bindings.get(&atom).copied() {
                return self.checked_binding_read(p, atom, value);
            }
            if let Some((env, slot)) = self.outer_environment_binding(
                self.captured_parent_environment(self.frames.len().saturating_sub(1)),
                atom,
            ) && let Some(value) = self.heap.environment_slot(env, slot)
            {
                return self.checked_binding_read(p, atom, value);
            }
            if let Some(value) = self.active_global_lexical_value(atom) {
                return self.checked_binding_read(p, atom, value);
            }
        }
        if !self.has_global_object_binding(p, atom)? {
            if allow_unresolvable {
                return Ok(Value::UNDEFINED);
            }
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        match cache {
            Some(cache) => self.get_field_cached(p, self.realm.globals, atom, cache),
            None => self.get_property(p, self.realm.globals, atom),
        }
    }

    pub(super) fn load_name_call(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: Option<u16>,
        allow_unresolvable: bool,
    ) -> Result<(Value, Value), JsError> {
        if let Some(frame) = self.frames.len().checked_sub(1)
            && let Some((object, key)) = self.find_with_binding(p, frame, atom)?
        {
            return Ok((self.get_with_binding_value(p, object, key, atom)?, object));
        }
        Ok((
            self.load_name_without_with(p, atom, cache, allow_unresolvable)?,
            Value::UNDEFINED,
        ))
    }

    pub(super) fn load_name_typeof(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: u16,
    ) -> Result<Value, JsError> {
        self.load_name_call(p, atom, Some(cache), true)
            .map(|(value, _)| value)
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
        initializing: bool,
    ) -> Result<(), JsError> {
        let name = self.atom_name(atom).to_owned();
        if !name.starts_with('\0') {
            if let Some(frame) = self.frames.len().checked_sub(1)
                && self.store_with_binding(
                    p,
                    frame,
                    atom,
                    value,
                    p.functions[self.frames[frame].function as usize].strict,
                )?
            {
                return Ok(());
            }
            if let Some(frame) = self.frames.len().checked_sub(1)
                && let Some((caller, binding)) = self.direct_eval_catch_binding(frame, atom)
                && let Some(program) = self.programs.get(self.frames[caller].program)
            {
                return self.store_name_binding(&program, caller, binding, value, false);
            }
            if self.store_direct_eval_var_binding(atom, value) {
                return Ok(());
            }
            if self.store_own_dynamic_binding(self.frames.len().saturating_sub(1), atom, value) {
                return Ok(());
            }
            if let Some(frame_index) = self.frames.len().checked_sub(1)
                && (self.frames[frame_index].function != 0
                    || p.functions[self.frames[frame_index].function as usize]
                        .global_lexical_atoms
                        .contains(&atom)
                    || self.root_local_var_binding(p, self.frames[frame_index].function, atom))
                && let Some(slot) =
                    self.local_binding_slot(p, self.frames[frame_index].function, atom)
            {
                if p.functions[self.frames[frame_index].function as usize]
                    .global_immutable_atoms
                    .contains(&atom)
                    && !initializing
                {
                    return Err(self.type_error(p, "assignment to constant binding".into()));
                }
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
                    if let Some(binding) = self.heap.environment_slot_mut(env, slot) {
                        *binding = value;
                    }
                } else {
                    self.frames[frame_index].locals[slot] = value;
                }
                self.mapped_argument_store(p, frame_index, slot, value);
                return Ok(());
            }
            if self.store_outer_dynamic_binding(self.frames.len().saturating_sub(1), atom, value) {
                return Ok(());
            }
            if let Some(frame) = self.frames.len().checked_sub(1)
                && let Some(binding) = self.name_binding(frame, atom)
            {
                let strict = p.functions[self.frames[frame].function as usize].strict;
                return self.store_name_binding(p, frame, binding, value, strict);
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
                return self.store_environment_binding(p, frame_index, env, slot as u16, value);
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

    /// Annex B targets the VariableEnvironment directly; object environments
    /// and the block's lexical binding do not participate in this write.
    pub(super) fn store_var_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        value: Value,
    ) -> Result<(), JsError> {
        let activation = &self.frames[frame];
        let atom = p.functions[activation.function as usize].local_atoms[slot];
        if let Some(caller) = self.direct_eval_variable_caller(frame, atom) {
            if !self.store_direct_eval_var_binding(atom, value) {
                // Sloppy SetMutableBinding recreates a deleted eval variable
                // in the selected VariableEnvironment, not the global object.
                self.own_dynamic_bindings_mut(caller)
                    .ok_or_else(|| JsError("invalid eval variable environment".into()))?
                    .push((atom, value));
            }
            return Ok(());
        }
        if self.global_object_var_atom(frame, slot).is_some() {
            self.set_property_with_receiver(
                p,
                self.realm.globals,
                atom,
                value,
                self.realm.globals,
            )?;
            return Ok(());
        }
        if !self.store_activation_binding(frame, slot, value) {
            return Err(JsError("invalid variable binding".into()));
        }
        Ok(())
    }

    pub(super) fn store_direct_eval_var_binding(&mut self, atom: Atom, value: Value) -> bool {
        let Some(frame) = self.frames.len().checked_sub(1) else {
            return false;
        };
        let Some(caller_index) = self.direct_eval_variable_caller(frame, atom) else {
            return false;
        };
        let slot = (!self.parameter_eval)
            .then(|| self.activation_binding_slot(caller_index, atom))
            .flatten();
        if let Some(slot) = slot {
            return self.store_activation_binding(caller_index, slot, value);
        }
        self.store_own_dynamic_binding(caller_index, atom, value)
    }

    fn store_global_var_binding(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
    ) -> Result<bool, JsError> {
        let Some(()) = self.frames.iter().rev().find_map(|frame| {
            if frame.function != super::ROOT_FUNCTION_ID || frame.this != self.realm.globals {
                return None;
            }
            let program = self.programs.get(frame.program)?;
            let function = program.functions.first()?;
            (!program.root_variables_are_local() && function.global_var_atoms.contains(&atom))
                .then_some(())
        }) else {
            return Ok(false);
        };
        let strict = self
            .frames
            .last()
            .and_then(|frame| p.functions.get(frame.function as usize))
            .is_some_and(|function| function.strict);
        if !self.set_property_with_receiver(
            p,
            self.realm.globals,
            atom,
            value,
            self.realm.globals,
        )? {
            return if strict {
                Err(self.type_error(p, "cannot assign to read-only global binding".into()))
            } else {
                Ok(true)
            };
        }
        Ok(true)
    }

    pub(super) fn mirror_global_var_property_write(
        &mut self,
        object: Value,
        atom: Atom,
        value: Value,
    ) {
        if object != self.realm.globals {
            return;
        }
        // Each script/eval activation has its own local layout. The global
        // object owns the binding; refresh every active projection in this realm.
        for frame in &mut self.frames {
            if frame.function != super::ROOT_FUNCTION_ID || frame.this != object {
                continue;
            }
            let Some(program) = self.programs.get(frame.program) else {
                continue;
            };
            let Some(function) = program.functions.first() else {
                continue;
            };
            if program.root_variables_are_local() || !function.global_var_atoms.contains(&atom) {
                continue;
            }
            let Some(slot) = function
                .local_atoms
                .iter()
                .position(|candidate| *candidate == atom)
            else {
                continue;
            };
            if frame.captured {
                if let Some(binding) = self.heap.environment_slot_mut(frame.env, slot) {
                    *binding = value;
                }
            } else {
                frame.locals[slot] = value;
            }
        }
    }
}
