//! Which shapes native code may hold across a call: the shape table's own record of departures.

use super::*;

impl Vm {
    /// Whether code may hold an object's exact `shape` past a call, under a dependency on its
    /// departures (`Heap::subscribe_shape`): no object has ever left it, which the table shows as
    /// a shape with no child (a departure adds, deletes, reconfigures or converts, each making a
    /// child), so the first departure retires the code and its recompile finds the child and
    /// guards the shape at each use instead. Only where slot representations are kept: their
    /// store gate keeps a native add, which moves its object to the child without telling the
    /// readers of the shape it leaves (`staged::add::add_property`), out of native code.
    pub fn shape_is_stable(&self, shape: u32) -> bool {
        let Some(children) = self.shapes.get(shape as usize + 1..) else { return false };
        self.heap.tracks_slot_representations() && !self.shape_is_dictionary(shape)
            && !children.iter().any(|child| child.parent() == Some(shape))
    }
}
