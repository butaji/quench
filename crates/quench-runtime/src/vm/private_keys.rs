//! Private fields as ordinary shape slots.
//!
//! A private field `#x` of a class evaluation (its home) is an own data property of the
//! object, keyed in the object's shape by a private atom (`AtomClass::PRIVATE`, hidden from
//! every key enumeration as an internal name). Its definition, reads and writes are the shape
//! transitions, slot loads and slot stores of any other property, so the field caches answer
//! them, and the brand check is a shape check: an object of a shape holding the key carries
//! the field.
//!
//! The key of a declared name follows its home (`Vm::private_field_key`). While one class
//! evaluation alone has declared the name (`PrivateDecl::Single`), that home's key is the
//! declared atom itself, which every access site names: no other code can name the atom, so a
//! site's field cache entry, which records a receiver shape, proves the brand without
//! resolving the home. A second evaluation of the same class (`PrivateDecl::Multi`) gives each
//! later home a fresh key, retires every field cache entry, and from then on its sites record
//! none: each access resolves its home first.

use super::{Atom, Host, ResidualProgram, Value, Vm};
use rustc_hash::FxHashMap;

/// Who declared a private field name (`Vm::private_field_key`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PrivateDecl {
    /// One home alone declared it: its key is the name's own atom.
    Single(Value),
    /// More than one home did; each home's key is in `PrivateKeys::keys`.
    Multi,
}

/// The private-name state the VM keeps: each frame's resolved home (`Vm::private_name_home`,
/// which this derefs to) and the declared fields' keys.
#[derive(Default)]
pub(super) struct PrivateHomes {
    resolved: FxHashMap<super::PrivateHomeKey, Value>,
    pub(super) fields: PrivateKeys,
}

impl std::ops::Deref for PrivateHomes {
    type Target = FxHashMap<super::PrivateHomeKey, Value>;
    fn deref(&self) -> &Self::Target { &self.resolved }
}

impl std::ops::DerefMut for PrivateHomes {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.resolved }
}

impl PrivateHomes {
    pub(super) fn clear(&mut self) {
        self.resolved.clear();
        self.fields.clear();
    }

    /// Drops what a collection freed: a resolved home of a swept environment, before its slot
    /// can name a new cell, and the fields' dead homes (`PrivateKeys::prune_dead`).
    pub(super) fn retain_live(&mut self, live: impl Fn(Value) -> bool) {
        self.resolved.retain(|key, _| live(key.environment));
        self.fields.prune_dead(live);
    }
}

impl crate::snapshot::Snap for PrivateHomes {
    fn snap(&self, writer: &mut crate::snapshot::Writer) -> crate::snapshot::SnapshotResult<()> {
        self.resolved.snap(writer)?;
        self.fields.snap(writer)
    }
    fn unsnap(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        Ok(Self { resolved: crate::snapshot::Snap::unsnap(reader)?, fields: crate::snapshot::Snap::unsnap(reader)? })
    }
}

#[derive(Default)]
pub(super) struct PrivateKeys {
    /// Each atom's declaration, indexed by atom: `UNDECLARED`, `MULTI`, or one more than the
    /// index of its home in `singles`, with `METHOD` for a private method's brand.
    decls: Vec<u32>,
    /// The `(name, home)` of each name one home declared; a name that turned `Multi` (once,
    /// for good) leaves its entry, which `decls` no longer indexes.
    singles: Vec<(Atom, Value)>,
    /// The key of each home of a `Multi` name, by home and name.
    pub(super) keys: FxHashMap<(Value, Atom), Atom>,
    /// Fresh keys made so far, numbering the next one.
    fresh: u32,
    /// Whether a home died since the last declaration (`prune_dead`): the field caches go
    /// before any later home's code runs (`Vm::declare_private_field`).
    flush: bool,
}

const UNDECLARED: u32 = 0;
const MULTI: u32 = u32::MAX >> 1;
/// The flag of a declared private method or accessor name: its key marks the brand of an
/// object its class constructed, a slot no read of the name returns.
const METHOD: u32 = !MULTI;

impl PrivateKeys {
    pub(super) fn clear(&mut self) {
        self.decls.clear();
        self.singles.clear();
        self.keys.clear();
    }

    fn set(&mut self, atom: Atom, decl: PrivateDecl) {
        let index = atom as usize;
        if self.decls.len() <= index {
            self.decls.resize(index + 1, UNDECLARED);
        }
        self.decls[index] = (self.decls[index] & METHOD) | match decl {
            PrivateDecl::Single(home) => {
                self.singles.push((atom, home));
                self.singles.len() as u32
            }
            PrivateDecl::Multi => MULTI,
        };
    }

    /// Who declared `atom`; `None` for a name no class declared as a field.
    #[inline(always)]
    pub(super) fn decl(&self, atom: Atom) -> Option<PrivateDecl> {
        match self.decls.get(atom as usize).copied().unwrap_or(UNDECLARED) & !METHOD {
            UNDECLARED => None,
            MULTI => Some(PrivateDecl::Multi),
            single => Some(PrivateDecl::Single(self.singles[single as usize - 1].1)),
        }
    }

    /// Every declared name with its declaration.
    pub(super) fn each_decl(&self) -> impl Iterator<Item = (Atom, PrivateDecl)> + '_ {
        (0..self.decls.len() as Atom).filter_map(|atom| Some((atom, self.decl(atom)?)))
    }

    /// Whether `atom` names a declared private field that only one home declared, whose
    /// key is `atom` itself.
    #[inline(always)]
    pub(super) fn single(&self, atom: Atom) -> bool {
        self.decls.get(atom as usize).is_some_and(|decl| !matches!(*decl, UNDECLARED | MULTI) && *decl & METHOD == 0)
    }

    /// Whether `atom` names a declared private field.
    #[inline(always)]
    pub(super) fn declared(&self, atom: Atom) -> bool {
        self.decls.get(atom as usize).is_some_and(|decl| *decl != UNDECLARED && *decl & METHOD == 0)
    }

    /// Whether `atom` names a declared private method or accessor, whose key is a brand.
    #[inline(always)]
    pub(super) fn method(&self, atom: Atom) -> bool {
        self.decls.get(atom as usize).is_some_and(|decl| *decl & METHOD != 0)
    }

    /// Whether one home alone declared `atom`, a field or a method.
    fn keyed(&self, atom: Atom) -> bool {
        self.decls.get(atom as usize).is_some_and(|decl| !matches!(*decl & !METHOD, UNDECLARED | MULTI))
    }

    /// Marks `atom`, before its first declaration, as a private method or accessor name.
    fn declare_method(&mut self, atom: Atom) {
        let index = atom as usize;
        if self.decls.len() <= index {
            self.decls.resize(index + 1, UNDECLARED);
        }
        self.decls[index] |= METHOD;
    }

    /// Drops what a collection freed: a `Multi` name's key of a dead home, and a dead
    /// `Single` home, whose name turns `Multi` so that no later home, which may be a new cell
    /// at the dead one's address, takes the name's own atom as its key. Whether a name turned
    /// `Multi`, which retires the field caches.
    pub(super) fn prune_dead(&mut self, live: impl Fn(Value) -> bool) {
        self.keys.retain(|(home, _), _| live(*home));
        let dead: Vec<Atom> = self.singles.iter()
            .filter(|(atom, home)| self.keyed(*atom) && !live(*home))
            .map(|(atom, _)| *atom)
            .collect();
        for atom in &dead {
            self.set(*atom, PrivateDecl::Multi);
        }
        self.flush |= !dead.is_empty();
    }
}

impl<H: Host> Vm<H> {
    /// `DeclarePrivateName`: the class evaluation whose home is `home` declares private field
    /// name `atom` (or, with `method`, a private method or accessor name an instance carries
    /// as its brand), before any of its code can access it.
    pub(super) fn declare_private_field(&mut self, home: Value, atom: Atom, method: bool) {
        if std::mem::take(&mut self.private_homes.fields.flush) {
            self.invalidate_field_caches();
        }
        match self.private_homes.fields.decl(atom) {
            // A method's brand takes a key of its own: the name's atom keys the method its
            // home holds, which an own slot of that key would shadow.
            None if method => {
                self.private_homes.fields.declare_method(atom);
                self.private_homes.fields.set(atom, PrivateDecl::Single(home));
                let key = self.fresh_private_key(atom);
                self.private_homes.fields.keys.insert((home, atom), key);
            }
            None => self.private_homes.fields.set(atom, PrivateDecl::Single(home)),
            Some(PrivateDecl::Single(first)) if first == home => {}
            Some(PrivateDecl::Single(first)) => {
                // The first home keeps the name's own atom, which its objects hold.
                self.private_homes.fields.set(atom, PrivateDecl::Multi);
                self.private_homes.fields.keys.entry((first, atom)).or_insert(atom);
                let key = self.fresh_private_key(atom);
                self.private_homes.fields.keys.insert((home, atom), key);
                // Entries recorded while the name had one home prove no brand now.
                self.invalidate_field_caches();
            }
            Some(PrivateDecl::Multi) => {
                if !self.private_homes.fields.keys.contains_key(&(home, atom)) {
                    let key = self.fresh_private_key(atom);
                    self.private_homes.fields.keys.insert((home, atom), key);
                }
            }
        }
    }

    fn fresh_private_key(&mut self, atom: Atom) -> Atom {
        self.private_homes.fields.fresh += 1;
        let name = format!("{}\u{0}{}", self.atom_name(atom), self.private_homes.fields.fresh);
        self.intern_atom(&name)
    }

    /// The shape key of private field name `atom` under `home`; `None` when `atom` names no
    /// declared field (a private method or accessor) or `home` never declared it.
    #[inline(always)]
    pub(super) fn private_field_key_of(&self, home: Value, atom: Atom) -> Option<Atom> {
        (!self.private_homes.fields.method(atom)).then(|| self.private_brand_key_of(home, atom))?
    }

    /// The shape key of declared private name `atom` under `home`, a field's or a method's
    /// brand (`PrivateKeys::method`), which `PrivateKeys::keys` holds for every home.
    #[inline(always)]
    pub(super) fn private_brand_key_of(&self, home: Value, atom: Atom) -> Option<Atom> {
        if self.private_homes.fields.method(atom) {
            return self.private_homes.fields.keys.get(&(home, atom)).copied();
        }
        match self.private_homes.fields.decl(atom)? {
            PrivateDecl::Single(first) => (first == home).then_some(atom),
            PrivateDecl::Multi => self.private_homes.fields.keys.get(&(home, atom)).copied(),
        }
    }

    /// The shape key of private field name `atom` as the running frame names it, and whether
    /// field caches may record it (the name's own atom); `None` for a private method or
    /// accessor name, or where the frame finds no home.
    #[inline(always)]
    pub(super) fn private_field_key(&mut self, p: &ResidualProgram, atom: Atom) -> Option<(Atom, bool)> {
        if self.private_homes.fields.single(atom) {
            return Some((atom, true));
        }
        if !self.private_homes.fields.declared(atom) {
            return None;
        }
        match self.private_homes.fields.decl(atom)? {
            PrivateDecl::Single(_) => Some((atom, true)),
            PrivateDecl::Multi => {
                let home = self.private_name_home(p, atom)?;
                Some((self.private_homes.fields.keys.get(&(home, atom)).copied()?, false))
            }
        }
    }

    /// The slot of `object`'s own private field keyed `key`.
    #[inline(always)]
    pub(super) fn private_field_slot(&self, object: Value, key: Atom) -> Option<usize> {
        let data = self.object_data(object)?;
        self.shape_slot(data.shape(), key)
    }

    /// The value of `object`'s own private field keyed `key`.
    #[inline(always)]
    pub(super) fn private_field_at(&self, object: Value, key: Atom) -> Option<Value> {
        let data = self.object_data(object)?;
        let slot = self.shape_slot(data.shape(), key)?;
        self.heap.property_get(data, slot)
    }

    /// A read of private field `atom` of `object` at field cache `site`: the field's value,
    /// with an own entry recorded for the site where the key is the name's own atom. `None`
    /// for a private method or accessor name, or an object without the field, which the
    /// general path answers or throws for.
    pub(super) fn private_field_read(&mut self, p: &ResidualProgram, object: Value, atom: Atom, site: Option<u16>) -> Option<Value> {
        let (key, records) = self.private_field_key(p, atom)?;
        let data = self.object_data(object)?;
        let shape = data.shape();
        let slot = self.shape_slot(shape, key)?;
        let value = self.heap.property_get(data, slot)?;
        if records && let Some(site) = site {
            self.record_private_field_entry(object, shape, slot, site);
        }
        Some(value)
    }

    /// A write of private field `atom` of `object` at field cache `site`; `false` for a
    /// private method or accessor name, or an object without the field.
    pub(super) fn private_field_write(&mut self, p: &ResidualProgram, object: Value, atom: Atom, value: Value, site: Option<u16>) -> bool {
        let Some((key, records)) = self.private_field_key(p, atom) else { return false };
        let Some(data) = self.object_data(object) else { return false };
        let shape = data.shape();
        let Some(slot) = self.shape_slot(shape, key) else { return false };
        self.heap.property_set(object, slot, value);
        if records && let Some(site) = site {
            self.record_private_field_entry(object, shape, slot, site);
        }
        true
    }

    /// Records the own entry of a private field at `slot` of `shape` for `site`, where its
    /// receiver is an ordinary object of exactly that shape, as a public field's would be.
    fn record_private_field_entry(&mut self, object: Value, shape: u32, slot: usize, site: u16) {
        if !self.specialized {
            return;
        }
        let Some(data) = self.heap.ordinary_object(object) else { return };
        if data.exact_shape() != shape
            || self.shape_is_dictionary(shape)
            || slot >= super::object::FIELD_CACHE_SLOT_CAPACITY
            || self.slot_attributes(data, slot).is_none_or(|attributes| attributes.accessor || !attributes.writable)
        {
            return;
        }
        let index = self.field_cache_index(site);
        self.record_field_cache(index, super::FieldCache { receiver: shape, holder: super::NO_FIELD_HOLDER, slot: slot as u16 });
    }

    /// `DefinePrivateField`: PrivateFieldAdd of `atom` under `home` to `object`. `None` where
    /// `home` declared no field `atom`, which the caller adds as before.
    pub(super) fn private_field_add(&mut self, p: &ResidualProgram, object: Value, home: Value, atom: Atom, value: Value) -> Option<Result<(), crate::JsError>> {
        let key = self.private_field_key_of(home, atom)?;
        Some(self.private_field_add_key(p, object, atom, key, value))
    }

    /// PrivateFieldAdd of name `atom` keyed `key`: an ordinary data property added through the
    /// shape transition any property add takes, whatever the object's extensibility.
    pub(super) fn private_field_add_key(&mut self, p: &ResidualProgram, object: Value, atom: Atom, key: Atom, value: Value) -> Result<(), crate::JsError> {
        let Some(addable) = self.object_data(object).map(|data| self.private_element_addable(data)) else {
            return Err(self.type_error(p, "Cannot add private field to a non-object".into()));
        };
        if self.private_field_slot(object, key).is_some() {
            let name = crate::bytecode::private_identity_label(self.atom_name(atom)).unwrap_or_else(|| self.atom_name(atom)).to_owned();
            return Err(self.type_error(p, format!("Cannot initialize #{name} twice on the same object")));
        }
        if !addable {
            return Err(self.type_error(p, "Cannot add private field to a non-extensible object".into()));
        }
        let shape = self.object_data(object).expect("checked object").shape();
        let next = self.transition_data_property_shape(shape, super::property_key::PropertyKey::string(key), super::DEFAULT_DATA_ATTRIBUTES);
        self.heap.property_add(object, value, next);
        Ok(())
    }

    /// `DefineField` of a private name: PrivateFieldAdd under the running frame's home, through
    /// the site's add cache where the key is the name's own atom, as a public field's define.
    pub(super) fn private_field_define(&mut self, p: &ResidualProgram, object: Value, atom: Atom, value: Value, site: u16) -> Result<(), crate::JsError> {
        let Some((key, records)) = self.private_field_key(p, atom) else {
            return match self.private_name_home(p, atom) {
                Some(home) => self.initialize_private_field(p, object, home, atom, value),
                None => Err(self.type_error(p, "Cannot add private field outside its class".into())),
            };
        };
        let shape = (records && self.specialized && p.specialized).then(|| self.class_field_define_target(object)).flatten();
        let Some(shape) = shape else {
            return self.private_field_add_key(p, object, atom, key, value);
        };
        let index = self.field_cache_index(site);
        if let Some((_, cache)) = self.field_add_entry(index, shape, atom) {
            let (target_shape, slot) = (cache.target_shape, cache.slot);
            self.apply_cached_field_add(object, atom, shape, target_shape, slot, value);
            return Ok(());
        }
        self.private_field_add_key(p, object, atom, key, value)?;
        let target_shape = self.object_data(object).map_or(shape, |data| data.shape());
        if !self.shape_is_dictionary(shape)
            && !self.shape_is_dictionary(target_shape)
            && let Some(slot) = self.shape_slot(target_shape, key)
            && slot == self.shapes[shape as usize].storage_len()
            && let Ok(slot) = u16::try_from(slot)
        {
            self.record_field_add_cache(index, super::FieldAddCache { atom, source_shape: shape, target_shape, slot, ..super::FieldAddCache::EMPTY });
        }
        Ok(())
    }
}

impl crate::snapshot::Snap for PrivateKeys {
    fn snap(&self, writer: &mut crate::snapshot::Writer) -> crate::snapshot::SnapshotResult<()> {
        let decls: Vec<(Atom, Option<Value>, bool)> = self.each_decl()
            .map(|(atom, decl)| (atom, match decl { PrivateDecl::Single(home) => Some(home), PrivateDecl::Multi => None }, self.method(atom)))
            .collect();
        decls.snap(writer)?;
        self.keys.snap(writer)?;
        self.fresh.snap(writer)
    }
    fn unsnap(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        let mut keys = Self::default();
        for (atom, home, method) in Vec::<(Atom, Option<Value>, bool)>::unsnap(reader)? {
            if method {
                keys.declare_method(atom);
            }
            keys.set(atom, home.map_or(PrivateDecl::Multi, PrivateDecl::Single));
        }
        keys.keys = crate::snapshot::Snap::unsnap(reader)?;
        keys.flush = true;
        keys.fresh = crate::snapshot::Snap::unsnap(reader)?;
        Ok(keys)
    }
}
