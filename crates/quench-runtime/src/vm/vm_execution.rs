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
            VmError::Interrupted => "Execution interrupted".to_string(),
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

pub fn execute_in_place(ops: &[Op], registers: &mut crate::register_file::RegisterFile) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_in_place_context(ops, registers, &context)
}

pub(crate) fn current_context_or_default() -> Rc<VmContext> {
    CURRENT_CONTEXT
        .with(|current| current.borrow().clone())
        .unwrap_or_else(|| Rc::new(VmContext::default()))
}

pub fn execute_with_context(ops: &[Op], context: &VmContext) -> Result<Value, VmError> {
    // Intrinsic overrides are scoped to one top-level evaluation; do not
    // carry mutations such as Math.random assignments into the next one.
    crate::builtins::reset_intrinsic_prototype_state();
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

pub fn execute_code_with_context(
    code: crate::machine::CodeView<'_>,
    context: &VmContext,
) -> Result<Value, VmError> {
    crate::builtins::reset_intrinsic_prototype_state();
    let result = crate::vm::with_current_context(context, || {
        let mut registers = crate::register_file::RegisterFile::new();
        prepare_register_stack(&mut registers);
        let environment = crate::environment::Environment::child_registers(
            &crate::environment::Environment::new(),
            registers.clone(),
        );
        execute_code_in_environment(code, &mut registers, context, environment)
    });
    let realm = context.realm();
    if realm::with_realm(realm, crate::promise::drain_microtasks_all).is_none() {
        crate::vm::with_current_context(context, crate::promise::drain_microtasks_all);
    }
    result
}

/// Execute a top-level code fragment in a fresh lexical frame without
/// draining promise jobs. Hosts use this for synchronous re-entry (for
/// example a worker script invoked from a callback): the caller's frame stays
/// intact while the host retains ownership of the event-loop turn.
pub fn execute_code_isolated_in_context(
    code: crate::machine::CodeView<'_>,
    context: &VmContext,
) -> Result<Value, VmError> {
    crate::builtins::reset_intrinsic_prototype_state();
    crate::vm::with_current_context(context, || {
        let mut registers = crate::register_file::RegisterFile::new();
        prepare_register_stack(&mut registers);
        let environment = crate::environment::Environment::child_registers(
            &crate::environment::Environment::new(),
            registers.clone(),
        );
        execute_code_in_environment(code, &mut registers, context, environment)
    })
}

/// The context currently active on this thread, or a default one.
/// Hosts use this to re-enter the VM from inside a capability call.
pub fn current_context() -> Rc<VmContext> {
    current_context_or_default()
}

/// Execute a classic script with the current host bindings and an optional
/// sandbox overlay.  Node-facing adapters use this entry point for
/// `vm.runInNewContext`/`runInContext`; realm and value identity mechanics stay
/// in the runtime rather than being duplicated by a host module.
pub fn execute_script_in_sandbox(
    source: &str,
    sandbox: Option<&Value>,
    filename: Option<&str>,
) -> Result<Value, VmError> {
    let program = crate::reduce::reduce_global_script_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let parent_context = current_context();
    let mut context = sandbox
        .map(|_| Rc::new(parent_context.child_realm()))
        .unwrap_or(parent_context);
    if let Some(sandbox @ Value::Object(_)) = sandbox {
        for key in crate::execute::own_enumerable_keys(sandbox) {
            let value = crate::execute::get_property_result(sandbox, &key)?;
            context = Rc::new((*context).clone().with_host_value(key, value));
        }
    }
    let global = current_global_object();
    if let Some(filename) = filename {
        let updated = crate::execute::set_property(
            global.clone(),
            "\0quench_vm_filename",
            Value::String(filename.to_owned()),
        );
        crate::execute::replace_value(&global, &updated);
    }
    let mut registers = crate::register_file::RegisterFile::new();
    let result = with_current_context(&context, || {
        execute_code_in_place_context(program.code(), &mut registers, &context)
    });
    if filename.is_some() {
        let updated = crate::execute::delete_property(global.clone(), "\0quench_vm_filename").0;
        crate::execute::replace_value(&global, &updated);
    }
    let result = result?;
    let marker_name = |value: &Value| match value {
        Value::ArrayBuffer(buffer) if buffer.shared => "\0vmSharedArrayBufferPrototype",
        _ => "\0vmArrayBufferPrototype",
    };
    let apply_realm_marker = |target: &Value, marker: Value| {
        if !matches!(marker, Value::Object(_)) {
            return;
        }
        let original = crate::execute::get_prototype_of(target).unwrap_or(Value::Null);
        let _ = crate::execute::set_prototype_of(&marker, &original);
        let _ = crate::execute::set_prototype_of(target, &marker);
    };
    let apply_to_buffer = |target: &Value| {
        let buffer = crate::execute::get_property(target, "buffer");
        if matches!(buffer, Value::ArrayBuffer(_)) {
            let marker = sandbox
                .map(|sandbox| crate::execute::get_property(sandbox, marker_name(&buffer)))
                .unwrap_or(Value::Undefined);
            apply_realm_marker(&buffer, marker);
        }
    };
    match &result {
        Value::ArrayBuffer(_) => {
            let marker = sandbox
                .map(|sandbox| crate::execute::get_property(sandbox, marker_name(&result)))
                .unwrap_or(Value::Undefined);
            apply_realm_marker(&result, marker);
        }
        Value::Float64Array(_)
        | Value::Float32Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Int32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Uint32Array(_)
        | Value::Uint8Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Uint16Array(_)
        | Value::DataView(_) => apply_to_buffer(&result),
        _ => {}
    }
    Ok(result)
}

/// Mark a JavaScript object as a script context while preserving its identity.
pub fn create_script_context(context: Value) -> Result<Value, VmError> {
    if !matches!(context, Value::Object(_) | Value::Array(_)) {
        return Err(crate::execute::type_error("context must be an object"));
    }
    let updated = crate::execute::set_property(context.clone(), "\0vmContext", Value::Boolean(true));
    let updated = crate::execute::set_property(
        updated,
        "\0vmArrayBufferPrototype",
        crate::host_api::object(Vec::new()),
    );
    let updated = crate::execute::set_property(
        updated,
        "\0vmSharedArrayBufferPrototype",
        crate::host_api::object(Vec::new()),
    );
    crate::execute::replace_value(&context, &updated);
    Ok(context)
}

pub fn is_script_context(value: &Value) -> bool {
    matches!(crate::execute::get_property(value, "\0vmContext"), Value::Boolean(true))
}

/// Call a function value from host code through the current context.
pub fn call_value(target: &Value, receiver: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    crate::functions::execute_target(target, receiver, arguments)
}

pub fn execute_with_registers_context(
    ops: &[Op],
    registers: Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    // Install the caller's context before creating environments or resolving
    // globals.  Host values (console/process/require) are carried by this
    // context; delaying installation until the first VM step lets setup and
    // re-entrant continuations observe the thread's stale/default realm.
    crate::vm::with_current_context(context, || {
        let mut registers = crate::register_file::RegisterFile::from_values(registers);
        prepare_register_stack(&mut registers);
        let environment = crate::environment::Environment::child_registers(
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
fn prepare_register_stack(registers: &mut crate::register_file::RegisterFile) {
    const INITIAL_REGISTER_CAPACITY: usize = 16;
    const MAX_REGISTER_COUNT: usize = u16::MAX as usize + 1;
    debug_assert!(registers.len() <= MAX_REGISTER_COUNT);
    if registers.capacity() < INITIAL_REGISTER_CAPACITY {
        registers.reserve(INITIAL_REGISTER_CAPACITY - registers.capacity());
    }
}
/// Execute a fragment inside the currently installed lexical environment.
pub fn execute_in_current_context(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_in_place_context(ops, registers, &context)
}

pub fn execute_in_place_context(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<Value, VmError> {
    prepare_register_stack(registers);
    let parent = crate::locals::current();
    let environment = crate::environment::Environment::child_registers(&parent, registers.clone());
    execute_in_environment(ops, registers, context, environment)
}

pub fn execute_code_in_place_context(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<Value, VmError> {
    prepare_register_stack(registers);
    let parent = crate::locals::current();
    let environment = crate::environment::Environment::child_registers(&parent, registers.clone());
    execute_code_in_environment(code, registers, context, environment)
}

pub(crate) fn execute_code_completion_in_current_frame(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, VmError> {
    let context = current_context_or_default();
    drive_code_completion(code, registers, &context)
}

/// Execute residual compact code with a context already owned by the caller.
/// Structured loops use this to avoid re-reading and cloning the same TLS
/// context for every iteration.
pub(crate) fn execute_code_completion_with_context(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    drive_code_completion(code, registers, context)
}

fn drive_code_completion(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    let mut pc = 0;
    loop {
        let step = run_code_completion_step_from(code, pc, registers, context)?;
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                if let Err(VmError::Thrown(value)) =
                    crate::vm::vm_ops::execute_call_continuation(registers, continuation)
                {
                    return Ok(crate::completion::Completion::Throw(value));
                }
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
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<Value, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(Rc::clone(&environment));
    let register_count = registers.len().min(usize::from(u16::MAX)) as u16;
    let mut machine = crate::machine::Machine::with_register_count(
        crate::machine::CodeId(0),
        crate::machine::EnvironmentRef(0),
        register_count,
    );
    machine.restore_registers(std::mem::take(registers));
    let mut pc = 0;
    loop {
        let step = run_ops_completion_step_from(ops, pc, machine.registers_mut(), context)?;
        let completion = step.completion;
        let next = step.next;
        match completion {
            crate::completion::Completion::Call(mut continuation) => {
                continuation.caller_code = machine.code_id();
                continuation.caller_pc = next as u32;
                machine.push_call_frame(continuation);
                let continuation = machine.pop_call_frame().expect("call frame just pushed");
                crate::vm::vm_ops::execute_call_continuation(
                    machine.registers_mut(),
                    continuation,
                )?;
                pc = next;
            }
            crate::completion::Completion::TailCall(request) => {
                // A top-level code slice has no caller continuation to
                // consume a promoted call.  Re-enter the packed call-frame
                // driver with a synthetic result register; it handles the
                // callee iteratively (including nested tail calls) and keeps
                // ordinary calls on the upstream fast path.
                let mut caller_registers = machine.take_registers();
                let continuation = crate::completion::CallContinuation {
                    callee: request.callee,
                    receiver: request.receiver,
                    arguments: request.arguments,
                    caller_code: crate::identity::CodeId(0),
                    caller_pc: 0,
                    caller_registers: std::mem::take(&mut caller_registers),
                    caller_environment: crate::identity::EnvironmentRef(0),
                    destination: 0,
                    guards: crate::completion::ContinuationGuards::default(),
                };
                crate::vm::vm_ops::execute_call_continuation(
                    &mut caller_registers,
                    continuation,
                )?;
                let value = crate::vm::read_register(&caller_registers, 0)?;
                *registers = caller_registers;
                return Ok(value);
            }
            completion => {
                *registers = machine.take_registers();
                return completion_result(completion);
            }
        }
    }
}

#[inline(never)]
pub(crate) fn execute_code_in_environment(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<Value, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let mut pc = 0;
    loop {
        let step = run_code_completion_step_from(code, pc, registers, context)?;
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                crate::vm::vm_ops::execute_call_continuation(registers, continuation)?;
            }
            crate::completion::Completion::TailCall(request) => {
                let mut caller_registers = std::mem::take(registers);
                let continuation = crate::completion::CallContinuation {
                    callee: request.callee,
                    receiver: request.receiver,
                    arguments: request.arguments,
                    caller_code: crate::identity::CodeId(0),
                    caller_pc: 0,
                    caller_registers: std::mem::take(&mut caller_registers),
                    caller_environment: crate::identity::EnvironmentRef(0),
                    destination: 0,
                    guards: crate::completion::ContinuationGuards::default(),
                };
                crate::vm::vm_ops::execute_call_continuation(
                    &mut caller_registers,
                    continuation,
                )?;
                let value = crate::vm::read_register(&caller_registers, 0)?;
                *registers = caller_registers;
                return Ok(value);
            }
            completion => return completion_result(completion),
        }
    }
}
pub(crate) fn execute_code_frame_completion(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<crate::completion::Completion, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    drive_code_completion(code, registers, context)
}
pub(crate) fn execute_indirect_eval(code: crate::machine::CodeView<'_>) -> Result<Value, VmError> {
    let context = CURRENT_CONTEXT
        .with(|current| current.borrow().clone())
        .unwrap_or_else(|| Rc::new(VmContext::default()));
    let caller = crate::locals::current();
    let caller_global = caller.get(0);
    if matches!(&caller_global, Value::Object(object) if object.iter().any(|(name, _)| name == crate::vm::SCRIPT_GLOBAL_VIEW)) {
        let environment = crate::environment::Environment::new();
        environment.set(0, caller_global.clone());
        let mut registers = crate::register_file::RegisterFile::new();
        let _with_scope = crate::with_scope::FunctionGuard::isolate();
        return execute_code_in_environment(code, &mut registers, &context, environment);
    }
    if realm::context(context.realm()).is_some() {
        return execute_indirect_eval_in_realm(context.realm(), code);
    }
    let global = match caller_global {
        Value::Undefined => current_global_object(),
        value => value,
    };
    let environment = crate::environment::Environment::new();
    environment.set(0, global.clone());
    let mut registers = crate::register_file::RegisterFile::new();
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let result = execute_code_in_environment(code, &mut registers, &context, environment);
    caller.replace_value(&global, &current_global_object());
    result
}
pub(crate) fn execute_indirect_eval_in_realm(
    realm_id: RealmId,
    code: crate::machine::CodeView<'_>,
) -> Result<Value, VmError> {
    realm::execute(realm_id, code)
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
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
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
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("bounded ordinary calls run");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn statement_completion_survives_replace_and_observed_loop_break() {
        let source = r#"
            var trim = /^\s*|\s*$/g;
            var text = "  value  ";
            for (var index = 0; index < 3; index++) {
                text.replace(trim, "");
            }
            if (text !== "  value  ") throw "replace result was assigned";
            if (eval("while (true) { 'last'; break; }") !== "last") {
                throw "loop completion was lost";
            }
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_code_with_context(
            program.code(),
            &crate::vm::VmContext::default(),
        )
        .expect("statement completions run");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn staged_global_resolves_math_for_property_assignment() {
        let source = r#"
            Math.random = function () { return 1; };
            if (Math.random() !== 1) throw "Math must resolve while globals are staged";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
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
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
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
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
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
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
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
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
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
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default());
        assert!(matches!(
            result,
            Err(crate::vm::VmError::Thrown(Value::Number(17.0)))
        ));
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
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
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
            .run_until_complete(
                crate::completion::Completion::Normal,
                |_, _| -> Result<crate::completion::Completion, ()> {
                    if remaining == 0 {
                        Ok(crate::completion::Completion::Return(Value::Number(7.0)))
                    } else {
                        remaining -= 1;
                        Ok(crate::completion::Completion::TailCall(
                            crate::completion::TailCallRequest {
                                callee: Value::Undefined,
                                receiver: Value::Undefined,
                                arguments: Vec::new().into(),
                            },
                        ))
                    }
                },
            )
            .expect("tail-call loop completes");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(Value::Number(7.0))
        );
        assert_eq!(machine.frame_count(), 0);
    }

    #[test]
    fn dispatch_loop_has_observable_result_and_budget() {
        use std::time::{Duration, Instant};

        // This intentionally exercises the ordinary run_op path rather than a
        // native shortcut. The loop's side effects keep dispatch observable.
        let source = r#"
            let value = 0;
            for (let i = 0; i < 50_000; i++) value += i;
            // Keep the arithmetic loop observable without coupling this
            // dispatch benchmark to call-frame register restoration.
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let operation_count = program.code().len();
        assert!(
            operation_count >= 10,
            "benchmark must contain a meaningful dispatch sequence"
        );
        let started = Instant::now();
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("dispatch benchmark runs");
        let elapsed = started.elapsed();
        assert_eq!(result, Value::Undefined);
        // Generous wall-clock guard: this is evidence against pathological
        // compatibility failures, not a machine-specific performance target.
        assert!(
            elapsed < Duration::from_secs(2),
            "dispatch loop exceeded 2s budget: {elapsed:?} ({operation_count} ops)"
        );
    }
    #[test]
    fn cold_not_callable_error_renders_canonical_type_error() {
        assert_eq!(
            super::not_callable().render(),
            "TypeError: value is not callable"
        );
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
