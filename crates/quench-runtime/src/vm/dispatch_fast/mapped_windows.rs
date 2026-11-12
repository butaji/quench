//! Parameters a mapped `arguments` object aliases, in the window of a suspended function
//! (`Function::arguments_ride_window`). The window's locals hold the parameters; the object made
//! on entry is the window's local `arguments_slot`, and its elements are the parameters' values
//! while the mapping holds, as the general path keeps them (`Vm::mapped_argument_load`,
//! `Vm::mapped_argument_store`). A read answers the element, a write goes to the local and the
//! element, and an element's write reaches the local (`LaneStack::window_locals`).
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

impl LaneStack {
    /// The locals of the innermost window activation of `code` whose `arguments` binding holds
    /// `object`, where its parameters are.
    pub(in crate::vm) fn window_locals(
        &self,
        code: &ResidualProgram,
        object: Value,
    ) -> Option<*mut Value> {
        self.activations.iter().rev().find_map(|activation| {
            activation.script().filter(|script| script.environment)?;
            let body = code.function_get(activation.function as usize)?;
            // SAFETY: an environment activation's window holds its locals after its registers.
            let locals = unsafe {
                activation
                    .window_ptr()
                    .add(usize::from(body.registers + WINDOW_LOCALS))
            };
            // SAFETY: the window holds a local for each slot the function binds.
            (unsafe { *locals.add(usize::from(body.arguments_slot?)) } == object).then_some(locals)
        })
    }
}

/// The `arguments` object the running window activation made on entry, whose window starts at
/// register `registers`; `None` in a frame's run, which the general path serves.
fn window_arguments_object<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    registers: u16,
) -> Option<Value> {
    if unsafe { (*cx).env }.is_deleted() {
        return None;
    }
    let function = unsafe { vm(cx) }.lane_stack.activations.last()?.function;
    // SAFETY: the context's program outlives the lane run.
    let slot = unsafe { &*(*cx).code }
        .function(function as usize)
        .arguments_slot?;
    Some(r.get(registers + WINDOW_LOCALS + slot))
}

script_handler! {
    /// A local load of a parameter a mapped `arguments` object aliases, in the window
    /// activation of a suspended function: the element the object maps it to, else the
    /// window's local. Any other run, a numeric local or an uninitialized binding takes the
    /// general path.
    fn lane_load_mapped_parameter<>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        let Some(arguments) = window_arguments_object(cx, r, registers).filter(|_| i.a & crate::bytecode::NUMERIC_LOCAL_TARGET == 0)
        else {
            return exit(ip);
        };
        let local = r.get(registers + WINDOW_LOCALS + slot as u16);
        let value = unsafe { vm(cx) }.mapped_window_element(arguments, slot).unwrap_or(local);
        if value.is_deleted() {
            return exit(ip);
        }
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `lane_load_mapped_parameter`'s store: the window's local and, while the mapping holds,
    /// the element (`Vm::store_mapped_argument`).
    fn lane_store_mapped_parameter<>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        let Some(arguments) = window_arguments_object(cx, r, registers) else {
            return exit(ip);
        };
        let value = r.get(i.a);
        // SAFETY: the activation's window holds its locals after its entries
        // (`lane_activate_environment`).
        let binding = unsafe { r.0.add(usize::from(registers + WINDOW_LOCALS) + slot) };
        if i.c == 0 && unsafe { *binding }.is_deleted() {
            return exit(ip);
        }
        unsafe { *binding = value };
        unsafe { vm(cx) }.store_mapped_argument(arguments, slot, value);
        if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
            r.set(register, value);
        }
        next!(cx, r, following(ip), view)
    }
}
