//! Parameters a mapped `arguments` object aliases, in the window of a suspended function
//! (`Function::arguments_ride_window`). The window holds the parameters; the object made
//! on entry is the function's local `arguments_slot` (`window_binding`), and its elements are the parameters' values
//! while the mapping holds, as the general path keeps them (`Vm::mapped_argument_load`,
//! `Vm::mapped_argument_store`). A read answers the element, a write goes to the parameter's
//! binding and the element, and an element's write reaches the binding
//! (`LaneStack::window_parameter`). A captured parameter rides the window's locals until a
//! closure makes the environment, and its slot there after (`ParameterHome`), as a captured
//! `arguments` object does.
use super::*;

// merge: HEAD's dispatcher, adapted to theirs' staged queries and handler names. Theirs'
// window_local_handler does not wire it (its row wiring is not merged), so it stays unused.
#[allow(dead_code)]
pub(super) fn mapped_parameter_handler<H: Host>(function: &Function, op: Op, slot: usize) -> Option<Handler> {
    let handler: Handler = if crate::staged::accesses_local_slot(op) && !crate::staged::stores_local_slot(op) {
        load_mapped_parameter::<H>
    } else if crate::staged::stores_local_slot(op) {
        store_mapped_parameter::<H>
    } else {
        return None;
    };
    (crate::vm::arguments::mapped_arguments_slot(function, slot).is_some() && function.arguments_ride_window())
        .then_some(handler)
}

/// Where a parameter lives in an environment window: its local among the window's locals, or,
/// for a parameter a closure captured once the window's environment exists, its slot there. A
/// captured parameter rides the window's locals until a closure makes the environment, which takes
/// it over (`window_environment`).
#[derive(Clone, Copy)]
pub(in crate::vm) enum ParameterHome {
    Local(*mut Value),
    Environment(Value, usize),
}

impl ParameterHome {
    /// The home of parameter `slot` of `function` in the window whose entries start at `entries`.
    ///
    /// # Safety
    /// `entries` starts the entries of a live environment window of `function`
    /// (`lane_activate_environment`), which holds a local for each of its slots.
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
    pub(in crate::vm) fn read(self, heap: &crate::heap::Heap) -> Value {
        match self {
            // SAFETY: a local of a live window.
            Self::Local(binding) => unsafe { *binding },
            Self::Environment(environment, slot) => heap.environment_slot(environment, slot).unwrap_or(Value::DELETED),
        }
    }

    /// Stores `value` to the parameter's binding, an environment's after its barrier.
    pub(in crate::vm) fn write<H: Host>(self, vm: &mut Vm<H>, value: Value) {
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

impl LaneStack {
    /// The home of parameter `slot` in the innermost window activation of `code` whose `arguments`
    /// binding holds `object`, where its parameters are.
    ///
    /// The object names its activation's depth (`Vm::record_window_owner`), so the lookup reads
    /// one activation; an object whose activation ended, or whose depth another activation took,
    /// fails the binding check.
    pub(in crate::vm) fn window_parameter(
        &self,
        heap: &crate::heap::Heap,
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
        let activation = self.activations().get(depth)?;
        activation.script().filter(|script| script.environment)?;
        let body = code.function_get(activation.function() as usize)?;
        // SAFETY: an environment activation's window holds its entries after its registers.
        Some((body, unsafe { activation.window_ptr().add(usize::from(body.registers)) }))
    }
}

impl<H: Host> Vm<H> {
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

/// The binding of local `slot` of `body`, the function of the running window activation whose
/// registers are `r`: its window's local, or its environment's slot once a closure made one.
pub(super) fn window_binding<H: Host>(vm: &Vm<H>, r: Registers, body: &Function, slot: usize) -> Value {
    // SAFETY: the running window activation's entries follow its function's registers.
    unsafe { ParameterHome::of(r.0.add(usize::from(body.registers)), body, slot) }.read(&vm.heap)
}

// merge: theirs' mapped-parameter handlers (`lane_load_mapped_parameter`, `lane_store_mapped_parameter`)
// are kept under ours-style names. They read the element through `Vm::mapped_window_element`, which
// only theirs' object_array/arguments.rs defines, and no row of ours requickens to them, so they are
// unused here until that helper and the row wiring are merged. Ours reads the element through
// `stored_arguments_element`.
/// The `arguments` object the running window activation made on entry, and the home of its
/// parameter `slot`, for the window whose entries start at register `registers`; `None` in a
/// frame's run, which the general path serves.
fn mapped_window<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    registers: u16,
    slot: usize,
) -> Option<(Value, ParameterHome)> {
    if unsafe { (*cx).registers }.env().is_deleted() {
        return None;
    }
    let vm = unsafe { vm(cx) };
    let function = vm.lane_stack.activations().last()?.function();
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(function as usize);
    Some((
        window_binding(vm, r, body, usize::from(body.arguments_slot?)),
        // SAFETY: the running window activation's entries start at `registers`.
        unsafe { ParameterHome::of(r.0.add(usize::from(registers)), body, slot) },
    ))
}

script_handler! {
    /// A local load of a parameter a mapped `arguments` object aliases, in the window
    /// activation of a suspended function: the element the object maps it to, else the
    /// parameter's binding. Any other run, a numeric local or an uninitialized binding takes the
    /// general path.
    fn load_mapped_parameter<>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        let Some((arguments, home)) = mapped_window(cx, r, registers, slot).filter(|_| i.a & crate::bytecode::NUMERIC_LOCAL_TARGET == 0)
        else {
            return exit(ip);
        };
        let vm = unsafe { vm(cx) };
        let value = vm.mapped_window_element(arguments, slot).unwrap_or_else(|| home.read(&vm.heap));
        if value.is_deleted() {
            return exit(ip);
        }
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `load_mapped_parameter`'s store: the parameter's binding and, while the mapping
    /// holds, the element (`Vm::store_mapped_argument`).
    fn store_mapped_parameter<>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        let Some((arguments, home)) = mapped_window(cx, r, registers, slot) else {
            return exit(ip);
        };
        let vm = unsafe { vm(cx) };
        if i.c == 0 && home.read(&vm.heap).is_deleted() {
            return exit(ip);
        }
        let value = r.get(i.a);
        home.write(vm, value);
        vm.store_mapped_argument(arguments, slot, value);
        if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
            r.set(register, value);
        }
        next!(cx, r, following(ip), view)
    }
}
