//! Private elements as ordinary shape slots. A private field of brand `(home, name)` is the
//! object's property under `PropertyKey::Private(home, name)`, added by a shape transition as
//! a public field is, so the shapes, slot storage, lookup memos and collector serve private
//! and public names alike. A private method or accessor brands the object with a slot under
//! the brand's name marked by `BRAND_ONLY` (its function lives with the class, not the
//! object), which no field read finds. Private keys appear in no key list: every key walk
//! takes string and symbol keys only, and integrity levels change those alone.

use super::property_key::PropertyKey;
use super::{Atom, DataAttributes, FieldCache, ResidualProgram, Value, Vm};
use crate::heap::PrivateBrand;

/// The bit of a private key's name that marks a brand-only entry (a private method or
/// accessor the object carries); atoms stay below it.
const BRAND_ONLY: u32 = 1 << 31;

/// The attributes of a private slot: never observable, so the defaults, which keep a
/// shape's all-default fact for its public slots.
const PRIVATE_ATTRIBUTES: DataAttributes = super::DEFAULT_DATA_ATTRIBUTES;

/// The key of the private field `brand` names.
#[inline(always)]
pub(super) fn private_field_key(brand: PrivateBrand) -> PropertyKey {
    PropertyKey::private(brand.home, brand.name)
}

/// The key of the brand-only entry of private method or accessor `brand`.
#[inline(always)]
fn private_brand_key(brand: PrivateBrand) -> PropertyKey {
    PropertyKey::private(brand.home, brand.name | BRAND_ONLY)
}

impl Vm {
    /// The slot of `object`'s private field `brand`.
    #[inline]
    pub fn private_field_slot(&self, object: Value, brand: PrivateBrand) -> Option<usize> {
        let data = self.heap.object(object)?;
        self.property_shape_slot(data.shape(), private_field_key(brand))
    }

    /// `object`'s private field `brand`; `None` for an object without one (a private
    /// method's or accessor's brand included).
    #[inline]
    pub fn private_field(&self, object: Value, brand: PrivateBrand) -> Option<Value> {
        let data = self.heap.object(object)?;
        let slot = self.property_shape_slot(data.shape(), private_field_key(brand))?;
        self.heap.property_get_ref(data, slot)
    }

    /// Whether `object` carries private name `brand`, a field or a method's or accessor's.
    pub fn has_private_name(&self, object: Value, brand: PrivateBrand) -> bool {
        self.heap.object(object).is_some_and(|data| {
            self.property_shape_slot(data.shape(), private_field_key(brand)).is_some()
                || self.property_shape_slot(data.shape(), private_brand_key(brand)).is_some()
        })
    }

    /// Stores `value` in `object`'s existing private field `brand`: false for an object
    /// without one.
    pub(super) fn set_private_field(&mut self, object: Value, brand: PrivateBrand, value: Value) -> bool {
        let Some(slot) = self.private_field_slot(object, brand) else {
            return false;
        };
        self.heap.property_set(object, slot, value);
        true
    }

    /// Adds private field `brand` to `object` with `value`: false when it has it already.
    /// Private elements ignore extensibility and integrity levels, which the caller applies
    /// as its host asks (`Vm::private_element_addable`).
    pub(super) fn add_private_field(&mut self, object: Value, brand: PrivateBrand, value: Value) -> bool {
        self.add_private_slot(object, private_field_key(brand), value)
    }

    /// Brands `object` with private method or accessor `brand`, once.
    pub(super) fn add_private_name(&mut self, object: Value, brand: PrivateBrand) {
        if !self.has_private_name(object, brand) {
            self.add_private_slot(object, private_brand_key(brand), Value::UNDEFINED);
        }
    }

    fn add_private_slot(&mut self, object: Value, key: PropertyKey, value: Value) -> bool {
        let Some(shape) = self.heap.object(object).map(|data| data.shape()) else {
            return false;
        };
        if self.property_shape_slot(shape, key).is_some() {
            return false;
        }
        let next = self.transition_data_property_shape(shape, key, PRIVATE_ATTRIBUTES);
        self.heap.property_add(object, value, next);
        true
    }

    /// The slot of `object`'s private field `atom` at private-name site `site` of `function`,
    /// run in a frame or not (`frame`) and capturing from `environment`: the site's last shape
    /// and slot under the home that environment finds (`PrivateSiteHomes::field_slot`), else
    /// the brand (`Vm::site_private_brand`) and the shape's slot, recorded for the site. `None`
    /// for an object without the field, a private method's or accessor's brand included.
    #[inline(always)]
    pub fn site_private_field_slot(
        &mut self,
        code: &ResidualProgram,
        site: usize,
        scope: (Value, u32, bool),
        object: Value,
        atom: Atom,
    ) -> Option<usize> {
        let shape = self.heap.object(object)?.shape();
        // A site whose name belongs to a class evaluated once answers from the shape alone.
        if self.private_classes.unique_name(atom)
            && let Some(slot) = self.private_site_homes.unique_field_slot(site, shape)
        {
            return Some(slot);
        }
        if let Some(slot) = self.private_site_homes.field_slot(site, scope.0, shape) {
            return Some(slot);
        }
        self.site_private_field_slot_miss(code, site, scope, object, shape, atom)
    }

    #[inline(never)]
    fn site_private_field_slot_miss(
        &mut self,
        code: &ResidualProgram,
        site: usize,
        (environment, function, frame): (Value, u32, bool),
        object: Value,
        shape: u32,
        atom: Atom,
    ) -> Option<usize> {
        let brand = self.site_private_brand(code, site, environment, function, frame, atom)?;
        let slot = self.private_field_slot(object, brand)?;
        if self.private_shape_cacheable(shape) {
            let unique = self.private_classes.register(atom, brand.home);
            self.private_site_homes.record_field_slot(site, brand.home, shape, slot, unique);
        }
        Some(slot)
    }

    /// `site_private_field_slot`'s value.
    #[inline(always)]
    pub fn site_private_field(
        &mut self,
        code: &ResidualProgram,
        site: usize,
        scope: (Value, u32, bool),
        object: Value,
        atom: Atom,
    ) -> Option<Value> {
        let slot = self.site_private_field_slot(code, site, scope, object, atom)?;
        self.heap.property_get_ref(self.heap.object(object)?, slot)
    }

    /// Whether a site may record `shape` for a private slot: a shape no edit in place
    /// changes, so its slots stay.
    pub fn private_shape_cacheable(&self, shape: u32) -> bool {
        !self.shape_is_dictionary(shape) && !self.owned_shape_edits_in_place(shape)
    }

    /// The own-field entry (`FieldCache::own`) of the private field site `site` read last
    /// under the home `environment` finds: a call memo's step (`MemoLeaf`), which holds while
    /// the memo's callee, and so its environment and home, does.
    pub fn site_private_field_entry(&self, site: usize, environment: Value) -> Option<FieldCache> {
        let (shape, slot) = self.private_site_homes.field_entry(site, environment)?;
        Some(FieldCache::own(shape, u16::try_from(slot).ok()?))
    }

    /// The private names `object` carries, each once, fields and methods alike.
    pub(super) fn private_names(&self, object: Value) -> Vec<PrivateBrand> {
        let Some(data) = self.heap.object(object) else {
            return Vec::new();
        };
        let mut brands = Vec::new();
        for key in self.shape_keys(data.shape()) {
            if let PropertyKey::Private(home, name) = key {
                let brand = PrivateBrand { home, name: name & !BRAND_ONLY };
                if !brands.contains(&brand) {
                    brands.push(brand);
                }
            }
        }
        brands
    }
}
