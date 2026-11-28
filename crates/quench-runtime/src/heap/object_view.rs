//! The views through which callers reach an ordinary object. `Heap::object` and `Heap::object_mut`
//! return an `ObjectRef` or `ObjectMut`: either a live class cell (`heap/class_space.rs`, fixed class,
//! fields inline) or a record (`&Object`, the record table).
//!
//! A cell's class facts (class, shape, prototype link, flags) live in its class word, so a cell view
//! reads and edits them through `Object::of_class`: a record of that layout with no payload. A live
//! cell has no extras and no arena run, so no other operation exists for it. Those operations (extras,
//! elements, arena runs, dictionary mode, prototype marking) are record-only: `ObjectRef::record_only` is
//! `None` for a cell, and `Heap::object_record_mut` answers `None` too.

#![allow(dead_code)]

use super::cell::{ElementFacts, Object, ObjectBrand, ObjectExtras, ProtoLink, StackData, StructuredSerialization};
use super::dependencies::Watch;
use super::class_space::{header_class, header_with_class};
use super::Heap;
use crate::bytecode::Atom;
use crate::value::Value;
use crate::value_vec::DictionaryTable;
use crate::vm::program_store::ProgramId;

mod extras;

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
    /// The cell's extras (`ExtrasSpace`), where its header names some.
    extras: Option<&'a ObjectExtras>,
}

/// A class cell's mutable view. Its class changes in place through its header word.
pub struct CellViewMut<'a> {
    header: &'a mut u64,
    fields: &'a mut [Value],
    run: &'a mut [Value],
    table: Option<&'a mut DictionaryTable>,
    /// The cell's extras (`ExtrasSpace`), where its header names some. A write that needs extras finds them made
    /// (`Heap::ensure_cell_extras`) before the view is taken.
    extras: Option<&'a mut ObjectExtras>,
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
        if let Some(extras) = self.extras.as_deref() {
            extras.note_prototype_layout_change(object.exact_shape());
        }
        edit(&mut object);
        *self.header = header_with_class(*self.header, object.layout_id());
    }

    /// The layout of the cell's object, which its descriptor marks retire readers of (`Object::exact_shape`).
    fn exact_shape(&self) -> u32 {
        Object::of_class(header_class(*self.header)).exact_shape()
    }

    /// `Object::mark_side_descriptors` on the cell: a prototype's layout change retires, then its extras take the fact.
    fn mark_side_descriptors(&mut self) {
        let shape = self.exact_shape();
        let extras = self.extras.as_deref_mut().expect("a cell's extras are made before a descriptor mark");
        extras.note_prototype_layout_change(shape);
        extras.mark_element_fact(ElementFacts::SIDE_DESCRIPTORS);
    }

    /// `Object::mark_indexed_descriptors` on the cell: as for its record, the layout change and the array proofs retire.
    fn mark_indexed_descriptors(&mut self) {
        let shape = self.exact_shape();
        let extras = self.extras.as_deref_mut().expect("a cell's extras are made before a descriptor mark");
        extras.note_prototype_layout_change(shape);
        super::array_proofs::retire_array_storage_proofs();
        extras.mark_element_fact(ElementFacts::INDEXED_DESCRIPTORS);
    }

    /// Records element fact `bit` in the cell's extras (`ObjectMut::mark_length_descriptor`).
    fn mark_element_fact(&mut self, bit: u8) {
        self.extras.as_deref_mut().expect("a cell's extras are made before a descriptor mark").mark_element_fact(bit);
    }

    /// `Object::mark_as_prototype` on the cell: its extras take the prototype mark.
    fn mark_as_prototype(&mut self) {
        let extras = self.extras.as_deref_mut().expect("a cell's extras are made before its prototype mark");
        if !extras.prototype_marked() {
            super::array_proofs::retire_array_storage_proofs();
            extras.set_prototype_setup();
        }
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
    /// change record data takes it through `Heap::object_record_mut`, which answers `None` for a cell.
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
            ObjectRef::Cell(cell) => cell.extras.is_some_and(ObjectExtras::holds_define_data),
        }
    }

    /// Whether the object's side descriptors may hold an entry (`Object::has_side_descriptors`); a cell's fact is in its
    /// extras, and a cell with none holds no side descriptor.
    #[inline(always)]
    pub fn has_side_descriptors(&self) -> bool {
        match self {
            ObjectRef::Record(object) => object.has_side_descriptors(),
            ObjectRef::Cell(cell) => cell.extras.is_some_and(|extras| extras.element_facts().has(ElementFacts::SIDE_DESCRIPTORS)),
        }
    }
    /// Whether the object holds index-keyed descriptors (`Object::has_indexed_descriptors`); a cell's fact is in its extras.
    #[inline(always)]
    pub fn has_indexed_descriptors(&self) -> bool {
        match self {
            ObjectRef::Record(object) => object.has_indexed_descriptors(),
            ObjectRef::Cell(cell) => cell.extras.is_some_and(|extras| extras.element_facts().has(ElementFacts::INDEXED_DESCRIPTORS)),
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
        has_exotic_access -> bool;
        holds_constant_mark -> bool;
        is_array -> bool;
        is_module_namespace -> bool;
        admits_cached_add -> bool;
    }

    /// Whether the object carries a prototype mark (`Object::was_used_as_prototype`); a cell's is in its extras.
    #[inline(always)]
    pub fn was_used_as_prototype(&self) -> bool {
        match self {
            ObjectRef::Record(object) => object.was_used_as_prototype(),
            ObjectRef::Cell(cell) => cell.extras.is_some_and(ObjectExtras::prototype_marked),
        }
    }

    /// This prototype's watch subject (`Object::watched_subject`); a cell's is in its extras.
    #[inline(always)]
    pub fn watched_subject(&self) -> Option<u32> {
        match self {
            ObjectRef::Record(object) => object.watched_subject(),
            ObjectRef::Cell(cell) => cell.extras.and_then(ObjectExtras::watched_subject),
        }
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
            ObjectMut::Cell(cell) => ObjectRef::Cell(CellView {
                header: &*cell.header,
                fields: &*cell.fields,
                run: &*cell.run,
                table: cell.table.as_deref(),
                extras: cell.extras.as_deref(),
            }),
        }
    }

    /// The record of the object, consuming the view so the record outlives it, for record data. `None` for a
    /// cell. A caller that must change record data takes it through `Heap::object_record_mut`, which answers
    /// `None` for a cell.
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

    /// Marks the object's side descriptors (`Object::mark_side_descriptors`): a cell's extras take the mark.
    #[inline(always)]
    pub fn mark_side_descriptors(&mut self) {
        match self {
            ObjectMut::Record(object) => object.mark_side_descriptors(),
            ObjectMut::Cell(cell) => cell.mark_side_descriptors(),
        }
    }
    /// Marks the object's index-keyed descriptors (`Object::mark_indexed_descriptors`).
    #[inline(always)]
    pub fn mark_indexed_descriptors(&mut self) {
        match self {
            ObjectMut::Record(object) => object.mark_indexed_descriptors(),
            ObjectMut::Cell(cell) => cell.mark_indexed_descriptors(),
        }
    }
    /// Marks the object's `length` descriptor (`Object::mark_length_descriptor`).
    #[inline(always)]
    pub fn mark_length_descriptor(&mut self) {
        match self {
            ObjectMut::Record(object) => object.mark_length_descriptor(),
            ObjectMut::Cell(cell) => cell.mark_element_fact(ElementFacts::LENGTH_DESCRIPTOR),
        }
    }
    /// Marks the object as a prototype (`Object::mark_as_prototype`): a cell's extras take the mark.
    #[inline(always)]
    pub fn mark_as_prototype(&mut self) {
        match self {
            ObjectMut::Record(object) => object.mark_as_prototype(),
            ObjectMut::Cell(cell) => cell.mark_as_prototype(),
        }
    }

    /// Readies a marked prototype's lookup (`Object::prepare_prototype_lookup`). A cell with no extras has no mark.
    #[inline(always)]
    pub fn prepare_prototype_lookup(&mut self) {
        match self {
            ObjectMut::Record(object) => object.prepare_prototype_lookup(),
            ObjectMut::Cell(cell) => {
                if let Some(extras) = cell.extras.as_deref_mut() {
                    extras.prepare_prototype();
                }
            }
        }
    }

    /// Gives the prototype its watch subject (`Object::watch_as`); a cell's is in its extras.
    #[inline(always)]
    pub fn watch_as(&mut self, watch: Watch) {
        match self {
            ObjectMut::Record(object) => object.watch_as(watch),
            ObjectMut::Cell(cell) => cell.extras.as_deref_mut().expect("a cell's extras are made before its watch").set_watch(watch),
        }
    }

    /// The class edge (R4-C4): makes `class` the object's class, its header word. A cell's header
    /// holds the class id in its low half (`header_with_class`); a record takes it through `Object::set_class`. The caller
    /// retires the readers of the old class (`Heap::set_class`).
    #[inline(always)]
    pub fn set_class(&mut self, class: u32) {
        match self {
            ObjectMut::Record(object) => object.set_class(class),
            ObjectMut::Cell(cell) => *cell.header = header_with_class(*cell.header, class),
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
        let (fields, run) = self.class_space.cell_words(granule, self.live_spill_from(granule));
        let table = self.cell_dictionary(granule);
        let extras = self.extras.get(self.class_space.extras_id(granule));
        ObjectRef::Cell(CellView { header: self.class_space.cell_header(granule), fields, run, table, extras })
    }

    /// The view of an ordinary object: a class cell, an object record, or a promise (`Heap::ordinary_object`'s
    /// forms). Any other record (an array, a function, a collection) is `None`, as `ordinary_object` makes it.
    #[inline(always)]
    pub(crate) fn ordinary_view(&self, value: Value) -> Option<ObjectRef<'_>> {
        if value.direct_object_index().is_some() {
            return self.object(value);
        }
        self.promise(value).map(|promise| ObjectRef::Record(&promise.object))
    }

    /// The exclusive view of live class cell `granule`, whose header and fields it edits in place. A dictionary
    /// cell's table is its view's fields (`CellViewMut::table`), in the value arena.
    #[inline(always)]
    pub(crate) fn live_cell_mut(&mut self, granule: u32) -> ObjectMut<'_> {
        let spill = self.live_spill_from(granule);
        let dictionary = self.dictionary_id_of_cell(granule);
        let extras_id = self.class_space.extras_id(granule);
        let extras = self.extras.get_mut(extras_id);
        let properties = &mut self.properties;
        let (header, fields, run) = self.class_space.cell_parts_mut(granule, spill);
        let table = dictionary.map(|id| properties.dictionary_mut(id));
        ObjectMut::Cell(CellViewMut { header, fields, run, table, extras })
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
