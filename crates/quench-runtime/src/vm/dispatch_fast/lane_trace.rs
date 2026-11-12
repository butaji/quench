//! What a lane stack keeps alive (`LaneStack::extend_roots`): the values of its windows, the
//! objects its activations name and what they own. One walk serves the stack of the running lane
//! and every stack a nested run parked (`Vm::lane_roots`).
use super::*;

impl LaneStack {
    /// Appends to `out` every value the stack's activations hold.
    pub(in crate::vm) fn extend_roots(&self, out: &mut Vec<Value>) {
        self.extend_window_roots(out);
        out.extend(
            self.activations
                .iter()
                .filter_map(Activation::script)
                .flat_map(|script| [script.callee, script.caller_this, script.caller_env]),
        );
        out.extend(self.with_object_roots());
        out.extend(self.forwarders.iter().flat_map(|forwarder| [forwarder.callee, forwarder.env]));
        out.extend(self.steps.iter().flat_map(LaneArrayStep::roots));
        for (_, record) in &self.records {
            record.extend_roots(out);
        }
    }

    /// Whether `extend_roots` reaches every value the stack holds: a Wasm activation's window
    /// holds its payload as raw bytes (`wasm_storage_bytes`), which no walk reads as values.
    pub(in crate::vm) fn traces_every_window(&self) -> bool {
        self.wasm_storage_bytes == 0
    }

    /// The values of the windows on the stack's own values: each script activation's span, up to
    /// the next window or the top. A window of another register file (`HEAP_WINDOW`) belongs to
    /// the record that owns it, which its callee keeps alive.
    fn extend_window_roots(&self, out: &mut Vec<Value>) {
        let (span, top) = (self.values.span(), self.values.top().addr());
        let mut windows = self.activations.iter().filter(|activation| span.contains(&activation.window_word())).peekable();
        while let Some(activation) = windows.next() {
            if activation.script().is_none() {
                continue;
            }
            let end = windows.peek().map_or(top, |next| next.window_word());
            let len = end.wrapping_sub(activation.window_word()) / std::mem::size_of::<Value>();
            // SAFETY: the values `base..top` are initialized, and the span lies within them.
            out.extend_from_slice(unsafe { std::slice::from_raw_parts(activation.window_ptr(), len) });
        }
    }
}
