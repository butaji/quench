//! A direct eval a lane activation runs without becoming a frame (`CallPlan::admits_direct_eval`).
//! The eval's variable environment is the activation's record (`VarOwner::Record`): an ordinary
//! activation has none, so the first eval makes it, owned by the lane stack until the activation
//! pops (`LaneStack::own_record`); a suspended function's is its owner's. The eval promotes the
//! window's environment into the record, as a closure made in the window would
//! (`window_environment`), so the record, which `CallerView` reads, holds the environment,
//! the dynamic bindings and the binding site the eval's code resolves names in. The eval's code
//! then runs above the activation as a nested lane run (`Vm::nested_lane_run`: its frames and
//! lane stack are its own, the activation's parked), which the activation continues from in the lane
//! when it returns, reading what the eval declared through the promoted environment
//! (`scoped_names`).
use super::*;
use super::scoped_names::window_scope;
use quench_runtime::vm::activation::FrameRecord;

#[quench_macros::ext(LaneStackOps10)]
impl LaneStack {
    /// Keeps `record`, made for the innermost activation, at `depth`, until that activation pops,
    /// and names it from the activation where it is (one-frame-one-call.md F4 (3)): a header's
    /// side record, which `frame::OWNS_FLAG` tags, or the record's `ScriptActivation::record`.
    fn own_record(&mut self, depth: usize, record: Box<FrameRecord>) -> *mut FrameRecord {
        debug_assert_eq!(depth + 1, self.activation_count(), "an eval runs in the innermost activation");
        self.records.push((depth, record));
        let record: *mut FrameRecord = &mut *self.records.last_mut().expect("the record just pushed").1;
        self.name_top_record(record);
        record
    }
}

/// The innermost activation's record and the entries of its window.
struct EvalWindow {
    record: *mut FrameRecord,
    entries: *mut Value,
}

/// The innermost activation made ready for the direct eval at `ip`: its record, made if it has
/// none, holds the environment its window made or now makes, the locals, the binding site and
/// the `with` chain of the call. `None`, before any effect, where the activation is no window
/// of a function that admits a direct eval, or its parameters' eval raises (`Function::parameter_eval_arguments_error`),
/// which the general step does.
fn promote_window(cx: *mut LaneContext, r: Registers, ip: Ip, parameter_eval: bool) -> Option<EvalWindow> {
    let vm = unsafe { vm(cx) };
    let scope = window_scope(cx, r, ip, true)?;
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let body = code.function(scope.function as usize);
    if !body.call_plan(code).admits_direct_eval || parameter_eval && body.parameter_eval_arguments_error {
        return None;
    }
    // SAFETY: the running lane's context, which nothing else accesses.
    let (this, env) = unsafe { ((*cx).registers.this(), (*cx).registers.env()) };
    let record = if scope.record.is_null() {
        let depth = vm.lane_stack.activation_count() - 1;
        let callee = vm.lane_stack.top_frame().and_then(|frame| frame.script())?.callee;
        vm.lane_stack.own_record(depth, FrameRecord::for_eval(scope.program, scope.function, callee, this, env))
    } else {
        scope.record
    };
    // SAFETY: the window's entries follow the function's registers (`activate_environment_in_lane`).
    let entries = unsafe { r.0.add(usize::from(body.registers)) };
    // SAFETY: the record is the activation's own, which nothing else accesses during the call;
    // its held bindings pass to the environment, as a closure's creation passes them.
    unsafe {
        let held = (*record).take_dynamic_bindings();
        window_environment(vm, code, entries, scope.function, env, scope.program, this, Some(held));
        (*record).pull_window(entries, usize::from(body.locals));
        (*record).replace_binding_site_pc(scope.site);
        let chain = scope.chain.into_vec();
        if !chain.is_empty() || !(*record).with_objects().is_empty() {
            *(*record).with_objects_mut() = chain;
        }
    }
    Some(EvalWindow { record, entries })
}

/// Hands `record` back after a direct eval ran over it: the receiver the eval replaced, the binding
/// site and the `with` scopes it left, and the window's locals, whose stores went through the record.
///
/// # Safety
/// `record` and `entries` are the window `promote_window` made.
unsafe fn settle_eval_record(record: *mut FrameRecord, previous_this: Option<Value>, entries: *mut Value) {
    // SAFETY: the caller's contract.
    unsafe {
        if let Some(this) = previous_this {
            (*record).replace_this(this);
        }
        (*record).end_eval_scope();
        (*record).push_window_locals(entries);
    }
}

/// The value of the direct eval the call record `i` at `ip` of the innermost window activation
/// makes, which runs above the activation with the record as its caller (`Vm::eval_callers`).
/// A raised error is stashed in `Vm::lane_raised` for the frames the general path makes to raise,
/// and `None` is returned, as for any step the lane makes in place (`in_place_in_lane`).
fn direct_eval_in_window(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 || !vm.calls_direct_eval(callee) {
        return None;
    }
    let parameter_eval = ImmediateLayout::parameter_eval(i.imm());
    let EvalWindow { record, entries } = promote_window(cx, r, ip, parameter_eval)?;
    // SAFETY: the context's program outlives the lane run; the call's window is registers of
    // the running function.
    let (code, args) = unsafe { (&*(*cx).code, call_window_in_lane(r, i)) };
    let receiver = r.get(i.c);
    let previous = (std::mem::replace(&mut vm.direct_eval, true), std::mem::replace(&mut vm.parameter_eval, parameter_eval));
    // SAFETY: as `promote_window`.
    let previous_this = (!receiver.is_undefined()).then(|| unsafe { (*record).replace_this(receiver) });
    vm.eval_callers.push(EvalCaller { frames: vm.frames.len(), record });
    // SAFETY: the running lane's context.
    let innermost = unsafe { [(*cx).registers.this(), (*cx).registers.env()] };
    let result = vm.nested_lane_run(innermost, |vm| vm.call_value(code, callee, receiver, args));
    vm.eval_callers.pop();
    (vm.direct_eval, vm.parameter_eval) = previous;
    // SAFETY: as above.
    unsafe { settle_eval_record(record, previous_this, entries) };
    note_allocation(vm);
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            vm.lane_completion.raise(error);
            None
        }
    }
}

script_handler! {
    /// The direct-eval call row of a window record (`window_records`, `WindowRecord::Eval`): the call-out
    /// the window's activation makes (`direct_eval_in_window`), its value in the call's register. A call the
    /// window declines, or whose eval raised, leaves to `direct_eval_miss`.
    #[inline(never)]
    fn direct_eval_row<>(cx, r, ip, i, view) {
        let Some(value) = direct_eval_in_window(cx, r, ip, i) else {
            sibling!(direct_eval_miss, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// The miss arm of the direct-eval row: the general step runs the call, which makes the eval's
    /// own frames and raises its errors.
    #[inline(never)]
    fn direct_eval_miss<>(cx, r, ip, i, view) {
        exit(ip)
    }
}

/// The handler of a direct eval call in a function whose window activations run it.
pub(crate) const fn direct_eval_row_handler() -> Handler {
    direct_eval_row
}
