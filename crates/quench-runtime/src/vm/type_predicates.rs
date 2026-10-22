use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn is_string(&self, v: Value) -> bool {
        self.heap.is_string(v)
    }
}
