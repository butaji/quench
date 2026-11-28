//! The proved plans a field store runs: its store arms, join and plan sources.

use super::*;
use super::super::field_cache_layout::{is_class_key, CLASS_KEY};

/// One own data slot a store site proved: objects of exact `shape` keep the field at `slot`.
/// `holds` is `None` where the value stored is of a type the slot's type holds. Otherwise it
/// is the type the row tests the value for before it stores in place, so the slot's
/// representation never widens under the in-place write; a value of any other type stores through
/// the VM, which joins the slot's type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StoreArm {
    pub shape: u32,
    /// The slot word: a field index of an own shape, or a class arm's `spilled_slot` (`class_slot_address`).
    pub slot: u32,
    pub holds: Option<Type>,
    /// The slot's type (`slot_ty` of `StoreArm::of`): a Number slot's words are raw doubles, so a
    /// store writes `slot_store::class_word(class, value)` whether or not `holds` tests the value.
    pub class: Type,
    /// Whether `shape` is an ahead image's field guard key (`ImageWord::Layout`): the arm guards the live
    /// cell by the class the loader resolves for that key, and stores the field of that cell
    /// (`property_write::image_arm_store`). Its `shape` is the key without `ic::SYMBOLIC_SHAPE`.
    pub image: bool,
}

impl StoreArm {
    /// The arm for the slot `slot` of objects of `shape`, whose type is `slot_ty`, under a
    /// value of type `fact`. A slot that holds the value's type stores it as is. Otherwise
    /// the arm admits an int32, or for a slot that holds any direct object, that type; `None`
    /// where the slot's type holds no such value, so every store of it goes through the VM.
    pub fn of((shape, slot): (u32, u16), slot_ty: Type, fact: Type) -> Option<Self> {
        let slot = u32::from(slot);
        if fact.is_subtype(slot_ty) { return Some(Self { shape, slot, holds: None, class: slot_ty, image: false }); }
        let holds = if slot_ty.direct_object_part() == Some(Type::DIRECT_OBJECT_FAMILY) { slot_ty } else { Type::INT32 };
        holds.is_subtype(slot_ty).then_some(Self { shape, slot, holds: Some(holds), class: slot_ty, image: false })
    }

    /// The inline record slot of a shape arm: a shape's field index, which fits a record's `u16` (`StoreArm::of`).
    pub fn record_slot(self) -> u16 {
        debug_assert!(!self.keys_class() && self.slot <= u32::from(u16::MAX), "a shape arm names an inline field");
        self.slot as u16
    }

    /// The arm of an ahead image's construction: `key` is its field guard key (`ImageWord::Layout`), and `slot` a field
    /// of the live class cell the loader's class names. Like `of_class`, it takes no slot type: the cell's gate admits
    /// or refuses a tracked slot.
    pub fn of_image(key: u32, slot: u32) -> Self {
        Self { shape: key, slot, holds: None, class: Type::ANY, image: true }
    }

    /// The arm of a class-keyed receiver: `key` is the entry's key (`class | CLASS_KEY`), and `slot` a field of the
    /// live class cell. It takes no slot type: the cell's store gate admits or refuses a tracked slot (`class_arm_store`).
    pub fn of_class(key: u32, slot: u32) -> Self {
        Self { shape: key, slot, holds: None, class: Type::ANY, image: false }
    }

    /// Whether the arm's key names a live class cell, not a shape (`field_cache_layout::is_class_key`).
    pub fn keys_class(self) -> bool {
        is_class_key(self.shape)
    }

    /// The class id a class-keyed arm names: its key without `CLASS_KEY` (`StoreArm::of_class`).
    pub fn class_id(self) -> u32 {
        self.shape & !CLASS_KEY
    }
}

/// How the VM completes a named store that an in-place arm does not: where no arm holds the
/// receiver's shape, or its value, its barrier or its storage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreJoin {
    /// The VM's own-field store hit, which a failure leaves at the miss edge.
    Host,
    /// The same hit through the site's bound slot first, which the frame addressed before the
    /// run (`PropertyFrame::field_store_bound`).
    Bound,
    /// The VM's adding entries, which complete a store of any shape without leaving; a frame
    /// that uses it holds no object proof past the store.
    Add,
}

/// How a named store site writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorePlan<'a> {
    /// The first of `arms` whose exact shape the receiver has stores in place where the
    /// value, the barrier and the storage admit it; every other store runs through `join`.
    Own { arms: &'a [StoreArm], join: StoreJoin },
    /// A receiver whose layout id is `id` (a site's layout hint, `Object::hint_shape`) stores
    /// at `slot` in place: one compare of the record's own word proves its shape, form and
    /// flags, and the form the id's entry names picks the store with no test
    /// (`store::own_store`, hinted). Every other store runs through `join`. The generated
    /// own-store rows build it (`RecordPlan::own_store`).
    Layout { id: u32, slot: u16, join: StoreJoin },
    /// The slot a loop's entry projected for this store, which the frame addresses
    /// (`PropertyFrame::entry_slot_store`).
    Entry,
    /// A store that may add the property or change the shape: the VM's adding entry.
    Add,
    /// A setter, a read-only or dictionary property, a proxy: the VM's store.
    Host,
    /// The VM's store through the slot the entry bound for the site.
    Bound,
    /// A store the record's own hint (`id`, `slot`) proves for a tracked or narrow store
    /// (`narrow`), which `PropertyFrame::field_store_hinted` makes in place; a refusal leaves to
    /// `join` (the VM's own-field store).
    Hinted { id: u32, slot: u16, narrow: bool, join: StoreJoin },
    /// A store the record's own hint, the site's polymorphic entries or the proved add completes
    /// (`PropertyFrame::field_store_polymorphic`); a refusal leaves at the miss edge.
    Polymorphic { id: u32, slot: u16 },
}

impl<'a> StorePlan<'a> {
    /// The plan that stores in place by `arms` and otherwise by `join`; by `join` alone with
    /// no arm.
    pub fn of(arms: &'a [StoreArm], join: StoreJoin) -> Self {
        match (arms.is_empty(), join) {
            (false, join) => Self::Own { arms, join },
            (true, StoreJoin::Host) => Self::Host,
            (true, StoreJoin::Bound) => Self::Bound,
            (true, StoreJoin::Add) => Self::Add,
        }
    }
}

/// The plans a frame's tier proved, by site. A frame with none answers `None`, and the
/// operation is left to the host path.
pub trait PlanSource {
    /// The read plan of `lookup`.
    fn field_plan(&self, lookup: FieldLookup) -> Option<FieldPlan<'_>>;
    /// The store plan of cache site `site`.
    fn store_plan(&self, site: u16) -> Option<StorePlan<'_>>;
    /// The two own hops `(guard, slot, form)` of a nested read `a.b.c` the source holds as data (a
    /// lane record's hint and its site's second entry): the guard an exact shape's compare
    /// (`IcOp::GuardShape`), or a layout id's that proves the form too (`IcOp::GuardFormed`); each form
    /// the storage form its site fixed (`Form::Either` where it fixed none); `None` where it holds none.
    #[inline(always)]
    fn nested_hops(&self) -> Option<[(IcOp<'static>, u16, Form); 2]> { None }
}

impl<P: PlanSource> PlanSource for &P {
    fn field_plan(&self, lookup: FieldLookup) -> Option<FieldPlan<'_>> { (**self).field_plan(lookup) }
    fn store_plan(&self, site: u16) -> Option<StorePlan<'_>> { (**self).store_plan(site) }
    #[inline(always)]
    fn nested_hops(&self) -> Option<[(IcOp<'static>, u16, Form); 2]> { (**self).nested_hops() }
}

/// The plans of a frame that proved none.
#[derive(Clone, Copy, Default)]
pub struct NoPlans;

impl PlanSource for NoPlans {
    fn field_plan(&self, _lookup: FieldLookup) -> Option<FieldPlan<'_>> { None }
    fn store_plan(&self, _site: u16) -> Option<StorePlan<'_>> { None }
}
