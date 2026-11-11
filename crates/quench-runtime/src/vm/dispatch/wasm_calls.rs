//! Wasm calls the general loop activates in place rather than on the native stack.

use super::*;

impl<H: Host> Vm<H> {
    /// The function of the running Wasm instance that `callee` names, if it names one.
    fn same_instance_wasm_function(&self, f: usize, callee: Value) -> Option<u32> {
        let (program, id, env) = self.same_realm_user_target(callee)?;
        (program == self.active_program && env == self.frames[f].env).then_some(id)
    }

    /// A non-tail Wasm reference call (`call_ref`, `call_indirect`, lowered to `Call`) of a
    /// function of the running instance, activated as a known call is; `None` leaves any other
    /// call to `step`.
    pub(super) fn wasm_reference_call_in_loop(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: usize,
    ) -> Option<Result<StepResult, JsError>> {
        if i.op() != Op::Call || p.kind != crate::bytecode::ProgramKind::Wasm || i.returns_from_frame() {
            return None;
        }
        let id = self.same_instance_wasm_function(f, self.read(f, i.register_b()))?;
        self.profile.call_source(CallSource::Dynamic);
        Some(self.call_wasm_in_loop(p, f, id, i, pc))
    }

    /// A non-tail call of function `id` of the running Wasm instance: its compiled body when
    /// one runs it, else an activation of the running loop, counted against the guest budget.
    pub(super) fn call_wasm_in_loop(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        id: u32,
        i: WideInstruction,
        pc: usize,
    ) -> Result<StepResult, JsError> {
        if let Some(result) = self.call_compiled(f, id, i.call_window()) {
            let value = result?;
            self.write(f, i.result_register(), value);
            return Ok(StepResult::Continue);
        }
        self.frames[f].pc = pc;
        self.frames[f].pc_is_resume = true;
        let call_guard = self.push_wasm_frame(p, f, id, i.call_window())?;
        Ok(StepResult::PushFrame {
            destination: i.result_register(),
            call_guard,
            completion: CallCompletion::Value,
        })
    }
}
