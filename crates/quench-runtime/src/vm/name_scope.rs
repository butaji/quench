//! Where a by-name read lands, and what keeps a lane record's answer that it lands on the
//! global object: the facts an event of the scope chain retires.
use super::caller_view::CallerView;
use super::environment::NameLookup;
use super::*;
use crate::bitset::BitSet;

/// Where a by-name read lands (`Vm::resolve_name_source`).
pub(super) enum NameSource {
    /// A binding's current value, which an uninitialized one makes an error to read.
    Value(Value),
    /// A named binding of the executing activation, read where it lives.
    Binding(crate::bytecode::EvalBinding),
    /// The global object's property, or no binding at all.
    GlobalObject,
}

/// The by-name read a lane record handed to the general path to settle
/// (`Vm::settle_name_probe`): where it is, in the running activation's frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct NameProbe {
    pub(super) program: ProgramId,
    pub(super) function: u32,
    pub(super) pc: u32,
}

/// What lane records that read a name from the global object past a scope that could have
/// declared it rely on.
#[derive(Default)]
pub(super) struct DynamicScopeNames {
    /// The names a scope declared dynamically at any time (`Vm::note_dynamic_scope_name`).
    declared: BitSet,
    /// The names whose by-name reads landed on the global object past such a scope
    /// (`Vm::name_lands_on_global_object`).
    landed: BitSet,
    /// The read the lane left for the general path to settle.
    pub(super) probe: Option<NameProbe>,
}

impl DynamicScopeNames {
    /// Whether a scope declared `atom` dynamically at any time.
    #[inline(always)]
    pub(super) fn declared(&self, atom: Atom) -> bool {
        self.declared.contains(atom as usize)
    }
}

impl<H: Host> Vm<H> {
    /// Whether a by-name read of `atom` in the executing activation lands on the global
    /// object, and keeps landing there until a dependency retires: no `with` object was ever
    /// entered, no scope ever declared the name dynamically (`note_dynamic_scope_name`), and
    /// the layers of the search hold nothing of it (`resolve_name_source`). The name is then
    /// recorded as depending on it, which a dynamic declaration of it retires.
    fn name_lands_on_global_object(&mut self, p: &ResidualProgram, owner: VarOwner, atom: Atom) -> bool {
        let lands = !self.with_scope_entered
            && !self.scope_names.declared(atom)
            && matches!(self.resolve_name_source(p, owner, atom), NameSource::GlobalObject);
        if lands {
            self.scope_names.landed.insert_growing(atom as usize);
        }
        lands
    }

    /// A scope declared `atom` dynamically (a sloppy direct eval's `var` or function in its
    /// caller's variable environment): by-name reads of it that landed on the global object
    /// (`name_lands_on_global_object`) may land there no more, so the hints and the code that
    /// relied on them retire (`note_global_lexical_names`). A name that never lands there
    /// retires nothing, and a declared name never lands again.
    pub(super) fn note_dynamic_scope_name(&mut self, atom: Atom) {
        let names = &mut self.scope_names;
        if names.declared.insert_growing(atom as usize) && names.landed.contains(atom as usize) {
            self.note_global_lexical_names();
        }
    }

    /// The general path's by-name read of `atom` at the running frame's current instruction,
    /// when the lane left it as a probe (`lane_load_name_probe`): its record becomes a hinted
    /// global read if the name lands on the global object, else it exits. A probe left at
    /// another instruction is dropped, its record probing again.
    pub(super) fn settle_name_probe(&mut self, p: &ResidualProgram, atom: Atom) {
        let Some(probe) = self.scope_names.probe.take() else { return };
        let Some(frame) = self.frames.last() else { return };
        let Some(pc) = frame.binding_site_pc.and_then(|resume| resume.checked_sub(1)) else { return };
        if probe != (NameProbe { program: frame.program, function: frame.function, pc }) {
            return;
        }
        let function = p.function(frame.function as usize);
        let op = function.widened(function.code[pc as usize]).op();
        let lands = self.name_lands_on_global_object(p, VarOwner::Frame(self.frames.len() - 1), atom);
        if let Some(view) = self
            .programs
            .lane_views(probe.program)
            .and_then(|views| views.get(probe.function as usize))
            .and_then(super::program_store::derived_lane_view)
        {
            dispatch_fast::settle_name_record::<H>(view, pc as usize, op, lands);
        }
    }

    /// The by-name read of `atom` by the activation `caller` past its `with` objects and dynamic
    /// bindings, which none of them answered: the global read the lane takes
    /// (`Vm::global_name_read`, its cached field at `site`) where no layer `resolve_name_source`
    /// searches before the global object binds the name. `None`, before any effect, where one
    /// does (a named binding of the activation or its closures, its own local, an outer static
    /// owner, an active global lexical binding) or the global read is not decided in place.
    pub(super) fn read_past_scopes(&mut self, p: &ResidualProgram, caller: &dyn CallerView, atom: Atom, site: u16) -> Option<Value> {
        // Once a `with` was entered, a scope the window does not hold may bind the name: the chain
        // eval code's environment captured (`captured_with_objects_for_function`).
        if self.with_scope_entered || self.binds_before_global_object(p, &NameLookup::new(caller, atom), caller.function(), atom) {
            return None;
        }
        match self.global_name_read(p, atom, site) {
            super::field_cache::GlobalNameRead::Value(value, _) => Some(value),
            _ => None,
        }
    }

    /// Whether a layer of `lookup`'s search for `atom` by `function` past the dynamic bindings,
    /// before the global object's, binds the name (`resolve_name_source`,
    /// `name_value_beyond_bindings`). Each answers as or before the realm's global lexical
    /// bindings, which `global_name_read` reads first, so the read past them is the general one.
    fn binds_before_global_object(&self, p: &ResidualProgram, lookup: &NameLookup, function: u32, atom: Atom) -> bool {
        lookup.named(self).is_some()
            || self.function_environment_binding(p, function, atom)
            || p.function(function as usize).global_lexical_atoms().contains(&atom)
            || self.root_local_var_binding(p, function, atom)
            || lookup.static_owner(self).is_some()
            || self.active_global_lexical_value(atom).is_some()
    }

    /// Where a by-name read of `atom` in `owner`, the executing activation, lands once no `with`
    /// object holds it: the layers of the search in order, each of which only reads.
    pub(super) fn resolve_name_source(&self, p: &ResidualProgram, owner: VarOwner, atom: Atom) -> NameSource {
        let internal = self.atom_class(atom).contains(AtomClass::INTERNAL);
        if !internal && let Some(source) = self.static_scope_name_source(p, owner, atom) {
            return source;
        }
        // One lookup consults the activation's static binding and the outer static owner from
        // more than one rule; each is found once.
        let lookup = NameLookup::new(self.owner_view(owner), atom);
        if let Some(value) = self.dynamic_binding_in(&lookup) {
            return NameSource::Value(value);
        }
        if let Some(binding) = lookup.named(self) {
            return NameSource::Binding(binding);
        }
        if !internal && let Some(value) = self.name_value_beyond_bindings(p, owner, atom, &lookup) {
            return NameSource::Value(value);
        }
        NameSource::GlobalObject
    }

    /// What a by-name read of `atom` finds where names resolve statically: the global object
    /// when no global lexical declaration shadows it, else the realm's binding, which no layer
    /// searched before it can hold the name past.
    fn static_scope_name_source(&self, p: &ResidualProgram, owner: VarOwner, atom: Atom) -> Option<NameSource> {
        if self.names_resolve_to_global_object(p, owner, atom) {
            return Some(NameSource::GlobalObject);
        }
        self.global_scope_names_resolve_statically(p, owner)
            .then(|| self.realm.global_lexical_bindings.get(&atom).copied().map(NameSource::Value))
            .flatten()
    }

    /// The value of the layers a by-name read of `atom` searches after the named bindings: the
    /// activation's own local, the realm's global lexical bindings and the outer static owner.
    fn name_value_beyond_bindings(&self, p: &ResidualProgram, owner: VarOwner, atom: Atom, lookup: &NameLookup) -> Option<Value> {
        let view = self.owner_view(owner);
        let function = view.function();
        if (self.function_environment_binding(p, function, atom)
                || p.function(function as usize).global_lexical_atoms().contains(&atom)
                || self.root_local_var_binding(p, function, atom))
            && let Some(slot) = self.local_binding_slot(p, function, atom)
        {
            return Some(if view.owns_environment() {
                self.heap.environment_slot(view.environment(), slot).unwrap_or(Value::UNDEFINED)
            } else {
                self.owner_locals(owner)[slot]
            });
        }
        self.realm
            .global_lexical_bindings
            .get(&atom)
            .copied()
            .or_else(|| {
                let (env, slot) = lookup.static_owner(self)?;
                self.heap.environment_slot(env, slot)
            })
            .or_else(|| self.active_global_lexical_value(atom))
    }
}
