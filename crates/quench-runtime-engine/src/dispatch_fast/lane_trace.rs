//! What a lane stack keeps alive (`LaneStack::extend_roots`): the values of its windows, the
//! objects its activations name and what they own. One walk serves the stack of the running lane
//! and every stack a nested run parked (`Vm::lane_roots`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

#[quench_macros::ext(LaneStackOps12)]
impl LaneStack {
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
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let held = vm.allocate_object(Vm::empty_object(vm.object_proto));
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
