//! The views through which callers reach an ordinary object. `Heap::object` and `Heap::object_mut`
//! return an `ObjectRef` or `ObjectMut`: either a class cell (`heap/class_space.rs`, fixed class, fields
//! inline) or a record (`&Object`, the record table, the form every object has today).
//!
//! The read and mutation methods here are the ones callers use on an object: class, shape, prototype and
//! flags. Each forwards to the `Object` method of the same name, so a caller keeps its method calls on the view. Anything else (extras, elements,
//! arena runs, inline values, payload words) is reached through `record()`: on a cell that takes the
//! departure edge (R4-C4), so the record view is always the full object.
//!
//! No class cell is allocated yet (R4-C2), so every `Cell` arm is unreachable for now.

#![allow(dead_code)]

use super::cell::{Object, ProtoLink};
use super::Heap;
use crate::value::Value;

/// A class cell's view: its header word (the class id in the low half, zero above) and its fields, in
/// constructor order, the slack fields holding `Value::DELETED`.
#[derive(Clone, Copy)]
pub struct CellView<'a> {
    header: &'a u64,
    fields: &'a [Value],
}

/// A class cell's mutable view. Its header is written only through the class edges (R4-C4).
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

// Read methods of `ObjectRef`: each forwards to the `Object` method of the same name.
macro_rules! read_methods {
    ($($name:ident -> $ret:ty;)*) => {
        $(
            #[inline(always)]
            pub fn $name(&self) -> $ret {
                match self {
                    ObjectRef::Record(object) => object.$name(),
                    ObjectRef::Cell(_) => unreachable!("R4-C2: no class cells are allocated yet"),
                }
            }
        )*
    };
}

impl<'a> ObjectRef<'a> {
    /// The record view of the object, for every operation a class cell cannot answer (extras, elements,
    /// arena runs, inline values, payload words). On a cell this will take the departure edge first
    /// (`Heap::depart_class_object`, R4-C4), so the returned record is the whole object.
    #[inline(always)]
    pub fn record(&self) -> &'a Object {
        match self {
            ObjectRef::Record(object) => *object,
            ObjectRef::Cell(_) => unreachable!("R4-C2: no class cells are allocated yet"),
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
            ObjectMut::Cell(_) => unreachable!("a class cell departs before its mutable view"),
        }
    }

    /// The record view of the object, for every operation a class cell cannot answer. On a cell this will
    /// take the departure edge first (`Heap::depart_class_object`, R4-C4).
    #[inline(always)]
    pub fn record(&mut self) -> &mut Object {
        match self {
            ObjectMut::Record(object) => &mut **object,
            ObjectMut::Cell(_) => unreachable!("a class cell departs before its mutable view"),
        }
    }

    /// The record view of the object for a mutation that needs record data (extras, arena runs, record
    /// flags). R4-C4 departs a cell first: on a cell this takes the departure edge, so the record is the whole object.
    #[inline(always)]
    pub fn record_mut_departing(&mut self) -> &mut Object {
        match self {
            ObjectMut::Record(object) => &mut **object,
            ObjectMut::Cell(_) => unreachable!("R4-C2: no class cells are allocated yet"),
        }
    }

    /// The record view for a mutation of record-only state, consuming the view so the record outlives it.
    /// R4-C4 departs a cell first; until then this is the Record arm only.
    #[inline(always)]
    pub fn record_mut_departing(self) -> &'a mut Object {
        match self {
            ObjectMut::Record(object) => object,
            ObjectMut::Cell(_) => unreachable!("R4-C2: no class cells are allocated yet"),
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

    /// The class edge (R4-C4): makes `class` the object's class, its header word. A cell's header
    /// holds the class id (`live_header`); a record takes it through `Object::set_class`. The caller
    /// retires the readers of the old class (`Heap::set_class`).
    #[inline(always)]
    pub fn set_class(&mut self, class: u32) {
        match self {
            ObjectMut::Record(object) => object.set_class(class),
            ObjectMut::Cell(cell) => *cell.header = super::class_space::live_header(class),
        }
    }

    // Mutations. A class cell departs before its mutable view is made (`Heap::object_mut`), so
    // these reach records only; the cell arms are unreachable by construction.

    #[inline(always)]
    pub fn set_proto(&mut self, proto: Value) {
        match self {
            ObjectMut::Record(object) => object.set_proto(proto),
            ObjectMut::Cell(_) => unreachable!("a class cell departs before its mutable view"),
        }
    }

    #[inline(always)]
    pub fn set_extensible(&mut self, value: bool) {
        match self {
            ObjectMut::Record(object) => object.set_extensible(value),
            ObjectMut::Cell(_) => unreachable!("a class cell departs before its mutable view"),
        }
    }

    #[inline(always)]
    pub fn set_frozen(&mut self, value: bool) {
        match self {
            ObjectMut::Record(object) => object.set_frozen(value),
            ObjectMut::Cell(_) => unreachable!("a class cell departs before its mutable view"),
        }
    }

    #[inline(always)]
    pub fn mark_as_prototype(&mut self) {
        match self {
            ObjectMut::Record(object) => object.mark_as_prototype(),
            ObjectMut::Cell(_) => unreachable!("a class cell departs before its mutable view"),
        }
    }

    #[inline(always)]
    pub fn mark_constant(&mut self) {
        match self {
            ObjectMut::Record(object) => object.mark_constant(),
            ObjectMut::Cell(_) => unreachable!("a class cell departs before its mutable view"),
        }
    }

    #[inline(always)]
    pub fn note_constant_write(&mut self) {
        match self {
            ObjectMut::Record(object) => object.note_constant_write(),
            ObjectMut::Cell(_) => unreachable!("a class cell departs before its mutable view"),
        }
    }
}

impl Heap {
    /// The value of `slot` on `object`, as `property_get` reads it: a class cell's field, or a record's
    /// inline value or arena run. Slot reads go through here because a record's values live in the arena.
    #[inline(always)]
    pub fn property_get_ref(&self, object: ObjectRef<'_>, slot: usize) -> Option<Value> {
        self.property_get(object.record(), slot)
    }
}
