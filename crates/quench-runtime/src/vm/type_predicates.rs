use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn is_string(&self, v: Value) -> bool {
        matches!(self.heap.get(v), Some(Cell::String(_)))
    }
}
