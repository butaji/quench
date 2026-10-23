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
        root.slot_declaration(slot)
            .global_var
            .then(|| root.local_atoms[slot])
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
        root.slot_declaration(slot)
            .global_lexical
            .then(|| root.local_atoms[slot])
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
            if !root.declares_global_lexical(atom) {
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
            root.declares_global_var(atom) || root.declares_global_lexical(atom)
        })
    }

    fn root_local_var_binding(&self, p: &ResidualProgram, function: u32, atom: Atom) -> bool {
        p.root_variables_are_local()
            && function == super::ROOT_FUNCTION_ID
            && p.function(super::ROOT_FUNCTION_ID as usize)
                .global_var_atoms()
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
            function,
            parent,
            scope,
            ..
        } = self.heap.get(env)?
        else {
            return None;
        };
        Some((
            super::ProgramId::from_raw(scope.program()?),
            *function,
            *parent,
        ))
    }

    pub(super) fn initialize_this_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.runtime_atoms.lexical_this;
        let owner = self.lexical_this_owner(frame, atom);
        if !self.store_own_dynamic_binding(frame, atom, value)
            && !self.store_outer_dynamic_binding(frame, atom, value)
        {
            return Err(self.reference_error(p, "this binding is unavailable".into()));
        }
        // The owning constructor's callers resolve none of its bindings: only its own
        // activation and those above it (arrows and eval code it encloses) can inherit the
        // update. Without a running owner, every activation is considered.
        let first = owner
            .and_then(|(program, function, _)| {
                (0..=frame).rev().find(|&index| {
                    self.frames[index].program == program && self.frames[index].function == function
                })
            })
            .unwrap_or(0);
        for index in first..=frame {
            // An ordinary activation's receiver is authoritative even when it
            // omits unobservable lexical-this bindings. Only activations that
            // resolve this through an environment can inherit super()'s update.
            if self.frames[index].fixed_this {
                continue;
            }
            if let Some(value) = self.dynamic_binding(index, atom) {
                self.frames[index].this = value;
            }
        }
        if let Some((program_id, function, parent)) = owner {
            let program = self
                .programs
                .get(program_id)
                .ok_or_else(|| JsError("constructor program is unavailable".into()))?;
            if let Some(initializer) = program.function(function as usize).instance_initializer {
                let previous = self.replace_active_program(program_id);
                let result = self.call_user(
                    &program,
                    initializer,
                    parent,
                    value,
                    &[],
                    CallContext::Internal,
                );
                self.set_active_program(previous);
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
        p.function_get(function as usize)?
            .local_atoms
            .iter()
            .position(|candidate| {
                *candidate == atom
                    || self
                        .atom_name(*candidate)
                        .strip_prefix(name)
                        .is_some_and(|suffix| {
                            suffix.starts_with(crate::bytecode::SELF_BINDING_MARKER)
                        })
            })
    }

    fn function_environment_binding(&self, p: &ResidualProgram, function: u32, atom: Atom) -> bool {
        p.function_get(function as usize)
            .is_some_and(|metadata| metadata.declares_environment_atom(atom))
    }

    pub(super) fn activation_binding_slot(&self, frame: usize, atom: Atom) -> Option<usize> {
        let frame = self.frames.get(frame)?;
        let program = self.programs.get(frame.program)?;
        let metadata = program.function_get(frame.function as usize)?;
        // Local storage also reserves slots for source names whose actual
        // binding lives in a catch scope or an enclosing environment.
        if !metadata.declares_environment_atom(atom) {
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
        let metadata = program.function_get(activation.function as usize)?;
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
        if let Some(binding) = metadata.name_bindings.get(atom) {
            return Some(binding);
        }
        if metadata.declares_environment_atom(atom) {
            return None;
        }
        // An eval-capable parent deliberately prevents static source captures.
        // Its lexical fallback remains authoritative after that parent returns.
        let mut depth = 0u16;
        let mut environment = self.capture_env(frame, depth)?;
        while let Some(Cell::Environment {
            parent,
            function,
            scope,
            ..
        }) = self.heap.get(environment)
        {
            let (program, binding_site_pc) = (&scope.program(), &scope.binding_site_pc());
            if let Some(program) = program.and_then(|id| {
                self.programs
                    .get(super::program_store::ProgramId::from_raw(id))
            }) && let Some(metadata) = program.function_get(*function as usize)
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
                if scoped_binding.is_none() && metadata.declares_environment_atom(atom) {
                    return None;
                }
                let binding = scoped_binding.or_else(|| metadata.name_bindings.get(atom));
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
            crate::bytecode::LexicalBindingKind::Immutable | crate::bytecode::LexicalBindingKind::PrivateName => {
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
        p: &ResidualProgram,
        frame: usize,
        slot: u16,
        value: Value,
    ) -> Result<(), JsError> {
        self.clone_frame_environment(frame, &[slot], &[])?;
        *self
            .frame_binding_mut(p, frame, usize::from(slot))
            .ok_or_else(|| JsError("invalid handler binding".into()))? = value;
        Ok(())
    }

    pub(super) fn activation_binding_value(&self, frame: usize, atom: Atom) -> Option<Value> {
        let slot = self.activation_binding_slot(frame, atom)?;
        let frame = self.frames.get(frame)?;
        let environment_owned = frame.captured
            && self.programs.get(frame.program).is_none_or(|program| {
                program
                    .function_get(frame.function as usize)
                    .is_none_or(|function| function.local_slot_uses_environment(slot))
            });
        if environment_owned {
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
        let (captured, env, program_id, function_id) = {
            let activation = &self.frames[frame];
            (
                activation.captured,
                activation.env,
                activation.program,
                activation.function,
            )
        };
        let environment_owned = captured
            && self.programs.get(program_id).is_none_or(|program| {
                program
                    .function_get(function_id as usize)
                    .is_none_or(|function| function.local_slot_uses_environment(slot))
            });
        let binding = if environment_owned {
            self.heap.environment_slot_mut(env, slot)
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
        program.function(super::ROOT_FUNCTION_ID as usize)
            .global_var_atoms()
            .contains(&atom)
            .then(|| frame.checked_sub(1))
            .flatten()
            .map(|caller| self.variable_environment_frame(caller))
    }

    /// The frame owning the variable environment `frame` runs in: itself, or,
    /// for the root of a sloppy direct eval in its caller's scope, its caller's.
    pub(super) fn variable_environment_frame(&self, mut frame: usize) -> usize {
        while frame > 0
            && self.frames[frame].function == super::ROOT_FUNCTION_ID
            && self
                .programs
                .runs_in_caller_scope(self.frames[frame].program)
        {
            frame -= 1;
        }
        frame
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
            .function_get(eval.function as usize)?
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
        if let Some(value) = self.frame_binding(p, frame, slot)
            && value.is_deleted()
        {
            let atom = p.function(self.frames[frame].function as usize)
                .local_atoms
                .get(slot)
                .copied()
                .ok_or_else(|| JsError("invalid local name".into()))?;
            self.checked_binding_read(p, atom, value)?;
        }
        Ok(())
    }

    #[inline(always)]
    pub(super) fn local_slot_is_environment_owned(
        &self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
    ) -> bool {
        let activation = &self.frames[frame];
        activation.captured
            && p.function(activation.function as usize).local_slot_uses_environment(slot)
    }

    /// The binding a frame owns for `slot`: its environment's slot once the environment is
    /// captured, else its local. `None` for a slot outside both.
    pub(super) fn frame_binding(
        &self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
    ) -> Option<Value> {
        if self.local_slot_is_environment_owned(p, frame, slot) {
            self.heap.environment_slot(self.frames[frame].env, slot)
        } else {
            self.frames[frame].locals.get(slot).copied()
        }
    }

    /// `frame_binding`, to write.
    pub(super) fn frame_binding_mut(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
    ) -> Option<&mut Value> {
        if self.local_slot_is_environment_owned(p, frame, slot) {
            self.heap.environment_slot_mut(self.frames[frame].env, slot)
        } else {
            self.frames[frame].locals.get_mut(slot)
        }
    }

    /// `load_local_binding` where it raises nothing: the initialized binding a frame other than
    /// a root body owns, read through a mapped `arguments`. `None` leaves the read to
    /// `load_local_binding`.
    pub(super) fn initialized_frame_binding(
        &self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
    ) -> Option<Value> {
        if self.frames[frame].function == super::ROOT_FUNCTION_ID {
            return None;
        }
        let value = self
            .frame_binding(p, frame, slot)
            .filter(|value| !value.is_deleted())?;
        Some(self.mapped_argument_load(p, frame, slot, value))
    }

    /// `StoreLocal`, and `StoreEnvLocal` of a function that resolves no names at run time
    /// (`Function::has_dynamic_scope_access`, which the caller decides), where they raise
    /// nothing and run no guest code: the store to an initialized (or initializing) binding a
    /// frame other than a root body owns, of a slot other than its self binding, mirrored into
    /// a mapped `arguments`. `false` leaves the store to the general path.
    pub(super) fn store_initialized_frame_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        value: Value,
        initializing: bool,
    ) -> bool {
        let function_id = self.frames[frame].function;
        if function_id == super::ROOT_FUNCTION_ID {
            return false;
        }
        let function = &p.function(function_id as usize);
        if function.is_self_binding_slot(slot) {
            return false;
        }
        match self.frame_binding_mut(p, frame, slot) {
            Some(binding) if initializing || !binding.is_deleted() => *binding = value,
            _ => return false,
        }
        self.mapped_argument_store(p, frame, slot, value);
        true
    }

    /// All local execution views read the same binding owner before coercion.
    pub(super) fn load_local_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        source_atom: Option<Atom>,
    ) -> Result<Value, JsError> {
        let value = match self.root_body_binding(p, frame, slot)? {
            Some(value) => value,
            None => self
                .frame_binding(p, frame, slot)
                .ok_or_else(|| JsError("invalid local slot".into()))?,
        };
        if value.is_deleted() {
            let atom = source_atom
                .or_else(|| {
                    p.function(self.frames[frame].function as usize)
                        .local_atoms
                        .get(slot)
                        .copied()
                })
                .ok_or_else(|| JsError("invalid local name".into()))?;
            self.checked_binding_read(p, atom, value)?;
        }
        Ok(self.mapped_argument_load(p, frame, slot, value))
    }

    /// A root body's slot that stands for a binding outside its frame: a direct eval's var
    /// redirected to its caller, a global lexical or var binding, or a module import. Every
    /// other function's slot is its own.
    fn root_body_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
    ) -> Result<Option<Value>, JsError> {
        if self.frames[frame].function != super::ROOT_FUNCTION_ID {
            return Ok(None);
        }
        if let Some(value) = self.direct_eval_var_binding(frame, slot) {
            return Ok(Some(value));
        }
        if let Some(value) = self.root_global_lexical_value(p, self.frames[frame].function, slot) {
            return Ok(Some(value));
        }
        if let Some(atom) = self.global_object_var_atom(frame, slot) {
            return self.get_property(p, self.realm.globals, atom).map(Some);
        }
        Ok(self.module_import_value(
            self.frames[frame].program,
            self.frames[frame].function,
            slot,
        ))
    }

    fn is_self_binding(&self, atom: Atom) -> bool {
        self.atom_class(atom).contains(AtomClass::SELF_BINDING)
    }

    pub(super) fn captured_lexical_this(&self, mut env: Value) -> Option<Value> {
        while let Some(Cell::Environment { parent, .. }) = self.heap.get(env) {
            let dynamic_bindings = self.heap.environment_bindings(env)?;
            if let Some((_, value)) = dynamic_bindings
                .iter()
                .rev()
                .find(|(atom, _)| *atom == self.runtime_atoms.lexical_this)
            {
                return Some(*value);
            }
            env = *parent;
        }
        None
    }

    pub(super) fn check_super_call(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let atom = self.runtime_atoms.lexical_this;
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
        let Some(unscopables) = self.well_known_symbols.get(WellKnownSymbol::Unscopables) else {
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
        while let Some(Cell::Environment { parent, scope, .. }) = self.heap.get(environment) {
            let with_objects = scope.with_objects();
            inherited_count += with_objects.len();
            before_binding &= boundary != Some(environment);
            if before_binding && !with_objects.is_empty() {
                layers.push(with_objects);
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
                    let Cell::Environment { function, scope, .. } = self.heap.get(owner)? else { return None; };
                    let pc = &scope.binding_site_pc()?;
                    let program = self.programs.get(super::ProgramId::from_raw(scope.program()?))?;
                    let metadata = program.function_get(*function as usize)?;
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
        // Object environments exist only after some `with` statement was entered.
        if !self.with_scope_entered || self.atom_class(atom).contains(AtomClass::INTERNAL) {
            return Ok(None);
        }
        let objects = self.with_objects_before_binding(frame, atom);
        if objects.is_empty() {
            return Ok(None);
        }
        let key = self.heap.alloc_string(self.atom_name(atom).into());
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
                .is_some_and(|frame| p.function(frame.function as usize).strict);
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
            function,
            slots,
            scope,
        }) = self.heap.get(env)
        {
            let program = &scope.program();
            if *function != u32::MAX
                && let Some(environment_program) = program.and_then(|program| {
                    self.programs
                        .get(super::program_store::ProgramId::from_raw(program))
                })
                && let Some(metadata) = environment_program.function_get(*function as usize)
                && if *function == super::ROOT_FUNCTION_ID {
                    metadata.declares_global_lexical(atom)
                        || self.root_local_var_binding(&environment_program, *function, atom)
                } else {
                    metadata.declares_environment_atom(atom)
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
        let key = self.heap.alloc_string(self.atom_name(atom).into());
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
                || p.function(self.frames[frame_index].function as usize)
                    .global_lexical_atoms()
                    .contains(&atom)
                || p.function(self.frames[frame_index].function as usize)
                    .global_var_atoms()
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
            if p.function(self.frames[frame_index].function as usize)
                .global_immutable_atoms()
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
        self.outer_dynamic_binding_in(&NameLookup::new(frame, atom))
    }

    fn outer_dynamic_binding_in(&self, lookup: &NameLookup) -> Option<(Value, usize)> {
        let (frame, atom) = (lookup.frame, lookup.atom);
        self.frames.get(frame)?;
        let mut env = self.captured_parent_environment(frame);
        let named = lookup.named(self);
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
        let static_owner = lookup.static_owner(self).map(|(env, _)| env);
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
        self.dynamic_binding_in(&NameLookup::new(frame, atom))
    }

    fn dynamic_binding_in(&self, lookup: &NameLookup) -> Option<Value> {
        let (frame, atom) = (lookup.frame, lookup.atom);
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
        let (env, index) = self.outer_dynamic_binding_in(lookup)?;
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
        let Some(metadata) = program.function_get(activation.function as usize) else {
            return false;
        };
        metadata.declares_environment_atom(atom)
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
        let metadata = p.function_get(activation.function as usize)?;
        if (metadata.declares_environment_atom(atom)
            || metadata.declares_global_lexical(atom)
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
        let key = self.heap.alloc_string(self.atom_name(atom).into());
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
        let key = self.heap.alloc_string(self.atom_name(atom).into());
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
        self.environment_capture_env(frame.env, frame.captured, depth)
    }

    /// `capture_env` from an activation's environment `env`, which is its own when
    /// `captured`.
    pub(super) fn environment_capture_env(
        &self,
        env: Value,
        captured: bool,
        depth: u16,
    ) -> Option<Value> {
        let mut env = if captured {
            match self.heap.get(env)? {
                Cell::Environment { parent, .. } => *parent,
                _ => return None,
            }
        } else {
            env
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

    #[inline(always)]
    fn skip_with_environment_layers(&self, env: Value) -> Option<Value> {
        // With-object and lexical-this wrappers are allocated function-less, and environment
        // clones keep their source's function, so a function's own layer ends the walk before
        // its boxed scope is read.
        match self.heap.get(env) {
            Some(Cell::Environment { function, .. }) if *function == u32::MAX => {
                self.skip_function_less_layers(env)
            }
            _ => Some(env),
        }
    }

    #[inline(never)]
    fn skip_function_less_layers(&self, mut env: Value) -> Option<Value> {
        while let Some(Cell::Environment {
            parent,
            function,
            slots,
            scope,
        }) = self.heap.get(env)
        {
            if *function != u32::MAX {
                debug_assert!(scope.with_objects().is_empty());
                break;
            }
            // Only a slotless, non-eval layer can be a lexical-this wrapper, so its dynamic
            // bindings are read only after those field checks pass.
            let lexical_this_wrapper = !scope.root_eval_scope() && slots.is_empty() && {
                let dynamic_bindings = self.heap.environment_bindings(env)?;
                dynamic_bindings.is_empty()
                    || (dynamic_bindings.len() == 1
                        && dynamic_bindings[0].0 == self.runtime_atoms.lexical_this)
            };
            if scope.with_objects().is_empty() && !lexical_this_wrapper {
                break;
            }
            env = *parent;
        }
        Some(env)
    }

    #[inline(always)]
    pub(super) fn capture(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        depth: u16,
        slot: u16,
    ) -> Result<Value, JsError> {
        match self.direct_capture(frame, depth, slot) {
            Some(value) => Ok(value),
            None => self.walked_capture(p, frame, depth, slot),
        }
    }

    /// The capture read when every layer on the walk belongs to a function, so no with-object
    /// or lexical-this wrapper is skipped, and the binding is an initialized slot its
    /// environment owns: a non-root slot, or a root slot no projection reads through. One cell
    /// read per layer; anything else is `None`.
    #[inline(always)]
    pub(super) fn direct_capture(&self, frame: usize, depth: u16, slot: u16) -> Option<Value> {
        let (slots, root) = self.direct_capture_slots(frame, depth)?;
        self.owned_capture(slots, root, slot)
    }

    /// `slot` of a capture walk's environment when the environment owns the binding: any slot
    /// of a non-root function's environment; of a root environment (`root` names its program),
    /// only a slot no projection reads through.
    #[inline(always)]
    pub(super) fn owned_capture(
        &self,
        slots: &crate::heap::EnvironmentSlots,
        root: Option<Option<u32>>,
        slot: u16,
    ) -> Option<Value> {
        if let Some(program) = root
            && !self.root_environment_slot_is_binding(program, slot)
        {
            return None;
        }
        let value = match slots.state(usize::from(slot))? {
            crate::heap::EnvironmentSlotState::Owned(value) => value,
            // A shared slot names its owner, which holds the slot itself.
            crate::heap::EnvironmentSlotState::Shared(owner) => {
                self.heap.environment_slot(owner, usize::from(slot))?
            }
        };
        (!value.is_deleted()).then_some(value)
    }

    /// The slots a direct capture at `depth` reads, and the program of a root environment
    /// (`owned_capture`): the walk from the frame's environment (past its own, once captured)
    /// when every layer belongs to a function.
    fn direct_capture_slots(
        &self,
        frame: usize,
        depth: u16,
    ) -> Option<(&crate::heap::EnvironmentSlots, Option<Option<u32>>)> {
        let frame = &self.frames[frame];
        self.environment_capture_slots(frame.env, frame.captured, depth)
    }

    /// `direct_capture_slots` from an activation's environment `env`, which is its own
    /// when `captured`.
    pub(super) fn environment_capture_slots(
        &self,
        env: Value,
        captured: bool,
        depth: u16,
    ) -> Option<(&crate::heap::EnvironmentSlots, Option<Option<u32>>)> {
        let mut cell = self.heap.get(env)?;
        if captured {
            let Cell::Environment { parent, .. } = cell else {
                return None;
            };
            cell = self.heap.get(*parent)?;
        }
        let mut remaining = depth;
        loop {
            let Cell::Environment {
                parent,
                function,
                slots,
                scope,
                ..
            } = cell
            else {
                return None;
            };
            if *function == u32::MAX {
                return None;
            }
            if remaining == 0 {
                return Some((
                    slots,
                    (*function == super::ROOT_FUNCTION_ID).then(|| scope.program()),
                ));
            }
            remaining -= 1;
            cell = self.heap.get(*parent)?;
        }
    }

    /// `slot` at the end of a capture walk `walked_capture_slots` remembers, when that end is
    /// a function environment holding the slot densely; `None` leaves the read to the walk.
    #[inline(always)]
    pub(super) fn remembered_capture(
        &self,
        env: Value,
        captured: bool,
        depth: u16,
        slot: u16,
    ) -> Option<Value> {
        let walk = &self.capture_walks[capture_walk_index(env, depth)?];
        if walk.depth != depth
            || walk.cache.env != env
            || walk.cache.captured != captured
            || walk.cache.epoch != self.heap.collection_epoch()
            || walk.cache.slots.is_null()
            || walk.cache.root.is_some()
        {
            return None;
        }
        // SAFETY: as `walked_capture_slots`: the walk ends at a live cell this collection.
        unsafe { &*walk.cache.slots }.dense()?.dense_plain(usize::from(slot))
    }

    /// `environment_capture_slots` through the walks it remembers (`capture_walks`): a walk
    /// from the same environment, equally its own or not, to the same depth in the same
    /// collection ends at the same live cell, whose slot storage never moves.
    #[inline(always)]
    pub(super) fn walked_capture_slots(
        &mut self,
        env: Value,
        captured: bool,
        depth: u16,
    ) -> Option<(*const crate::heap::EnvironmentSlots, Option<Option<u32>>)> {
        let epoch = self.heap.collection_epoch();
        let index = capture_walk_index(env, depth)?;
        let walk = &self.capture_walks[index];
        if walk.depth == depth
            && walk.cache.env == env
            && walk.cache.captured == captured
            && walk.cache.epoch == epoch
            && !walk.cache.slots.is_null()
        {
            return Some((walk.cache.slots, walk.cache.root));
        }
        let (slots, root) = self.environment_capture_slots(env, captured, depth)?;
        let slots: *const crate::heap::EnvironmentSlots = slots;
        self.capture_walks[index] = CaptureWalk {
            depth,
            cache: CaptureCache { env, captured, epoch, slots, root },
        };
        Some((slots, root))
    }

    /// The frame's cached innermost slots and root program while the cache is current
    /// (`cached_direct_capture`).
    #[inline(always)]
    pub(super) fn current_capture_slots(
        &self,
        frame: usize,
    ) -> Option<(*const crate::heap::EnvironmentSlots, Option<Option<u32>>)> {
        let frame = &self.frames[frame];
        let cache = &frame.capture_cache;
        (cache.env == frame.env
            && cache.captured == frame.captured
            && cache.epoch == self.heap.collection_epoch()
            && !cache.slots.is_null())
        .then_some((cache.slots, cache.root))
    }


    /// `direct_capture` through the frame's cached innermost slots. The cache names the
    /// environment, whether it is the frame's own, and the collection it was taken in: while
    /// all three are unchanged the walk ends at the same live cell, whose slot storage never
    /// moves.
    #[inline(always)]
    pub(super) fn cached_direct_capture(
        &mut self,
        frame: usize,
        depth: u16,
        slot: u16,
    ) -> Option<Value> {
        if depth != 0 {
            let (env, captured) = (self.frames[frame].env, self.frames[frame].captured);
            let (slots, root) = self.walked_capture_slots(env, captured, depth)?;
            // SAFETY: as `walked_capture_slots`: the walk ends at a live cell this collection.
            return self.owned_capture(unsafe { &*slots }, root, slot);
        }
        let (slots, root) = if let Some(current) = self.current_capture_slots(frame) {
            current
        } else {
            let epoch = self.heap.collection_epoch();
            let (slots, root) = self.direct_capture_slots(frame, 0)?;
            let slots: *const crate::heap::EnvironmentSlots = slots;
            self.frames[frame].capture_cache = CaptureCache {
                env: self.frames[frame].env,
                captured: self.frames[frame].captured,
                epoch,
                slots,
                root,
            };
            (slots, root)
        };
        // SAFETY: see above; the pointer was taken from the frame's current environment.
        self.owned_capture(unsafe { &*slots }, root, slot)
    }

    /// A capture that can be read from an owned or shared environment slot after applying the same
    /// functionless-layer skipping as `capture`, without projected property lookup.
    pub(super) fn jit_owned_capture(
        &mut self,
        frame: usize,
        depth: u16,
        slot: u16,
    ) -> Option<Value> {
        if let Some(value) = self.cached_direct_capture(frame, depth, slot) {
            return Some(value);
        }
        let env = self.capture_env(frame, depth)?;
        let Some(Cell::Environment {
            function,
            scope,
            ..
        }) = self.heap.get(env)
        else {
            return None;
        };
        let root = *function == super::ROOT_FUNCTION_ID;
        if root && !self.root_environment_slot_is_binding(scope.program(), slot) {
            return None;
        }
        let Some(value) = self
            .heap
            .environment_slot(env, usize::from(slot))
            .filter(|value| !value.is_deleted())
        else {
            return None;
        };
        Some(value)
    }

    #[cold]
    #[inline(never)]
    fn walked_capture(
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

    #[inline(always)]
    fn load_environment_binding(
        &mut self,
        p: &ResidualProgram,
        env: Value,
        slot: u16,
    ) -> Result<Value, JsError> {
        // Global var, global lexical and module-import projections exist only for a program's
        // root function, so any other initialized environment slot, and a root slot of a
        // program without projections, is the binding itself.
        if let Some(Cell::Environment {
            function, scope, ..
        }) = self.heap.get(env)
            && (*function != super::ROOT_FUNCTION_ID
                || self.root_environment_slot_is_binding(scope.program(), slot))
            && let Some(value) = self.heap.environment_slot(env, usize::from(slot))
            && !value.is_deleted()
        {
            return Ok(value);
        }
        self.load_projected_environment_binding(p, env, slot)
    }

    /// Whether `slot` of a root environment owned by `program` (the active program when
    /// `None`) is the binding itself rather than a projection.
    #[inline(always)]
    fn root_environment_slot_is_binding(&self, program: Option<u32>, slot: u16) -> bool {
        self.programs.root_slot_is_binding(
            program.map_or(self.active_program, super::ProgramId::from_raw),
            slot,
            !self.eval_script_context,
        )
    }

    /// `load_projected_environment_binding` where reading runs no guest code and throws
    /// nothing: a global var held as a data property of the global object or its prototypes
    /// (`Vm::plain_named_get`), or, in an eval script's context, an initialized global
    /// lexical binding (`root_global_lexical_value`). `None` for any other projection or
    /// read.
    pub(super) fn plain_projected_root_binding(&self, env: Value, slot: u16) -> Option<Value> {
        let Cell::Environment { function, scope, .. } = self.heap.get(env)? else {
            return None;
        };
        let owner_id = scope
            .program()
            .map_or(self.active_program, super::ProgramId::from_raw);
        let owner = self.programs.residual(owner_id)?;
        if let Some(atom) = self.root_global_var_atom(owner, owner_id, *function, usize::from(slot)) {
            return self.plain_named_get(self.realm.globals, atom);
        }
        self.root_global_lexical_value(owner, *function, usize::from(slot))
            .filter(|value| !value.is_deleted())
    }

    /// Reads a root-function projection (global var, global lexical or module import) or
    /// reports an uninitialized binding.
    #[inline(never)]
    fn load_projected_environment_binding(
        &mut self,
        p: &ResidualProgram,
        env: Value,
        slot: u16,
    ) -> Result<Value, JsError> {
        let Cell::Environment {
            function, scope, ..
        } = self
            .heap
            .get(env)
            .ok_or_else(|| JsError("invalid capture".into()))?
        else {
            return Err(JsError("invalid capture".into()));
        };
        let program = scope.program();
        let slot = usize::from(slot);
        let owner_id = program
            .map(super::ProgramId::from_raw)
            .unwrap_or(self.active_program);
        let owner = self.programs.residual(owner_id).unwrap_or(p);
        let atom = self.root_global_var_atom(owner, owner_id, *function, slot);
        let lexical_atom = self.root_global_lexical_atom(owner, *function, slot);
        if let Some(atom) = atom {
            return self.get_property(p, self.realm.globals, atom);
        }
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
                        .function_get(*function as usize)
                        .and_then(|metadata| metadata.local_atoms.get(slot))
                        .copied()
                })
                .or_else(|| {
                    p.function_get(*function as usize)
                        .and_then(|metadata| metadata.local_atoms.get(slot))
                        .copied()
                })
                .unwrap_or_default();
            return Err(self.uninitialized_binding_error(p, atom));
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
        if self.direct_store_environment(p, self.frames[frame].program, env, slot, value) {
            return Ok(());
        }
        self.store_environment_binding(p, frame, env, slot, value)
    }

    /// The capture write when the binding is an initialized slot of a nested function's own
    /// environment in the executing program, and neither that function's self binding nor a
    /// parameter aliased by a mapped `arguments` object: then the slot write is the whole
    /// store. Root bindings, with-object layers and everything else return false for
    /// `store_environment_binding`.
    #[inline(always)]
    pub(super) fn direct_store_environment(
        &mut self,
        p: &ResidualProgram,
        program: super::ProgramId,
        env: Value,
        slot: u16,
        value: Value,
    ) -> bool {
        let Some(Cell::Environment {
            function, scope, ..
        }) = self.heap.get(env)
        else {
            return false;
        };
        debug_assert_eq!(program, self.active_program);
        if !environment_admits_direct_store(p, program, *function, scope, slot) {
            return false;
        }
        let slot = usize::from(slot);
        match self.heap.environment_slot_mut(env, slot) {
            Some(binding) if !binding.is_deleted() => {
                *binding = value;
                true
            }
            _ => false,
        }
    }

    /// `direct_store_environment` of a slot `env` owns: one lookup, which also makes the
    /// write barrier's note, for the running frameless activation's own environment. A
    /// slot another environment holds, or one holding any marker (an uninitialized binding),
    /// returns `false` for the general store.
    #[inline(always)]
    pub(super) fn direct_store_own_environment(
        &mut self,
        p: &ResidualProgram,
        program: super::ProgramId,
        env: Value,
        slot: u16,
        value: Value,
    ) -> bool {
        let Some(Cell::Environment {
            function,
            scope,
            slots,
            ..
        }) = self.heap.get_mut(env)
        else {
            return false;
        };
        if !environment_admits_direct_store(p, program, *function, scope, slot) {
            return false;
        }
        match slots.owned_mut(usize::from(slot)) {
            Some(binding) if !binding.is_internal_marker() => {
                *binding = value;
                true
            }
            _ => false,
        }
    }

    /// Whether `environment_admits_direct_store` admits a store to every slot of `env` in
    /// the executing program: its function has no self binding and maps no parameter.
    pub(super) fn admits_every_direct_store(
        &self,
        p: &ResidualProgram,
        program: super::ProgramId,
        env: Value,
    ) -> bool {
        let Some(Cell::Environment {
            function, scope, ..
        }) = self.heap.get(env)
        else {
            return false;
        };
        direct_store_owner(p, program, *function, scope).is_some_and(owner_admits_every_direct_store)
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
                function, scope, ..
            }) => (
                *function,
                scope.program(),
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
            .function_get(function as usize)
            .and_then(|metadata| metadata.local_atoms.get(usize::from(slot)))
            .copied();
        if deleted {
            let atom = owner
                .function_get(function as usize)
                .and_then(|metadata| metadata.local_atoms.get(usize::from(slot)))
                .copied()
                .unwrap_or_default();
            return Err(self.uninitialized_binding_error(p, atom));
        }
        if atom.is_some_and(|atom| self.is_self_binding(atom)) {
            if p.function(self.frames[frame].function as usize).strict {
                return Err(self.type_error(p, "assignment to function name binding".into()));
            }
            return Ok(());
        }
        if atom.is_some_and(|atom| {
            owner.function(function as usize)
                .global_immutable_atoms()
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
                    p.function(self.frames[frame].function as usize).strict,
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
                self.realm.bind_global_lexical(atom, value);
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
            && !owner.function(0).declares_global_lexical(atom)
            && !self.root_local_var_binding(owner, function, atom)
        {
            self.set_property_with_program_mode(
                p,
                self.realm.globals,
                atom,
                value,
                p.function(self.frames[frame].function as usize).strict,
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

    /// The value an intrinsic name (`bytecode::intrinsic_name_native`, or the RegExp
    /// intrinsic) stands for, which no binding can shadow.
    pub(super) fn intrinsic_name_value(&mut self, atom: Atom) -> Option<Value> {
        let name = self.atom_name(atom);
        if name == crate::bytecode::INTRINSIC_REGEXP_BINDING {
            return Some(self.regexp_intrinsic_constructor());
        }
        let native = crate::bytecode::intrinsic_name_native(name)?;
        Some(self.native_value(native))
    }

    fn load_name_without_with(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: Option<u16>,
        allow_unresolvable: bool,
    ) -> Result<Value, JsError> {
        let internal = self.atom_class(atom).contains(AtomClass::INTERNAL);
        if internal {
            if let Some(value) = self.intrinsic_name_value(atom) {
                return Ok(value);
            }
        }
        if !internal && self.names_resolve_to_global_object(p, atom) {
            return self.load_global_object_name(p, atom, cache, allow_unresolvable);
        }
        // Where names resolve statically, no layer searched before the realm's global lexical
        // bindings can hold the name, so a binding there is the one the search finds.
        if !internal
            && self.global_scope_names_resolve_statically(p)
            && let Some(value) = self.realm.global_lexical_bindings.get(&atom).copied()
        {
            return self.checked_binding_read(p, atom, value);
        }
        // One lookup consults the frame's static binding and the outer static owner from
        // more than one rule; each is found once.
        let lookup = NameLookup::new(self.frames.len().saturating_sub(1), atom);
        if let Some(value) = self.dynamic_binding_in(&lookup) {
            return self.checked_binding_read(p, atom, value);
        }
        if !self.frames.is_empty()
            && let Some(binding) = lookup.named(self)
        {
            return self.load_name_binding(p, lookup.frame, binding);
        }
        if !internal {
            if let Some(frame) = self.frames.last()
                && (self.function_environment_binding(p, frame.function, atom)
                    || p.function(frame.function as usize)
                        .global_lexical_atoms()
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
            if let Some((env, slot)) = lookup.static_owner(self)
                && let Some(value) = self.heap.environment_slot(env, slot)
            {
                return self.checked_binding_read(p, atom, value);
            }
            if let Some(value) = self.active_global_lexical_value(atom) {
                return self.checked_binding_read(p, atom, value);
            }
        }
        self.load_global_object_name(p, atom, cache, allow_unresolvable)
    }

    /// Whether `atom` loaded by name in the executing activation can only denote a global
    /// object property: its function is nested and, with every enclosing function, free of
    /// dynamic scope access, so the compiler resolved every lexical binding statically; the
    /// code runs in the global environment, as script code or a global-scope eval; and the
    /// realm declares no global lexical binding of that name, which every started script
    /// records before it runs. None of the dynamic layers `load_name_without_with` searches
    /// can then hold the name.
    #[inline(always)]
    fn names_resolve_to_global_object(&self, p: &ResidualProgram, atom: Atom) -> bool {
        !self.realm.global_lexical_declarations.contains(&atom)
            && self.global_scope_names_resolve_statically(p)
    }

    /// Whether the executing activation's names, with every enclosing function's, the compiler
    /// resolved statically where only global bindings lie beyond them
    /// (`ProgramStore::unresolved_names_are_global`): a by-name load there can only denote a
    /// global binding.
    #[inline(always)]
    fn global_scope_names_resolve_statically(&self, p: &ResidualProgram) -> bool {
        self.frames.last().is_some_and(|frame| {
            self.programs.unresolved_names_are_global(frame.program, frame.function)
                && p.function_get(frame.function as usize)
                    .is_some_and(|function| function.names_resolve_statically(p))
        })
    }

    /// The global object step of a by-name load: the property, or a `ReferenceError` (or
    /// `undefined` for `typeof`) when the global object has no such binding.
    fn load_global_object_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        cache: Option<u16>,
        allow_unresolvable: bool,
    ) -> Result<Value, JsError> {
        if let Some(cache) = cache
            && let super::object::FieldCacheProbe::Hit(value) =
                self.cached_field_hit(p, self.realm.globals, cache)
        {
            return Ok(value);
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
            return Err(self.uninitialized_binding_error(p, atom));
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
                    p.function(self.frames[frame].function as usize).strict,
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
                    || p.function(self.frames[frame_index].function as usize)
                        .global_lexical_atoms()
                        .contains(&atom)
                    || self.root_local_var_binding(p, self.frames[frame_index].function, atom))
                && let Some(slot) =
                    self.local_binding_slot(p, self.frames[frame_index].function, atom)
            {
                if p.function(self.frames[frame_index].function as usize)
                    .global_immutable_atoms()
                    .contains(&atom)
                    && !initializing
                {
                    return Err(self.type_error(p, "assignment to constant binding".into()));
                }
                let binding =
                    p.function(self.frames[frame_index].function as usize).local_atoms[slot];
                if self.is_self_binding(binding) {
                    if p.function(self.frames[frame_index].function as usize).strict {
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
                let strict = p.function(self.frames[frame].function as usize).strict;
                return self.store_name_binding(p, frame, binding, value, strict);
            }
            if self.realm.global_lexical_bindings.contains_key(&atom) {
                if self.realm.immutable_global_lexical_bindings.contains(&atom) {
                    return Err(self.type_error(p, "assignment to constant binding".into()));
                }
                self.realm.bind_global_lexical(atom, value);
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
            .and_then(|frame| p.function_get(frame.function as usize))
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
        let atom = p.function(activation.function as usize).local_atoms[slot];
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
            (!program.root_variables_are_local() && function.declares_global_var(atom))
                .then_some(())
        }) else {
            return Ok(false);
        };
        let strict = self
            .frames
            .last()
            .and_then(|frame| p.function_get(frame.function as usize))
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
        for frame in self.frames.iter_mut() {
            if frame.function != super::ROOT_FUNCTION_ID || frame.this != object {
                continue;
            }
            let Some(program) = self.programs.get(frame.program) else {
                continue;
            };
            let Some(function) = program.functions.first() else {
                continue;
            };
            if program.root_variables_are_local() || !function.declares_global_var(atom) {
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

/// The facts one by-name lookup of `atom` from `frame` consults under more than one rule:
/// the frame's static binding for the name (`Vm::name_binding`) and the outer environment
/// that statically declares it (`Vm::outer_environment_binding` from the frame's captured
/// parent). Each is found on first use and stays valid for the lookup, which runs no guest
/// code between consulting them.
struct NameLookup {
    frame: usize,
    atom: Atom,
    named: std::cell::OnceCell<Option<crate::bytecode::EvalBinding>>,
    static_owner: std::cell::OnceCell<Option<(Value, usize)>>,
}

impl NameLookup {
    fn new(frame: usize, atom: Atom) -> Self {
        Self {
            frame,
            atom,
            named: std::cell::OnceCell::new(),
            static_owner: std::cell::OnceCell::new(),
        }
    }

    fn named<H: Host>(&self, vm: &Vm<H>) -> Option<crate::bytecode::EvalBinding> {
        *self.named.get_or_init(|| vm.name_binding(self.frame, self.atom))
    }

    fn static_owner<H: Host>(&self, vm: &Vm<H>) -> Option<(Value, usize)> {
        *self.static_owner.get_or_init(|| {
            vm.frames.get(self.frame)?;
            vm.outer_environment_binding(vm.captured_parent_environment(self.frame), self.atom)
        })
    }
}

/// Whether a store to `slot` of an environment of `function` with `scope` is a plain slot
/// write in `program`: not the root's, a function-less layer's or another program's, nor a
/// self binding or a mapped arguments parameter, which the general store handles.
#[inline(always)]
fn environment_admits_direct_store(
    p: &ResidualProgram,
    program: super::ProgramId,
    function: u32,
    scope: &crate::heap::EnvironmentScopeRef,
    slot: u16,
) -> bool {
    let slot = usize::from(slot);
    direct_store_owner(p, program, function, scope).is_some_and(|owner| {
        !owner.is_self_binding_slot(slot)
            && super::arguments::mapped_arguments_slot(owner, slot).is_none()
    })
}

/// The function owning an environment of `function` with `scope` whose slots
/// `environment_admits_direct_store` may admit: not the root's, a function-less layer's or
/// another program's.
#[inline(always)]
/// Whether a store to any captured slot of `owner`'s environment is the whole write: no slot
/// is its own name's binding (`Function::self_binding_slot`) or a parameter a mapped
/// `arguments` object aliases. A mapped object aliases every parameter or none, so parameter 0
/// answers for all of them.
pub(super) fn owner_admits_every_direct_store(owner: &crate::bytecode::Function) -> bool {
    owner.self_binding_slot.is_none()
        && (owner.params == 0 || super::arguments::mapped_arguments_slot(owner, 0).is_none())
}

pub(super) fn direct_store_owner<'p>(
    p: &'p ResidualProgram,
    program: super::ProgramId,
    function: u32,
    scope: &crate::heap::EnvironmentScopeRef,
) -> Option<&'p crate::bytecode::Function> {
    if function == super::ROOT_FUNCTION_ID
        || function == u32::MAX
        || scope.program().is_some_and(|owner| owner != program.raw())
    {
        return None;
    }
    // Read on demand: an environment restored from a startup snapshot can outlive its
    // function's record in this process.
    p.function_get(function as usize)
}

/// The `Vm::capture_walks` entry a walk from `env` to `depth` uses.
#[inline(always)]
fn capture_walk_index(env: Value, depth: u16) -> Option<usize> {
    Some((env.heap_index()? as usize ^ usize::from(depth) * (CAPTURE_WALKS / 2 + 1)) & (CAPTURE_WALKS - 1))
}
