//! The generation a lane stack resolves under (`LaneStack::generation`): every cached target and
//! memo names the one it was resolved in, and a new one retires them all at once.
use super::*;

#[quench_macros::ext(LaneStackOps9)]
impl LaneStack {
    /// The generation of a lane run under `key`: the current one while the key holds, else
    /// a new one, which retires every target resolved before.
    pub(super) fn enter_generation(&mut self, key: (u64, Value)) {
        // The bounds this run's native calls are checked against (`Vm::call_native_from_lane`).
        self.native_stack = quench_runtime::stack::StackLimit::here();
        if self.generation == NO_SCRIPT_GENERATION || self.generation_key != key {
            self.generation_key = key;
            self.advance_generation();
        }
    }

    /// Takes a new generation, which retires every target and memo resolved before
    /// (`enter_generation`, `Vm::enter_with_scope`).
    pub(crate) fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        // An in-place native row's admitted callee names a target of the generation retired here.
        self.native_identities.forget();
        if self.generation == NO_SCRIPT_GENERATION {
            // A wrapped number could name entries of a generation long past.
            self.clear_script_targets();
            self.generation = NO_SCRIPT_GENERATION + 1;
        }
        self.admission = Admission(Admission::new(self.generation, self.admission.program()).0 | self.refused);
    }
}
