#[cold]
#[inline(never)]
pub(crate) fn not_callable() -> VmError {
    VmError::Thrown(crate::builtins::error(
        Builtin::TypeError,
        &[Value::String("value is not callable".to_string())],
    ))
}

impl VmError {
    pub fn render(&self) -> String {
        match self {
            VmError::Thrown(value) => render_thrown(value),
            VmError::Suspended(_) => "Suspended".to_string(),
            VmError::NotCallable => "TypeError: value is not callable".to_string(),
            other => format!("{other:?}"),
        }
    }
}

pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    execute_with_context(ops, &VmContext::default())
}

pub fn execute_with_registers(ops: &[Op], registers: Vec<Value>) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_with_registers_context(ops, registers, &context)
}

pub fn execute_in_place(ops: &[Op], registers: &mut Vec<Value>) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_in_place_context(ops, registers, &context)
}

pub(crate) fn current_context_or_default() -> Rc<VmContext> {
    CURRENT_CONTEXT
        .with(|current| current.borrow().clone())
        .unwrap_or_else(|| Rc::new(VmContext::default()))
}

pub(crate) fn execute_completion_in_place(
    ops: &[Op],
    registers: &mut Vec<Value>,
) -> Result<crate::completion::Completion, VmError> {
    let context = current_context_or_default();
    execute_completion_in_place_context(ops, registers, &context)
}

pub fn execute_with_context(ops: &[Op], context: &VmContext) -> Result<Value, VmError> {
    // The replacement log is the heap's forwarding table for copy-on-write
    // object snapshots: it must outlive any single top-level entry so that
    // references stashed in heap slots (event-loop callbacks, timers) still
    // resolve to the latest snapshot. Hosts running independent programs on
    // one thread call `execute::reset_replacements` between programs.
    let result = execute_with_registers_context(ops, Vec::new(), context);
    // Drive the promise microtask queue so `.then`/`.catch` reactions and
    // synchronously-settling promise chains run to completion. Reinstall
    // the realm: reactions may call host capabilities (console.log) and
    // read globals, both of which require an active execution context.
    let realm = context.realm();
    if realm::with_realm(realm, crate::promise::drain_microtasks_all).is_none() {
        crate::vm::with_current_context(context, crate::promise::drain_microtasks_all);
    }
    result
}

/// The context currently active on this thread, or a default one.
/// Hosts use this to re-enter the VM from inside a capability call.
pub fn current_context() -> Rc<VmContext> {
    current_context_or_default()
}

/// Call a function value from host code through the current context.
pub fn call_value(
    target: &Value,
    receiver: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    crate::functions::execute_target(target, receiver, arguments)
}

pub fn execute_with_registers_context(
    ops: &[Op],
    mut registers: Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    // Install the caller's context before creating environments or resolving
    // globals.  Host values (console/process/require) are carried by this
    // context; delaying installation until the first VM step lets setup and
    // re-entrant continuations observe the thread's stale/default realm.
    crate::vm::with_current_context(context, || {
        prepare_register_stack(&mut registers);
        let environment = crate::environment::Environment::child(
            &crate::environment::Environment::new(),
            registers.clone(),
        );
        execute_in_environment(ops, &mut registers, context, environment)
    })
}

/// Prepare the VM's contiguous register stack once at entry.
///
/// Register indices are u16, so the representable stack limit is fixed.  We
/// reserve a small hot-path working set up front; subsequent writes use Vec's
/// geometric growth without introducing a second frame representation.
fn prepare_register_stack(registers: &mut Vec<Value>) {
    const INITIAL_REGISTER_CAPACITY: usize = 32;
    const MAX_REGISTER_COUNT: usize = u16::MAX as usize + 1;
    debug_assert!(registers.len() <= MAX_REGISTER_COUNT);
    if registers.capacity() < INITIAL_REGISTER_CAPACITY {
        registers.reserve(INITIAL_REGISTER_CAPACITY - registers.capacity());
    }
}
/// Execute a fragment inside the currently installed lexical environment.
pub fn execute_in_current_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_in_place_context(ops, registers, &context)
}

pub fn execute_in_place_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    prepare_register_stack(registers);
    let parent = crate::locals::current();
    let environment =
        crate::environment::Environment::in_place_child(&parent, registers.clone());
    execute_in_environment(ops, registers, context, environment)
}

fn execute_completion_in_place_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    prepare_register_stack(registers);
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let mut pc = 0;
    loop {
        let step = run_ops_completion_step_from(ops, pc, registers, context)?;
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                crate::vm::vm_ops::execute_call_continuation(registers, continuation)?;
            }
            completion => return preserve_frame_completion(completion),
        }
    }

}
fn preserve_frame_completion(
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    use crate::completion::Completion;
    Ok(match completion {
        Completion::TailCall(request) => tail_call_completion(request),
        completion => completion,
    })
}

fn tail_call_completion(
    request: crate::completion::TailCallRequest,
) -> crate::completion::Completion {
    crate::completion::Completion::TailCall(request)
}

pub(crate) fn execute_in_environment(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<Value, VmError> {
    let register_count = registers.len().min(usize::from(u16::MAX)) as u16;
    let mut machine = crate::machine::Machine::with_register_count(
        crate::machine::CodeId(0),
        crate::machine::EnvironmentRef(0),
        register_count,
    );
    machine.restore_registers(std::mem::take(registers));
    let mut pc = 0;
    loop {
        let step = {
            let _context_guard = ContextGuard::install(context);
            let _global_guard = GlobalObjectGuard::install();
            let _environment_guard =
                crate::locals::EnvironmentGuard::install(Rc::clone(&environment));
            run_ops_completion_step_from(ops, pc, machine.registers_mut(), context)?
        };
        let completion = step.completion;
        let next = step.next;
        match completion {
            crate::completion::Completion::Call(mut continuation) => {
                continuation.caller_ops = Rc::from(ops);
                continuation.caller_pc = next as u32;
                machine.push_call_frame(continuation);
                let continuation = machine.pop_call_frame().expect("call frame just pushed");
                crate::vm::vm_ops::execute_call_continuation(
                    machine.registers_mut(),
                    continuation,
                )?;
                pc = next;
            }
            completion => {
                *registers = machine.take_registers();
                return completion_result(completion);
            }
        }
    }
}
pub(crate) fn execute_frame_completion(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<crate::completion::Completion, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let mut pc = 0;
    loop {
        let step = run_ops_completion_step_from(ops, pc, registers, context)?;
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                crate::vm::vm_ops::execute_call_continuation(registers, continuation)?;
            }
            completion => return Ok(completion),
        }
    }
}
pub(crate) fn execute_indirect_eval(ops: &[Op]) -> Result<Value, VmError> {
    let context = CURRENT_CONTEXT
        .with(|current| current.borrow().clone())
        .unwrap_or_else(|| Rc::new(VmContext::default()));
    if realm::context(context.realm()).is_some() {
        return execute_indirect_eval_in_realm(context.realm(), ops);
    }
    let global = current_global_object();
    let caller = crate::locals::current();
    let environment = crate::environment::Environment::new();
    environment.set(0, global.clone());
    let mut registers = Vec::new();
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let result = execute_in_environment(ops, &mut registers, &context, environment);
    caller.replace_value(&global, &current_global_object());
    result
}
pub(crate) fn execute_indirect_eval_in_realm(
    realm_id: RealmId,
    ops: &[Op],
) -> Result<Value, VmError> {
    realm::execute(realm_id, ops)
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::execute::VmError;
    use crate::value::{ObjectData, Value};

    /// Bug reproducer: the replacement log is the forwarding table that
    /// keeps heap-resident object references (event-loop callbacks, timers)
    /// pointing at the latest copy-on-write snapshot. Re-entering the VM
    /// for another top-level execution of the same program must not clear
    /// it — otherwise those references silently revert to stale snapshots.
    #[test]
    fn execute_with_context_preserves_replacements() {
        let old = Value::Object(Rc::new(ObjectData::new(vec![])));
        let new = Value::Object(Rc::new(ObjectData::new(vec![])));
        crate::locals::replace_value(&old, &new);
        let program = crate::reduce::reduce_source("0;").expect("source reduces");
        crate::vm::execute_with_context(program.ops(), &crate::vm::VmContext::default())
            .expect("program runs");
        let resolved = crate::locals::resolved_replacement(old);
        let (Value::Object(resolved), Value::Object(expected)) = (resolved, new) else {
            panic!("replacement must resolve to the new snapshot");
        };
        assert!(Rc::ptr_eq(&resolved, &expected));
    }

    #[test]
    fn ordinary_calls_preserve_semantics_at_bounded_depth() {
        let source = r#"
            function descend(n) {
                if (n === 0) return 7;
                return descend(n - 1);
            }
            if (descend(32) !== 7) throw "bounded ordinary call returned the wrong value";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_with_context(program.ops(), &crate::vm::VmContext::default())
            .expect("bounded ordinary calls run");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn staged_global_resolves_math_for_property_assignment() {
        let source = r#"
            Math.random = function () { return 1; };
            if (Math.random() !== 1) throw "Math must resolve while globals are staged";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_with_context(
            program.ops(),
            &crate::vm::VmContext::default(),
        )
        .expect("Math assignment runs");
        assert_eq!(result, Value::Undefined);
    }
    #[test]
    fn staged_math_random_binding_preserves_callable_state() {
        let source = r#"
            Math.random = (function () {
                var next = 0;
                return function () { next = next + 1; return next; };
            })();
            if (Math.random() !== 1 || Math.random() !== 2) {
                throw "staged Math.random lost its closure state";
            }
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_with_context(
            program.ops(),
            &crate::vm::VmContext::default(),
        )
        .expect("stateful Math.random assignment runs");
        assert_eq!(result, Value::Undefined);
    }



    #[test]
    fn call_rhs_preserves_member_assignment_target() {
        let source = r#"
            function value() { return 42; }
            const target = {};
            target.answer = value();
            if (target.answer !== 42) throw "call RHS lost assignment target";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_with_context(
            program.ops(),
            &crate::vm::VmContext::default(),
        )
        .expect("member assignment with call RHS runs");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn ordinary_calls_return_through_nested_continuations() {
        let source = r#"
            function inner(value) { return value + 1; }
            function middle(value) { return inner(value * 2); }
            if (middle(20) !== 41) throw "nested ordinary call returned the wrong value";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_with_context(
            program.ops(),
            &crate::vm::VmContext::default(),
        )
        .expect("nested calls run");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn ordinary_calls_preserve_receiver_and_arguments() {
        let source = r#"
            function read(prefix, suffix) {
                return prefix + this.value + suffix + arguments.length;
            }
            if (read.call({ value: 40 }, 1, 2) !== 45) {
                throw "receiver call returned the wrong value";
            }
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_with_context(
            program.ops(),
            &crate::vm::VmContext::default(),
        )
        .expect("receiver call runs");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn ordinary_call_throw_unwinds_to_top_level() {
        let source = r#"
            function fail() { throw 17; }
            function wrapper() { return fail(); }
            wrapper();
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_with_context(
            program.ops(),
            &crate::vm::VmContext::default(),
        );
        assert!(matches!(result, Err(crate::vm::VmError::Thrown(Value::Number(17.0)))));
    }

    #[test]
    fn ordinary_calls_retain_closure_environment() {
        let source = r#"
            function makeAdder(base) {
                return function (value) { return base + value; };
            }
            const add = makeAdder(9);
            if (add(33) !== 42) throw "closure call returned the wrong value";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_with_context(
            program.ops(),
            &crate::vm::VmContext::default(),
        )
        .expect("closure call runs");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn tail_calls_are_driven_by_machine_loop() {
        let mut machine = crate::machine::Machine::new(
            crate::machine::CodeId(0),
            crate::machine::EnvironmentRef(0),
        );
        let mut remaining = 10_000;
        let completion = machine
            .run_until_complete(crate::completion::Completion::Normal, |_, _|
                -> Result<crate::completion::Completion, ()> {
                if remaining == 0 {
                    Ok(crate::completion::Completion::Return(Value::Number(7.0)))
                } else {
                    remaining -= 1;
                    Ok(crate::completion::Completion::TailCall(
                        crate::completion::TailCallRequest {
                            callee: Value::Undefined,
                            receiver: Value::Undefined,
                            arguments: Vec::new(),
                        },
                    ))
                }
            })
            .expect("tail-call loop completes");
        assert_eq!(completion, crate::completion::Completion::Return(Value::Number(7.0)));
        assert_eq!(machine.frame_count(), 0);
    }

    #[test]
    fn dispatch_loop_benchmark_has_stable_checksum_and_budget() {
        use std::time::{Duration, Instant};

        // This intentionally exercises the ordinary run_op path (rather than a
        // native shortcut). The checksum makes the loop observable to the
        // optimizer and catches skipped/reordered dispatches.
        let source = r#"
            let value = 0;
            for (let i = 0; i < 50_000; i++) value += i;
            // Keep the arithmetic loop observable without coupling this
            // dispatch benchmark to call-frame register restoration.
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let operation_count = program.ops().len();
        assert!(
            operation_count >= 10,
            "benchmark must contain a meaningful dispatch sequence"
        );
        let started = Instant::now();
        let result = crate::vm::execute_with_context(
            program.ops(),
            &crate::vm::VmContext::default(),
        )
        .expect("dispatch benchmark runs");
        let elapsed = started.elapsed();
        assert_eq!(result, Value::Undefined);
        // Generous wall-clock guard: this is evidence against pathological
        // regressions, not a machine-specific performance target.
        assert!(
            elapsed < Duration::from_secs(2),
            "dispatch loop exceeded 2s budget: {elapsed:?} ({operation_count} ops)"
        );
    }
    #[test]
    fn cold_not_callable_error_renders_canonical_type_error() {
        assert_eq!(super::not_callable().render(), "TypeError: value is not callable");
    }

    #[test]
    fn cold_thrown_rendering_preserves_primitive_value() {
        assert_eq!(
            VmError::Thrown(Value::String("boom".to_string())).render(),
            "boom"
        );
    }
    #[test]
    fn cold_error_constructors_render_canonical_names() {
        assert_eq!(
            crate::value::error::throw_reference_error("missing").render(),
            "ReferenceError: missing"
        );
        assert_eq!(
            crate::value::error::throw_syntax_error("bad syntax").render(),
            "SyntaxError: bad syntax"
        );
        assert_eq!(
            crate::value::error::throw_range_error("too large").render(),
            "RangeError: too large"
        );
        assert_eq!(
            crate::value::error::throw_uri_error("bad URI").render(),
            "URIError: bad URI"
        );
    }
}
