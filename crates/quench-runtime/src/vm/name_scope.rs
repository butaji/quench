//! Where a by-name read lands, and what keeps a lane record's answer that it lands on the
//! global object: the facts an event of the scope chain retires.
use super::caller_view::CallerView;
use super::environment::NameLookup;
use super::*;
use crate::bitset::BitSet;

/// Where a by-name read lands (`Vm::locate_name`), in the order the layers of the search are
/// consulted. Locating runs guest code only for `WithObject`; every other location is read with
/// no effect in between, so a read applies the location's own rule (`Vm::read_name_location`).
#[derive(Clone, Copy)]
pub(super) enum NameLocation {
    /// A slot of the executing activation's own locals, which keep its bindings.
    FrameLocal(usize),
    /// A slot of an environment that holds the executing activation's bindings.
    EnvironmentSlot(Value, usize),
    /// A dynamic binding of the executing activation or of an environment outside it.
    Dynamic(DynamicSlot),
    /// A named binding of the executing activation, read where it lives.
    Named(crate::bytecode::EvalBinding),
    /// A realm global lexical binding.
    GlobalLexical(Atom),
    /// The innermost object environment that has the name, and the key it was found by.
    WithObject(Value, Value),
    /// The value an intrinsic name stands for, which no binding can shadow.
    Intrinsic(Value),
    /// The global object's property, or no binding at all.
    Global,
}

/// A dynamic binding's place: among the executing activation's own, or in an environment
/// outside it (`Vm::dynamic_binding_slot_in`).
#[derive(Clone, Copy)]
pub(super) enum DynamicSlot {
    Own(usize),
    Outer(Value, usize),
}

/// The by-name read a lane record handed to the general path to settle
/// (`Vm::settle_name_probe`): where it is, in the running activation's frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NameProbe {
    pub program: ProgramId,
    pub function: u32,
    pub pc: u32,
}

/// What lane records that read a name from the global object past a scope that could have
/// declared it rely on.
#[derive(Default)]
pub struct DynamicScopeNames {
    /// The names a scope declared dynamically at any time (`Vm::note_dynamic_scope_name`).
    declared: BitSet,
    /// The names whose by-name reads landed on the global object past such a scope
    /// (`Vm::name_lands_on_global_object`).
    landed: BitSet,
    /// The read the lane left for the general path to settle.
    pub probe: Option<NameProbe>,
}

impl DynamicScopeNames {
    /// Whether a scope declared `atom` dynamically at any time.
    #[inline(always)]
    pub fn declared(&self, atom: Atom) -> bool {
        self.declared.contains(atom as usize)
    }
}

impl Vm {
    /// Whether a by-name read of `atom` in the executing activation lands on the global
    /// object, and keeps landing there until a dependency retires: no `with` object was ever
    /// entered, no scope ever declared the name dynamically (`note_dynamic_scope_name`), and
    /// the layers of the search hold nothing of it (`resolve_name_source`). The name is then
    /// recorded as depending on it, which a dynamic declaration of it retires.
    fn name_lands_on_global_object(&mut self, p: &ResidualProgram, owner: VarOwner, atom: Atom) -> bool {
        let lands = !self.with_scope_entered
            && !self.scope_names.declared(atom)
            && matches!(self.resolve_name_source(p, owner, atom), NameLocation::Global);
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
    /// when the lane left it as a probe (`FLAG_PROBE`): its record becomes a hinted
    /// global read if the name lands on the global object, else it exits. A probe left at
    /// another instruction is dropped, its record probing again.
    pub(super) fn settle_name_probe(&mut self, p: &ResidualProgram, atom: Atom) {
        let Some(probe) = self.scope_names.probe.take() else { return };
        let Some(frame) = self.frames.last() else { return };
        let Some(pc) = frame.binding_site_pc.and_then(|resume| resume.checked_sub(1)) else { return };
        if probe != (NameProbe { program: frame.program, function: frame.function, pc }) {
            return;
        }
        // The name's landing is recorded (`name_lands_on_global_object`), so a later declaration of it retires the
        // dependency; the record of a probed read takes the general step, whose scope may declare the name later.
        self.name_lands_on_global_object(p, VarOwner::Frame(self.frames.len() - 1), atom);
        if let Some(view) = self
            .programs
            .views_in_lane(probe.program)
            .and_then(|views| views.get(probe.function as usize))
            .and_then(super::program_store::derived_lane_view)
        {
            crate::vm::engine_calls::settle_name_record(view, pc as usize);
        }
    }

    /// The by-name read of `atom` by the activation `caller` past its `with` objects and dynamic
    /// bindings, which none of them answered: the global read the lane takes
    /// (`Vm::global_name_read`, its cached field at `site`) where no layer `resolve_name_source`
    /// searches before the global object binds the name. `None`, before any effect, where one
    /// does (a named binding of the activation or its closures, its own local, an outer static
    /// owner, an active global lexical binding) or the global read is not decided in place.
    pub fn read_past_scopes(&mut self, p: &ResidualProgram, caller: &dyn CallerView, atom: Atom, site: u16) -> Option<Value> {
        // Once a `with` was entered, a scope the window does not hold may bind the name: the chain
        // eval code's environment captured (`captured_with_objects_for_function`).
        if self.with_scope_entered || self.binds_before_global_object(p, &NameLookup::new(caller, atom), caller.function(), atom) {
            return None;
        }
        match self.global_name_read(p, atom, site) {
            super::field_cache::GlobalNameRead::Value(value) => Some(value),
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
            || self.realm.global_lexical_bindings.contains_key(&atom)
    }

    /// Where a by-name read of `atom` in `owner`, the executing activation, lands once no `with`
    /// object holds it: the layers of the search in order, each of which only reads.
    pub(super) fn resolve_name_source(&self, p: &ResidualProgram, owner: VarOwner, atom: Atom) -> NameLocation {
        let internal = self.atom_class(atom).contains(AtomClass::INTERNAL);
        if !internal && let Some(location) = self.static_scope_name_location(p, owner, atom) {
            return location;
        }
        // One lookup consults the activation's static binding and the outer static owner from
        // more than one rule; each is found once.
        let lookup = NameLookup::new(self.owner_view(owner), atom);
        if let Some(slot) = self.dynamic_binding_slot_in(&lookup) {
            return NameLocation::Dynamic(slot);
        }
        if let Some(binding) = lookup.named(self) {
            return NameLocation::Named(binding);
        }
        if !internal && let Some(location) = self.location_beyond_bindings(p, owner, atom, &lookup) {
            return location;
        }
        NameLocation::Global
    }

    /// Where a by-name read of `atom` lands where names resolve statically: the global object
    /// when no global lexical declaration shadows it, else the realm's binding, which no layer
    /// searched before it can hold the name past.
    fn static_scope_name_location(&self, p: &ResidualProgram, owner: VarOwner, atom: Atom) -> Option<NameLocation> {
        if self.names_resolve_to_global_object(p, owner, atom) {
            return Some(NameLocation::Global);
        }
        (self.global_scope_names_resolve_statically(p, owner)
            && self.realm.global_lexical_bindings.contains_key(&atom))
        .then_some(NameLocation::GlobalLexical(atom))
    }

    /// The location of the layers a by-name read of `atom` searches after the named bindings:
    /// the activation's own local, the realm's global lexical bindings and the outer static owner.
    fn location_beyond_bindings(&self, p: &ResidualProgram, owner: VarOwner, atom: Atom, lookup: &NameLookup) -> Option<NameLocation> {
        let view = self.owner_view(owner);
        let function = view.function();
        if (self.function_environment_binding(p, function, atom)
                || p.function(function as usize).global_lexical_atoms().contains(&atom)
                || self.root_local_var_binding(p, function, atom))
            && let Some(slot) = self.local_binding_slot(p, function, atom)
        {
            return Some(if view.owns_environment() {
                NameLocation::EnvironmentSlot(view.environment(), slot)
            } else {
                NameLocation::FrameLocal(slot)
            });
        }
        if self.realm.global_lexical_bindings.contains_key(&atom) {
            return Some(NameLocation::GlobalLexical(atom));
        }
        if let Some((env, slot)) = lookup.static_owner(self)
            && self.heap.environment_slot(env, slot).is_some()
        {
            return Some(NameLocation::EnvironmentSlot(env, slot));
        }
        None
    }
}
