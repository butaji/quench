//! Entering an exception handler: the state settled before the handler's block runs, shared by
//! the interpreter's unwinding (`Vm::exception_handler_target`) and a compiled body's or loop
//! region's native catch (`dispatch_fast::handler_edge`), in a VM frame or in the
//! environment window of a lane activation (`HandlerHome`): one implementation of each step.
use crate::vm::engine_types::{WINDOW_ENVIRONMENT, WINDOW_LOCALS, WINDOW_SLOTS};
use super::*;
use crate::bytecode::{Handler, ThrownValueSlot};

/// Where the state a handler's entry settles lives: a frame of the VM's stack, or the window of
/// a lane activation (a suspended function's, an ordinary one's) over the lane stack's scope chain.
#[derive(Clone, Copy)]
pub enum HandlerHome {
    Frame(usize),
    Window(WindowHome),
}

/// A lane activation a handler is entered in: its index on the lane stack, which owns its `with`
/// objects (`LaneStack::with_objects`), the function and program it runs, and its window's first
/// register, after whose function's registers its environment entries and locals follow
/// (`activate_environment_in_lane`).
#[derive(Clone, Copy)]
pub struct WindowHome {
    pub(in crate::vm) depth: usize,
    pub(in crate::vm) program: ProgramId,
    pub(in crate::vm) function: u32,
    pub(in crate::vm) window: *mut Value,
}

impl Vm {
    /// Leaves the scopes of the `with` statements the exception left in `home`: those past the
    /// depth `handler` was entered at.
    pub fn unwind_handler_scopes(&mut self, home: HandlerHome, handler: Handler) {
        let entered = usize::from(handler.with_depth);
        match home {
            HandlerHome::Frame(frame) => self.with_stack.truncate(self.frames[frame].with_base + entered),
            HandlerHome::Window(window) => self.lane_stack.unwind_with_objects(window.depth, entered),
        }
    }

    /// Stores the thrown `value` to the binding `slot` names in `home`: a promoted register, or a
    /// local, which a captured activation's handler entry gives a fresh environment
    /// (`Vm::initialize_handler_binding`, `Vm::bind_window_local`).
    pub fn bind_thrown_value(
        &mut self,
        program: &ResidualProgram,
        home: HandlerHome,
        slot: ThrownValueSlot,
        value: Value,
    ) -> Result<(), JsError> {
        let frame = match home {
            HandlerHome::Frame(frame) => frame,
            HandlerHome::Window(window) => return self.bind_window_value(program, window, slot, value),
        };
        match slot {
            ThrownValueSlot::Local(local) if program.kind == crate::bytecode::ProgramKind::Wasm => {
                self.frames[frame].locals[usize::from(local)] = value;
                Ok(())
            }
            ThrownValueSlot::Local(local) => {
                self.initialize_handler_binding(program, frame, local, value)
            }
            // A promoted binding is a plain local, which no environment captures.
            ThrownValueSlot::Register(register) => {
                self.frames[frame].registers[usize::from(register)] = value;
                Ok(())
            }
        }
    }

    /// `bind_thrown_value` in a lane activation's window.
    fn bind_window_value(
        &mut self,
        program: &ResidualProgram,
        window: WindowHome,
        slot: ThrownValueSlot,
        value: Value,
    ) -> Result<(), JsError> {
        match slot {
            // SAFETY: the window holds the function's registers, which nothing else writes while
            // the handler enters.
            ThrownValueSlot::Register(register) => unsafe { window.window.add(usize::from(register)).write(value) },
            ThrownValueSlot::Local(local) => return self.bind_window_local(program, window, local, value),
        }
        Ok(())
    }

    /// Stores the thrown `value` to local `slot` of an environment window, as
    /// `Vm::initialize_handler_binding` does for a frame: a slot the window's environment owns
    /// once a closure made it gets a fresh copy of the environment per entry, which closures of
    /// earlier entries do not share (`Vm::iteration_environment`); any other slot is among the
    /// window's locals.
    fn bind_window_local(
        &mut self,
        program: &ResidualProgram,
        window: WindowHome,
        local: u16,
        value: Value,
    ) -> Result<(), JsError> {
        let function = program.function(window.function as usize);
        let slot = usize::from(local);
        // SAFETY: an environment activation's window holds its entries after its registers
        // (`activate_environment_in_lane`), which nothing else writes while the handler enters.
        let entries = unsafe { window.window.add(usize::from(function.registers)) };
        let own = unsafe { *entries.add(usize::from(WINDOW_ENVIRONMENT)) };
        if own.is_deleted() || !function.local_slot_uses_environment(slot) {
            // SAFETY: as above, the locals follow the entries.
            unsafe { *entries.add(usize::from(WINDOW_LOCALS) + slot) = value };
            return Ok(());
        }
        let environment = self.iteration_environment(own, window.program, window.function, &[local], crate::bytecode::Resets::listed(&[]))?;
        let Some(crate::heap::EnvironmentRecord { slots, .. }) = self.heap.environment(environment) else {
            return Err(JsError("invalid handler binding".into()));
        };
        let storage = std::ptr::from_ref(slots);
        // SAFETY: as above; the copy's slot storage never moves while it lives.
        unsafe {
            entries.add(usize::from(WINDOW_ENVIRONMENT)).write(environment);
            entries.add(usize::from(WINDOW_SLOTS)).write(std::mem::transmute::<u64, Value>(storage as u64));
        }
        *self.heap.environment_slot_mut(environment, slot).ok_or_else(|| JsError("invalid handler binding".into()))? = value;
        Ok(())
    }
}
