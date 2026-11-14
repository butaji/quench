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
        if entry.generation() != self.lane_stack.generation || entry.program() != self.active_program {
            return None;
        }
        let (program, function, env) = self.same_realm_user_target(callee)?;
        if program != entry.program() || function != entry.function {
            return None;
        }
        if let Some(target) = self.sibling_closure_target(&entry, callee, env) {
            // The site's entry turns to the closure in hand in place: a site meeting each
            // fresh closure of one function in turn keeps one entry, with no set displaced.
            self.lane_stack.script_targets.entries[linked] = target;
            return Some(linked);
        }
        let known = self.lane_stack.function_target(program, function, false)?;
        // SAFETY: no program is installed or retired while a call resolves.
        let code = unsafe { &*std::ptr::from_ref(self.programs.residual(program)?) };
        let target = self.closure_target(code, program, callee, env, known)?;
        self.lane_stack.remember_script_target(target, self.heap.memory_policy());
        self.script_call_index(callee)
    }

    /// `relink_closure` for a call-only closure (`Value::known_closure`): its function id and
    /// environment are in the value, and every environment it names is an activation of its
    /// function's enclosing one in the running program, so the site's entry for that function
    /// turns to it with only its environment's storage read.
    #[inline(never)]
    pub(super) fn relink_known_closure(&mut self, callee: Value, linked: usize) -> Option<usize> {
        let (function, env) = callee.as_known_closure()?;
        let entry = self.lane_stack.script_targets.entries.get(linked)?;
        if entry.generation() != self.lane_stack.generation
            || entry.program() != self.active_program
            || entry.function != function
            || !entry.this_binding.binds_receiver()
        {
            return None;
        }
        let capture = if entry.view.is_null() {
            entry.capture
        } else {
            if entry.capture.is_null() {
                return None;
            }
            let stores = entry.capture.addr() & CAPTURE_STORES;
            let Some(Cell::Environment { slots, .. }) = self.heap.get(env) else {
                return None;
            };
            let storage = slots.dense_storage()?;
            if stores != 0 {
                self.heap.remember_cell(env);
            }
            storage.map_addr(|address| address | stores)
        };
        let entry = &mut self.lane_stack.script_targets.entries[linked];
        entry.callee = callee;
        entry.env = env;
        entry.capture = capture;
        Some(linked)
    }

    /// `entry`'s target for `callee`, another closure of its function, read from the entry
    /// alone where the closures' environments are activations of one function in one
    /// program: their capture facts then match (`target_capture`), and a receiver-binding
    /// callee has no lexical `this` to read. Anything else resolves in `closure_target`.
    #[inline(always)]
    fn sibling_closure_target(&mut self, entry: &ScriptCallTarget, callee: Value, env: Value) -> Option<ScriptCallTarget> {
        if !entry.this_binding.binds_receiver() {
            return None;
        }
        if entry.view.is_null() {
            return Some(ScriptCallTarget { callee, env, ..*entry });
        }
        if entry.capture.is_null() {
            return None;
        }
        let Some(Cell::Environment { function: old, scope: old_scope, .. }) = self.heap.get(entry.env) else {
            return None;
        };
        let (old, old_program) = (*old, old_scope.program());
        let Some(Cell::Environment { function, slots, scope, .. }) = self.heap.get(env) else {
            return None;
        };
        if *function != old || scope.program() != old_program {
            return None;
        }
        let storage = slots.dense_storage()?;
        let stores = entry.capture.addr() & CAPTURE_STORES;
        if stores != 0 {
            self.heap.remember_cell(env);
        }
        Some(ScriptCallTarget { callee, env, capture: storage.map_addr(|address| address | stores), ..*entry })
    }
}
