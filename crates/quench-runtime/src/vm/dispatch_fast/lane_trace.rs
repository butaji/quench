//! What a lane stack keeps alive (`LaneStack::extend_roots`): the values of its windows, the
//! objects its activations name and what they own. One walk serves the stack of the running lane
//! and every stack a nested run parked (`Vm::lane_roots`).
use super::*;
use crate::vm::program_store::ProgramStore;

impl LaneStack {
    /// Appends to `out` every value the stack's activations hold; `programs` hold the liveness
    /// of the Wasm activations' registers.
    pub(in crate::vm) fn extend_roots(&self, programs: &ProgramStore, out: &mut Vec<Value>) {
        self.extend_window_roots(programs, out);
        out.extend(
            self.activations
                .iter()
                .filter_map(Activation::script)
                .map(|script| script.callee),
        );
        out.extend(self.with_object_roots());
        out.extend(self.forwarders.iter().flat_map(|forwarder| [forwarder.callee, forwarder.env]));
        out.extend(self.steps.iter().flat_map(LaneArrayStep::roots));
        for (_, record) in &self.records {
            record.extend_roots(out);
        }
    }

    /// The values of the windows on the stack's own values: each script activation's registers,
    /// up to the next window's context words (`lane_values::CONTEXT`, which end every span, the
    /// top's included), with its own receiver and environment words, and each Wasm activation's
    /// live registers. The capture word is an address, not a value, and is never traced. A
    /// window of another register file (`HEAP_WINDOW`) belongs to the record that owns it,
    /// which its callee keeps alive.
    fn extend_window_roots(&self, programs: &ProgramStore, out: &mut Vec<Value>) {
        let (span, top) = (self.values.span(), self.values.top().addr());
        let mut windows = self.activations.iter().enumerate().filter(|(_, activation)| span.contains(&activation.window_word())).peekable();
        while let Some((depth, activation)) = windows.next() {
            let end = windows.peek().map_or(top, |(_, next)| next.window_word());
            let len = end.wrapping_sub(activation.window_word()) / std::mem::size_of::<Value>() - lane_values::CONTEXT;
            // SAFETY: the values `base..top` are initialized, and the span lies within them.
            let window = unsafe { std::slice::from_raw_parts(activation.window_ptr(), len) };
            match activation.kind {
                ActivationKind::Script(_) => {
                    let registers = Registers(activation.window_ptr());
                    out.extend([registers.this(), registers.env()]);
                    out.extend_from_slice(window);
                }
                ActivationKind::Wasm { .. } => self.extend_wasm_roots(programs, depth, window, out),
                // An export's activation also holds the instance environment it runs over.
                ActivationKind::WasmEntry { instance, .. } => {
                    out.push(instance);
                    self.extend_wasm_roots(programs, depth, window, out);
                }
                ActivationKind::Native => {}
            }
        }
    }

    /// The registers of the Wasm activation at `depth` that `window` holds and the function's
    /// liveness keeps at the call its callee resumes at, as its frame's registers are scanned
    /// (`Vm::collect_slow`): every register where no activation is above it or the function
    /// has no liveness. Its registers hold Wasm values the collector reads as it reads a frame's.
    fn extend_wasm_roots(&self, programs: &ProgramStore, depth: usize, window: &[Value], out: &mut Vec<Value>) {
        let activation = &self.activations[depth];
        let masks = programs.residual(activation.caller_program).and_then(|program| program.register_root_masks(activation.function));
        let call_pc = self.activations.get(depth + 1).and_then(Activation::caller_pc);
        match masks.zip(call_pc) {
            Some((masks, pc)) => masks.extend_live(pc, window, out),
            None => out.extend_from_slice(window),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::TestHost;
    use crate::Engine;

    #[test]
    fn a_collection_in_a_nested_run_keeps_the_references_of_a_parked_wasm_activation() {
        let program = Engine::specialize("globalThis.seed = 1;", "parked-wasm.js").unwrap();
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&program).unwrap();
        let held = vm.heap.alloc_object(Vm::<TestHost>::empty_object(vm.object_proto));
        let registers = [held, Value::integer(7)];
        let wasm = Activation {
            native: crate::vm::compiled::ShadowFrame::EMPTY,
            function: 0,
            call: std::ptr::null(),
            caller_base: std::ptr::null(),
            caller_registers: Registers(std::ptr::null_mut()),
            caller_program: ProgramId::MAIN,
            kind: ActivationKind::Wasm { memory: MemoryView::NONE, frame_storage_bytes: std::mem::size_of_val(&registers) },
        };
        let window = vm.lane_stack.values.top();
        // SAFETY: the fresh stack's capacity holds the window, the next one's context words and
        // the activation, and the window's values are initialized.
        unsafe {
            window.copy_from_nonoverlapping(registers.as_ptr(), registers.len());
            vm.lane_stack.values.set_top(window.add(registers.len() + lane_values::CONTEXT));
            vm.lane_stack.push_activation(wasm, window);
        }
        vm.nested_lane_run([Value::UNDEFINED; 2], |vm| vm.collect_now(&program));
        assert!(vm.heap.is_live(held), "the parked activation's reference survived the nested collection");
        vm.lane_stack.pop_activation();
        vm.lane_stack.values.clear();
    }
}
