//! What a lane stack keeps alive (`LaneStack::extend_roots`): the values of its windows, the
//! objects its activations name and what they own. One walk serves the stack of the running lane
//! and every stack a nested run parked (`Vm::lane_roots`).
use super::*;
use quench_runtime::vm::program_store::ProgramStore;
use quench_runtime::vm::activation::extend_this_env_roots;

impl LaneStack {
    /// Appends to `out` every value the stack's activations hold; `programs` hold the liveness
    /// of the Wasm activations' registers.
    pub(crate) fn extend_roots(&self, programs: &ProgramStore, out: &mut Vec<Value>) {
        self.extend_window_roots(programs, out);
        out.extend(self.frames().filter_map(|frame| frame.script()).map(|script| script.callee));
        out.extend(self.with_object_roots());
        out.extend(self.side_roots());
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
        let mut windows = self.frames().enumerate().filter(|(_, activation)| span.contains(&activation.window_word())).peekable();
        while let Some((depth, activation)) = windows.next() {
            let end = windows.peek().map_or(top, |(_, next)| next.window_word());
            let len = end.wrapping_sub(activation.window_word()) / std::mem::size_of::<Value>() - lane_values::CONTEXT;
            // SAFETY: the values `base..top` are initialized, and the span lies within them.
            let window = unsafe { std::slice::from_raw_parts(activation.window_ptr(), len) };
            match activation.kind() {
                ActivationKind::Script(_) => {
                    let registers = Registers(activation.window_ptr());
                    extend_this_env_roots(out, registers.this(), registers.env());
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

    /// Records `pc` as the record the top activation, if a Wasm one, is at for a callout that may
    /// run code (`in_place_in_lane`): the collector reads its registers' liveness there
    /// (`extend_wasm_roots`). A Wasm activation's callouts all note it before they run.
    pub(crate) fn note_wasm_pc(&mut self, pc: usize) {
        // A plain header is a script frame; a Wasm activation is always a record. The note moves
        // no frame, so the records are written without `records_mut`'s bound reset.
        if self.plain.depth.get() != 0 {
            return;
        }
        let pc = pc as u32;
        if let Some(ActivationKind::Wasm { pc: at, .. } | ActivationKind::WasmEntry { pc: at, .. }) =
            self.activation_records.get_mut().last_mut().map(|activation| activation.kind_mut())
        {
            *at = pc;
        }
    }

    /// The registers of the Wasm activation at `depth` that `window` holds and the function's
    /// liveness keeps at the record it is at: the record its callee resumes at, or, for the
    /// innermost activation or one whose callee is native, the record it last noted
    /// (`note_wasm_pc`). Its registers hold Wasm values the collector reads as it reads a frame's.
    /// A function with no masks has its whole window scanned, so its window is initialized whole.
    fn extend_wasm_roots(&self, programs: &ProgramStore, depth: usize, window: &[Value], out: &mut Vec<Value>) {
        let Some(activation) = self.frame(depth) else { return out.extend_from_slice(window) };
        let (program, noted) = match activation.kind() {
            ActivationKind::Wasm { program, pc, .. } | ActivationKind::WasmEntry { program, pc, .. } => (program, pc as usize),
            ActivationKind::Native | ActivationKind::Script(_) => return out.extend_from_slice(window),
        };
        let call_pc = self.frame(depth + 1).and_then(|above| above.caller_pc()).unwrap_or(noted);
        let masks = programs.residual(program).and_then(|program| program.register_root_masks(activation.function()));
        let raw = programs.raw_register_masks(program, activation.function());
        match masks {
            Some(masks) => masks.extend_live_except(call_pc, raw.as_deref(), window, out),
            None => out.extend_from_slice(window),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_runtime::test_host::TestHost;
    use quench_runtime::Engine;

    #[test]
    fn a_collection_in_a_nested_run_keeps_the_references_of_a_parked_wasm_activation() {
        let mut program = Engine::specialize("globalThis.seed = 1;", "parked-wasm.js").unwrap();
        // An uncovered function's window is scanned whole, as a parked Wasm window with no masks is.
        program.functions[0].register_roots = quench_runtime::bytecode::RegisterRoots::new(false);
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&program).unwrap();
        let held = vm.heap.alloc_object(Vm::empty_object(vm.object_proto));
        let registers = [held, Value::integer(7)];
        let wasm = Activation::of_parts(
            0,
            std::ptr::null(),
            std::ptr::null(),
            Registers(std::ptr::null_mut()),
            ProgramId::MAIN,
            ActivationKind::Wasm { memory: MemoryView::NONE, frame_storage_bytes: std::mem::size_of_val(&registers), program: ProgramId::MAIN, pc: 0 },
        );
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
