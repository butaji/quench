//! Readers of the settled storage facts a lowering reads at load: where a class's instances keep their fields (`ClassForm`),
//! which object holds a named field a read finds (`FieldHolder`), how a store of a field is kept (`StoreForm`) and the one
//! class a field holds (`hop_class`). Each reads the union layouts, the fixed cells, the instance and prototype layouts and
//! the closed-world `extends` chain; none runs an analysis of its own.

use super::AotFacts;
use crate::typed_ir::{Classes, Type};

/// Where the instances of a class keep their fields, as the heap places a class cell's words (`heap::spill`): in the cell's own
/// words, or past its spill slot (`AotFacts::spill_from`) in the spill run. A record's narrow run and its extras are record forms
/// the heap decides per object at run time (`value_vec::NARROW_FORM`, `EXTRAS_FORM`); a fixed cell has neither, so the closed
/// world never names them for a class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassForm {
    /// Fields past the cell's spill slot, held in the spill run the cell points to.
    Run,
    /// Held in the cell's own words, no spill.
    Own,
}

/// The object a named field is held by, for a read off an instance (`AotFacts::field_holder`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    /// The instance itself, its field in `class_layout`.
    Own,
    /// The prototype object `depth` links up the chain: 1 is the class's own prototype, 2 its parent's, and so on.
    Prototype(u32),
}

/// Where a named field is held: the holder, and the field's slot in it (its index in `class_layout` for `Holder::Own`, in
/// `prototype_layout` of the holder's class otherwise).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldHolder {
    pub holder: Holder,
    pub slot: u32,
}

/// How a store of a field into an instance is kept (`AotFacts::store_form`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreForm {
    /// The slot takes any value the field's domain allows, with no representation to retire.
    Own,
    /// The field's domain is narrower than Any: a store of a value outside it retires the store, as
    /// `Heap::tracks_slot_representations` retires an arena-wide store, here per class and field.
    Tracked,
}

impl AotFacts {
    /// The slot of named field `atom` in instances of `class` (its index in `class_layout`), where the layout is proven: a
    /// fixed offset a lowering reads in place of the field's name. `FunctionFacts::field_slot` answers through this.
    pub fn field_slot(&self, class: u32, atom: u32) -> Option<u16> {
        let slot = self.class_layout(class)?.iter().position(|&field| field == atom)?;
        u16::try_from(slot).ok()
    }

    /// The storage form of the instances of `class` (`ClassForm`), where the closed world fixes it: a fixed cell
    /// (`class_fixed_cell`), whose fields are its own words, and past its spill slot (`spill_from`) the spill run. The
    /// union's Inline or Flat split is a record's split (`INLINE_PROPERTY_COUNT`); a cell holds up to its region's
    /// capacity in its own words either way, so it does not decide the cell's form. None where the instances are not
    /// fixed cells: a record's form is decided per object at run time.
    /// Read by the own and local field reads (round2 sites 2, 3, 5, 11, 12, 13, 58, category A) and by `store_form`.
    pub fn class_form(&self, class: u32) -> Option<ClassForm> {
        if !self.class_fixed_cell(class) {
            return None;
        }
        Some(if self.spill_from(class).is_some() { ClassForm::Run } else { ClassForm::Own })
    }

    /// The holder and slot of named field `atom` read off an instance of `class` (`FieldHolder`): its own slot where
    /// `class_layout` holds it, else the first prototype up the closed-world `extends` chain (`parents`) whose
    /// `prototype_layout` names it. None unless the world is closed and the instance layout is proven (an own field of
    /// unknown presence cannot be ruled out). None too where some accessor defines the name (`accessor_atoms`): a getter
    /// is no prototype field, so the walk would find a holder past it.
    /// Read by the holder and inherited reads (round2 sites 43, 53, category A and E).
    pub fn field_holder(&self, class: u32, atom: u32) -> Option<FieldHolder> {
        if !self.closed() || self.class_layout(class).is_none() {
            return None;
        }
        if let Some(slot) = self.field_slot(class, atom) {
            return Some(FieldHolder { holder: Holder::Own, slot: u32::from(slot) });
        }
        if self.accessor_atoms.contains(&atom) {
            return None;
        }
        let mut current = class;
        for depth in 1..=self.parents.len() {
            let names = self.prototype_layout(current)?;
            if let Some(slot) = names.iter().position(|&name| name == atom) {
                return Some(FieldHolder { holder: Holder::Prototype(u32::try_from(depth).ok()?), slot: u32::try_from(slot).ok()? });
            }
            current = *self.parents.get(&current)?;
        }
        None
    }

    /// How a store of field `atom` into an instance of `class` is kept (`StoreForm`), where the store is no add
    /// (`field_always_stored`) into a fixed cell (narrow runs and extras are record forms, never a fixed cell's): Tracked where the field's domain
    /// (`slot_domain`) is narrower than Any, so a store outside it must retire; else Own. None where the store may add.
    /// Read by the store sites (round2 sites 15, 46 to 52, 61, category G).
    pub fn store_form(&self, class: u32, atom: u32) -> Option<StoreForm> {
        if !self.field_always_stored(class, atom) {
            return None;
        }
        let domain = self.slot_domain(class, atom)?;
        Some(if domain.ty == Type::ANY { StoreForm::Own } else { StoreForm::Tracked })
    }

    /// The one class a field `atom` of `class` holds, where the field's domain proves it: the `Classes::Set` of
    /// `slot_domain` with one proven member. None otherwise.
    /// Read by the nested reads, whose second hop takes its class here (round2 sites 14, 62, category B).
    pub fn hop_class(&self, class: u32, atom: u32) -> Option<u32> {
        match self.slot_domain(class, atom)?.classes {
            Classes::Set { ids, len: 1, proven: true } => Some(ids[0]),
            _ => None,
        }
    }
}
