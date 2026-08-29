const _: () = {
    let _ = drain_microtasks as fn();
    let _ = new_promise as fn() -> Value;
    let _ = resolve_promise as fn(&Rc<PromiseData>, Value);
    let _ = reject_promise as fn(&Rc<PromiseData>, Value);
    let _ = promise_resolve as fn(&[Value]) -> Value;
    let _ = promise_reject as fn(&[Value]) -> Value;
    let _ = promise_then as fn(Option<&Value>, &[Value]) -> Result<Value, VmError>;
    let _ = promise_catch as fn(Option<&Value>, &[Value]) -> Result<Value, VmError>;
    let _ = promise_finally as fn(Option<&Value>, &[Value]) -> Result<Value, VmError>;
    let _ =
        execute_builtin as fn(Builtin, Option<&Value>, &[Value]) -> Option<Result<Value, VmError>>;
};
