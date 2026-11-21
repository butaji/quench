//! What a root body's local slot stands for beyond the activation's own storage: the RootBindings
//! hook every execution of a root reads, through the `CallerView` of its activation (a VM
//! frame, or the window of a lane activation, `scoped_names::WindowScope`). A script root's
//! global lexical binding lives in the realm's bindings, a script's top-level `var` as a property
//! of the global object, and a module's import in the exporting module's environment record
//! (`ProgramStore::module_import`); every other slot of a root, and every slot of any other
//! function, is the activation's own.
use super::caller_view::CallerView;
use super::*;

/// A root slot's binding where it is not the activation's own.
pub(in crate::vm) enum RootBinding {
    /// A global lexical binding's value in the realm (`Vm::root_global_lexical_value`); `DELETED`
    /// while it is uninitialized.
    GlobalLexical(Value),
    /// A global `var`, the property `atom` of the global object (`Vm::global_object_var_atom`).
    GlobalVar(Atom),
    /// A module import's value, read through the exporting module's environment record
    /// (`Vm::module_import_value`).
    Import(Value),
}

impl Vm {
    /// What slot `slot` of `caller`'s function stands for outside the activation: `None` for
    /// the activation's own, which is every slot of a function other than a program's root.
    pub(super) fn root_binding(&self, caller: &dyn CallerView, slot: usize) -> Option<RootBinding> {
        let (program, function) = (caller.program(), caller.function());
        if function != super::ROOT_FUNCTION_ID {
            return None;
        }
        let code = self.programs.get(program)?;
        if let Some(value) = self.root_global_lexical_value(&code, function, slot) {
            return Some(RootBinding::GlobalLexical(value));
        }
        if let Some(atom) = self.global_object_var_atom(caller, slot) {
            return Some(RootBinding::GlobalVar(atom));
        }
        self.module_import_value(program, function, slot).map(RootBinding::Import)
    }
}
