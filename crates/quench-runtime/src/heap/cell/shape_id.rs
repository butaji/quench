//! An object's shape id, written apart from the layout change it usually records.
use crate::value_vec::ValueVec;

impl super::Object {
    /// Sets the shape id alone. `set_shape` records the layout change around it; shape
    /// compaction renumbers an unchanged shape and retires every reader of the old ids at once
    /// (`Heap::remap_live_object_shapes`), not one prototype at a time.
    #[inline(always)]
    pub(crate) fn renumber_shape(&mut self, shape: u32) {
        let mut properties = ValueVec::from_parts(0, self.aux());
        properties.set_auxiliary(shape);
        self.set_aux(properties.into_parts().1);
    }

    /// The layout a record of layout `layout` takes when its shape is renumbered to `shape` and
    /// nothing else changes (`renumber_shape`): an add that writes into storage it already has.
    /// The staged add rows store it (`staged::add`), whose sites record it with the layout they
    /// guard (`staged::add_plan::Transition`).
    pub(crate) fn layout_with_shape(layout: u32, shape: u32) -> u32 {
        let word = super::super::layouts::word(layout);
        let mut properties = ValueVec::from_parts(0, word as u32);
        properties.set_auxiliary(shape);
        super::super::layouts::intern(super::header_word((word >> u32::BITS) as u32, properties.into_parts().1))
    }

    /// Whether a record of layout `layout` keeps its values in a wide arena run (or an empty
    /// vector without one) of an ordinary, unmarked object outside dictionary storage: no
    /// inline, extras or narrow storage form, no constant mark, no exotic access. An add into
    /// such storage keeps every bit of the layout but the shape (`layout_with_shape`).
    pub(crate) fn layout_is_wide_run(layout: u32) -> bool {
        use crate::value_vec::{CONSTANT_FORM, DICTIONARY_STORAGE, EXOTIC_ACCESS, STORAGE_FORM};
        super::super::layouts::auxiliary(layout) & (STORAGE_FORM | CONSTANT_FORM | DICTIONARY_STORAGE | EXOTIC_ACCESS) == 0
    }

    /// The layouts an add site records from a receiver of layout `layout` moving to shape
    /// `shape` (`staged::add_plan::Transition::layouts`): the layout the staged rows guard and
    /// the one they store.
    pub(crate) fn add_layouts(layout: u32, shape: u32) -> [u32; 2] {
        [layout, Self::layout_with_shape(layout, shape)]
    }
}
