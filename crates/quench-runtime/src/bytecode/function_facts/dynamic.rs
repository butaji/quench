//! Whether a function reads its scope dynamically, and which names resolve statically.

use super::super::*;

impl Function {
    /// Whether the function itself can reach bindings by name at run time: it was created
    /// inside a `with` scope, records by-name lexical sites, resolves names dynamically, or
    /// calls direct eval (also from a parameter scope).
    /// Derived once per function: lane views, capture layouts and name resolution each ask,
    /// and the instruction scan had cost 1.3% of the axios npm scenario's instructions.
    pub(crate) fn has_dynamic_scope_access(&self) -> bool {
        *self.dynamic_scope_access.get_or_init(|| self.derive_dynamic_scope_access())
    }

    pub(super) fn derive_dynamic_scope_access(&self) -> bool {
        self.inherited_with_scope
            || !self.binding_sites.is_empty()
            || self.instructions().any(WideInstruction::resolves_dynamic_scope)
    }

    /// Whether the body enters a `with` statement, whose by-name loads may denote
    /// any enclosing function's binding. `is_with_enter` recognizes the atom id of
    /// [`WITH_ENTER_BINDING`] in the owning program's atom table.
    pub(crate) fn contains_with_statement(&self, is_with_enter: impl Fn(usize) -> bool) -> bool {
        self.instructions().any(|instruction| {
            instruction.op().is_throwing_name_load()
                && usize::try_from(instruction.atom_index()).is_ok_and(&is_with_enter)
        })
    }

    /// Whether neither this function nor any enclosing one has dynamic scope access or
    /// contains a `with` statement. The compiler then resolved every lexical binding the body
    /// names, so a by-name load in it can only denote a global binding.
    /// `program` holds this function; an enclosing function whose record it has not read yet
    /// is read here.
    pub(crate) fn names_resolve_statically(&self, program: &ResidualProgram) -> bool {
        *self.static_name_scope.get_or_init(|| self.derive_names_resolve_statically(program))
    }

    pub(super) fn derive_names_resolve_statically(&self, program: &ResidualProgram) -> bool {
        self.own_names_resolve_statically(program)
            && self.parent.is_none_or(|parent| {
                program
                    .function_get(parent as usize)
                    .is_some_and(|parent| parent.names_resolve_statically(program))
            })
    }

    /// `names_resolve_statically` but for the enclosing functions: this function itself has
    /// no dynamic scope access and contains no `with` statement.
    pub(crate) fn own_names_resolve_statically(&self, tables: &impl FactTables) -> bool {
        !self.has_dynamic_scope_access()
            && !self.contains_with_statement(|atom| tables.atom_name(atom) == Some(WITH_ENTER_BINDING))
    }

    pub(crate) fn plain_local_slot_is_safe(&self, slot: usize) -> bool {
        self.plain_local_slot_ineligibility(slot).is_none()
    }

    pub(crate) fn plain_local_slot_ineligibility(
        &self,
        slot: usize,
    ) -> Option<PlainLocalSlotIneligibility> {
        if self.local_atoms.get(slot).is_none() {
            return Some(PlainLocalSlotIneligibility::MissingAtom);
        }
        if self.is_self_binding_slot(slot) {
            return Some(PlainLocalSlotIneligibility::SelfBinding);
        }
        if self
            .selective_capture_slots
            .as_ref()
            .is_some_and(|captured| {
                u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok())
            })
        {
            return Some(PlainLocalSlotIneligibility::Captured);
        }
        if slot < usize::from(self.params)
            && self.arguments_slot.is_some()
            && self.arguments_alias_parameters()
        {
            return Some(PlainLocalSlotIneligibility::MappedArgument);
        }
        None
    }

    pub(crate) fn is_self_binding_slot(&self, slot: usize) -> bool {
        self.self_binding_slot
            .is_some_and(|self_slot| usize::from(self_slot) == slot)
    }

    pub(crate) fn local_slot_uses_environment(&self, slot: usize) -> bool {
        self.selective_capture_slots
            .as_ref()
            .is_none_or(|captured| {
                u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok())
            })
    }
}
