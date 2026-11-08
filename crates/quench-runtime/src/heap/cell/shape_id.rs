//! An object's shape id, written apart from the layout change it usually records.
use crate::value_vec::ValueVec;

impl super::Object {
    /// Sets the shape id alone. `set_shape` records the layout change around it; shape
    /// compaction renumbers an unchanged shape and retires every reader of the old ids at once
    /// (`Heap::remap_live_object_shapes`), not one prototype at a time.
    #[inline(always)]
    pub(crate) fn renumber_shape(&mut self, shape: u32) {
        let mut properties = ValueVec::from_parts(0, self.auxiliary);
        properties.set_auxiliary(shape);
        self.auxiliary = properties.into_parts().1;
    }
}
