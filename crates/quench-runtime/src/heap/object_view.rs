//! The views through which callers reach an ordinary object. `Heap::object` and `Heap::object_mut`
//! return an `ObjectRef` or `ObjectMut`: either a live class cell (`heap/class_space.rs`, fixed class,
//! fields inline) or a record (`&Object`, the record table).
//!
//! A cell's class facts (class, shape, prototype link, flags) live in its class word, so a cell view
//! reads and edits them through `Object::of_class`: a record of that layout with no payload. A live
//! cell has no extras and no arena run, so no other operation exists for it. Those operations (extras,
//! elements, arena runs, dictionary mode, prototype marking) are record-only: `ObjectRef::record_only` is
//! `None` for a cell, and `Heap::object_record_mut` departs the cell first, so the record it returns is whole.

#![allow(dead_code)]

use super::cell::{Object, ProtoLink};
use super::class_space::{header_class, live_header};
use super::dictionary_space::DictionaryTable;
use super::{Heap, layouts};
use crate::value::Value;

/// A class cell's view: its header word (the class id in the low half, zero above), its inline fields in
/// constructor order, the slack fields holding `Value::DELETED`, and the words of its spill run, which follow the
/// inline fields (empty where the cell does not spill, `heap::spill`). A dictionary cell's fields are its table
/// (`table`), and its inline words are its dictionary word alone.
#[derive(Clone, Copy)]
pub struct CellView<'a> {
    header: &'a u64,
    fields: &'a [Value],
    run: &'a [Value],
    table: Option<&'a DictionaryTable>,
}

/// A class cell's mutable view. Its class changes in place through its header word.
pub struct CellViewMut<'a> {
    header: &'a mut u64,
    fields: &'a mut [Value],
    run: &'a mut [Value],
    table: Option<&'a mut DictionaryTable>,
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

impl CellView<'_> {
    /// The object the cell's class word describes: the reads of its class facts.
    #[inline(always)]
    fn object(&self) -> Object {
        Object::of_class(header_class(*self.header))
    }

    /// The word of slot `slot`: the table's word for a dictionary cell, else an inline field, else the run's word
    /// past the inline fields.
    #[inline(always)]
    fn word(&self, slot: usize) -> Option<Value> {
        if let Some(table) = self.table {
            return table.get(&(slot as u32)).copied();
        }
        match self.fields.get(slot) {
            Some(&value) => Some(value),
            None => self.run.get(slot - self.fields.len()).copied(),
        }
    }
}

impl CellViewMut<'_> {
    /// Edits the cell's class in place: `edit` runs on the object its class word describes, and the cell
    /// takes the class that object ends with. Only class-word edits go through here.
    #[inline(always)]
    fn edit_class(&mut self, edit: impl FnOnce(&mut Object)) {
        let mut object = Object::of_class(header_class(*self.header));
        edit(&mut object);
        *self.header = live_header(object.layout_id());
    }
}

// Read methods of `ObjectRef`: each forwards to the `Object` method of the same name, on the record or
// on the object the cell's class word describes.
macro_rules! read_methods {
    ($($name:ident -> $ret:ty;)*) => {
        $(
            #[inline(always)]
            pub fn $name(&self) -> $ret {
                match self {
                    ObjectRef::Record(object) => object.$name(),
                    ObjectRef::Cell(cell) => cell.object().$name(),
                }
            }
        )*
    };
}

impl<'a> ObjectRef<'a> {
    /// The record of the object, for the operations a class cell cannot answer (extras, elements, arena runs,
    /// inline values, payload words). `None` for a cell, which has none of that data. A caller that must
    /// change record data takes it through `Heap::object_record_mut`, which departs a cell first.
    #[inline(always)]
    pub fn record_only(&self) -> Option<&'a Object> {
        match self {
            ObjectRef::Record(object) => Some(*object),
            ObjectRef::Cell(_) => None,
        }
    }

    /// Whether the object holds extras-borne data that a cached define must not overwrite past: a boxed
    /// primitive, stack or error data. A live class cell holds no extras.
    #[inline(always)]
    pub fn holds_define_extras(&self) -> bool {
        match self {
            ObjectRef::Record(object) => {
                object.has_extras()
                    && (object.boxed_primitive().is_some() || object.stack_data().is_some() || object.has_error_data())
            }
            ObjectRef::Cell(_) => false,
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
        admits_cached_add -> bool;
    }
}

/// A record is the view of its own object, so a caller holding a record passes it where a view is taken.
impl<'a> From<&'a Object> for ObjectRef<'a> {
    #[inline(always)]
    fn from(object: &'a Object) -> Self {
        ObjectRef::Record(object)
    }
}

/// A view is `Copy`, so a caller holding a reference to one passes the view itself.
impl<'a> From<&ObjectRef<'a>> for ObjectRef<'a> {
    #[inline(always)]
    fn from(object: &ObjectRef<'a>) -> Self {
        *object
    }
}

impl<'a> ObjectMut<'a> {
    /// The shared view of the same object.
    #[inline(always)]
    pub fn as_shared(&self) -> ObjectRef<'_> {
        match self {
            ObjectMut::Record(object) => ObjectRef::Record(&**object),
            ObjectMut::Cell(cell) => ObjectRef::Cell(CellView { header: &*cell.header, fields: &*cell.fields, run: &*cell.run, table: cell.table.as_deref() }),
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

    /// The class edge (R4-C4): makes `class` the object's class, its header word. A cell's header
    /// holds the class id (`live_header`); a record takes it through `Object::set_class`. The caller
    /// retires the readers of the old class (`Heap::set_class`).
    #[inline(always)]
    pub fn set_class(&mut self, class: u32) {
        match self {
            ObjectMut::Record(object) => object.set_class(class),
            ObjectMut::Cell(cell) => *cell.header = live_header(class),
        }
    }

    // Flag edges with a class form (R4-C4): a cell takes the class the edit gives, in place.

    #[inline(always)]
    pub fn set_proto(&mut self, proto: Value) {
        match self {
            ObjectMut::Record(object) => object.set_proto(proto),
            ObjectMut::Cell(cell) => cell.edit_class(|object| object.set_proto(proto)),
        }
    }

    #[inline(always)]
    pub fn set_extensible(&mut self, value: bool) {
        match self {
            ObjectMut::Record(object) => object.set_extensible(value),
            ObjectMut::Cell(cell) => cell.edit_class(|object| object.set_extensible(value)),
        }
    }

    #[inline(always)]
    pub fn set_frozen(&mut self, value: bool) {
        match self {
            ObjectMut::Record(object) => object.set_frozen(value),
            ObjectMut::Cell(cell) => cell.edit_class(|object| object.set_frozen(value)),
        }
    }

    #[inline(always)]
    pub fn mark_constant(&mut self) {
        match self {
            ObjectMut::Record(object) => object.mark_constant(),
            ObjectMut::Cell(cell) => cell.edit_class(|object| object.mark_constant()),
        }
    }

    #[inline(always)]
    pub fn note_constant_write(&mut self) {
        match self {
            ObjectMut::Record(object) => object.note_constant_write(),
            ObjectMut::Cell(cell) => cell.edit_class(|object| object.note_constant_write()),
        }
    }
}

impl Heap {
    /// The shared view of live class cell `granule`.
    #[inline(always)]
    pub(crate) fn live_cell(&self, granule: u32) -> ObjectRef<'_> {
        let class = header_class(self.class_space.header(granule));
        let (fields, run) = self.class_space.cell_words(granule, self.live_spill_from(granule));
        let table = self.class_space.dictionary_of(granule, class);
        ObjectRef::Cell(CellView { header: self.class_space.cell_header(granule), fields, run, table })
    }

    /// The exclusive view of live class cell `granule`, whose header and fields it edits in place. A dictionary
    /// cell's table is its view's fields (`CellViewMut::table`).
    #[inline(always)]
    pub(crate) fn live_cell_mut(&mut self, granule: u32) -> ObjectMut<'_> {
        let spill = self.live_spill_from(granule);
        let dictionary = self.dictionary_id_of_cell(granule);
        let (header, fields, run, dictionaries) = self.class_space.cell_parts_mut(granule, spill);
        let table = dictionary.map(|id| dictionaries.get_mut(id));
        ObjectMut::Cell(CellViewMut { header, fields, run, table })
    }

    /// The dictionary id of live cell `granule` when its class is dictionary storage (`dictionary_cell`).
    #[inline(always)]
    pub(crate) fn dictionary_id_of_cell(&self, granule: u32) -> Option<u32> {
        let class = header_class(self.class_space.header(granule));
        layouts::class_is_dictionary(class).then(|| self.class_space.dictionary_id(granule))
    }

    /// The value of `slot` on `object`, as `property_get` reads it: a class cell's field, or a record's
    /// inline value or arena run. Slot reads go through here because a record's values live in the arena.
    #[inline(always)]
    pub fn property_get_ref(&self, object: ObjectRef<'_>, slot: usize) -> Option<Value> {
        match object {
            ObjectRef::Record(record) => self.property_get(record, slot),
            ObjectRef::Cell(cell) => {
                if slot >= self.properties.shape_length(cell.shape()) {
                    return None;
                }
                cell.word(slot).filter(|value| !value.is_deleted())
            }
        }
    }
}
