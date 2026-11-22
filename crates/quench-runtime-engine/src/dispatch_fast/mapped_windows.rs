//! Parameters a mapped `arguments` object aliases, in the window of a suspended function
//! (`Function::arguments_ride_window`). The window holds the parameters; the object made
//! on entry is the function's local `arguments_slot` (`ParameterHome`), and its elements are the parameters' values
//! while the mapping holds, as the general path keeps them (`Vm::mapped_argument_load`,
//! `Vm::mapped_argument_store`). A read answers the element, a write goes to the parameter's
//! binding and the element, and an element's write reaches the binding
//! (`LaneStack::window_parameter`). A captured parameter rides the window's locals until a
//! closure makes the environment, and its slot there after (`ParameterHome`), as a captured
//! `arguments` object does.
use super::*;

/// Where a parameter lives in an environment window: its local among the window's locals, or,
/// for a parameter a closure captured once the window's environment exists, its slot there. A
/// captured parameter rides the window's locals until a closure makes the environment, which takes
/// it over (`window_environment`).
#[derive(Clone, Copy)]
pub(crate) enum ParameterHome {
    Local(*mut Value),
    Environment(Value, usize),
}

impl ParameterHome {
    /// The home of parameter `slot` of `function` in the window whose entries start at `entries`.
    ///
    /// # Safety
    /// `entries` starts the entries of a live environment window of `function`
    /// (`activate_environment_in_lane`), which holds a local for each of its slots.
    pub(super) unsafe fn of(entries: *mut Value, function: &Function, slot: usize) -> Self {
        // SAFETY: as the caller guarantees.
        let environment = unsafe { *entries.add(usize::from(WINDOW_ENVIRONMENT)) };
        if !environment.is_deleted() && function.local_slot_uses_environment(slot) {
            return Self::Environment(environment, slot);
        }
        // SAFETY: as above.
        Self::Local(unsafe { entries.add(usize::from(WINDOW_LOCALS) + slot) })
    }

    /// The parameter's binding as it stands, `DELETED` for one not initialized.
    pub(crate) fn read(self, heap: &quench_runtime::heap::Heap) -> Value {
        match self {
            // SAFETY: a local of a live window.
            Self::Local(binding) => unsafe { *binding },
            Self::Environment(environment, slot) => heap.environment_slot(environment, slot).unwrap_or(Value::DELETED),
        }
    }

    /// Stores `value` to the parameter's binding, an environment's after its barrier.
    pub(crate) fn write(self, vm: &mut Vm, value: Value) {
        match self {
            // SAFETY: a local of a live window.
            Self::Local(binding) => unsafe { *binding = value },
            Self::Environment(environment, slot) => {
                vm.heap.remember_cell(environment);
                if let Some(binding) = vm.heap.environment_slot_mut(environment, slot) {
                    *binding = value;
                }
            }
        }
    }
}

#[quench_macros::ext(LaneStackOps11)]
impl LaneStack {
    /// The home of parameter `slot` in the innermost window activation of `code` whose `arguments`
    /// binding holds `object`, where its parameters are.
    ///
    /// The object names its activation's depth (`Vm::record_window_owner`), so the lookup reads
    /// one activation; an object whose activation ended, or whose depth another activation took,
    /// fails the binding check.
    pub(crate) fn window_parameter(
        &self,
        heap: &quench_runtime::heap::Heap,
        code: &ResidualProgram,
        object: Value,
        slot: usize,
    ) -> Option<ParameterHome> {
        let (body, entries) = self.window_at(code, heap.object(object)?.window_depth()?)?;
        // SAFETY: an environment activation's window holds a local for each slot the function binds.
        let arguments = unsafe { ParameterHome::of(entries, body, usize::from(body.arguments_slot?)) }.read(heap);
        (arguments == object).then(|| unsafe { ParameterHome::of(entries, body, slot) })
    }

    /// The function of the environment window activation at `depth` and where its entries start.
    fn window_at<'a>(&self, code: &'a ResidualProgram, depth: usize) -> Option<(&'a Function, *mut Value)> {
        let activation = self.frame(depth)?;
        activation.script().filter(|script| script.environment)?;
        let body = code.function_get(activation.function() as usize)?;
        // SAFETY: an environment activation's window holds its entries after its registers.
        Some((body, unsafe { activation.window_ptr().add(usize::from(body.registers)) }))
    }
}

#[quench_macros::ext(DispatchFastMappedWindowsVm)]
impl Vm {
    /// Records the depth of the window activation just pushed at `depth` on the `arguments`
    /// object its window holds, where an element's write finds its parameter
    /// (`LaneStack::window_parameter`).
    pub(super) fn record_window_owner(&mut self, code: &ResidualProgram, depth: usize) {
        let Some((body, entries)) = self.lane_stack.window_at(code, depth) else {
            return;
        };
        let Some(slot) = body.arguments_slot.filter(|_| body.arguments_ride_window()) else {
            return;
        };
        // SAFETY: an environment activation's window holds a local for each slot the function binds.
        let arguments = unsafe { ParameterHome::of(entries, body, usize::from(slot)) }.read(&self.heap);
        if let Some(object) = self.heap.object_mut(arguments) {
            object.set_window_depth(depth);
        }
    }
}
