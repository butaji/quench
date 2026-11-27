//! The views through which callers reach an ordinary object. `Heap::object`, `Heap::object_mut` and
//! `Heap::object_store_mut` return an `ObjectRef` or `ObjectMut`: either a live class cell (its class id in
//! the header, its fields inline: `heap/class_space.rs`) or a record (`&Object`, the record table).
//!
//! A cell answers every read and every class edit from the class table (`heap/layouts.rs`) and its fields.
//! Its class word is the header word of the equivalent `Object`, so each method answers as the record method
//! of the same name does. Anything that needs record data (extras, elements, arena runs, inline values) is
//! reached through `record_only` or `record_only_mut`, which are `None` for a cell: a cell has no such data.
//! A caller that must change record data departs the cell first (`Heap::object_record_mut`).

#![allow(dead_code)]

use super::Heap;
use super::cell::{Object, ProtoLink, retire_prototype_values};
use super::class_space::live_header;
use super::layouts::{auxiliary, class_flags, class_link, class_shape, intern_class, word};
use crate::staged::object_encoding::{FROZEN, NON_EXTENSIBLE, OBJECT_ARRAY};
use crate::value::Value;
use crate::value_vec::{CONSTANT_FORM, EXOTIC_ACCESS};

/// A class cell's view: its class id (the header word, whose high half is zero) and its fields, in
/// constructor order. The slack fields hold `Value::DELETED`.
#[derive(Clone, Copy)]
pub struct CellView<'a> {
    class: u32,
    fields: &'a [Value],
}

/// A class cell's exclusive view. Its header is written only by the class edits below, which intern the
/// new class word (`intern_class`), and its fields by `set_slot`.
pub struct CellViewMut<'a> {
    header: &'a mut u64,
    fields: &'a mut [Value],
}

/// A shared view of an ordinary object: a class cell, or a record.
#[derive(Clone, Copy)]
pub enum ObjectRef<'a> {
    Cell(CellView<'a>),
    Record(&'a Object),
}

/// An exclusive view of an ordinary object: a class cell, or a record.
pub enum ObjectMut<'a> {
    Cell(CellViewMut<'a>),
    Record(&'a mut Object),
}

/// The `aux` half of class `class`'s word: its shape and its aux flags (`CONSTANT_FORM`, `EXOTIC_ACCESS`,
/// `DICTIONARY_STORAGE`).
#[inline(always)]
fn class_aux(class: u32) -> u32 {
    auxiliary(class)
}

/// The `meta` half of class `class`'s word: its prototype link bits and its `OBJECT_ARRAY`, `NON_EXTENSIBLE`
/// and `FROZEN` flags.
#[inline(always)]
fn class_meta(class: u32) -> u32 {
    (word(class) >> u32::BITS) as u32
}

/// An `Object` meta flag in `CLASS_FLAG_MASK` position (the high half of a class's flags).
#[inline(always)]
const fn meta_flag(flag: u32) -> u64 {
    (flag as u64) << u32::BITS
}

impl<'a> CellView<'a> {
    /// The view of a cell of class `class` whose fields are `fields`.
    #[inline(always)]
    pub(super) fn new(class: u32, fields: &'a [Value]) -> Self {
        Self { class, fields }
    }

    /// The class id of the cell (`layout_id`).
    #[inline(always)]
    pub fn class(&self) -> u32 {
        self.class
    }

    /// The cell's field `slot`, in constructor order (`None` past the cell's capacity).
    #[inline(always)]
    pub fn field(&self, slot: usize) -> Option<Value> {
        self.fields.get(slot).copied()
    }

    #[inline(always)]
    pub fn layout_id(&self) -> u32 {
        self.class
    }

    #[inline(always)]
    pub fn hint_shape(&self) -> u32 {
        self.class
    }

    #[inline(always)]
    pub fn proto_link(&self) -> ProtoLink {
        class_link(self.class)
    }

    #[inline(always)]
    pub fn proto(&self) -> Value {
        class_link(self.class).prototype()
    }

    #[inline(always)]
    pub fn shape(&self) -> u32 {
        class_shape(self.class)
    }

    /// The shape with its exotic bit, without its constant mark (`Object::exact_shape`).
    #[inline(always)]
    pub fn exact_shape(&self) -> u32 {
        class_aux(self.class) & !CONSTANT_FORM
    }

    #[inline(always)]
    pub fn is_extensible(&self) -> bool {
        class_meta(self.class) & NON_EXTENSIBLE == 0
    }

    #[inline(always)]
    pub fn is_frozen(&self) -> bool {
        class_meta(self.class) & FROZEN != 0
    }

    /// A class cell is never a prototype: its first use as one departs it (`DictionaryTrigger::PrototypeUse`).
    #[inline(always)]
    pub fn was_used_as_prototype(&self) -> bool {
        false
    }

    #[inline(always)]
    pub fn has_exotic_access(&self) -> bool {
        class_aux(self.class) & EXOTIC_ACCESS != 0
    }

    #[inline(always)]
    pub fn holds_constant_mark(&self) -> bool {
        class_aux(self.class) & CONSTANT_FORM != 0
    }

    #[inline(always)]
    pub fn is_array(&self) -> bool {
        class_meta(self.class) & OBJECT_ARRAY != 0
    }

    /// A namespace's brand lives in the record's extras, so a cell is never one.
    #[inline(always)]
    pub fn is_module_namespace(&self) -> bool {
        false
    }

    /// A cell has no extras, so no watch subject.
    #[inline(always)]
    pub fn watched_subject(&self) -> Option<u32> {
        None
    }
}

impl<'a> CellViewMut<'a> {
    /// The exclusive view of a cell: its header word and its fields.
    #[inline(always)]
    pub(super) fn new(header: &'a mut u64, fields: &'a mut [Value]) -> Self {
        Self { header, fields }
    }

    #[inline(always)]
    fn class(&self) -> u32 {
        *self.header as u32
    }

    #[inline(always)]
    fn holds_constant_mark(&self) -> bool {
        class_aux(self.class()) & CONSTANT_FORM != 0
    }

    /// Field `slot` of the cell, for a store. The caller applies the write barrier (`Heap::object_store_mut`).
    #[inline(always)]
    pub fn set_slot(&mut self, slot: usize, value: Value) {
        self.fields[slot] = value;
    }

    /// The class edge: makes `class` the cell's class, in place (`live_header`).
    #[inline(always)]
    pub fn set_class(&mut self, class: u32) {
        *self.header = live_header(class);
    }

    /// Re-interns the cell's class with `set` flags added and `clear` flags removed (`CLASS_FLAG_MASK`
    /// positions), and writes it in place. The shape and prototype link are the cell's own.
    fn edit_flags(&mut self, set: u64, clear: u64) {
        let class = self.class();
        let flags = (class_flags(class) | set) & !clear;
        let next = intern_class(class_shape(class), class_link(class), flags);
        self.set_class(next);
    }

    fn set_meta_flag(&mut self, flag: u32, on: bool) {
        if on {
            self.edit_flags(meta_flag(flag), 0);
        } else {
            self.edit_flags(0, meta_flag(flag));
        }
    }

    /// `Object::set_proto` for a cell: the class with the new prototype link. A constant mark ends first.
    pub fn set_proto(&mut self, proto: Value) {
        self.note_constant_write();
        let class = self.class();
        let next = intern_class(class_shape(class), ProtoLink::of(proto), class_flags(class));
        self.set_class(next);
    }

    /// `Object::set_extensible` for a cell: the class with `NON_EXTENSIBLE` set or cleared.
    pub fn set_extensible(&mut self, value: bool) {
        self.set_meta_flag(NON_EXTENSIBLE, !value);
    }

    /// `Object::set_frozen` for a cell: the class with `FROZEN` set or cleared. Frozen elements retire the
    /// array proofs, as the record's do.
    pub fn set_frozen(&mut self, value: bool) {
        super::array_proofs::retire_array_storage_proofs();
        self.set_meta_flag(FROZEN, value);
    }

    /// `Object::mark_constant` for a cell: the class with `CONSTANT_FORM` set.
    pub fn mark_constant(&mut self) {
        self.edit_flags(CONSTANT_FORM as u64, 0);
    }

    /// `Object::note_constant_write` for a cell: a constant mark retires its readers and clears.
    pub fn note_constant_write(&mut self) {
        if self.holds_constant_mark() {
            retire_prototype_values(class_aux(self.class()) & !CONSTANT_FORM);
            self.edit_flags(0, CONSTANT_FORM as u64);
        }
    }
}

// Read methods of `ObjectRef`: each forwards to the `Object` or `CellView` method of the same name.
macro_rules! read_methods {
    ($($name:ident -> $ret:ty;)*) => {
        $(
            #[inline(always)]
            pub fn $name(&self) -> $ret {
                match self {
                    ObjectRef::Record(object) => object.$name(),
                    ObjectRef::Cell(cell) => cell.$name(),
                }
            }
        )*
    };
}

impl<'a> ObjectRef<'a> {
    /// The record of the object, for the operations a class cell cannot answer (extras, elements, arena runs,
    /// inline values, payload words). `None` for a cell, which has none of that data.
    #[inline(always)]
    pub fn record_only(&self) -> Option<&'a Object> {
        match self {
            ObjectRef::Record(object) => Some(*object),
            ObjectRef::Cell(_) => None,
        }
    }

    read_methods! {
        layout_id -> u32;
        proto_link -> ProtoLink;
        proto -> Value;
        shape -> u32;
        exact_shape -> u32;
        hint_shape -> u32;
        is_extensible -> bool;
        is_frozen -> bool;
        was_used_as_prototype -> bool;
        has_exotic_access -> bool;
        holds_constant_mark -> bool;
        is_array -> bool;
        is_module_namespace -> bool;
        watched_subject -> Option<u32>;
    }
}

impl<'a> ObjectMut<'a> {
    /// The shared view of the same object.
    #[inline(always)]
    pub fn as_shared(&self) -> ObjectRef<'_> {
        match self {
            ObjectMut::Record(object) => ObjectRef::Record(&**object),
            ObjectMut::Cell(cell) => ObjectRef::Cell(CellView { class: cell.class(), fields: &*cell.fields }),
        }
    }

    /// The record of the object, consuming the view so the record outlives it, for record data. `None` for a
    /// cell. A caller that must change record data takes it through `Heap::object_record_mut`, which departs
    /// a cell first.
    #[inline(always)]
    pub fn record_only_mut(self) -> Option<&'a mut Object> {
        match self {
            ObjectMut::Record(object) => Some(object),
            ObjectMut::Cell(_) => None,
        }
    }

    // Reads share `ObjectRef`'s methods through `as_shared`.
    #[inline(always)]
    pub fn layout_id(&self) -> u32 {
        self.as_shared().layout_id()
    }
    #[inline(always)]
    pub fn proto_link(&self) -> ProtoLink {
        self.as_shared().proto_link()
    }
    #[inline(always)]
    pub fn proto(&self) -> Value {
        self.as_shared().proto()
    }
    #[inline(always)]
    pub fn shape(&self) -> u32 {
        self.as_shared().shape()
    }
    #[inline(always)]
    pub fn exact_shape(&self) -> u32 {
        self.as_shared().exact_shape()
    }
    #[inline(always)]
    pub fn hint_shape(&self) -> u32 {
        self.as_shared().hint_shape()
    }
    #[inline(always)]
    pub fn is_extensible(&self) -> bool {
        self.as_shared().is_extensible()
    }
    #[inline(always)]
    pub fn is_frozen(&self) -> bool {
        self.as_shared().is_frozen()
    }
    #[inline(always)]
    pub fn was_used_as_prototype(&self) -> bool {
        self.as_shared().was_used_as_prototype()
    }
    #[inline(always)]
    pub fn has_exotic_access(&self) -> bool {
        self.as_shared().has_exotic_access()
    }
    #[inline(always)]
    pub fn holds_constant_mark(&self) -> bool {
        self.as_shared().holds_constant_mark()
    }
    #[inline(always)]
    pub fn is_array(&self) -> bool {
        self.as_shared().is_array()
    }
    #[inline(always)]
    pub fn watched_subject(&self) -> Option<u32> {
        self.as_shared().watched_subject()
    }
    #[inline(always)]
    pub fn is_module_namespace(&self) -> bool {
        self.as_shared().is_module_namespace()
    }

    // Class edits: in place on a cell (`intern_class` with the new flags or link), on a record through the record.

    /// Makes `class` (a class id) the object's class.
    #[inline(always)]
    pub fn set_class(&mut self, class: u32) {
        match self {
            ObjectMut::Record(object) => object.set_class(class),
            ObjectMut::Cell(cell) => cell.set_class(class),
        }
    }

    #[inline(always)]
    pub fn set_proto(&mut self, proto: Value) {
        match self {
            ObjectMut::Record(object) => object.set_proto(proto),
            ObjectMut::Cell(cell) => cell.set_proto(proto),
        }
    }

    #[inline(always)]
    pub fn set_extensible(&mut self, value: bool) {
        match self {
            ObjectMut::Record(object) => object.set_extensible(value),
            ObjectMut::Cell(cell) => cell.set_extensible(value),
        }
    }

    #[inline(always)]
    pub fn set_frozen(&mut self, value: bool) {
        match self {
            ObjectMut::Record(object) => object.set_frozen(value),
            ObjectMut::Cell(cell) => cell.set_frozen(value),
        }
    }

    #[inline(always)]
    pub fn mark_constant(&mut self) {
        match self {
            ObjectMut::Record(object) => object.mark_constant(),
            ObjectMut::Cell(cell) => cell.mark_constant(),
        }
    }

    #[inline(always)]
    pub fn note_constant_write(&mut self) {
        match self {
            ObjectMut::Record(object) => object.note_constant_write(),
            ObjectMut::Cell(cell) => cell.note_constant_write(),
        }
    }
}

impl Heap {
    /// `slot` of the object `object` views, as `property_get` reads it: a cell's field inside its shape, or a
    /// record's value (inline or in the arena).
    #[inline(always)]
    pub fn property_get_ref(&self, object: ObjectRef<'_>, slot: usize) -> Option<Value> {
        match object {
            ObjectRef::Record(record) => self.property_get(record, slot),
            ObjectRef::Cell(cell) => {
                if slot >= self.properties.shape_length(cell.shape()) {
                    return None;
                }
                cell.field(slot).filter(|value| !value.is_deleted())
            }
        }
    }
}
