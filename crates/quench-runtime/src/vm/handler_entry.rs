//! Entering an exception handler: the frame state settled before the handler's block runs,
//! shared by the interpreter's unwinding (`Vm::exception_handler_target`) and a compiled
//! body's native catch (`dispatch_fast::lane_handler_edge`).
use super::*;
use crate::bytecode::{Handler, ThrownValueSlot};

impl<H: Host> Vm<H> {
    /// Leaves the scopes of the `with` statements `frame`'s exception left: those past the
    /// depth `handler` was entered at.
    pub(super) fn unwind_handler_scopes(&mut self, frame: usize, handler: Handler) {
        self.with_stack
            .truncate(self.frames[frame].with_base + usize::from(handler.with_depth));
    }

    /// Stores the thrown `value` to the binding `slot` names in `frame`: a promoted register,
    /// or a local, which a captured frame's handler entry gives a fresh environment
    /// (`Vm::initialize_handler_binding`).
    pub(super) fn bind_thrown_value(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        slot: ThrownValueSlot,
        value: Value,
    ) -> Result<(), JsError> {
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
}
