//! The native stack guard a general dispatch loop run holds.

use super::*;

impl<H: Host> Vm<H> {
    /// The native stack guard of a general loop run. A lane exit's activations are already
    /// frames, so exhaustion on resuming them is raised at the exit's record (`raised`) and
    /// unwinds them, as an error of the lane's call would; any other entry fails with it.
    #[allow(clippy::type_complexity)]
    pub(super) fn enter_general_loop_stack(
        &mut self,
        kind: crate::bytecode::ProgramKind,
        raised: Option<Option<JsError>>,
    ) -> Result<(Option<crate::stack::StackGuard>, Option<Option<JsError>>), JsError> {
        if kind == crate::bytecode::ProgramKind::Wasm {
            let guard = crate::stack::StackGuard::enter()
                .map_err(|crate::stack::StackExhausted| JsError::wasm_trap_error(crate::WasmTrap::CallStackExhausted))?;
            return Ok((Some(guard), raised));
        }
        match (self.enter_stack(), raised) {
            (Ok(guard), raised) => Ok((Some(guard), raised)),
            (Err(error), Some(raised)) => Ok((None, Some(raised.or(Some(error))))),
            (Err(error), None) => Err(error),
        }
    }
}
