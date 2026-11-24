//! F4: a function's call target serves each of its closures, whose own facts alone are
//! rebuilt when a call site meets a fresh one.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

#[quench_macros::ext(DispatchFastClosureTargetsVm)]
impl Vm {
    /// Whether plain calls of `body` (`plan`) may settle in the caller's lane: of the running
    /// program, unless `framed` (across programs, or numeric), under a host that observes no
    /// promise lifecycle; and whether they would settle from an environment activation
    /// (`settles_in_environment`).
    pub(crate) fn settling_calls(&self, body: &quench_runtime::bytecode::Function, plan: &quench_runtime::bytecode::CallPlan, framed: bool) -> (bool, bool) {
        let may_settle = !framed && !self.host.observes_promise_lifecycle();
        (may_settle, may_settle && settles_in_environment(body, plan))
    }

    /// `same_realm_user_target` of a callee the lane may resolve a target for: not a function of
    /// eval code once a `with` statement was entered, whose frame takes the chain its environment
    /// captured (`Vm::captured_with_objects_for_function`), which a lane activation does not
    /// push; its calls take the general path's frame (`frame_call_in_lane`).
    pub(crate) fn user_target_in_lane(&self, callee: Value) -> Option<(ProgramId, u32, Value)> {
        let target = self.same_realm_user_target(callee)?;
        let takes_with_chain = self.with_scope_entered
            && self.programs.residual(target.0).is_some_and(|code| code.kind == quench_runtime::bytecode::ProgramKind::Eval);
        (!takes_with_chain).then_some(target)
    }

    /// `known`, a function's target, for its closure `callee` of `env`: only the closure's facts read.
    pub(crate) fn closure_target(&mut self, code: &ResidualProgram, program: ProgramId, callee: Value, env: Value,
        known: ScriptCallTarget) -> Option<ScriptCallTarget> {
        let capture = if known.view.is_null() { std::ptr::null() } else { self.target_capture(code, program, env) };
        // An arrow's `this` is its own closure's environment's, like `capture`.
        let (this_binding, lexical_this) = match known.this_binding {
            ThisBinding::Lexical | ThisBinding::CallerReceiver => match self.captured_lexical_this(env) {
                Some(this) if this.is_deleted() => return None,
                Some(this) => (ThisBinding::Lexical, this),
                None => (ThisBinding::CallerReceiver, Value::UNDEFINED),
            },
            // A sloppy function's plain `this`, the global object, is its function's.
            binding => (binding, known.lexical_this),
        };
        Some(ScriptCallTarget { callee, env, capture, this_binding, lexical_this, ..known })
    }

    /// `callee`'s entry, made from the one `linked` names where that holds another closure of
    /// `callee`'s function in the running program (F4): the function's target is kept and
    /// only the closure's facts are read (`closure_target`), with no resolution; it joins
    /// `callee`'s own set, where every lookup finds it.
    #[cold]
    #[inline(never)]
    pub(crate) fn relink_closure(&mut self, callee: Value, linked: usize) -> Option<usize> {
        let entry = *self.lane_stack.script_targets.entries.get(linked)?;
        if !entry.admission.admits(self.lane_stack.admission) {
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
        self.lane_stack.remember_script_target(target, self.heap_reads().memory_policy());
        self.script_call_index(callee)
    }

    /// `relink_closure` for a call-only closure (`Value::known_closure`): its function id and
    /// environment are in the value, and every environment it names is an activation of its
    /// function's enclosing one in the running program, so the site's entry for that function
    /// turns to it with only its environment's storage read.
    #[inline(never)]
    pub(crate) fn relink_known_closure(&mut self, callee: Value, linked: usize) -> Option<usize> {
        let (function, env) = callee.as_known_closure()?;
        let entry = self.lane_stack.script_targets.entries.get(linked)?;
        if !entry.admission.admits(self.lane_stack.admission)
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
            let Some(quench_runtime::tier_surface::EnvironmentRecord { slots, .. }) = self.heap_reads().environment(env) else {
                return None;
            };
            let storage = slots.dense_storage()?;
            if stores != 0 {
                self.write_barrier(env);
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
    /// program: their capture facts then match (`target_capture`); an arrow's lexical `this`
    /// is read from its own environment. Anything else resolves in `closure_target`.
    #[inline(always)]
    fn sibling_closure_target(&mut self, entry: &ScriptCallTarget, callee: Value, env: Value) -> Option<ScriptCallTarget> {
        // A sibling over the entry's own environment, as closures made over their maker's
        // parent are (`Vm::maker_free_environment`), shares every fact the entry read of it:
        // its captures and an arrow's lexical `this`.
        if env == entry.env {
            return Some(ScriptCallTarget { callee, ..*entry });
        }
        let mut target = self.sibling_capture_target(entry, callee, env)?;
        // An arrow's `this` is its own environment's: read for the new one, as
        // `closure_target` does, with every other fact kept.
        // An arrow that reads none of its maker's implicit bindings keeps the entry's.
        if entry.forms & TARGET_READS_IMPLICIT != 0 {
            (target.this_binding, target.lexical_this) = match self.captured_lexical_this(env) {
                Some(this) if this.is_deleted() => return None,
                Some(this) => (ThisBinding::Lexical, this),
                None => (ThisBinding::CallerReceiver, Value::UNDEFINED),
            };
        }
        Some(target)
    }

    /// `sibling_closure_target`'s entry for `callee` over `env`, its captures read.
    #[inline(always)]
    fn sibling_capture_target(&mut self, entry: &ScriptCallTarget, callee: Value, env: Value) -> Option<ScriptCallTarget> {
        // A closure of the same function over the same environment, as an immediately
        // invoked function expression makes at each run of its enclosing body: every fact
        // of the entry holds for it but its identity.
        if entry.view.is_null() || env == entry.env {
            return Some(ScriptCallTarget { callee, env, ..*entry });
        }
        if entry.capture.is_null() {
            return None;
        }
        let Some(quench_runtime::tier_surface::EnvironmentRecord { function: old, scope: old_scope, .. }) = self.heap_reads().reachable_environment(entry.env) else {
            return None;
        };
        let (old, old_program) = (*old, old_scope.program());
        let Some(quench_runtime::tier_surface::EnvironmentRecord { function, slots, scope, .. }) = self.heap_reads().reachable_environment(env) else {
            return None;
        };
        if *function != old || scope.program() != old_program {
            return None;
        }
        let storage = slots.dense_storage()?;
        let stores = entry.capture.addr() & CAPTURE_STORES;
        if stores != 0 {
            self.write_barrier(env);
        }
        Some(ScriptCallTarget { callee, env, capture: storage.map_addr(|address| address | stores), ..*entry })
    }
}

/// The entry a call of `body` takes, whose resolution `shape` records: settling, an environment
/// activation, a gather of rest parameters or a keeping of the arguments take their own, a class
/// constructor its first resolution's, and any other the activation or leaf its shape allows.
pub(crate) fn target_entry(body: &quench_runtime::bytecode::Function, plan: &quench_runtime::bytecode::CallPlan, shape: &TargetBody, (settles, environment, cross): (bool, bool, bool)) -> CallEntry {
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
        // A leaf runs as one across programs too (`cross_leaf_call_in_lane`); no other
        // body form's answer reads another program's caches.
        shape.leaf.map_or(CallEntry::Activates, CallEntry::leaf)
    } else {
        // Leaf-ness is the view's, fixed with its returns (`TargetBody::leaf`).
        shape.leaf.map_or(shape.entry, CallEntry::leaf)
    }
}

/// Whether async `body`, whose plan is `plan`, settles its call's promise from an environment
/// activation (`TARGET_SETTLES_IN_ENVIRONMENT`): a plain async function whose suspension keeps
/// its environment and locals (`CallPlan::suspended_environment`), laid out as an environment
/// activation lays out an ordinary body (`CallPlan::environment_activation`): each parameter a
/// frame local, every register starting `undefined`. Its `await` suspends it in place, its frame
/// taking the window's environment (`generator_yields::suspend_settling`).
pub(crate) fn settles_in_environment(body: &quench_runtime::bytecode::Function, plan: &quench_runtime::bytecode::CallPlan) -> bool {
    body.is_async
        && !body.is_generator
        && !body.is_class_constructor
        && !body.derived_constructor
        && plan.suspended_environment
        && body.parent.is_some()
        && body.local_registers.is_empty()
        && body.initial_register == quench_runtime::bytecode::InitialRegister::Undefined
}
