//! Parameters a mapped `arguments` object aliases, in the window of a suspended function
//! (`Function::arguments_ride_window`). The window holds the parameters; the object made
//! on entry is the window's local `arguments_slot`, and its elements are the parameters' values
//! while the mapping holds, as the general path keeps them (`Vm::mapped_argument_load`,
//! `Vm::mapped_argument_store`). A read answers the element, a write goes to the parameter's
//! binding and the element, and an element's write reaches the binding
//! (`LaneStack::window_parameter`). A captured parameter rides the window's locals until a
//! closure makes the environment, and its slot there after (`ParameterHome`).
use super::*;

/// The handler of `class`'s record of parameter `slot` when the parameter is one a mapped
/// `arguments` object aliases in a suspended function's window; `None` for any other record.
pub(super) fn mapped_parameter_handler<H: Host>(
    function: &Function,
    class: Class,
    slot: usize,
) -> Option<Handler> {
    let handler: Handler = match class {
        Class::LoadLocal | Class::LoadLocalPlain | Class::LoadEnvLocal => {
            lane_load_mapped_parameter::<H>
        }
        Class::StoreLocal | Class::StoreLocalPlain | Class::StoreEnvLocal => {
            lane_store_mapped_parameter::<H>
        }
        _ => return None,
    };
    (crate::vm::arguments::mapped_arguments_slot(function, slot).is_some()
        && function.arguments_ride_window())
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
    unsafe fn of(entries: *mut Value, function: &Function, slot: usize) -> Self {
        // SAFETY: as the caller guarantees.
        let environment = unsafe { *entries.add(usize::from(WINDOW_ENVIRONMENT)) };
        if !environment.is_deleted() && function.local_slot_uses_environment(slot) {
            return Self::Environment(environment, slot);
        }
        // SAFETY: as above.
        Self::Local(unsafe { entries.add(usize::from(WINDOW_LOCALS) + slot) })
    }

    /// The parameter's binding as it stands, `DELETED` for one not initialized.
    pub(in crate::vm) fn read<H: Host>(self, vm: &Vm<H>) -> Value {
        match self {
            // SAFETY: a local of a live window.
            Self::Local(binding) => unsafe { *binding },
            Self::Environment(environment, slot) => vm.heap.environment_slot(environment, slot).unwrap_or(Value::DELETED),
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
    pub(in crate::vm) fn window_parameter(
        &self,
        code: &ResidualProgram,
        object: Value,
        slot: usize,
    ) -> Option<ParameterHome> {
        self.activations.iter().rev().find_map(|activation| {
            activation.script().filter(|script| script.environment)?;
            let body = code.function_get(activation.function as usize)?;
            // SAFETY: an environment activation's window holds its entries after its registers.
            let entries = unsafe { activation.window_ptr().add(usize::from(body.registers)) };
            let arguments = unsafe { *entries.add(usize::from(WINDOW_LOCALS) + usize::from(body.arguments_slot?)) };
            // SAFETY: the window holds a local for each slot the function binds.
            (arguments == object).then(|| unsafe { ParameterHome::of(entries, body, slot) })
        })
    }
}

/// The `arguments` object the running window activation made on entry, and the home of its
/// parameter `slot`, for the window whose entries start at register `registers`; `None` in a
/// frame's run, which the general path serves.
fn mapped_window<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    registers: u16,
    slot: usize,
) -> Option<(Value, ParameterHome)> {
    if unsafe { (*cx).env }.is_deleted() {
        return None;
    }
    let function = unsafe { vm(cx) }.lane_stack.activations.last()?.function;
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(function as usize);
    Some((
        r.get(registers + WINDOW_LOCALS + body.arguments_slot?),
        // SAFETY: the running window activation's entries start at `registers`.
        unsafe { ParameterHome::of(r.0.add(usize::from(registers)), body, slot) },
    ))
}

script_handler! {
    /// A local load of a parameter a mapped `arguments` object aliases, in the window
    /// activation of a suspended function: the element the object maps it to, else the
    /// parameter's binding. Any other run, a numeric local or an uninitialized binding takes the
    /// general path.
    fn lane_load_mapped_parameter<>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        let Some((arguments, home)) = mapped_window(cx, r, registers, slot).filter(|_| i.a & crate::bytecode::NUMERIC_LOCAL_TARGET == 0)
        else {
            return exit(ip);
        };
        let vm = unsafe { vm(cx) };
        let value = vm.mapped_window_element(arguments, slot).unwrap_or_else(|| home.read(vm));
        if value.is_deleted() {
            return exit(ip);
        }
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `lane_load_mapped_parameter`'s store: the parameter's binding and, while the mapping
    /// holds, the element (`Vm::store_mapped_argument`).
    fn lane_store_mapped_parameter<>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        let Some((arguments, home)) = mapped_window(cx, r, registers, slot) else {
            return exit(ip);
        };
        let vm = unsafe { vm(cx) };
        if i.c == 0 && home.read(vm).is_deleted() {
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
