//! Parameters a mapped `arguments` object aliases, in the window of a suspended function
//! (`Function::arguments_ride_window`). The window holds the parameters; the object made
//! on entry is the function's local `arguments_slot` (`ParameterHome`), and its elements are the parameters' values
//! while the mapping holds, as the general path keeps them (`Vm::mapped_argument_load`,
//! `Vm::mapped_argument_store`). A read answers the element, a write goes to the parameter's
//! binding and the element, and an element's write reaches the binding
//! (`LaneStack::window_parameter`). A captured parameter rides the window's locals until a
//! closure makes the environment, and its slot there after (`ParameterHome`), as a captured
//! `arguments` object does.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::ParameterHome;





#[quench_macros::ext(DispatchFastMappedWindowsVm)]
impl Vm {
    /// Records the depth of the window activation just pushed at `depth` on the `arguments`
    /// object its window holds, where an element's write finds its parameter
    /// (`LaneStack::window_parameter`).
    pub(crate) fn record_window_owner(&mut self, code: &ResidualProgram, depth: usize) {
        let Some((body, entries)) = self.lane_stack.window_at(code, depth) else {
            return;
        };
        let Some(slot) = body.arguments_slot.filter(|_| body.arguments_ride_window()) else {
            return;
        };
        // SAFETY: an environment activation's window holds a local for each slot the function binds.
        let arguments = unsafe { ParameterHome::of(entries, body, usize::from(slot)) }.read(self.heap_reads());
        if let Some(mut object) = self.object_mut_departing(arguments) {
            object.record().set_window_depth(depth);
        }
    }
}
