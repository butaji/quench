//! A by-name read of the global scope, one decision for the lane's handler
//! (`lane_load_global_name_refresh`) and for compiled code (`Vm::lane_global_read`), so a
//! read the interpreter answers natively compiled code answers natively too.
use super::*;

/// What the global scope's fast read found for a name.
pub(in crate::vm) enum GlobalNameRead {
    /// A global lexical binding not yet initialized, which the general path throws for.
    Uninitialized,
    /// The global object's own data field `hint` names, which a record may keep.
    Hinted(Value, super::super::FieldCache),
    /// An initialized global lexical binding, or the site's cached field.
    Read(Value),
    /// Neither: an accessor, an absent name, or a field no cache names yet.
    Missed,
}

impl<H: Host> Vm<H> {
    /// The read of global `atom` through field cache `site` of `code`: a global lexical
    /// binding first, as the general path's search finds it; else the global object's own
    /// field the site's entry proves, which a record may keep as its hint; else the cache's other
    /// tiers; else the global object's own data field by its exact shape, which the site then
    /// caches, so the next read, native or the lane's, is the cache's.
    pub(in crate::vm) fn global_name_read(&mut self, code: &ResidualProgram, atom: Atom, site: u16) -> GlobalNameRead {
        if self.realm.global_lexical_declarations.contains(&atom) {
            return match self.realm.global_lexical_bindings.get(&atom).copied().filter(|value| !value.is_deleted()) {
                Some(value) => GlobalNameRead::Read(value),
                None => GlobalNameRead::Uninitialized,
            };
        }
        let globals = self.realm.globals;
        let hintable = self.global_name_hintable(atom);
        let entry = self.field_cache_entry(site);
        // A record's hint names an own field (`LaneInstruction::field_hint`).
        if hintable && entry.holder == NO_FIELD_HOLDER && let Some(value) = self.record_field_entry_hit(entry, globals) {
            return GlobalNameRead::Hinted(value, entry);
        }
        if let super::super::object::FieldCacheProbe::Hit(value) = self.cached_field_hit(code, globals, site) {
            return GlobalNameRead::Read(value);
        }
        if hintable
            && let Some(hint) = self.own_data_field_hint::<false>(globals, atom)
            && let Some(value) = self.record_field_entry_hit(hint, globals)
        {
            self.record_field_cache(self.field_cache_index(site), hint);
            return GlobalNameRead::Hinted(value, hint);
        }
        GlobalNameRead::Missed
    }

    /// Whether global `atom` is a fixed variable: a non-configurable own data property of the
    /// global object (a `var` or function declaration's) that no global lexical binding
    /// shadows. None can come to (a lexical declaration of its name throws), nor can it go.
    pub(in crate::vm) fn fixed_global_variable(&self, atom: Atom) -> bool {
        let globals = self.realm.globals;
        self.global_name_hintable(atom)
            && !self.realm.global_lexical_declarations.contains(&atom)
            && self.own_data_field_hint::<false>(globals, atom).is_some_and(|hint| {
                let object = self.heap.object_record_by_id(globals.direct_object_index().expect("the global object is direct"));
                self.slot_attributes(object, usize::from(hint.slot)).is_some_and(|attributes| !attributes.configurable)
            })
    }
}
