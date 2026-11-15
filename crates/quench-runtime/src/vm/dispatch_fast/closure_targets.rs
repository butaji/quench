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
        if entry.admission != self.lane_stack.admission {
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
        if entry.admission != self.lane_stack.admission
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
        // A sibling over the entry's own environment, as closures made over their maker's
        // parent are (`Vm::maker_free_environment`), shares every fact the entry read of it:
        // its captures and an arrow's lexical `this`.
        if env == entry.env {
            return Some(ScriptCallTarget { callee, ..*entry });
        }
        if !entry.this_binding.binds_receiver() {
            return None;
        }
        // A closure of the same function over the same environment, as an immediately
        // invoked function expression makes at each run of its enclosing body: every fact
        // of the entry holds for it but its identity.
        if entry.view.is_null() {
            return Some(ScriptCallTarget { callee, env, ..*entry });
        }
        if entry.capture.is_null() {
            return None;
        }
        let storage = self.sibling_environment_storage(entry.env, env)?;
        let stores = entry.capture.addr() & CAPTURE_STORES;
        if stores != 0 {
            self.heap.remember_cell(env);
        }
        Some(ScriptCallTarget { callee, env, capture: storage.map_addr(|address| address | stores), ..*entry })
    }

    /// The dense storage of `env`, when it is an activation of the same function in the same
    /// program as the environment `old_env`.
    #[inline(always)]
    fn sibling_environment_storage(&self, old_env: Value, env: Value) -> Option<*const usize> {
        let Some(Cell::Environment { function: old, scope: old_scope, .. }) = self.heap.get(old_env) else {
            return None;
        };
        let (old, old_program) = (*old, old_scope.program());
        let Some(Cell::Environment { function, slots, scope, .. }) = self.heap.get(env) else {
            return None;
        };
        if *function != old || scope.program() != old_program {
            return None;
        }
        slots.dense_storage()
    }
}

/// The entry a call of `body` takes, whose resolution `shape` records: settling, an environment
/// activation, a gather of rest parameters or a keeping of the arguments take their own, a class
/// constructor its first resolution's, and any other the activation or leaf its shape allows.
pub(super) fn target_entry(body: &crate::bytecode::Function, plan: &crate::bytecode::CallPlan, shape: &TargetBody, (settles, environment, cross): (bool, bool, bool)) -> CallEntry {
    if settles {
        CallEntry::Settles
    } else if body.is_class_constructor {
        shape.entry
    } else if environment {
        CallEntry::Environment
    } else if body.rest {
        CallEntry::Gathers
    } else if plan.keeps_arguments {
        CallEntry::Keeps
    } else if cross {
        // A leaf runs as one across programs too (`lane_cross_leaf_call`); no other
        // body form's answer reads another program's caches.
        shape.leaf.map_or(CallEntry::Activates, CallEntry::leaf)
    } else {
        // Leaf-ness is the view's, fixed with its returns (`TargetBody::leaf`).
        shape.leaf.map_or(shape.entry, CallEntry::leaf)
    }
}
