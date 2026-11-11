//! Reads, writes and brand checks of private names past their sites' caches, over the keys
//! `private_keys` gives declared fields and method brands.

use super::{Atom, Host, ResidualProgram, Value, Vm};
use super::private_keys::PrivateDecl;

impl<H: Host> Vm<H> {
    /// The value of the private field `brand` names on `object`: its shape slot, else a
    /// private element of an undeclared name (`Object::private_field`).
    #[inline(always)]
    pub(super) fn brand_field_value(&self, object: Value, brand: crate::heap::PrivateBrand) -> Option<Value> {
        if let Some(key) = self.private_field_key_of(brand.home, brand.name) {
            return self.private_field_at(object, key);
        }
        self.object_data(object)?.private_field(brand)
    }

    /// Stores the private field `brand` names on `object`; `false` where it has none.
    #[inline(always)]
    pub(super) fn set_brand_field(&mut self, object: Value, brand: crate::heap::PrivateBrand, value: Value) -> bool {
        if let Some(key) = self.private_field_key_of(brand.home, brand.name) {
            let Some(slot) = self.private_field_slot(object, key) else { return false };
            self.heap.property_set(object, slot, value);
            return true;
        }
        self.object_data_mut(object).is_some_and(|data| data.set_private_field(brand, value))
    }

    /// Whether `object` carries `brand`: a declared field's shape key, or a private element.
    #[inline(always)]
    pub(super) fn carries_brand(&self, object: Value, brand: crate::heap::PrivateBrand) -> bool {
        match self.private_brand_key_of(brand.home, brand.name) {
            Some(key) => self.private_field_slot(object, key).is_some(),
            None => self.object_data(object).is_some_and(|data| data.has_private_name(brand)),
        }
    }

    /// The private field names `home` declared, for an eval in its scope.
    pub(super) fn declared_private_fields(&self, home: Value) -> Vec<Atom> {
        self.private_homes.fields.each_decl()
            .filter(|(atom, _)| !self.private_homes.fields.method(*atom))
            .filter(|(atom, decl)| match decl {
                PrivateDecl::Single(first) => *first == home,
                PrivateDecl::Multi => self.private_homes.fields.keys.contains_key(&(home, *atom)),
            })
            .map(|(atom, _)| atom)
            .collect()
    }
}

impl<H: Host> Vm<H> {
    /// A general read of private name `atom` of `object` at `site`: a field the site's own
    /// entry or the object's shape holds, else an undeclared name's private element. `None`
    /// leaves methods, accessors and a missing brand to the ordinary `[[Get]]`.
    #[inline(always)]
    pub(super) fn private_field_get(&mut self, p: &ResidualProgram, object: Value, atom: Atom, site: u16) -> Option<Value> {
        if self.private_homes.fields.single(atom)
            && let super::object::FieldCacheProbe::Hit(value) = self.cached_field_hit(p, object, site)
        {
            return Some(value);
        }
        self.private_field_read(p, object, atom, Some(site)).or_else(|| self.private_field_value(p, object, atom))
    }
}

impl<H: Host> Vm<H> {
    /// Whether reads of `atom` need more than a field cache's own entries: a private name
    /// other than a field one home alone declared (`PrivateKeys::single`).
    #[inline(always)]
    pub(super) fn private_special(&self, atom: Atom) -> bool {
        self.is_private_name(atom) && !self.private_homes.fields.single(atom)
    }

    /// A read of private name `atom` past its site's caches: a declared field's value, or a
    /// TypeError for an object without it; `None` for a private method or accessor name.
    pub(super) fn private_get_miss(&mut self, p: &ResidualProgram, object: Value, atom: Atom, site: u16) -> Option<Result<Value, crate::JsError>> {
        if let Some(value) = self.private_field_read(p, object, atom, Some(site)) {
            return Some(Ok(value));
        }
        if !self.private_homes.fields.declared(atom) {
            return None;
        }
        let name = self.atom_name(atom);
        let member = crate::bytecode::private_identity_label(name).unwrap_or(name).to_owned();
        Some(Err(self.type_error(p, format!("Cannot read private member #{member} from an object whose class did not declare it"))))
    }
}

impl<H: Host> Vm<H> {
    /// A field read's answer past its site's probe for a private name (`private_get_miss`):
    /// its shape slot or a TypeError, which no prototype answers.
    #[inline(always)]
    pub(super) fn private_miss_read(&mut self, p: &ResidualProgram, object: Value, atom: Atom, site: u16, probe: &super::object::FieldCacheProbe) -> Option<Result<Value, crate::JsError>> {
        if matches!(probe, super::object::FieldCacheProbe::Hit(_)) || !self.is_private_name(atom) {
            return None;
        }
        self.private_get_miss(p, object, atom, site)
    }

    /// A store of private name `atom` past its caches: `true` once a field's slot or an
    /// undeclared name's private element took it, a TypeError for a declared field's name on
    /// an object without the field, `false` for a method or accessor name.
    pub(super) fn private_set_answered(&mut self, p: &ResidualProgram, object: Value, atom: Atom, value: Value, site: u16) -> Result<bool, crate::JsError> {
        if self.private_field_write(p, object, atom, value, Some(site)) || self.set_private_field_value(p, object, atom, value) {
            return Ok(true);
        }
        if self.private_homes.fields.declared(atom) {
            return Err(self.type_error(p, "Cannot write private member to an object whose class did not declare it".into()));
        }
        Ok(false)
    }

    /// `DefineField`: a private name's PrivateFieldAdd (`private_field_define`), else
    /// CreateDataProperty through the site's add cache (`define_field_cached`).
    #[inline(always)]
    pub(super) fn define_field(&mut self, p: &ResidualProgram, object: Value, atom: Atom, value: Value, site: u16) -> Result<(), crate::JsError> {
        if self.is_private_name(atom) {
            return self.private_field_define(p, object, atom, value, site);
        }
        self.define_field_cached(p, object, atom, value, site)
    }
}
