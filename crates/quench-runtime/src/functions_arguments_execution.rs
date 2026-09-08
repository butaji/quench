pub(crate) fn function_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match builtin {
        crate::ops::Builtin::Function
        | crate::ops::Builtin::AsyncFunction
        | crate::ops::Builtin::GeneratorFunction
        | crate::ops::Builtin::AsyncGeneratorFunction => {
            crate::functions_dynamic::construct_builtin(builtin, arguments)
                .unwrap_or_else(|| Err(crate::execute::VmError::NotCallable))
        }
        crate::ops::Builtin::FunctionCall => execute_function_call(receiver, arguments),
        crate::ops::Builtin::FunctionApply => {
            crate::vm::execute_function_apply(receiver, arguments)
        }
        crate::ops::Builtin::FunctionBind => bind_function_target(receiver, arguments),
        crate::ops::Builtin::ArrayJoin => crate::builtins::array_join(receiver, arguments),
        crate::ops::Builtin::TypedArrayJoin => crate::arrays::typed_array_join(receiver, arguments),
        crate::ops::Builtin::ArrayToString => crate::builtins::array_to_string(receiver),
        crate::ops::Builtin::ArrayPush => crate::builtins::array_push(receiver, arguments),
        crate::ops::Builtin::ArrayShift => crate::builtins::array_shift(receiver),
        crate::ops::Builtin::ArrayReverse => crate::builtins::array_reverse(receiver),
        crate::ops::Builtin::ArrayPop => crate::builtins::array_pop(receiver),
        crate::ops::Builtin::ArrayUnshift => crate::builtins::array_unshift(receiver, arguments),
        crate::ops::Builtin::ArrayFill => crate::builtins::array_fill(receiver, arguments),
        crate::ops::Builtin::ArrayCopyWithin => {
            crate::builtins::array_copy_within(receiver, arguments)
        }
        crate::ops::Builtin::ArrayFindLast => crate::builtins::array_find_last(receiver, arguments),
        crate::ops::Builtin::ArrayFindLastIndex => {
            crate::builtins::array_find_last_index(receiver, arguments)
        }
        crate::ops::Builtin::ArrayFindIndex => crate::arrays::find_index(receiver, arguments),
        crate::ops::Builtin::ArrayToSorted => crate::builtins::array_to_sorted(receiver, arguments),
        crate::ops::Builtin::ArrayToSpliced => {
            crate::builtins::array_to_spliced(receiver, arguments)
        }
        crate::ops::Builtin::ArrayWith => crate::builtins::array_with(receiver, arguments),
        crate::ops::Builtin::ObjectPropertyIsEnumerable => {
            crate::builtins::object::object_property_is_enumerable(receiver, arguments)
        }
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

pub(crate) fn try_execute_specialized(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    crate::execution_trace::function_call_shape(
        function.params,
        function.code.capture_slots().len(),
        function.code.code(),
    );
    if is_class_constructor(function) {
        return Err(crate::value::error::throw_type_error(
            "Class constructor cannot be invoked without 'new'",
        ));
    }
    let receiver = crate::vm::bare_call_receiver(function, this_value);
    if matches!(function.kind, FunctionKind::Generator) {
        return crate::generator::create(function, &receiver, arguments).map(Some);
    }
    if function.is_async {
        // Parameter evaluation belongs to the async call's completion. A
        // direct-eval SyntaxError in a default parameter rejects the promise
        // instead of escaping as a synchronous host error.
        let generator = match crate::generator::create(function, &receiver, arguments) {
            Ok(generator) => generator,
            Err(error) => return Ok(Some(crate::promise::from_async_completion(Err(error)))),
        };
        let generator = match generator {
            crate::value::Value::Generator(generator) => generator,
            _ => unreachable!("generator creation must return a generator"),
        };
        let completion = crate::generator::resume(
            &generator,
            crate::generator::Resume::Next(crate::value::Value::Undefined),
        );
        return Ok(Some(crate::promise::from_async_function_completion(
            completion, generator,
        )));
    }
    try_execute_physical(function, arguments)
}

fn try_execute_physical(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    if let Some(value) = try_execute_typed_lane(function, arguments)? {
        record_typed_lane(function);
        return Ok(Some(crate::value::Value::Number(value)));
    }
    if let Some(value) = try_execute_counted_i32(function)? {
        record_counted_i32(function);
        return Ok(Some(crate::value::Value::Number(value)));
    }
    if let Some(value) = try_execute_two_state_i32(function)? {
        record_two_state_i32(function);
        return Ok(Some(crate::value::Value::Number(f64::from(value))));
    }
    if let Some(value) = try_execute_matrix_reduction(function, arguments)? {
        record_matrix_reduction(function);
        return Ok(Some(crate::value::Value::Number(value)));
    }
    if let Some(value) = try_execute_switch_reduction(function, arguments) {
        record_switch_reduction(function);
        return Ok(Some(crate::value::Value::Number(value as f64)));
    }
    if let Some(value) = try_execute_nested_xor(function, arguments) {
        record_nested_xor(function);
        return Ok(Some(crate::value::Value::Number(value as f64)));
    }
    if let Some(value) = try_execute_branch_recurrence(function, arguments) {
        record_branch_recurrence(function);
        return Ok(Some(crate::value::Value::Number(value as f64)));
    }
    if let Some(value) = try_execute_boolean_reduction(function, arguments) {
        record_boolean_reduction(function);
        return Ok(Some(crate::value::Value::Number(f64::from(value))));
    }
    if let Some((value, native)) = try_execute_counter_recurrence(function, arguments) {
        record_counter_recurrence(function, native);
        return Ok(Some(crate::value::Value::Number(f64::from(value))));
    }
    #[cfg(not(target_arch = "aarch64"))]
    if let Some(fact) = function.code.numeric_affine_named_loop() {
        match execute_numeric_affine_named_loop(function, arguments, &fact) {
            Ok(value) => {
                crate::execution_trace::kernel("PrecompiledAffineNamedLoop", false);
                #[cfg(test)]
                AFFINE_NAMED_LOOP_HITS.set(AFFINE_NAMED_LOOP_HITS.get().saturating_add(1));
                return Ok(Some(crate::value::Value::Number(f64::from(value))));
            }
            Err(rejection) => crate::execution_trace::leaf_rejection(rejection.trace_name()),
        }
    }
    if let Some(value) = try_execute_numeric_affine(function, arguments) {
        crate::execution_trace::kernel("PrecompiledAffineI32", false);
        return Ok(Some(crate::value::Value::Number(f64::from(value))));
    }
    Ok(None)
}

fn try_execute_typed_lane(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
) -> Result<Option<f64>, crate::execute::VmError> {
    if function.params < 1 || !crate::functions::direct_call_eligible(function) {
        return Ok(None);
    }
    let Some(code) = function.code.code() else {
        return Ok(None);
    };
    let Some(fact) = crate::stencil_typed_lane::select_function(code) else {
        return Ok(None);
    };
    fact.execute_native(function, arguments)
}

fn record_typed_lane(function: &crate::value::FunctionValue) {
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if let Some(code) = function.code.code() {
        crate::execution_trace::stencil_observation(code, 0, "typed_lane_arithmetic_region", true);
    }
    #[cfg(test)]
    crate::test_execution_profile::dynamic_region_route(["typed_lane_arithmetic"]);
}

fn try_execute_counted_i32(
    function: &crate::value::FunctionValue,
) -> Result<Option<f64>, crate::execute::VmError> {
    if function.params != 0 || !crate::functions::direct_call_eligible(function) {
        return Ok(None);
    }
    let Some(code) = function.code.code() else {
        return Ok(None);
    };
    let Some(selected) = crate::stencil_counted_i32_recurrence::select_function(code) else {
        return Ok(None);
    };
    selected.execute_native(function)
}

fn record_counted_i32(function: &crate::value::FunctionValue) {
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if let Some(code) = function.code.code() {
        crate::execution_trace::stencil_observation(code, 0, "counted_i32_recurrence", true);
    }
    #[cfg(test)]
    crate::test_execution_profile::dynamic_region_route([
        "counted_region",
        "i32_ushr_xor_imul",
    ]);
}

fn try_execute_two_state_i32(
    function: &crate::value::FunctionValue,
) -> Result<Option<i32>, crate::execute::VmError> {
    if function.params != 0 || !crate::functions::direct_call_eligible(function) {
        return Ok(None);
    }
    let Some(code) = function.code.code() else {
        return Ok(None);
    };
    let Some(selected) = crate::stencil_two_state_i32::select_function(code) else {
        return Ok(None);
    };
    selected.execute_native()
}

fn record_two_state_i32(function: &crate::value::FunctionValue) {
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if let Some(code) = function.code.code() {
        crate::execution_trace::stencil_observation(code, 0, "recurrence_branch_region", true);
    }
    #[cfg(test)]
    crate::test_execution_profile::dynamic_region_route(["recurrence_branch"]);
}

fn try_execute_matrix_reduction(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
) -> Result<Option<f64>, crate::execute::VmError> {
    if function.params < 2 || !crate::functions::direct_call_eligible(function) {
        return Ok(None);
    }
    let Some(code) = function.code.code() else {
        return Ok(None);
    };
    let Some(fact) = crate::stencil_matrix_reduction::select_function(code) else {
        return Ok(None);
    };
    fact.execute_native(function, arguments)
}

fn record_matrix_reduction(function: &crate::value::FunctionValue) {
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if let Some(code) = function.code.code() {
        crate::execution_trace::stencil_observation(code, 0, "dense_matrix_reduction_region", true);
    }
    #[cfg(test)]
    crate::test_execution_profile::dynamic_region_route(["dense_matrix_reduction"]);
}

fn try_execute_switch_reduction(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
) -> Option<i64> {
    let eligible = arguments.is_empty()
        && function.params == 0
        && crate::functions::direct_call_eligible(function);
    eligible.then_some(())?;
    let fact = crate::stencil_switch_reduction::select_function(function.code.code()?)?;
    fact.execute_native()
}

fn record_switch_reduction(function: &crate::value::FunctionValue) {
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if let Some(code) = function.code.code() {
        crate::execution_trace::stencil_observation(code, 0, "switch_dispatch_region", true);
    }
    #[cfg(test)]
    crate::test_execution_profile::dynamic_region_route(["switch_dispatch"]);
}

fn try_execute_nested_xor(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
) -> Option<i64> {
    let eligible = arguments.is_empty()
        && function.params == 0
        && crate::functions::direct_call_eligible(function);
    eligible.then_some(())?;
    let fact = crate::stencil_nested_xor::select_function(function.code.code()?)?;
    fact.execute_native()
}

fn record_nested_xor(function: &crate::value::FunctionValue) {
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if let Some(code) = function.code.code() {
        crate::execution_trace::stencil_observation(code, 0, "nested_count_region", true);
    }
    #[cfg(test)]
    crate::test_execution_profile::dynamic_region_route(["nested_count"]);
}

fn try_execute_branch_recurrence(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
) -> Option<i64> {
    let eligible = arguments.is_empty()
        && function.params == 0
        && crate::functions::direct_call_eligible(function);
    eligible.then_some(())?;
    let fact = crate::stencil_branch_recurrence::select_function(function.code.code()?)?;
    fact.execute_native()
}

fn record_branch_recurrence(function: &crate::value::FunctionValue) {
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if let Some(code) = function.code.code() {
        crate::execution_trace::stencil_observation(code, 0, "branch_predict_region", true);
    }
    #[cfg(test)]
    crate::test_execution_profile::dynamic_region_route(["branch_predict"]);
}

fn try_execute_boolean_reduction(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
) -> Option<i32> {
    let eligible = arguments.is_empty()
        && function.params == 0
        && crate::functions::direct_call_eligible(function);
    eligible.then_some(())?;
    let fact = crate::stencil_boolean_reduction::select_function(function.code.code()?)?;
    fact.execute_native()
}

fn record_boolean_reduction(function: &crate::value::FunctionValue) {
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if let Some(code) = function.code.code() {
        crate::execution_trace::stencil_observation(code, 0, "boolean_short_circuit_region", true);
    }
    #[cfg(test)]
    crate::test_execution_profile::dynamic_region_route(["boolean_short_circuit"]);
}

fn try_execute_counter_recurrence(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
) -> Option<(i32, bool)> {
    (function.params >= 2 && crate::functions::direct_call_eligible(function)).then_some(())?;
    let fact = function.code.numeric_counter_recurrence()?;
    let first = u16::try_from(function.captures.len()).ok()?;
    (fact.value_parameter == first && fact.counter_parameter == first.checked_add(1)?)
        .then_some(())?;
    fact.execute_native(arguments)
        .or_else(|| fact.execute(arguments).map(|value| (value, false)))
}

fn record_counter_recurrence(function: &crate::value::FunctionValue, native: bool) {
    crate::execution_trace::kernel("PrecompiledI32CounterRecurrence", false);
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if let Some(code) = function.code.code() {
        crate::execution_trace::stencil_observation(code, 0, "i32_counter_recurrence", true);
    }
    #[cfg(test)]
    {
        if !native {
            crate::test_execution_profile::portable_recipe();
        }
        crate::test_execution_profile::dynamic_region_route(["i32_counter_recurrence"]);
    }
}

#[cfg(test)]
thread_local! {
    static AFFINE_NAMED_LOOP_HITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn take_affine_named_loop_hits() -> u64 {
    AFFINE_NAMED_LOOP_HITS.replace(0)
}

const MAX_PRECOMPILED_LOOP_ITERATIONS: i32 = 4096;

#[derive(Clone, Copy)]
enum AffineNamedLoopRejection {
    Parameter,
    Argument,
    Object,
    Seed,
    Bound,
    Range,
    Method,
    Callee,
}

impl AffineNamedLoopRejection {
    const fn trace_name(self) -> &'static str {
        match self {
            Self::Parameter => "affine_named_parameter",
            Self::Argument => "affine_named_argument",
            Self::Object => "affine_named_object",
            Self::Seed => "affine_named_seed",
            Self::Bound => "affine_named_bound",
            Self::Range => "affine_named_range",
            Self::Method => "affine_named_method",
            Self::Callee => "affine_named_callee",
        }
    }
}

fn execute_numeric_affine_named_loop(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
    fact: &crate::function_physical::NumericAffineNamedLoop,
) -> Result<i32, AffineNamedLoopRejection> {
    if usize::from(fact.parameter_slot) != function.captures.len() {
        return Err(AffineNamedLoopRejection::Parameter);
    }
    let Some(crate::value::Value::Object(receiver)) = arguments.first() else {
        return Err(AffineNamedLoopRejection::Argument);
    };
    guarded_loop_object(receiver).ok_or(AffineNamedLoopRejection::Object)?;
    let mut value = own_i32(receiver, &fact.seed_key).ok_or(AffineNamedLoopRejection::Seed)?;
    let end = own_i32(receiver, &fact.bound_key).ok_or(AffineNamedLoopRejection::Bound)?;
    if !(0..=MAX_PRECOMPILED_LOOP_ITERATIONS).contains(&end) {
        return Err(AffineNamedLoopRejection::Range);
    }
    if end == 0 {
        return Ok(value);
    }
    let callee =
        own_function(receiver, &fact.method_key).ok_or(AffineNamedLoopRejection::Method)?;
    let affine = guarded_affine_callee(&callee).ok_or(AffineNamedLoopRejection::Callee)?;
    for _ in 0..end {
        value = affine
            .execute(f64::from(value))
            .ok_or(AffineNamedLoopRejection::Callee)?;
    }
    Ok(value)
}

fn guarded_loop_object(object: &crate::value::ObjectData) -> Option<()> {
    (!object.has_replacement()
        && !object.is_dictionary()
        && !object.is_realm_global()
        && !object.is_script_global_view()
        && !object.has_regexp_internal_slot())
    .then_some(())
}

fn own_i32(object: &crate::value::ObjectData, key: &str) -> Option<i32> {
    let crate::value::Value::Number(value) = crate::vm::proven_own_word(object, key)?.load() else {
        return None;
    };
    (value.is_finite() && value >= i32::MIN as f64 && value <= i32::MAX as f64)
        .then(|| value as i32)
        .filter(|integer| f64::from(*integer) == value)
}

fn own_function(
    object: &crate::value::ObjectData,
    key: &str,
) -> Option<std::rc::Rc<crate::value::FunctionValue>> {
    let pointer = crate::vm::proven_own_word(object, key)?.function_ptr()?;
    unsafe { std::rc::Rc::increment_strong_count(pointer) };
    Some(unsafe { std::rc::Rc::from_raw(pointer) })
}

fn guarded_affine_callee(
    function: &crate::value::FunctionValue,
) -> Option<crate::function_physical::NumericAffineI32> {
    (function.params == 1 && crate::functions::direct_call_eligible(function)).then_some(())?;
    let fact = function.code.numeric_affine_i32()?;
    (usize::from(fact.parameter_slot) == function.captures.len()).then_some(fact)
}

fn try_execute_numeric_affine(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
) -> Option<i32> {
    if function.params != 1 {
        return None;
    }
    let fact = function.code.numeric_affine_i32()?;
    if usize::from(fact.parameter_slot) != function.captures.len() {
        return None;
    }
    let crate::value::Value::Number(input) = arguments.first()? else {
        return None;
    };
    fact.execute(*input)
}

/// Admission fact shared by ordinary and named calls.  The continuation
/// gateway remains authoritative for every function outside this compact
/// synchronous shape.
#[inline]
pub(crate) fn direct_call_eligible(function: &crate::value::FunctionValue) -> bool {
    !function.is_async
        && !matches!(function.kind, crate::ops::FunctionKind::Generator)
        && !crate::functions::is_class_constructor(function)
        && function.with_captures.is_empty()
        && !function.private_environment.has_names()
        && !crate::with_scope::is_active()
}

/// Enter a function whose ordinary synchronous shape has already been
/// established by a call-site guard. Physical body selection remains shared
/// with cold calls; only the already-proven semantic call classification is
/// skipped before the canonical interpreter fallback.
#[inline(never)]
pub(crate) fn execute_direct(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if let Some(value) = try_execute_physical(function, arguments)? {
        return Ok(value);
    }
    stacker::maybe_grow(64 * 1024 * 1024, 256 * 1024 * 1024, || {
        execute_interpreter(function, this_value, arguments)
    })
}

#[inline(never)]
pub(crate) fn execute(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let specialized = try_execute_specialized(function, this_value, arguments);
    match specialized {
        Ok(Some(result)) => Ok(result),
        Ok(None) => stacker::maybe_grow(64 * 1024 * 1024, 256 * 1024 * 1024, || {
            execute_interpreter(function, this_value, arguments)
        }),
        Err(error) => Err(error),
    }
}

fn execute_interpreter(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let receiver = crate::vm::bare_call_receiver(function, this_value);

    // The packed continuation path intentionally omits dynamic object-scope
    // guards.  A function created inside `with` must retain that scope while a
    // promoted tail call replaces its activation, so drive this small class
    // through the guard-aware machine loop instead.
    if !function.with_captures.is_empty() || crate::with_scope::is_active() {
        return execute_with_dynamic_scope(function, receiver, arguments);
    }

    // Ordinary calls use the same bounded tail-call machine as dynamic-scope
    // calls. Tail-call promotion is a representation detail; it must never
    // escape as an observable VM error merely because the callee has no
    // `with` capture.
    let mut function = std::rc::Rc::clone(function);
    let mut receiver = receiver;
    let mut arguments = std::borrow::Cow::Borrowed(arguments);
    loop {
        let _ = function.code.enter_invocation();
        let (mut registers, environment) =
            build_registers(&function, &receiver, arguments.as_ref());
        let _private_environment = crate::private_environment::Guard::install_environment(
            function.private_environment.clone(),
        );
        let _home = crate::super_scope::Guard::install(&function, &receiver);
        let _with_scope = crate::with_scope::FunctionGuard::install(&function.with_captures);
        let completion = crate::vm::execute_code_frame_completion_with_owner(
            function
                .code
                .code()
                .ok_or(crate::execute::VmError::MissingReturn)?,
            &function.code,
            &mut registers,
            &crate::vm::current_context(),
            environment,
        )?;
        let crate::completion::Completion::TailCall(request) = completion else {
            return crate::vm::completion_result(completion);
        };
        let crate::value::Value::Function(next) = request.callee else {
            return crate::functions::execute_target(
                &request.callee,
                &request.receiver,
                &request.arguments,
            );
        };
        function = next;
        receiver = crate::vm::bare_call_receiver(&function, &request.receiver);
        arguments = std::borrow::Cow::Owned(request.arguments.into_vec());
    }
}

fn execute_with_dynamic_scope(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let mut function = std::rc::Rc::clone(function);
    let mut receiver = receiver;
    let mut arguments = std::borrow::Cow::Borrowed(arguments);
    loop {
        let _ = function.code.enter_invocation();
        let (registers, environment) =
            crate::functions::build_registers(&function, &receiver, arguments.as_ref());
        let _private_environment = crate::private_environment::Guard::install_environment(
            function.private_environment.clone(),
        );
        let _home = crate::super_scope::Guard::install(&function, &receiver);
        let _with_scope = crate::with_scope::FunctionGuard::install(&function.with_captures);
        let mut registers = registers;
        let completion = crate::vm::execute_code_frame_completion_with_owner(
            function
                .code
                .code()
                .ok_or(crate::execute::VmError::MissingReturn)?,
            &function.code,
            &mut registers,
            &crate::vm::current_context(),
            environment,
        )?;
        let crate::completion::Completion::TailCall(request) = completion else {
            return crate::vm::completion_result(completion);
        };
        let crate::value::Value::Function(next) = request.callee else {
            return crate::functions::execute_target(
                &request.callee,
                &request.receiver,
                &request.arguments,
            );
        };
        function = next;
        receiver = crate::vm::bare_call_receiver(&function, &request.receiver);
        arguments = std::borrow::Cow::Owned(request.arguments.into_vec());
    }
}
