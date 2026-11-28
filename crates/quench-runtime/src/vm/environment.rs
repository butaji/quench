//! # Name resolution entry points
//!
//! By-name reads walk one chain, the layers `locate_name` searches in order: `with` objects
//! (`find_with_binding`), then the static layers (`resolve_name_source`: dynamic bindings,
//! named bindings, locals, global lexicals, outer static owner, active global lexical). The
//! shared walk is `locate_name` / `locate_name_without_with` (location) followed by
//! `read_name_location` (value); the entries below differ only in what they do with it.
//!
//! - `load_name`: value of a by-name read; unresolvable global throws `ReferenceError`.
//! - `load_name_call`: value and the `with` object it came from (the call's `this`); throws like `load_name`.
//! - `load_name_typeof`: `load_name_call` with unresolvable globals read as `undefined`.
//! - `locate_name`: the core; the `with` object if one holds the name, else the static location. Runs guest code only for `with` has/unscopables.
//! - `locate_name_without_with`: the core minus `with`; intrinsics first, then `resolve_name_source`.
//! - `resolve_name_source` (name_scope.rs): static layers only, returns a `NameLocation`, never reads or throws.
//! - `read_name_location`: the read each location implies (TDZ via `checked_binding_read`, global `ReferenceError`/`undefined`).
//! - `load_resolved_name`: value of a reference from `resolve_name`; strict unresolvable throws.
//! - `resolve_name` / `resolve_name_reference`: ResolveName, producing the reference base (object, sentinel or `UNRESOLVABLE_NAME_REFERENCE`); no read.
//! - `has_name_binding`: boolean existence for strict ResolveName; only guest `has` traps can run.
//! - `name_binding`: the static `EvalBinding` the caller's metadata or capture chain declares; no value, no guest code.
//! - `dynamic_binding_slot_in` / `dynamic_binding_in` / `dynamic_binding`: the dynamic record binding (own or outer) as a slot or value.
//! - `outer_dynamic_binding` / `outer_dynamic_binding_in`: the owning environment and index of an outer dynamic binding.
//! - `outer_environment_binding`: the static slot of an enclosing environment that declares the name.
//! - `with_objects_before_binding`, `find_with_binding`, `first_with_binding`, `plain_first_with_binding`, `with_binding`: the `with` objects in scope before the binding, and their has test.
//! - `store_name`: PutValue by name; creates a sloppy global property when nothing binds the name. `store_resolved_name` does the same for a resolved reference.
//! - `store_with_binding`, `store_outer_dynamic_binding`, `store_direct_eval_var_binding`, `store_catch_binding`: the write-side layers of the same chain.
//! - `direct_eval_variable_caller`, `direct_eval_catch_binding`, `direct_eval_var_binding`: a sloppy direct eval's variable scope and Annex B catch bindings.
//! - `delete_environment_binding`: `delete name`; returns `None` where no layer declares it.
//! - `lexical_this_owner`, `check_super_call`: the `this` binding, stored under the reserved lexical-this atom.
//! - `load_global_object_name`, `has_global_object_binding`, `load_global_lexical_binding`, `store_global_lexical_assignment`: the global object and global lexical ends of the chain.
//! - `dispatch_fast::lane_name_resolution`: a JIT lane's compile-time classifier (global, intrinsic, or general); it reads no bindings and does not walk the chain.
//!
//! Not merged into the core on purpose: the write paths (`store_name`, `store_resolved_name`)
//! interleave PutValue ordering, mapped arguments and `with` side effects that a read does not
//! have; `resolve_name` produces a reference, not a value, so its result is not a location to read.

use super::name_scope::{DynamicSlot, NameLocation};
use super::caller_view::CallerView;
use super::property_key::PropertyKey;
use super::*;

// ResolveName bases are objects; this primitive preserves an unresolvable
// Reference without allocating or rejecting it before the RHS executes.
const UNRESOLVABLE_NAME_REFERENCE: Value = Value::UNDEFINED;
// A realm lexical binding is selected before the RHS. Its declarative owner
// cannot be replaced by a new object property or a binding created by the RHS.
// ResolveName's other bases are objects, so null names this allocation-free base.
const GLOBAL_LEXICAL_NAME_REFERENCE: Value = Value::NULL;

impl Vm {
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

    pub(super) fn global_object_var_atom(&self, caller: &dyn CallerView, slot: usize) -> Option<Atom> {
        if caller.function() != super::ROOT_FUNCTION_ID {
            return None;
        }
        let program = self.programs.get(caller.program())?;
        self.root_global_var_atom(&program, caller.program(), caller.function(), slot)
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
        self.root_global_lexical_atom(program, function, slot)
            .and_then(|atom| self.realm.global_lexical_bindings.get(&atom).copied())
    }

    /// Runs the root activation `root` of `p` to completion, recorded in `active_roots` while it
    /// runs. The root's frame is pushed at the frame count taken here, so deferred lane activations
    /// are materialized first: they would otherwise land below the root.
    pub(super) fn call_root(&mut self, p: &ResidualProgram, root: Value, this: Value) -> Result<Value, super::exit::Exit> {
        self.materialize_deferred_lane();
        self.active_roots.push(self.frames.len());
        let result = self.call_value(p, root, this, &[]).map_err(super::exit::Exit::of);
        self.active_roots.pop();
        result
    }

    fn root_declares_binding(&self, program: &ResidualProgram, atom: Atom) -> bool {
        program.functions.first().is_some_and(|root| {
            root.declares_global_var(atom) || root.declares_global_lexical(atom)
        })
    }

    pub(super) fn root_local_var_binding(&self, p: &ResidualProgram, function: u32, atom: Atom) -> bool {
        p.root_variables_are_local()
            && function == super::ROOT_FUNCTION_ID
            && p.function(super::ROOT_FUNCTION_ID as usize)
                .global_var_atoms()
                .contains(&atom)
    }

    /// The activation owning lexical this also owns constructor initialization.
    pub(super) fn lexical_this_owner(
        &self,
        caller: &dyn CallerView,
        atom: Atom,
    ) -> Option<(super::ProgramId, u32, Value)> {
        if self
            .own_dynamic_bindings(caller)?
            .iter()
            .any(|(key, _)| *key == atom)
        {
            return Some((
                caller.program(),
                caller.function(),
                self.captured_parent_environment(caller),
            ));
        }
        let (env, _) = self.outer_dynamic_binding(caller, atom)?;
        let crate::heap::EnvironmentRecord {
            function,
            parent,
            scope,
            ..
        } = self.heap.environment(env)?;
        Some((
            super::ProgramId::from_raw(scope.program()?),
            *function,
            *parent,
        ))
    }

    pub fn initialize_this_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.runtime_atoms.lexical_this;
        let owner = self.lexical_this_owner(self.frame_caller(frame), atom);
        if !self.store_owner_binding(VarOwner::Frame(frame), atom, value)
            && !self.store_outer_dynamic_binding(VarOwner::Frame(frame), atom, value)
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
            if let Some(value) = self.dynamic_binding(self.frame_caller(index), atom) {
                self.frames[index].this = value;
            }
        }
        if let Some((program_id, function, parent)) = owner {
            let program = self
                .programs
                .get(program_id)
                .ok_or_else(|| JsError("constructor program is unavailable".into()))?;
            if let Some(initializer) = program.function(function as usize).instance_initializer() {
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

    pub(super) fn function_environment_binding(&self, p: &ResidualProgram, function: u32, atom: Atom) -> bool {
        p.function_get(function as usize)
            .is_some_and(|metadata| metadata.declares_environment_atom(atom))
    }

    pub(super) fn activation_binding_slot(&self, caller: &dyn CallerView, atom: Atom) -> Option<usize> {
        let program = self.programs.get(caller.program())?;
        let metadata = program.function_get(caller.function() as usize)?;
        // Local storage also reserves slots for source names whose actual
        // binding lives in a catch scope or an enclosing environment.
        if !metadata.declares_environment_atom(atom) {
            return None;
        }
        metadata.local_slot(atom)
    }

    pub(super) fn name_binding(
        &self,
        caller: &dyn CallerView,
        atom: Atom,
    ) -> Option<crate::bytecode::EvalBinding> {
        let program = self.programs.get(caller.program())?;
        let metadata = program.function_get(caller.function() as usize)?;
        if let Some(site_pc) = caller.binding_site()
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
        let mut environment = self.caller_capture_env(caller, depth)?;
        while let Some(crate::heap::EnvironmentRecord {
            parent,
            function,
            scope,
            ..
        }) = self.heap.environment(environment)
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
        owner: VarOwner,
        binding: crate::bytecode::EvalBinding,
    ) -> Result<Value, JsError> {
        match binding.location {
            crate::bytecode::EvalBindingLocation::Capture { depth, slot } => {
                self.capture(p, owner, depth, slot)
            }
            crate::bytecode::EvalBindingLocation::Local(slot) => {
                self.load_local_binding(p, owner, usize::from(slot), Some(binding.atom))
            }
        }
    }

    pub(super) fn store_name_binding(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        binding: crate::bytecode::EvalBinding,
        value: Value,
        strict: bool,
    ) -> Result<(), JsError> {
        self.load_name_binding(p, owner, binding)?;
        if !self.check_named_binding_assignment(p, binding.kind, strict)? {
            return Ok(());
        }
        match binding.location {
            crate::bytecode::EvalBindingLocation::Capture { depth, slot } => {
                self.store_capture(p, owner, depth, slot, value)
            }
            crate::bytecode::EvalBindingLocation::Local(slot) => {
                if self.store_owner_slot(owner, usize::from(slot), value) {
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
            crate::bytecode::LexicalBindingKind::Immutable
            | crate::bytecode::LexicalBindingKind::PrivateName => {
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
        self.clone_frame_environment(frame, &[slot], crate::bytecode::Resets::listed(&[]))?;
        *self
            .owner_binding_mut(p, VarOwner::Frame(frame), usize::from(slot))
            .ok_or_else(|| JsError("invalid handler binding".into()))? = value;
        Ok(())
    }

    /// The owner of the variable environment the eval whose root runs in `frame` declares `atom`
    /// in: its caller's, a frame's or, for an eval a lane activation called, that activation's
    /// record. `None` where `frame` is no eval root running in its caller's scope, or its
    /// program does not declare `atom`.
    pub(super) fn direct_eval_variable_caller(&self, owner: VarOwner, atom: Atom) -> Option<VarOwner> {
        let program_id = self.direct_eval_var_program?;
        let frame = self.owner_frame(owner)?;
        let eval = self.frames.get(frame)?;
        if eval.program != program_id || eval.function != super::ROOT_FUNCTION_ID {
            return None;
        }
        let program = self.programs.get(program_id)?;
        if !program.function(super::ROOT_FUNCTION_ID as usize).global_var_atoms().contains(&atom) {
            return None;
        }
        Some(match self.eval_record_at(frame) {
            Some(record) => VarOwner::Record(record),
            None => self.variable_environment_owner(VarOwner::Frame(frame.checked_sub(1)?)),
        })
    }

    fn direct_eval_catch_binding(
        &self,
        owner: VarOwner,
        atom: Atom,
    ) -> Option<(VarOwner, crate::bytecode::EvalBinding)> {
        let caller = self.direct_eval_variable_caller(owner, atom)?;
        let binding = self.name_binding(self.owner_view(caller), atom)?;
        (binding.declaration == crate::bytecode::EvalBindingDeclaration::CatchParameter)
            .then_some((caller, binding))
    }

    /// `value` stored to the catch parameter `binding` of the activation `owner` names, which
    /// a direct eval's `var` of its name leaves visible to ordinary references (Annex B).
    pub(super) fn store_catch_binding(
        &mut self,
        owner: VarOwner,
        binding: crate::bytecode::EvalBinding,
        value: Value,
    ) -> Result<(), JsError> {
        let invalid = || JsError("invalid catch binding".into());
        match (owner, binding.location) {
            // The binding is the caller's, whose program is not the eval's.
            (VarOwner::Frame(_), _) => {
                let program = self.programs.get(self.owner_view(owner).program()).ok_or_else(invalid)?;
                self.store_name_binding(&program, owner, binding, value, false)
            }
            (VarOwner::Record(_) | VarOwner::Window, crate::bytecode::EvalBindingLocation::Local(slot)) => {
                self.store_owner_slot(owner, usize::from(slot), value).then_some(()).ok_or_else(invalid)
            }
            (VarOwner::Record(_) | VarOwner::Window, crate::bytecode::EvalBindingLocation::Capture { depth, slot }) => {
                let environment = self.owner_capture_env(owner, depth).ok_or_else(invalid)?;
                *self.heap.environment_slot_mut(environment, usize::from(slot)).ok_or_else(invalid)? = value;
                Ok(())
            }
        }
    }

    pub(super) fn direct_eval_var_binding(&self, owner: VarOwner, slot: usize) -> Option<Value> {
        let frame = self.owner_frame(owner)?;
        let eval = self.frames.get(frame)?;
        let program = self.programs.get(eval.program)?;
        let atom = *program
            .function_get(eval.function as usize)?
            .local_atoms
            .get(slot)?;
        let caller = self.direct_eval_variable_caller(owner, atom)?;
        // Annex B leaves the catch binding visible to ordinary references,
        // while declaration instantiation creates the separate variable binding.
        if let Some((caller, binding)) = self.direct_eval_catch_binding(owner, atom) {
            let view = self.owner_view(caller);
            let (environment, slot) = match binding.location {
                crate::bytecode::EvalBindingLocation::Local(slot) => {
                    if !view.owns_environment() {
                        return self.owner_local(caller, usize::from(slot));
                    }
                    (view.environment(), slot)
                }
                crate::bytecode::EvalBindingLocation::Capture { depth, slot } => {
                    (self.owner_capture_env(caller, depth)?, slot)
                }
            };
            return self.heap.environment_slot(environment, usize::from(slot));
        }
        (!self.parameter_eval)
            .then(|| self.owner_binding_value(caller, atom))
            .flatten()
            .or_else(|| self.dynamic_binding(self.owner_view(caller), atom))
    }

    pub(super) fn check_local_assignment_initialized(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        slot: usize,
        initializing: bool,
    ) -> Result<(), JsError> {
        if initializing {
            return Ok(());
        }
        if let Some(value) = self.owner_binding(p, owner, slot)
            && value.is_deleted()
        {
            let atom = p.function(self.owner_view(owner).function() as usize)
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
        owner: VarOwner,
        slot: usize,
        source_atom: Option<Atom>,
    ) -> Result<Value, JsError> {
        let value = match self.root_body_binding(p, owner, slot)? {
            Some(value) => value,
            None => self
                .owner_binding(p, owner, slot)
                .ok_or_else(|| JsError("invalid local slot".into()))?,
        };
        if value.is_deleted() {
            let atom = source_atom
                .or_else(|| {
                    p.function(self.owner_view(owner).function() as usize)
                        .local_atoms
                        .get(slot)
                        .copied()
                })
                .ok_or_else(|| JsError("invalid local name".into()))?;
            self.checked_binding_read(p, atom, value)?;
        }
        Ok(self.mapped_argument_load(p, owner, slot, value))
    }

    /// A root body's slot that stands for a binding outside its activation: a direct eval's var
    /// redirected to its caller, a global lexical or var binding, or a module import. Every
    /// other function's slot is its own.
    fn root_body_binding(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        slot: usize,
    ) -> Result<Option<Value>, JsError> {
        if self.owner_view(owner).function() != super::ROOT_FUNCTION_ID {
            return Ok(None);
        }
        if let Some(value) = self.direct_eval_var_binding(owner, slot) {
            return Ok(Some(value));
        }
        match self.root_binding(self.owner_view(owner), slot) {
            Some(RootBinding::GlobalLexical(value) | RootBinding::Import(value)) => Ok(Some(value)),
            Some(RootBinding::GlobalVar(atom)) => self.get_property(p, self.realm.globals, atom).map(Some),
            None => Ok(None),
        }
    }

    fn is_self_binding(&self, atom: Atom) -> bool {
        self.atom_class(atom).contains(AtomClass::SELF_BINDING)
    }

    pub fn captured_lexical_this(&self, mut env: Value) -> Option<Value> {
        while let Some(crate::heap::EnvironmentRecord { parent, .. }) = self.heap.environment(env) {
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
        match self.dynamic_binding(self.frame_caller(frame), atom) {
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
    pub fn with_objects_before_binding(&self, caller: &dyn CallerView, atom: Atom) -> Vec<Value> {
        let parent = self.captured_parent_environment(caller);
        let own_dynamic = self
            .own_dynamic_bindings(caller)
            .is_some_and(|bindings| bindings.iter().any(|(candidate, _)| *candidate == atom));
        let named = self.name_binding(caller, atom);
        let own_static = self.activation_has_static_binding(caller, atom)
            && self
                .activation_binding_slot(caller, atom)
                .is_some_and(|slot| self.global_object_var_atom(caller, slot).is_none());
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
                self.caller_capture_env(caller, depth)
            }
            crate::bytecode::EvalBindingLocation::Local(_) => None,
        });
        let boundary = self
            .outer_dynamic_binding(caller, atom)
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
        while let Some(crate::heap::EnvironmentRecord { parent, scope, .. }) = self.heap.environment(environment) {
            let with_objects = scope.with_objects();
            inherited_count += with_objects.len();
            before_binding &= boundary != Some(environment);
            if before_binding && !with_objects.is_empty() {
                layers.push(with_objects);
            }
            environment = *parent;
        }
        let chain = caller.scope_chain(&self.with_stack);
        let mut objects = layers
            .into_iter()
            .rev()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        objects.extend_from_slice(&chain[inherited_count.min(chain.len())..]);
        // A lexical declaration inside an active with scope precedes that
        // object environment. Later entered with scopes still precede it.
        let declaration_depth = named.filter(|_| local || boundary == named_owner)
            .map_or(0, |binding| {
                let owner_depth = named_owner.and_then(|owner| {
                    let crate::heap::EnvironmentRecord { function, scope, .. } = self.heap.environment(owner)?;
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
        owner: VarOwner,
        atom: Atom,
    ) -> Result<Option<(Value, Value)>, JsError> {
        // Object environments exist only after some `with` statement was entered.
        if !self.with_scope_entered || self.atom_class(atom).contains(AtomClass::INTERNAL) {
            return Ok(None);
        }
        let objects = self.with_objects_before_binding(self.owner_view(owner), atom);
        self.first_with_binding(p, &objects, atom)
    }

    /// The innermost of the scope chain `objects` (outermost first) that has a binding for
    /// `atom`, with the key it was found by: the one has-binding test over a chain, which a
    /// frame's name lookups (`find_with_binding`) make.
    pub(super) fn first_with_binding(
        &mut self,
        p: &ResidualProgram,
        objects: &[Value],
        atom: Atom,
    ) -> Result<Option<(Value, Value)>, JsError> {
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

    /// `first_with_binding` where no object's test can run guest code, which a window
    /// activation's name lookup (`LaneStack::with_chain`) makes in place: the innermost of
    /// `objects` that has `atom` and its value there, or `Some(None)` where none has it. `None`
    /// before any effect, leaves a chain the shapes alone do not decide (a Proxy, an accessor,
    /// an exotic object) to `first_with_binding`.
    pub fn plain_first_with_binding(&self, objects: &[Value], atom: Atom) -> Option<Option<(Value, Value)>> {
        for object in objects.iter().rev().copied() {
            if let Some(value) = self.plain_with_binding(object, atom)? {
                return Some(Some((object, value)));
            }
        }
        Some(None)
    }

    /// `with_binding` where neither the has test nor the `@@unscopables` read can run guest
    /// code: the value `atom` has on `object`, `Some(None)` where it has no binding for it.
    /// `None` leaves the object to `with_binding`.
    fn plain_with_binding(&self, object: Value, atom: Atom) -> Option<Option<Value>> {
        let key = PropertyKey::string(atom);
        let Some(value) = self.plain_property_lookup(object, key)? else {
            return Some(None);
        };
        let Some(unscopables) = self.well_known_symbols.get(WellKnownSymbol::Unscopables) else {
            return Some(Some(value));
        };
        let Some(exclusions) = self.plain_property_lookup(object, PropertyKey::symbol(unscopables))? else {
            return Some(Some(value));
        };
        if !self.is_object_like(exclusions) {
            return Some(Some(value));
        }
        let excluded = self.plain_property_lookup(exclusions, key)?;
        Some(excluded.is_none_or(|excluded| !self.truthy(excluded)).then_some(value))
    }

    pub(super) fn store_with_binding(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        atom: Atom,
        value: Value,
        strict: bool,
    ) -> Result<bool, JsError> {
        let Some((object, _)) = self.find_with_binding(p, owner, atom)? else {
            return Ok(false);
        };
        self.set_property_with_program_mode(p, object, atom, value, strict)?;
        Ok(true)
    }

    fn get_with_binding_value(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        object: Value,
        key: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if !self.has_property(p, object, key)? {
            let strict = p.function(self.owner_view(owner).function() as usize).strict;
            if strict {
                return Err(self.name_not_defined(p, atom));
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
        while let Some(crate::heap::EnvironmentRecord {
            parent,
            function,
            slots,
            scope,
        }) = self.heap.environment(env)
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

    pub(super) fn store_resolved_name(&mut self, p: &ResidualProgram, object: Value, atom: Atom, value: Value, strict: bool) -> Result<(), JsError> {
        if object == self.realm.globals && self.names_resolve_to_global_object(p, VarOwner::Frame(self.frames.len().saturating_sub(1)), atom) { return self.store_resolved_global(p, atom, value, strict); }
        if object == UNRESOLVABLE_NAME_REFERENCE {
            return if strict {
                Err(self.name_not_defined(p, atom))
            } else {
                self.set_property_with_program_mode(p, self.realm.globals, atom, value, false)
            };
        }
        if object == GLOBAL_LEXICAL_NAME_REFERENCE {
            return self.store_global_lexical_assignment(p, atom, value);
        }
        if let Some((environment, slot, kind)) = self.binding_reference(object) {
            self.load_environment_binding(p, environment, slot)?;
            if !self.check_named_binding_assignment(p, kind, strict)? {
                return Ok(());
            }
            return self.store_environment_binding(
                p,
                VarOwner::Frame(self.frames.len() - 1),
                environment,
                slot,
                value,
            );
        }
        if let Some(dynamic_bindings) = self.heap.environment_bindings_mut(object) {
            if let Some((_, binding)) = dynamic_bindings.iter_mut().rev().find(|(candidate, _)| *candidate == atom) {
                *binding = value;
                return Ok(());
            }
            if !strict {
                dynamic_bindings.push((atom, value));
                self.note_dynamic_scope_name(atom);
                return Ok(());
            }
            return Err(self.name_not_defined(p, atom));
        }
        let key = self.heap.alloc_string(self.atom_name(atom).into());
        // ResolveName has already selected a non-global object environment.
        // Preserve that reference even if evaluating the RHS removed the
        // binding before PutValue (the specification's captured Reference).
        if object != self.realm.globals {
            let still_exists = self.has_property(p, object, key)?;
            if strict && !still_exists {
                return Err(self.name_not_defined(p, atom));
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

        // A global environment reference consults its declarative record at
        // PutValue, including declarations installed while evaluating the RHS.
        if self.realm.global_lexical_declarations.contains(&atom) {
            return self.store_global_lexical_assignment(p, atom, value);
        }

        if let Some(frame_index) = self.frames.len().checked_sub(1)
            && (self.frames[frame_index].function != 0
                || p.function(self.frames[frame_index].function as usize)
                    .declares_global_lexical(atom)
                || p.function(self.frames[frame_index].function as usize)
                    .declares_global_var(atom)
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
                .declares_global_immutable(atom)
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
            self.mapped_argument_store(p, VarOwner::Frame(frame_index), slot, value);
            return Ok(());
        }
        if let Some(frame_index) = self.frames.len().checked_sub(1)
            && let Some((env, slot)) =
                self.outer_environment_binding(self.captured_parent_environment(self.frame_caller(frame_index)), atom)
        {
            return self.store_environment_binding(p, VarOwner::Frame(frame_index), env, slot as u16, value);
        }
        self.store_resolved_global(p, atom, value, strict)
    }

    /// A store to the global object past every scope that may bind `atom`: a global `var`, else the property (required if strict).
    fn store_resolved_global(&mut self, p: &ResidualProgram, atom: Atom, value: Value, strict: bool) -> Result<(), JsError> {
        if self.store_global_var_binding(p, atom, value, self.owner_is_strict(p, VarOwner::Frame(self.frames.len() - 1)))? {
            return Ok(());
        }
        if strict { self.require_global_assignment_binding(p, atom)?; }
        self.set_property_with_program_mode(p, self.realm.globals, atom, value, strict)
    }

    /// Whether the function the activation `owner` runs is strict.
    fn owner_is_strict(&self, p: &ResidualProgram, owner: VarOwner) -> bool {
        p.function_get(self.owner_view(owner).function() as usize).is_some_and(|function| function.strict)
    }

    fn require_global_assignment_binding(&mut self, p: &ResidualProgram, atom: Atom) -> Result<(), JsError> {
        if self.has_global_object_binding(p, atom)? { Ok(()) }
        else { Err(self.name_not_defined(p, atom)) }
    }

    fn outer_dynamic_binding(&self, caller: &dyn CallerView, atom: Atom) -> Option<(Value, usize)> {
        self.outer_dynamic_binding_in(&NameLookup::new(caller, atom))
    }

    fn outer_dynamic_binding_in(&self, lookup: &NameLookup) -> Option<(Value, usize)> {
        let (caller, atom) = (lookup.caller, lookup.atom);
        let mut env = self.captured_parent_environment(caller);
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
                self.caller_capture_env(caller, depth)
            }
            _ => None,
        });
        let lexical_capture = named.is_some_and(|binding| {
            binding.declaration != crate::bytecode::EvalBindingDeclaration::Variable
                && binding.kind != crate::bytecode::LexicalBindingKind::FunctionName
        });
        let static_owner = lookup.static_owner(self).map(|(env, _)| env);
        while let Some(crate::heap::EnvironmentRecord { parent, .. }) = self.heap.environment(env) {
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
        owner: VarOwner,
        atom: Atom,
        value: Value,
    ) -> bool {
        let Some((env, index)) = self.outer_dynamic_binding(self.owner_view(owner), atom) else {
            return false;
        };
        let Some(dynamic_bindings) = self.heap.environment_bindings_mut(env) else {
            return false;
        };
        dynamic_bindings[index].1 = value;
        true
    }

    pub fn dynamic_binding(&self, caller: &dyn CallerView, atom: Atom) -> Option<Value> {
        self.dynamic_binding_in(&NameLookup::new(caller, atom))
    }

    pub(super) fn dynamic_binding_in(&self, lookup: &NameLookup) -> Option<Value> {
        self.dynamic_slot_value(lookup.caller, self.dynamic_binding_slot_in(lookup)?)
    }

    /// The dynamic binding `lookup`'s name has: the activation's own (last declared), else the
    /// outer one, unless the activation declares the name statically.
    pub(super) fn dynamic_binding_slot_in(&self, lookup: &NameLookup) -> Option<DynamicSlot> {
        let (caller, atom) = (lookup.caller, lookup.atom);
        if let Some(index) = self
            .own_dynamic_bindings(caller)
            .and_then(|bindings| bindings.iter().rposition(|(candidate, _)| *candidate == atom))
        {
            return Some(DynamicSlot::Own(index));
        }
        if self.activation_has_static_binding(caller, atom) {
            return None;
        }
        let (env, index) = self.outer_dynamic_binding_in(lookup)?;
        Some(DynamicSlot::Outer(env, index))
    }

    /// The value at `slot` of the dynamic bindings `caller` reads it from (`dynamic_binding_slot_in`).
    pub(super) fn dynamic_slot_value(&self, caller: &dyn CallerView, slot: DynamicSlot) -> Option<Value> {
        match slot {
            DynamicSlot::Own(index) => self.own_dynamic_bindings(caller)?.get(index).map(|(_, value)| *value),
            DynamicSlot::Outer(env, index) => self.heap.environment_bindings(env)?.get(index).map(|(_, value)| *value),
        }
    }

    fn activation_has_static_binding(&self, caller: &dyn CallerView, atom: Atom) -> bool {
        let Some(program) = self.programs.get(caller.program()) else {
            return false;
        };
        let Some(metadata) = program.function_get(caller.function() as usize) else {
            return false;
        };
        metadata.declares_environment_atom(atom)
            && (!self.parameter_eval
                || caller.function() == super::ROOT_FUNCTION_ID
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
            && self.local_binding_slot(p, activation.function, atom).is_some()
        {
            return Some(false);
        }
        let caller = self.frame_caller(frame);
        if let Some(index) = self
            .own_dynamic_bindings(caller)?
            .iter()
            .rposition(|(candidate, _)| *candidate == atom)
        {
            self.owner_bindings_mut(VarOwner::Frame(frame))?.remove(index);
            return Some(true);
        }
        if self.name_binding(caller, atom).is_some() && self.dynamic_binding(caller, atom).is_none() {
            return Some(false);
        }
        let mut env = self.frames[frame].env;
        let static_owner = self.outer_environment_binding(env, atom).map(|(env, _)| env);
        while let Some(crate::heap::EnvironmentRecord { parent, .. }) = self.heap.environment(env) {
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

    fn name_not_defined(&mut self, p: &ResidualProgram, atom: Atom) -> JsError {
        self.reference_error(p, format!("{} is not defined", self.atom_name(atom)))
    }

    pub(super) fn resolve_name(&mut self, p: &ResidualProgram, atom: Atom, strict: bool) -> Result<Value, JsError> {
        // Only the global object can bind the name, and owns it: no scope to walk (`names_resolve_to_global_object`).
        let global = strict && self.names_resolve_to_global_object(p, VarOwner::Frame(self.frames.len().saturating_sub(1)), atom);
        if global && self.own_property(self.realm.globals, atom).is_some() { return Ok(self.realm.globals); }
        if strict && !self.has_name_binding(p, atom)? { return Ok(UNRESOLVABLE_NAME_REFERENCE); }
        Ok(self.resolve_name_reference(p, atom)?.0)
    }

    pub(super) fn load_resolved_name(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        strict: bool,
    ) -> Result<Value, JsError> {
        if object == UNRESOLVABLE_NAME_REFERENCE {
            return Err(self.name_not_defined(p, atom));
        }
        if object == GLOBAL_LEXICAL_NAME_REFERENCE {
            return self.load_global_lexical_binding(p, atom);
        }
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
                None => Err(self.name_not_defined(p, atom)),
            };
        }
        if object == self.realm.globals {
            let owner = self.running_owner();
            let location = self.locate_name_without_with(p, owner, atom);
            return self.read_name_location(p, owner, atom, location, None, false);
        }
        let key = self.heap.alloc_string(self.atom_name(atom).into());
        if !self.has_property(p, object, key)? {
            if strict {
                return Err(self.name_not_defined(p, atom));
            }
            return Ok(Value::UNDEFINED);
        }
        self.get_property(p, object, atom)
    }

    fn has_name_binding(&mut self, p: &ResidualProgram, atom: Atom) -> Result<bool, JsError> {
        if self.atom_name(atom).starts_with('\0') {
            return Ok(true);
        }
        let frame = self.frames.len().checked_sub(1);
        if let Some(frame) = frame
            && self.find_with_binding(p, VarOwner::Frame(frame), atom)?.is_some()
        {
            return Ok(true);
        }
        let caller = frame.map(|frame| self.frame_caller(frame));
        Ok(caller.and_then(|caller| self.dynamic_binding(caller, atom)).is_some()
            || self.realm.global_lexical_declarations.contains(&atom)
            || self.frames.last().is_some_and(|frame| self.function_environment_binding(p, frame.function, atom))
            || caller.is_some_and(|caller| self.name_binding(caller, atom).is_some())
            || self.outer_environment_binding(self.captured_parent_environment(self.frame_caller(self.frames.len().saturating_sub(1))), atom).is_some()
            || self.has_global_object_binding(p, atom)?)
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

    /// The reference object to the eval-declared `binding` that frame `frame` names.
    fn eval_binding_reference(&mut self, frame: usize, binding: crate::bytecode::EvalBinding) -> Result<Value, JsError> {
        let (environment, slot) = match binding.location {
            crate::bytecode::EvalBindingLocation::Local(slot) => (self.promote_frame_environment(frame), slot),
            crate::bytecode::EvalBindingLocation::Capture { depth, slot } => (
                self.owner_capture_env(VarOwner::Frame(frame), depth)
                    .ok_or_else(|| JsError("invalid named reference".into()))?,
                slot,
            ),
        };
        let environment = self
            .heap
            .environment_slot_owner(environment, usize::from(slot))
            .ok_or_else(|| JsError("invalid named reference slot".into()))?;
        Ok(self.heap.alloc(Cell::BindingReference { environment, slot, kind: binding.kind }))
    }

    fn resolve_name_reference(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
    ) -> Result<(Value, bool), JsError> {
        if let Some(frame) = self.frames.len().checked_sub(1)
            && let Some((object, _)) = self.find_with_binding(p, VarOwner::Frame(frame), atom)?
        {
            return Ok((object, true));
        }
        if let Some(frame) = self.frames.len().checked_sub(1) {
            let caller = self.frame_caller(frame);
            if self
                .own_dynamic_bindings(caller)
                .is_some_and(|bindings| bindings.iter().any(|(candidate, _)| *candidate == atom))
            {
                return Ok((self.promote_frame_environment(frame), true));
            }
            if !self.activation_has_static_binding(caller, atom)
                && let Some((env, _)) = self.outer_dynamic_binding(caller, atom)
            {
                return Ok((env, true));
            }
        }
        if let Some(frame) = self.frames.len().checked_sub(1)
            && let Some(binding) = self.name_binding(self.frame_caller(frame), atom)
        {
            return Ok((self.eval_binding_reference(frame, binding)?, true));
        }
        if self.realm.global_lexical_declarations.contains(&atom) {
            return Ok((GLOBAL_LEXICAL_NAME_REFERENCE, false));
        }
        Ok((self.realm.globals, false))
    }

    fn load_global_lexical_binding(&mut self, p: &ResidualProgram, atom: Atom) -> Result<Value, JsError> {
        let Some(value) = self.realm.global_lexical_bindings.get(&atom).copied() else {
            return Err(self.uninitialized_binding_error(p, atom));
        };
        self.checked_binding_read(p, atom, value)
    }

    fn store_global_lexical_assignment(&mut self, p: &ResidualProgram, atom: Atom, value: Value) -> Result<(), JsError> {
        self.load_global_lexical_binding(p, atom)?;
        if self.realm.immutable_global_lexical_bindings.contains(&atom) {
            return Err(self.type_error(p, "assignment to constant binding".into()));
        }
        self.bind_global_lexical(atom, value);
        Ok(())
    }

    /// The environment `depth` function layers out from an activation's captures
    /// (`owner_capture_env`), from its environment `env`, which is its own when `captured`.
    pub fn environment_capture_env(
        &self,
        env: Value,
        captured: bool,
        depth: u16,
    ) -> Option<Value> {
        let mut env = if captured {
            match self.heap.reachable_environment(env)? {
                crate::heap::EnvironmentRecord { parent, .. } => *parent,
            }
        } else {
            env
        };
        env = self.skip_with_environment_layers(env)?;
        for _ in 0..depth {
            env = match self.heap.reachable_environment(env)? {
                crate::heap::EnvironmentRecord { parent, .. } => *parent,
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
        match self.heap.reachable_environment(env) {
            Some(crate::heap::EnvironmentRecord { function, .. }) if *function == u32::MAX => {
                self.skip_function_less_layers(env)
            }
            _ => Some(env),
        }
    }

    #[inline(never)]
    fn skip_function_less_layers(&self, mut env: Value) -> Option<Value> {
        while let Some(crate::heap::EnvironmentRecord {
            parent,
            function,
            slots,
            scope,
        }) = self.heap.environment(env)
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
        owner: VarOwner,
        depth: u16,
        slot: u16,
    ) -> Result<Value, JsError> {
        match self.direct_capture(owner, depth, slot) {
            Some(value) => Ok(value),
            None => self.walked_capture(p, owner, depth, slot),
        }
    }

    /// The capture read when every layer on the walk belongs to a function, so no with-object
    /// or lexical-this wrapper is skipped, and the binding is an initialized slot its
    /// environment owns: a non-root slot, or a root slot no projection reads through. One cell
    /// read per layer; anything else is `None`.
    #[inline(always)]
    pub(super) fn direct_capture(&self, owner: VarOwner, depth: u16, slot: u16) -> Option<Value> {
        let view = self.owner_view(owner);
        let (slots, root) = self.environment_capture_slots(view.environment(), view.owns_environment(), depth)?;
        self.owned_capture(slots, root, slot)
    }

    /// `slot` of a capture walk's environment when the environment owns the binding: any slot
    /// of a non-root function's environment; of a root environment (`root` names its program),
    /// only a slot no projection reads through.
    #[inline(always)]
    pub fn owned_capture(
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
        self.slot_binding(slots, slot)
    }

    /// The initialized binding `slot` of `slots` holds, itself or through its owner.
    #[inline(always)]
    fn slot_binding(&self, slots: &crate::heap::EnvironmentSlots, slot: u16) -> Option<Value> {
        let value = match slots.state(usize::from(slot))? {
            crate::heap::EnvironmentSlotState::Owned(value) => value,
            // A shared slot names its owner, which holds the slot itself.
            crate::heap::EnvironmentSlotState::Shared(owner) => {
                self.heap.environment_slot(owner, usize::from(slot))?
            }
        };
        (!value.is_deleted()).then_some(value)
    }

    /// `owned_capture`, also answering a root slot's projection where reading it runs no guest
    /// code and throws nothing (`plain_projected_root_binding`): one decision per read, from
    /// the slot's rule (`ProgramStore::root_slot`); with the global var whose global object
    /// property answered it.
    #[inline(always)]
    pub fn plain_capture(
        &self,
        slots: &crate::heap::EnvironmentSlots,
        root: Option<Option<u32>>,
        slot: u16,
    ) -> Option<Value> {
        use crate::bytecode::RootSlotRule;
        let Some(program) = root else {
            return self.slot_binding(slots, slot);
        };
        let owner = program.map_or(self.active_program, super::ProgramId::from_raw);
        match self.programs.root_slot(owner, slot) {
            RootSlotRule::Binding => self.slot_binding(slots, slot),
            RootSlotRule::GlobalLexical(atom) => match self.realm.global_lexical_bindings.get(&atom) {
                Some(value) => (!value.is_deleted()).then_some(*value),
                None => self.slot_binding(slots, slot),
            },
            RootSlotRule::GlobalVar(atom) if self.direct_eval_var_program != Some(owner) => {
                self.plain_named_get(self.realm.globals, atom)
            }
            RootSlotRule::GlobalVar(_) | RootSlotRule::Import => None,
        }
    }

    /// The slots a direct capture at `depth` reads, and a root environment's program (`owned_capture`):
    /// the walk from an activation's environment `env` (past its own once `captured`), function layers only.
    pub fn environment_capture_slots(
        &self,
        env: Value,
        captured: bool,
        depth: u16,
    ) -> Option<(&crate::heap::EnvironmentSlots, Option<Option<u32>>)> {
        let mut cell = self.heap.environment(env)?;
        if captured {
            let crate::heap::EnvironmentRecord { parent, .. } = cell;
            cell = self.heap.environment(*parent)?;
        }
        let mut remaining = depth;
        loop {
            let crate::heap::EnvironmentRecord {
                parent,
                function,
                slots,
                scope,
                ..
            } = cell;
            if *function == u32::MAX {
                return None;
            }
            if remaining == 0 {
                // A root whose every slot is the binding (an import-free module's, a strict
                // eval's) is read as any function environment: its dense slots, no projection.
                let root = (*function == super::ROOT_FUNCTION_ID)
                    .then(|| scope.program())
                    .filter(|program| {
                        !self.programs.root_slots_are_bindings(program.map_or(self.active_program, super::ProgramId::from_raw))
                    });
                return Some((slots, root));
            }
            remaining -= 1;
            cell = self.heap.environment(*parent)?;
        }
    }

    /// Links each function environment's dense storage on the walk from `env` to `depth` to
    /// its parent's (`EnvironmentSlots::link`), for an activation's later outer captures
    /// (`Reads::of_captures`): only where both are function environments other than a
    /// root, whose dense slots a capture walk reads as they are.
    #[inline(never)]
    pub fn link_capture_chain(&self, env: Value, depth: u16) {
        let epoch = self.heap.collection_epoch();
        let function_storage = |cell: Option<&crate::heap::EnvironmentRecord>| match cell {
            Some(crate::heap::EnvironmentRecord { parent, function, slots, .. })
                if *function != u32::MAX && *function != super::ROOT_FUNCTION_ID =>
            {
                slots.dense_storage().map(|storage| (*parent, storage))
            }
            _ => None,
        };
        let Some((mut parent, mut storage)) = function_storage(self.heap.environment(env)) else {
            return;
        };
        for _ in 0..depth {
            let Some((next_parent, parent_storage)) = function_storage(self.heap.environment(parent)) else {
                return;
            };
            // SAFETY: both are dense storages of live heap environments in this collection.
            unsafe { crate::heap::EnvironmentSlots::link(storage, parent_storage, epoch) };
            (parent, storage) = (next_parent, parent_storage);
        }
    }

    /// `slot` at the end of a capture walk `walked_capture_slots` remembers, when that end is
    /// a function environment holding the slot densely; `None` leaves the read to the walk.
    #[inline(always)]
    pub fn remembered_capture(
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
            || walk.cache.epoch != self.cache_epochs.gc
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
    pub fn walked_capture_slots(
        &mut self,
        env: Value,
        captured: bool,
        depth: u16,
    ) -> Option<(*const crate::heap::EnvironmentSlots, Option<Option<u32>>)> {
        let epoch = self.cache_epochs.gc;
        let index = capture_walk_index(env, depth)?;
        let walk = &self.capture_walks[index];
        if walk.depth == depth
            && walk.cache.env == env
            && walk.cache.captured == captured
            && walk.cache.epoch == epoch
            && !walk.cache.slots.is_null()
        {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::capture(super::lane_exits::CaptureEvent::WalkHit);
            return Some((walk.cache.slots, walk.cache.root));
        }
        #[cfg(feature = "profile-lane-exits")]
        {
            super::lane_exits::capture(super::lane_exits::CaptureEvent::WalkMiss);
            super::lane_exits::record_detail(format!(
                "capture miss empty={} same_epoch={} same_env={} same_depth={} same_captured={} requested_depth={depth}",
                walk.cache.slots.is_null(), walk.cache.epoch == epoch,
                walk.cache.env == env, walk.depth == depth, walk.cache.captured == captured,
            ));
        }
        let (slots, root) = self.environment_capture_slots(env, captured, depth)?;
        #[cfg(feature = "profile-lane-exits")]
        eprintln!("capture-resolved {:p} {epoch} {} {depth} {captured}", self.capture_walks.as_ptr(), env.to_bits());
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
    pub fn current_capture_slots(
        &self,
        frame: usize,
    ) -> Option<(*const crate::heap::EnvironmentSlots, Option<Option<u32>>)> {
        let frame = &self.frames[frame];
        let cache = &frame.capture_cache;
        (cache.env == frame.env
            && cache.captured == frame.captured
            && cache.epoch == self.cache_epochs.gc
            && !cache.slots.is_null())
        .then_some((cache.slots, cache.root))
    }

    /// `direct_capture` through the frame's cached innermost slots. The cache names the
    /// environment, whether it is the frame's own, and the collection it was taken in: while
    /// all three are unchanged the walk ends at the same live cell, whose slot storage never
    /// moves.
    #[inline(always)]
    pub fn cached_direct_capture(
        &mut self,
        frame: usize,
        depth: u16,
        slot: u16,
    ) -> Option<Value> {
        self.cached_capture(frame, depth, slot, Self::owned_capture).flatten()
    }

    /// `cached_direct_capture` that also answers a root projection's plain read
    /// (`plain_capture`), for the interpreter's reads, which take the value however held.
    #[inline(always)]
    pub fn cached_plain_capture(&mut self, frame: usize, depth: u16, slot: u16) -> Option<Value> {
        self.cached_capture(frame, depth, slot, Self::plain_capture).flatten()
    }

    #[inline(always)]
    fn cached_capture<R>(
        &mut self,
        frame: usize,
        depth: u16,
        slot: u16,
        read: fn(&Self, &crate::heap::EnvironmentSlots, Option<Option<u32>>, u16) -> R,
    ) -> Option<R> {
        if depth != 0 {
            let (env, captured) = (self.frames[frame].env, self.frames[frame].captured);
            let (slots, root) = self.walked_capture_slots(env, captured, depth)?;
            // SAFETY: as `walked_capture_slots`: the walk ends at a live cell this collection.
            return Some(read(self, unsafe { &*slots }, root, slot));
        }
        let (slots, root) = if let Some(current) = self.current_capture_slots(frame) {
            current
        } else {
            let epoch = self.cache_epochs.gc;
            let (env, captured) = (self.frames[frame].env, self.frames[frame].captured);
            let (slots, root) = self.environment_capture_slots(env, captured, 0)?;
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
        Some(read(self, unsafe { &*slots }, root, slot))
    }

    #[cold]
    #[inline(never)]
    fn walked_capture(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        depth: u16,
        slot: u16,
    ) -> Result<Value, JsError> {
        let env = self
            .owner_capture_env(owner, depth)
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
        if let Some(crate::heap::EnvironmentRecord {
            function, scope, ..
        }) = self.heap.environment(env)
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
        let owner = program.map_or(self.active_program, super::ProgramId::from_raw);
        match self.programs.root_slot(owner, slot) {
            crate::bytecode::RootSlotRule::Binding => true,
            // A live script owns its slots until its bindings are retained in the
            // realm. Captures must then read the realm record, even in later scripts.
            crate::bytecode::RootSlotRule::GlobalLexical(atom) => {
                !self.realm.global_lexical_bindings.contains_key(&atom)
            }
            crate::bytecode::RootSlotRule::GlobalVar(_) | crate::bytecode::RootSlotRule::Import => false,
        }
    }

    /// `load_projected_environment_binding` where reading runs no guest code and throws
    /// nothing: a global var held as a data property of the global object or its prototypes
    /// (`Vm::plain_named_get`), or an initialized retained global
    /// lexical binding (`root_global_lexical_value`). `None` for any other projection or
    /// read.
    pub fn plain_projected_root_binding(&self, env: Value, slot: u16) -> Option<Value> {
        let crate::heap::EnvironmentRecord { function, scope, .. } = self.heap.environment(env)?;
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
        let crate::heap::EnvironmentRecord {
            function, scope, ..
        } = self
            .heap
            .environment(env)
            .ok_or_else(|| JsError("invalid capture".into()))?;
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
        if let Some(atom) = lexical_atom
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
        owner: VarOwner,
        depth: u16,
        slot: u16,
        value: Value,
    ) -> Result<(), JsError> {
        let env = self
            .owner_capture_env(owner, depth)
            .ok_or_else(|| JsError("invalid capture environment".into()))?;
        if self.direct_store_environment(p, self.owner_view(owner).program(), env, slot, value) {
            return Ok(());
        }
        self.store_environment_binding(p, owner, env, slot, value)
    }

    /// The capture write when the binding is an initialized slot of a nested function's own
    /// environment in the executing program, and neither that function's self binding nor a
    /// parameter aliased by a mapped `arguments` object: then the slot write is the whole
    /// store. Root bindings, with-object layers and everything else return false for
    /// `store_environment_binding`.
    #[inline(always)]
    pub fn direct_store_environment(
        &mut self,
        p: &ResidualProgram,
        program: super::ProgramId,
        env: Value,
        slot: u16,
        value: Value,
    ) -> bool {
        let Some(crate::heap::EnvironmentRecord {
            function, scope, ..
        }) = self.heap.environment(env)
        else {
            return false;
        };
        debug_assert_eq!(program, self.active_program);
        if !environment_admits_direct_store(p, program, *function, scope, slot)
            && !root_binding_store(&self.programs, program, *function, scope, slot)
        {
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
    pub fn direct_store_own_environment(
        &mut self,
        p: &ResidualProgram,
        program: super::ProgramId,
        env: Value,
        slot: u16,
        value: Value,
    ) -> bool {
        let Some(crate::heap::EnvironmentRecord {
            function,
            scope,
            slots,
            ..
        }) = self.heap.environment_mut(env)
        else {
            return false;
        };
        if !environment_admits_direct_store(p, program, *function, scope, slot)
            && !root_binding_store(&self.programs, program, *function, scope, slot)
        {
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
    pub fn admits_every_direct_store(
        &self,
        p: &ResidualProgram,
        program: super::ProgramId,
        env: Value,
    ) -> bool {
        let Some(crate::heap::EnvironmentRecord {
            function, scope, ..
        }) = self.heap.environment(env)
        else {
            return false;
        };
        direct_store_owner(p, program, *function, scope).is_some_and(owner_admits_every_direct_store)
    }

    fn store_environment_binding(
        &mut self,
        p: &ResidualProgram,
        activation: VarOwner,
        env: Value,
        slot: u16,
        value: Value,
    ) -> Result<(), JsError> {
        let (function, program, deleted) = match self.heap.environment(env) {
            Some(crate::heap::EnvironmentRecord {
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
        let running = self.owner_view(activation);
        let (running_program, strict) = (running.program(), p.function(running.function() as usize).strict);
        let owner_id = program
            .map(super::ProgramId::from_raw)
            .unwrap_or(running_program);
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
            if strict {
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
            if !self.store_global_var_binding(p, atom, value, strict)? {
                self.set_property_with_program_mode(
                    p,
                    self.realm.globals,
                    atom,
                    value,
                    strict,
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
        if function == 0
            && let Some(atom) = atom
            && !owner.function(0).declares_global_lexical(atom)
            && !self.root_local_var_binding(owner, function, atom)
        {
            self.set_property_with_program_mode(p, self.realm.globals, atom, value, strict)?;
        }
        Ok(())
    }

    pub(super) fn load_name(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        atom: Atom,
        cache: Option<u16>,
    ) -> Result<Value, JsError> {
        self.load_name_call(p, owner, atom, cache, false)
            .map(|(value, _)| value)
    }

    /// The value an intrinsic name (`bytecode::intrinsic_name_native`, or the RegExp
    /// intrinsic) stands for, which no binding can shadow.
    pub fn intrinsic_name_value(&mut self, atom: Atom) -> Option<Value> {
        let name = self.atom_name(atom);
        if name == crate::bytecode::INTRINSIC_REGEXP_BINDING {
            return Some(self.regexp_intrinsic_constructor());
        }
        let native = crate::bytecode::intrinsic_name_native(name)?;
        Some(self.native_value(native))
    }

    /// Where a by-name read of `atom` in `owner` lands, with the `with` objects searched first.
    /// This is the one lookup the by-name loads share (`load_name_call`).
    pub(super) fn locate_name(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        atom: Atom,
    ) -> Result<NameLocation, JsError> {
        if let Some((object, key)) = self.find_with_binding(p, owner, atom)? {
            return Ok(NameLocation::WithObject(object, key));
        }
        Ok(self.locate_name_without_with(p, owner, atom))
    }

    /// `locate_name` without the object environments: an internal name's intrinsic, else the
    /// static and dynamic layers (`resolve_name_source`).
    pub(super) fn locate_name_without_with(&mut self, p: &ResidualProgram, owner: VarOwner, atom: Atom) -> NameLocation {
        if self.atom_class(atom).contains(AtomClass::INTERNAL)
            && let Some(value) = self.intrinsic_name_value(atom)
        {
            return NameLocation::Intrinsic(value);
        }
        self.resolve_name_source(p, owner, atom)
    }

    /// The value a located by-name read of `atom` yields. `cache` and `allow_unresolvable` apply
    /// to the global object only (`load_global_object_name`).
    fn read_name_location(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        atom: Atom,
        location: NameLocation,
        cache: Option<u16>,
        allow_unresolvable: bool,
    ) -> Result<Value, JsError> {
        let value = match location {
            NameLocation::WithObject(object, key) => {
                return self.get_with_binding_value(p, owner, object, key, atom);
            }
            NameLocation::Intrinsic(value) => return Ok(value),
            NameLocation::Named(binding) => return self.load_name_binding(p, owner, binding),
            NameLocation::Global => {
                return self.load_global_object_name(p, atom, cache, allow_unresolvable);
            }
            NameLocation::FrameLocal(slot) => self.owner_locals(owner)[slot],
            NameLocation::EnvironmentSlot(env, slot) => {
                self.heap.environment_slot(env, slot).unwrap_or(Value::UNDEFINED)
            }
            NameLocation::Dynamic(slot) => self
                .dynamic_slot_value(self.owner_view(owner), slot)
                .ok_or_else(|| JsError("invalid dynamic binding".into()))?,
            NameLocation::GlobalLexical(name) => *self
                .realm
                .global_lexical_bindings
                .get(&name)
                .ok_or_else(|| JsError("invalid global lexical binding".into()))?,
        };
        self.checked_binding_read(p, atom, value)
    }

    /// Whether `atom` loaded by name in the executing activation can only denote a global
    /// object property: its function is nested and, with every enclosing function, free of
    /// dynamic scope access, so the compiler resolved every lexical binding statically; the
    /// code runs in the global environment, as script code or a global-scope eval; and the
    /// realm declares no global lexical binding of that name, which every started script
    /// records before it runs. None of the dynamic layers `locate_name_without_with` searches
    /// can then hold the name.
    #[inline(always)]
    pub(super) fn names_resolve_to_global_object(&self, p: &ResidualProgram, owner: VarOwner, atom: Atom) -> bool {
        !self.realm.global_lexical_declarations.contains(&atom)
            && self.global_scope_names_resolve_statically(p, owner)
    }

    /// Whether the executing activation's names, with every enclosing function's, the compiler
    /// resolved statically where only global bindings lie beyond them
    /// (`ProgramStore::names_are_global`): a by-name load there can only denote a
    /// global binding.
    #[inline(always)]
    pub(super) fn global_scope_names_resolve_statically(&self, p: &ResidualProgram, owner: VarOwner) -> bool {
        let view = self.owner_view(owner);
        self.programs.names_are_global(view.program())
            && p.function_get(view.function() as usize)
                .is_some_and(|function| function.names_resolve_statically(p))
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
                self.cached_field_hit(self.realm.globals, cache)
        {
            return Ok(value);
        }
        if !self.has_global_object_binding(p, atom)? {
            if allow_unresolvable {
                return Ok(Value::UNDEFINED);
            }
            return Err(self.name_not_defined(p, atom));
        }
        match cache {
            Some(cache) => self.get_field_cached(p, self.realm.globals, atom, cache),
            None => self.get_property(p, self.realm.globals, atom),
        }
    }

    pub(super) fn load_name_call(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        atom: Atom,
        cache: Option<u16>,
        allow_unresolvable: bool,
    ) -> Result<(Value, Value), JsError> {
        if self.scope_names.probe.is_some() {
            self.settle_name_probe(p, atom);
        }
        let location = self.locate_name(p, owner, atom)?;
        let this = match location {
            NameLocation::WithObject(object, _) => object,
            _ => Value::UNDEFINED,
        };
        let value = self.read_name_location(p, owner, atom, location, cache, allow_unresolvable)?;
        Ok((value, this))
    }

    /// `load_name_call` where an unresolvable global reads as `undefined` (`typeof name`). It
    /// shares `load_name_call`'s single locate-then-read, so nothing is looked up twice.
    pub(super) fn load_name_typeof(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        atom: Atom,
        cache: u16,
    ) -> Result<Value, JsError> {
        self.load_name_call(p, owner, atom, Some(cache), true)
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
        owner: VarOwner,
        atom: Atom,
        value: Value,
        cache: u16,
        initializing: bool,
    ) -> Result<(), JsError> {
        let name = self.atom_name(atom).to_owned();
        if !name.starts_with('\0') {
            let function = self.owner_view(owner).function();
            if self.store_with_binding(p, owner, atom, value, p.function(function as usize).strict)? {
                return Ok(());
            }
            if let Some((caller, binding)) = self.direct_eval_catch_binding(owner, atom) {
                return self.store_catch_binding(caller, binding, value);
            }
            if self.store_direct_eval_var_binding(owner, atom, value) {
                return Ok(());
            }
            if self.store_owner_binding(owner, atom, value) {
                return Ok(());
            }
            if (function != 0
                || p.function(function as usize)
                    .global_lexical_atoms()
                    .contains(&atom)
                || self.root_local_var_binding(p, function, atom))
                && let Some(slot) = self.local_binding_slot(p, function, atom)
            {
                if p.function(function as usize)
                    .global_immutable_atoms()
                    .contains(&atom)
                    && !initializing
                {
                    return Err(self.type_error(p, "assignment to constant binding".into()));
                }
                let binding = p.function(function as usize).local_atoms[slot];
                if self.is_self_binding(binding) {
                    if p.function(function as usize).strict {
                        return Err(
                            self.type_error(p, "assignment to function name binding".into())
                        );
                    }
                    return Ok(());
                }
                if let Some(binding) = self.owner_binding_mut(p, owner, slot) {
                    *binding = value;
                }
                self.mapped_argument_store(p, owner, slot, value);
                return Ok(());
            }
            if self.store_outer_dynamic_binding(owner, atom, value) {
                return Ok(());
            }
            if let Some(binding) = self.name_binding(self.owner_view(owner), atom) {
                let strict = p.function(function as usize).strict;
                return self.store_name_binding(p, owner, binding, value, strict);
            }
            if self.realm.global_lexical_declarations.contains(&atom) {
                return self.store_global_lexical_assignment(p, atom, value);
            }
            if let Some((env, slot)) = self
                .outer_environment_binding(self.captured_parent_environment(self.owner_view(owner)), atom)
            {
                return self.store_environment_binding(p, owner, env, slot as u16, value);
            }
        }
        if self.store_global_var_binding(p, atom, value, self.owner_is_strict(p, owner))? {
            return Ok(());
        }
        let root_declared = self.owner_view(owner).function() == super::ROOT_FUNCTION_ID
            && self.root_declares_binding(p, atom);
        if root_declared
            && self.own_property(self.realm.globals, atom).is_none()
            && self
                .heap.object(self.realm.globals)
                .is_some_and(|object| !object.is_extensible())
        {
            return Ok(());
        }
        let strict = self.owner_is_strict(p, owner);
        if strict && !initializing && !name.starts_with('\0') {
            self.require_global_assignment_binding(p, atom)?;
        }
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
        owner: VarOwner,
        slot: usize,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = p.function(self.owner_view(owner).function() as usize).local_atoms[slot];
        if let Some(caller) = self.direct_eval_variable_caller(owner, atom) {
            if !self.store_direct_eval_var_binding(owner, atom, value) {
                // Sloppy SetMutableBinding recreates a deleted eval variable
                // in the selected VariableEnvironment, not the global object.
                self.owner_bindings_mut(caller)
                    .ok_or_else(|| JsError("invalid eval variable environment".into()))?
                    .push((atom, value));
                self.note_dynamic_scope_name(atom);
            }
            return Ok(());
        }
        if self.global_object_var_atom(self.owner_view(owner), slot).is_some() {
            self.set_property_with_receiver(
                p,
                self.realm.globals,
                atom,
                value,
                self.realm.globals,
            )?;
            return Ok(());
        }
        if !self.store_owner_slot(owner, slot, value) {
            return Err(JsError("invalid variable binding".into()));
        }
        Ok(())
    }

    pub(super) fn store_direct_eval_var_binding(&mut self, running: VarOwner, atom: Atom, value: Value) -> bool {
        let Some(owner) = self.direct_eval_variable_caller(running, atom) else {
            return false;
        };
        let slot = (!self.parameter_eval)
            .then(|| self.activation_binding_slot(self.owner_view(owner), atom))
            .flatten();
        if let Some(slot) = slot {
            return self.store_owner_slot(owner, slot, value);
        }
        self.store_owner_binding(owner, atom, value)
    }

    fn store_global_var_binding(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
        strict: bool,
    ) -> Result<bool, JsError> {
        // Script and eval roots are the only ROOT_FUNCTION_ID activations whose `this` is the
        // global object; both are recorded in `active_roots` (`call_root`, `call_eval_closure`).
        let Some(()) = self.active_roots.iter().rev().find_map(|&index| {
            let frame = self.frames.get(index)?;
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
        // Only script and eval roots (`active_roots`) project global vars into their locals.
        for &index in &self.active_roots {
            let Some(frame) = self.frames.get_mut(index) else {
                continue;
            };
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
            let Some(slot) = function.local_slot(atom) else {
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

/// The facts one by-name lookup of `atom` from `caller` consults under more than one rule:
/// the caller's static binding for the name (`Vm::name_binding`) and the outer environment
/// that statically declares it (`Vm::outer_environment_binding` from the caller's captured
/// parent). Each is found on first use and stays valid for the lookup, which runs no guest
/// code between consulting them.
pub(super) struct NameLookup<'a> {
    caller: &'a dyn CallerView,
    atom: Atom,
    named: std::cell::OnceCell<Option<crate::bytecode::EvalBinding>>,
    static_owner: std::cell::OnceCell<Option<(Value, usize)>>,
}

impl<'a> NameLookup<'a> {
    pub(super) fn new(caller: &'a dyn CallerView, atom: Atom) -> Self {
        Self {
            caller,
            atom,
            named: std::cell::OnceCell::new(),
            static_owner: std::cell::OnceCell::new(),
        }
    }

    pub(super) fn named(&self, vm: &Vm) -> Option<crate::bytecode::EvalBinding> {
        *self.named.get_or_init(|| vm.name_binding(self.caller, self.atom))
    }

    pub(super) fn static_owner(&self, vm: &Vm) -> Option<(Value, usize)> {
        *self.static_owner.get_or_init(|| {
            vm.outer_environment_binding(vm.captured_parent_environment(self.caller), self.atom)
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

/// Whether a store to `slot` of a root environment of `function` with `scope` is a plain
/// slot write in `program`: the root of `program` itself, whose slot holds its binding
/// (`RootSlotRule::Binding`), as a module's top-level `let` does; no global var, global
/// lexical or import projection reads through it. The slot's marker of an uninitialized
/// binding still sends the store to the general path.
#[inline(always)]
fn root_binding_store(
    programs: &super::program_store::ProgramStore,
    program: super::ProgramId,
    function: u32,
    scope: &crate::heap::EnvironmentScopeRef,
    slot: u16,
) -> bool {
    function == super::ROOT_FUNCTION_ID
        && scope.program() == Some(program.raw())
        && programs.root_slot(program, slot) == crate::bytecode::RootSlotRule::Binding
}

/// The function owning an environment of `function` with `scope` whose slots
/// `environment_admits_direct_store` may admit: not the root's, a function-less layer's or
/// another program's.
#[inline(always)]
/// Whether a store to any captured slot of `owner`'s environment is the whole write: no slot
/// is its own name's binding (`Function::self_binding_slot`) or a parameter a mapped
/// `arguments` object aliases. A mapped object aliases every parameter or none, so parameter 0
/// answers for all of them.
pub fn owner_admits_every_direct_store(owner: &crate::bytecode::Function) -> bool {
    owner.self_binding_slot.is_none()
        && (owner.params == 0 || super::arguments::mapped_arguments_slot(owner, 0).is_none())
}

pub fn direct_store_owner<'p>(
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
    Some((env.heap_index()? as usize ^ (usize::from(depth) * (CAPTURE_WALKS / 2 + 1))) & (CAPTURE_WALKS - 1))
}
