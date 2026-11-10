//! F4: a function's call target serves each of its closures, whose own facts alone are
//! rebuilt when a call site meets a fresh one.
use super::*;

impl<H: Host> Vm<H> {
    /// `known`, a function's target, for its closure `callee` of `env`: only the closure's facts read.
    pub(super) fn closure_target(&mut self, code: &ResidualProgram, program: ProgramId, callee: Value, env: Value,
        known: ScriptCallTarget) -> Option<ScriptCallTarget> {
        let capture = if known.view.is_null() { std::ptr::null() } else { self.target_capture(code, program, env) };
        // An arrow's `this` is its own closure's environment's, like `capture`.
        let (this_binding, lexical_this) = match known.this_binding {
            ThisBinding::Lexical | ThisBinding::CallerReceiver => match self.captured_lexical_this(env) {
                Some(this) if this.is_deleted() => return None,
                Some(this) => (ThisBinding::Lexical, this),
                None => (ThisBinding::CallerReceiver, Value::UNDEFINED),
            },
            binding => (binding, Value::UNDEFINED),
        };
        Some(ScriptCallTarget { callee, env, capture, this_binding, lexical_this, ..known })
    }

    /// `callee`'s entry, made from the one `linked` names where that holds another closure of
    /// `callee`'s function in the running program (F4): the function's target is kept and
    /// only the closure's facts are read (`closure_target`), with no resolution; it joins
    /// `callee`'s own set, where every lookup finds it.
    #[cold]
    #[inline(never)]
    pub(super) fn relink_closure(&mut self, callee: Value, linked: usize) -> Option<usize> {
        let entry = *self.lane_stack.script_targets.entries.get(linked)?;
        if entry.generation != self.lane_stack.generation || entry.program != self.active_program {
            return None;
        }
        let (program, function, env) = self.same_realm_user_target(callee)?;
        if program != entry.program || function != entry.function {
            return None;
        }
        let known = self.lane_stack.function_target(program, function, false)?;
        // SAFETY: no program is installed or retired while a call resolves.
        let code = unsafe { &*std::ptr::from_ref(self.programs.residual(program)?) };
        let target = self.closure_target(code, program, callee, env, known)?;
        self.lane_stack.remember_script_target(target, self.heap.memory_policy());
        self.script_call_index(callee)
    }
}
