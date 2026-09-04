// L1 admission is frozen here. New fused kernels are selected from OXC AST
// facts and emitted as KernelIds; this interpreter file only executes the
// existing compatibility set and must not grow new recognize/admit templates.

pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &crate::ops::Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (label, init, test, body, update, post_test, dst, per_iteration) = match op {
        crate::ops::Op::Loop {
            label,
            init,
            test,
            body,
            update,
            post_test,
            dst,
            per_iteration,
        } => (
            label,
            init,
            test,
            body,
            update,
            post_test,
            *dst,
            per_iteration.as_slice(),
        ),
        _ => return Err(crate::execute::VmError::MissingReturn),
    };
    let body_capture_slots = body.capture_slots();
    let Some(body) = body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(init) = init.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(test) = test.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(update) = update.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    run_loop(
        label,
        init,
        test,
        body,
        update,
        (*post_test, dst, per_iteration, body_capture_slots),
        registers,
    )
}

fn run_loop(
    label: &Option<String>,
    init: crate::machine::CodeView<'_>,
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    config: (bool, u16, &[u16], &[u16]),
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    stacker::maybe_grow(64 * 1024 * 1024, 256 * 1024 * 1024, || {
        run_loop_inner(label, init, test, body, update, config, registers)
    })
}

fn run_loop_inner(
    label: &Option<String>,
    init: crate::machine::CodeView<'_>,
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    config: (bool, u16, &[u16], &[u16]),
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::LoopEntry);
    let loop_shape = crate::execution_trace::loop_shape(body);
    let (post_test, dst, per_iteration, body_capture_slots) = config;
    run_fragment(init, registers)?;
    if label.is_none() && !post_test && !has_immutable_marker(update) {
        if let Some(fact) = CountedForFact::recognize(test, update) {
            trace_counted_recognition(body, fact, per_iteration);
            if let Some(completion) =
                run_pair_word_walk(fact, body, dst, per_iteration, registers, loop_shape)
            {
                return Ok(completion);
            }
            if let Some(completion) =
                run_regexp_exec_loop(fact, body, dst, per_iteration, registers, loop_shape)
            {
                return Ok(completion);
            }
            if let Some(completion) =
                run_invariant_sum_kernel(fact, body, dst, per_iteration, registers, loop_shape)
            {
                return Ok(completion);
            }
        } else {
            dump_counted_rejection(loop_shape, test, body, update, per_iteration);
        }
    }
    if label.is_none() && !post_test && !has_immutable_marker(update) {
        if let Some(fact) = CountedForFact::recognize(test, update) {
            // A compact body cannot allocate or expose the lexical binding.
            // Direct index loads are ordinary uses, not evidence of capture.
            let stable_iteration_binding = per_iteration.is_empty() || per_iteration == [fact.slot];
            let compact_body = (0..body.len()).all(|pc| {
                body.instruction(pc)
                    .is_some_and(|instruction| instruction.opcode != crate::ir::Opcode::Slow)
            });
            let pure_body = counted_body_is_pure(body);
            let guarded_body = counted_body_is_guarded(body, fact.slot);
            if stable_iteration_binding
                && compact_body
                && (pure_body || guarded_body)
            {
                if let Some(completion) = run_counted_for(
                    fact,
                    body,
                    update,
                    dst,
                    registers,
                    loop_shape,
                    !capture_slots_use(body_capture_slots, fact.slot),
                    !pure_body,
                )? {
                    return Ok(completion);
                }
            }
        }
    }
    refresh_per_iteration(per_iteration);
    loop {
        crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
        crate::execution_trace::loop_shape_iteration(loop_shape);
        if !post_test && !loop_test(test, registers)? {
            break;
        }
        crate::execute::write_value(registers, dst, crate::value::Value::Undefined);
        match execute_loop_body(registers, label, body)? {
            crate::completion::LoopTransition::Continue(value) => {
                store_loop_value(registers, dst, value)?;
            }
            crate::completion::LoopTransition::Break(value) => {
                store_loop_value(registers, dst, value)?;
                break;
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                return update_empty_from(registers, dst, completion);
            }
        }
        refresh_per_iteration(per_iteration);
        run_fragment(update, registers)?;
        if post_test && !loop_test(test, registers)? {
            break;
        }
    }
    Ok(crate::completion::Completion::Normal)
}

#[inline]
fn has_immutable_marker(update: crate::machine::CodeView<'_>) -> bool {
    update.position_cold(|op| matches!(op, crate::ops::Op::MarkImmutable { .. })).is_some()
}

#[inline]
fn capture_slots_use(slots: &[u16], slot: u16) -> bool {
    slots.binary_search(&u16::MAX).is_ok() || slots.binary_search(&slot).is_ok()
}

fn run_invariant_sum_kernel(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    dst: u16,
    per_iteration: &[u16],
    registers: &mut crate::register_file::RegisterFile,
    loop_shape: u64,
) -> Option<crate::completion::Completion> {
    let plan = recognize_invariant_sum(body, dst, fact, per_iteration)?;
    let environment = crate::locals::current();
    let index = environment.get_number(fact.slot)?;
    let bound = fact.bound.number(&environment)?;
    crate::execution_trace::event(crate::execution_trace::Event::CountedForAttempt);
    let result = match plan.addend {
        CountedAddend::PackedMasked { array, mask } => run_packed_masked_sum(
            &environment,
            fact,
            index,
            bound,
            plan.total,
            array,
            mask,
            loop_shape,
        )?,
        addend => run_invariant_add(
            &environment,
            fact,
            index,
            bound,
            plan.total,
            addend,
            loop_shape,
        )?,
    };
    environment.set(plan.total, crate::value::Value::Number(result.total));
    environment.set(fact.slot, crate::value::Value::Number(result.index));
    if result.iterations != 0 {
        registers.write_number(usize::from(dst), result.total);
    }
    Some(crate::completion::Completion::Normal)
}

#[derive(Clone, Copy)]
struct CountedSumResult {
    index: f64,
    total: f64,
    iterations: usize,
}

#[derive(Clone, Copy)]
struct InvariantSumPlan {
    total: u16,
    addend: CountedAddend,
}

#[derive(Clone, Copy)]
enum CountedAddend {
    Local(u16),
    ArrayLength(u16),
    PackedMasked { array: u16, mask: usize },
    WordCall { function: u16 },
}

impl CountedAddend {
    fn number(self, environment: &crate::environment::Environment) -> Option<f64> {
        match self {
            Self::Local(slot) => environment.get_number(slot),
            Self::ArrayLength(slot) => {
                match crate::locals::resolved_replacement(environment.get(slot)) {
                    crate::value::Value::Array(array) => Some(array.header_length() as f64),
                    _ => None,
                }
            }
            Self::WordCall { function } => match environment.get(function) {
                crate::value::Value::Function(function) => {
                    crate::functions::word_add_constant(&function)
                }
                _ => None,
            },
            Self::PackedMasked { .. } => None,
        }
    }

    fn kernel(self) -> &'static str {
        match self {
            Self::WordCall { .. } => "counted_word_call",
            _ => "invariant_sum",
        }
    }
}

fn run_invariant_add(
    environment: &crate::environment::Environment,
    fact: CountedForFact,
    mut index: f64,
    bound: f64,
    total_slot: u16,
    addend: CountedAddend,
    loop_shape: u64,
) -> Option<CountedSumResult> {
    let mut total = environment.get_number(total_slot)?;
    let value = addend.number(environment)?;
    let mut iterations = 0;
    while counted_comparison(fact.comparison, index, bound) {
        trace_counted_sum_iteration(loop_shape, addend.kernel());
        if matches!(addend, CountedAddend::WordCall { .. }) {
            crate::execution_trace::event(crate::execution_trace::Event::LeafAttempt);
            crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
        }
        total += value;
        index += fact.step;
        iterations += 1;
    }
    Some(CountedSumResult {
        index,
        total,
        iterations,
    })
}

fn run_packed_masked_sum(
    environment: &crate::environment::Environment,
    fact: CountedForFact,
    index: f64,
    bound: f64,
    total_slot: u16,
    array_slot: u16,
    mask: usize,
    loop_shape: u64,
) -> Option<CountedSumResult> {
    (fact.comparison == crate::ops::BinaryOp::LessThan && fact.step == 1.0).then_some(())?;
    let start = exact_nonnegative_index(index)?;
    let iterations = unit_less_than_iterations(index, bound)?;
    let end = start.checked_add(iterations)?;
    (end <= i32::MAX as usize).then_some(())?;
    let value = crate::locals::resolved_replacement(environment.get(array_slot));
    let crate::value::Value::Array(array) = value else {
        return None;
    };
    let cells = array
        .is_packed_ordinary()
        .then(|| array.numeric_cells())??;
    (mask < cells.len()).then_some(())?;
    let cells = cells.as_ptr();
    let mut total = environment.get_number(total_slot)?;
    for counter in start..end {
        trace_counted_sum_iteration(loop_shape, "counted_packed_sum");
        crate::execution_trace::event(crate::execution_trace::Event::PackedArrayGet);
        // SAFETY: admission proves `mask < len`, and bitwise masking cannot
        // produce an index larger than `mask`.
        total += unsafe { (*cells.add(counter & mask)).get() };
    }
    Some(CountedSumResult {
        index: end as f64,
        total,
        iterations,
    })
}

fn exact_nonnegative_index(value: f64) -> Option<usize> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0)
        .then(|| value as usize)
}

fn unit_less_than_iterations(index: f64, bound: f64) -> Option<usize> {
    (index.is_finite() && bound.is_finite()).then_some(())?;
    if bound <= index {
        return Some(0);
    }
    let iterations = (bound - index).ceil();
    (iterations <= usize::MAX as f64).then_some(iterations as usize)
}

#[inline(always)]
fn trace_counted_sum_iteration(loop_shape: u64, kernel: &'static str) {
    crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
    crate::execution_trace::loop_shape_iteration(loop_shape);
    crate::execution_trace::event(crate::execution_trace::Event::CountedForHit);
    crate::execution_trace::kernel(kernel, false);
}

fn recognize_invariant_sum(
    body: crate::machine::CodeView<'_>,
    dst: u16,
    fact: CountedForFact,
    per_iteration: &[u16],
) -> Option<InvariantSumPlan> {
    (per_iteration == [fact.slot] && fact.timing == CountedStepTiming::AfterBody).then_some(())?;
    recognize_local_invariant_sum(body, dst)
        .or_else(|| recognize_array_length_sum(body, dst))
        .or_else(|| recognize_word_call_sum(body, dst))
        .or_else(|| recognize_packed_masked_sum(body, dst, fact))
}

fn recognize_word_call_sum(
    body: crate::machine::CodeView<'_>,
    dst: u16,
) -> Option<InvariantSumPlan> {
    (body.len() == 5).then_some(())?;
    let [function, total, call, store, result] =
        std::array::from_fn(|pc| body.instruction(pc).unwrap());
    (is_local_load(function)
        && is_local_load(total)
        && call.opcode == crate::ir::Opcode::Call
        && call.flags == 1
        && (call.b, call.c) == (function.a, total.a)
        && is_local_store(store)
        && (store.a, store.b) == (total.b, call.a)
        && result.opcode == crate::ir::Opcode::Move
        && (result.a, result.b) == (dst, call.a))
        .then_some(InvariantSumPlan {
            total: total.b,
            addend: CountedAddend::WordCall {
                function: function.b,
            },
        })
}

fn recognize_local_invariant_sum(
    body: crate::machine::CodeView<'_>,
    dst: u16,
) -> Option<InvariantSumPlan> {
    (body.len() == 5).then_some(())?;
    let [total, value, add, store, result] =
        std::array::from_fn(|pc| body.instruction(pc).unwrap());
    invariant_sum_tail(total, value.a, add, store, result, dst)?;
    is_local_load(value).then_some(InvariantSumPlan {
        total: total.b,
        addend: CountedAddend::Local(value.b),
    })
}

fn recognize_array_length_sum(
    body: crate::machine::CodeView<'_>,
    dst: u16,
) -> Option<InvariantSumPlan> {
    (body.len() == 6).then_some(())?;
    let [total, array, length, add, store, result] =
        std::array::from_fn(|pc| body.instruction(pc).unwrap());
    (is_local_load(array)
        && length.opcode == crate::ir::Opcode::GetN
        && length.b == array.a
        && body.metadata_at(2)?.name.as_deref() == Some("length"))
    .then_some(())?;
    invariant_sum_tail(total, length.a, add, store, result, dst)?;
    Some(InvariantSumPlan {
        total: total.b,
        addend: CountedAddend::ArrayLength(array.b),
    })
}

fn recognize_packed_masked_sum(
    body: crate::machine::CodeView<'_>,
    dst: u16,
    fact: CountedForFact,
) -> Option<InvariantSumPlan> {
    (body.len() == 9).then_some(())?;
    let [total, array, index, constant, masked, get, add, store, result] =
        std::array::from_fn(|pc| body.instruction(pc).unwrap());
    let (_, crate::ops::Constant::Number(mask)) = body.constant_at(3)? else {
        return None;
    };
    let mask = exact_nonnegative_index(*mask)?;
    (mask <= i32::MAX as usize
        && is_local_load(array)
        && is_local_load(index)
        && index.b == fact.slot
        && constant.opcode == crate::ir::Opcode::LoadConst
        && masked.opcode == crate::ir::Opcode::Binary
        && crate::ir::compact_binary_operator(masked.flags)
            == Some(crate::ops::BinaryOp::BitwiseAnd)
        && (masked.b, masked.c) == (index.a, constant.a)
        && get.opcode == crate::ir::Opcode::AGetI
        && (get.b, get.c) == (array.a, masked.a))
        .then_some(())?;
    invariant_sum_tail(total, get.a, add, store, result, dst)?;
    Some(InvariantSumPlan {
        total: total.b,
        addend: CountedAddend::PackedMasked {
            array: array.b,
            mask,
        },
    })
}

fn invariant_sum_tail(
    total: crate::ir::Instruction,
    addend: u16,
    add: crate::ir::Instruction,
    store: crate::ir::Instruction,
    result: crate::ir::Instruction,
    dst: u16,
) -> Option<()> {
    (total.opcode == crate::ir::Opcode::LoadLocal
        && add.opcode == crate::ir::Opcode::Add
        && (add.b, add.c) == (total.a, addend)
        && is_local_store(store)
        && (store.a, store.b) == (total.b, add.a)
        && result.opcode == crate::ir::Opcode::Move
        && (result.a, result.b) == (dst, add.a))
        .then_some(())
}

fn is_local_load(instruction: crate::ir::Instruction) -> bool {
    matches!(
        instruction.opcode,
        crate::ir::Opcode::LoadLocal | crate::ir::Opcode::LoadLocalChecked
    )
}

fn is_local_store(instruction: crate::ir::Instruction) -> bool {
    matches!(
        instruction.opcode,
        crate::ir::Opcode::StoreLocal | crate::ir::Opcode::StoreLocalChecked
    )
}

fn trace_counted_recognition(
    body: crate::machine::CodeView<'_>,
    fact: CountedForFact,
    per_iteration: &[u16],
) {
    crate::execution_trace::event(crate::execution_trace::Event::CountedForRecognized);
    if !per_iteration.is_empty() {
        crate::execution_trace::event(crate::execution_trace::Event::CountedForPerIteration);
    }
    dump_counted_shape(body, fact, per_iteration);
}

#[cfg(feature = "execution-trace")]
fn dump_counted_rejection(
    fingerprint: u64,
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    per_iteration: &[u16],
) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<u64>>> =
        std::sync::OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("QUENCH_DUMP_LOOP_SHAPES").is_some())
        || !SEEN
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(fingerprint)
    {
        return;
    }
    eprintln!(
        "LOOP_REJECT hash={fingerprint} test_len={} body_len={} update_len={} per_iteration={per_iteration:?}",
        test.len(),
        body.len(),
        update.len()
    );
    dump_loop_fragment("test", test);
    dump_loop_fragment("body", body);
    dump_loop_fragment("update", update);
}

#[cfg(feature = "execution-trace")]
fn dump_loop_fragment(name: &str, code: crate::machine::CodeView<'_>) {
    for pc in 0..code.len() {
        let instruction = code.instruction(pc).unwrap();
        let cold = code.cold(instruction).map(crate::ops::Op::variant_name);
        let operands = code.operand_window_at(pc);
        let property = code
            .metadata_at(pc)
            .and_then(|metadata| metadata.name.as_deref());
        let constant = code.constant_at(pc).map(|(_, constant)| constant);
        eprintln!(
            "  {name}[{pc}]: {instruction:?} cold={cold:?} name={property:?} constant={constant:?} operands={operands:?}"
        );
    }
}

#[cfg(not(feature = "execution-trace"))]
fn dump_counted_rejection(
    _: u64,
    _: crate::machine::CodeView<'_>,
    _: crate::machine::CodeView<'_>,
    _: crate::machine::CodeView<'_>,
    _: &[u16],
) {
}

#[cfg(feature = "execution-trace")]
fn dump_counted_shape(
    body: crate::machine::CodeView<'_>,
    fact: CountedForFact,
    per_iteration: &[u16],
) {
    use std::hash::{Hash, Hasher};
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<u64>>> =
        std::sync::OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("QUENCH_DUMP_LOOP_SHAPES").is_some()) {
        return;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for pc in 0..body.len() {
        let instruction = body.instruction(pc).unwrap();
        (instruction.opcode as u8).hash(&mut hasher);
        instruction.flags.hash(&mut hasher);
        instruction.a.hash(&mut hasher);
        instruction.b.hash(&mut hasher);
        instruction.c.hash(&mut hasher);
    }
    let fingerprint = hasher.finish();
    if !SEEN
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(fingerprint)
    {
        return;
    }
    eprintln!(
        "LOOP_SHAPE len={} hash={fingerprint} slot={} bound={:?} comparison={:?} step={} timing={:?} per_iteration={per_iteration:?}",
        body.len(),
        fact.slot,
        fact.bound,
        fact.comparison,
        fact.step,
        fact.timing
    );
    for pc in 0..body.len() {
        let instruction = body.instruction(pc).unwrap();
        let cold = body.cold(instruction).map(crate::ops::Op::variant_name);
        eprintln!("  {pc}: {instruction:?} cold={cold:?}");
        if let Some(crate::ops::Op::Branch {
            then_ops, else_ops, ..
        }) = body.cold(instruction)
        {
            dump_loop_fragment("then", then_ops.code().unwrap());
            dump_loop_fragment("else", else_ops.code().unwrap());
        }
    }
}

#[cfg(not(feature = "execution-trace"))]
fn dump_counted_shape(_: crate::machine::CodeView<'_>, _: CountedForFact, _: &[u16]) {}

fn run_counted_for(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    dst: u16,
    registers: &mut crate::register_file::RegisterFile,
    loop_shape: u64,
    index_unused_by_body: bool,
    guard_mutations: bool,
) -> Result<Option<crate::completion::Completion>, crate::execute::VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::CountedForAttempt);
    let environment = crate::locals::current();
    let context = crate::vm::current_context_or_default();
    #[cfg(not(feature = "execution-trace"))]
    if let Some(completion) = run_dense_array_copy(fact, body, dst, registers, &environment) {
        return Ok(Some(completion));
    }
    let word_body = (!guard_mutations).then(|| proven_word_move_body(&environment, body)).flatten();
    let word_calls = (!guard_mutations)
        .then(|| proven_word_call_body(&environment, body, dst, registers))
        .flatten();
    let word_call_chain = word_calls
        .as_deref()
        .is_some_and(crate::register_file::ImmediateNumberCallPlan::is_chain);
    if index_unused_by_body
        && fact.timing == CountedStepTiming::AfterBody
        && matches!(fact.bound, CountedBound::Constant(_))
    {
        let Some(mut index) = environment.get_number(fact.slot) else {
            crate::execution_trace::event(crate::execution_trace::Event::CountedForDeopt);
            crate::execution_trace::kernel("counted_for", true);
            return Ok(None);
        };
        let CountedBound::Constant(bound) = fact.bound else {
            unreachable!("constant-bound admission checked above")
        };
        if let Some(plans) = word_calls.as_deref() {
            let Some(iterations) = counted_call_iterations(fact, index, bound) else {
                return Ok(None);
            };
            #[cfg(not(feature = "execution-trace"))]
            if crate::register_file::ImmediateNumberCallPlan::execute_chain_iterations(
                plans, iterations,
            ) {
                index += fact.step * iterations as f64;
                environment.set(fact.slot, crate::value::Value::Number(index));
                return Ok(Some(crate::completion::Completion::Normal));
            }
            for _ in 0..iterations {
                trace_counted_call_iteration(loop_shape, plans.len());
                execute_word_call_plans(plans, word_call_chain);
            }
            index += fact.step * iterations as f64;
            environment.set(fact.slot, crate::value::Value::Number(index));
            return Ok(Some(crate::completion::Completion::Normal));
        }
        #[cfg(not(feature = "execution-trace"))]
        if let Some(plans) = word_body.as_deref() {
            while counted_comparison(fact.comparison, index, bound) {
                for plan in plans {
                    plan.execute();
                }
                index += fact.step;
            }
            environment.set(fact.slot, crate::value::Value::Number(index));
            return Ok(Some(crate::completion::Completion::Normal));
        }
        while counted_comparison(fact.comparison, index, bound) {
            crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
            crate::execution_trace::loop_shape_iteration(loop_shape);
            crate::execution_trace::event(crate::execution_trace::Event::CountedForHit);
            crate::execution_trace::kernel("counted_for", false);
            let body_index = index;
            let body_bound = fact.bound.number(&environment);
            match execute_counted_body(body, registers, &context, word_body.as_deref())? {
                crate::completion::LoopTransition::Continue(value) => {
                    if guard_mutations
                        && !counted_state_unchanged(
                            fact,
                            body_index,
                            body_bound,
                            &environment,
                        )
                    {
                        run_fragment(update, registers)?;
                        return Ok(None);
                    }
                    store_loop_value(registers, dst, value)?;
                }
                crate::completion::LoopTransition::Break(value) => {
                    store_loop_value(registers, dst, value)?;
                    environment.set(fact.slot, crate::value::Value::Number(index));
                    return Ok(Some(crate::completion::Completion::Normal));
                }
                crate::completion::LoopTransition::Propagate(completion) => {
                    environment.set(fact.slot, crate::value::Value::Number(index));
                    return update_empty_from(registers, dst, completion).map(Some);
                }
            }
            index += fact.step;
        }
        environment.set(fact.slot, crate::value::Value::Number(index));
        return Ok(Some(crate::completion::Completion::Normal));
    }
    loop {
        crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
        crate::execution_trace::loop_shape_iteration(loop_shape);
        let Some(mut index) = environment.get_number(fact.slot) else {
            crate::execution_trace::event(crate::execution_trace::Event::CountedForDeopt);
            crate::execution_trace::kernel("counted_for", true);
            return Ok(None);
        };
        if fact.timing == CountedStepTiming::BeforeTest {
            let Some((_, updated)) = environment.update_number(fact.slot, fact.step) else {
                crate::execution_trace::event(crate::execution_trace::Event::CountedForDeopt);
                crate::execution_trace::kernel("counted_for", true);
                return Ok(None);
            };
            index = updated;
        }
        let Some(bound) = fact.bound.number(&environment) else {
            crate::execution_trace::event(crate::execution_trace::Event::CountedForDeopt);
            crate::execution_trace::kernel("counted_for", true);
            return Ok(None);
        };
        if !counted_comparison(fact.comparison, index, bound) {
            return Ok(Some(crate::completion::Completion::Normal));
        }
        crate::execution_trace::event(crate::execution_trace::Event::CountedForHit);
        crate::execution_trace::kernel("counted_for", false);
        let body_index = index;
        let body_bound = Some(bound);
        match execute_counted_body(body, registers, &context, word_body.as_deref())? {
            crate::completion::LoopTransition::Continue(value) => {
                if guard_mutations
                    && !counted_state_unchanged(
                        fact,
                        body_index,
                        body_bound,
                        &environment,
                    )
                {
                    run_fragment(update, registers)?;
                    return Ok(None);
                }
                store_loop_value(registers, dst, value)?;
            }
            crate::completion::LoopTransition::Break(value) => {
                store_loop_value(registers, dst, value)?;
                return Ok(Some(crate::completion::Completion::Normal));
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                return update_empty_from(registers, dst, completion).map(Some);
            }
        }
        if fact.timing == CountedStepTiming::AfterBody {
            let Some((_, _)) = environment.update_number(fact.slot, fact.step) else {
                crate::execution_trace::event(crate::execution_trace::Event::CountedForDeopt);
                crate::execution_trace::kernel("counted_for", true);
                run_fragment(update, registers)?;
                return Ok(None);
            };
        }
    }
}

/// Execute the closed array-copy loop emitted for a reverse numeric walk.
///
/// This is admitted from the instruction shape, never from source text.  A
/// preflight proves every read is numeric and every write targets an existing
/// plain dense slot before mutating anything; otherwise the caller keeps the
/// ordinary interpreter path, preserving partial-write and prototype
/// semantics for all uncertain cases.
#[cfg(not(feature = "execution-trace"))]
#[inline]
fn run_dense_array_copy(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    dst: u16,
    registers: &mut crate::register_file::RegisterFile,
    environment: &crate::environment::Environment,
) -> Option<crate::completion::Completion> {
    if fact.timing != CountedStepTiming::AfterBody
        || fact.step != -1.0
        || fact.comparison != crate::ops::BinaryOp::GreaterEqual
        || !matches!(fact.bound, CountedBound::Constant(0.0))
        || body.len() != 12
    {
        return None;
    }
    let matches = |pc, opcode| {
        body.instruction(pc).is_some_and(|item| item.opcode == opcode && item.flags == 0)
    };
    use crate::ir::Opcode;
    if !(matches(0, Opcode::LoadLocal)
        && matches(1, Opcode::Move)
        && matches(2, Opcode::LoadLocal)
        && matches(3, Opcode::LoadLocal)
        && matches(4, Opcode::Add)
        && matches(5, Opcode::Move)
        && matches(6, Opcode::LoadLocal)
        && matches(7, Opcode::Slow)
        && matches(8, Opcode::LoadLocal)
        && matches(9, Opcode::AGetI)
        && matches(10, Opcode::ASetI)
        && matches(11, Opcode::Move))
    {
        return None;
    }
    let i0 = body.instruction(0)?;
    let i1 = body.instruction(1)?;
    let i2 = body.instruction(2)?;
    let i3 = body.instruction(3)?;
    let i4 = body.instruction(4)?;
    let i5 = body.instruction(5)?;
    let i6 = body.instruction(6)?;
    let i7 = body.instruction(7)?;
    let i8 = body.instruction(8)?;
    let i9 = body.instruction(9)?;
    let i10 = body.instruction(10)?;
    let i11 = body.instruction(11)?;
    if i0.b != i1.b
        || i1.b != i0.a
        || i5.b != i1.a
        || i2.b != fact.slot
        || i4.b != i2.a
        || i4.c != i3.a
        || i6.b == fact.slot
        || i8.b != fact.slot
        || i9.b != i6.a
        || i9.c != i8.a
        || i10.a != i5.a
        || i10.b != i4.a
        || i10.c != i9.a
        || i11.a != dst
        || i11.b != i9.a
    {
        return None;
    }
    let Some(crate::ops::Op::RequireObjectCoercible { src }) = body.cold(i7) else {
        return None;
    };
    if *src != i6.a {
        return None;
    }
    let source = match environment.get(i6.b) {
        crate::value::Value::Array(array) => array,
        _ => return None,
    };
    let target = match environment.get(i0.b) {
        crate::value::Value::Array(array) => array,
        _ => return None,
    };
    if !source.is_plain_dense_access() || !target.is_plain_dense_access() {
        return None;
    }
    let offset = environment.get_number(i3.b)?;
    if !offset.is_finite() || offset.fract() != 0.0 {
        return None;
    }
    let mut index = environment.get_number(fact.slot)?;
    if !index.is_finite() || index.fract() != 0.0 || index < 0.0 {
        return None;
    }

    // Preflight all iterations.  This makes the fast path atomic with respect
    // to the fallback: no array write occurs until every indexed read/write is
    // known to be an ordinary numeric operation.
    let mut count = 0usize;
    while index >= 0.0 {
        let source_index = usize::try_from(index as u128).ok()?;
        let target_number = index + offset;
        if !target_number.is_finite() || target_number.fract() != 0.0 || target_number < 0.0 {
            return None;
        }
        let target_index = usize::try_from(target_number as u128).ok()?;
        source.dense_number_at(source_index)?;
        if target.dense_number_at(target_index).is_none() {
            return None;
        }
        count = count.checked_add(1)?;
        index -= 1.0;
    }

    index = environment.get_number(fact.slot)?;
    for _ in 0..count {
        let source_index = index as usize;
        let target_index = (index + offset) as usize;
        let number = source.dense_number_at(source_index)?;
        debug_assert!(target.set_plain_existing_f64(target_index, number));
        registers.write_number(usize::from(dst), number);
        index -= 1.0;
    }
    environment.set(fact.slot, crate::value::Value::Number(index));
    Some(crate::completion::Completion::Normal)
}

/// Counted-loop admission must not turn an arbitrary compact instruction
/// stream into a second call/property interpreter. Calls and observable
/// reads/writes stay on the ordinary loop path; only register/local arithmetic
/// has a closed transition that the Rust loop can replay safely.
#[inline]
fn counted_body_is_pure(body: crate::machine::CodeView<'_>) -> bool {
    (0..body.len()).all(|pc| {
        body.instruction(pc).is_some_and(|instruction| {
            matches!(
                instruction.opcode,
                crate::ir::Opcode::LoadConst
                    | crate::ir::Opcode::Move
                    | crate::ir::Opcode::Add
                    | crate::ir::Opcode::Sub
                    | crate::ir::Opcode::Mul
                    | crate::ir::Opcode::Div
                    | crate::ir::Opcode::LoadLocal
                    | crate::ir::Opcode::LoadLocalChecked
                    | crate::ir::Opcode::UpdateLocal
                    | crate::ir::Opcode::StoreLocal
                    | crate::ir::Opcode::StoreLocalChecked
                    | crate::ir::Opcode::InitLocal
                    | crate::ir::Opcode::Binary
            )
        })
    })
}

/// Admit compact loops with calls only when the loop-control locals are not
/// written directly. Calls remain on the complete VM path; the executor
/// checks the counter and bound after each body and deopts if an indirect call
/// mutates either one.
#[inline]
fn counted_body_is_guarded(body: crate::machine::CodeView<'_>, counter: u16) -> bool {
    !body.is_empty()
        && (0..body.len()).all(|pc| {
            let Some(instruction) = body.instruction(pc) else {
                return false;
            };
            match instruction.opcode {
                crate::ir::Opcode::StoreLocal
                | crate::ir::Opcode::StoreLocalChecked
                | crate::ir::Opcode::InitLocal => instruction.a != counter,
                crate::ir::Opcode::UpdateLocal => instruction.c != counter,
                _ => true,
            }
        })
}

#[inline]
fn counted_state_unchanged(
    fact: CountedForFact,
    index: f64,
    bound: Option<f64>,
    environment: &crate::environment::Environment,
) -> bool {
    environment.get_number(fact.slot) == Some(index)
        && match (fact.bound, bound) {
            (CountedBound::Constant(_), _) => true,
            (CountedBound::Slot(slot), Some(value)) => environment.get_number(slot) == Some(value),
            (CountedBound::Slot(_), None) => false,
        }
}

#[inline(always)]
fn execute_word_call_plans(
    plans: &[crate::register_file::ImmediateNumberCallPlan],
    admitted_chain: bool,
) {
    if admitted_chain {
        crate::register_file::ImmediateNumberCallPlan::execute_admitted_chain(plans);
        return;
    }
    match plans {
        [first] => first.execute(),
        [first, second] => {
            first.execute();
            second.execute();
        }
        [first, second, third] => {
            first.execute();
            second.execute();
            third.execute();
        }
        [first, second, third, fourth] => {
            first.execute();
            second.execute();
            third.execute();
            fourth.execute();
        }
        _ => plans.iter().for_each(|plan| plan.execute()),
    }
}

fn counted_call_iterations(fact: CountedForFact, index: f64, bound: f64) -> Option<usize> {
    (fact.comparison == crate::ops::BinaryOp::LessThan && fact.step == 1.0).then_some(())?;
    unit_less_than_iterations(index, bound)
}

fn proven_word_call_body(
    environment: &crate::environment::Environment,
    body: crate::machine::CodeView<'_>,
    dst: u16,
    registers: &mut crate::register_file::RegisterFile,
) -> Option<Vec<crate::register_file::ImmediateNumberCallPlan>> {
    (body.len() >= 5 && body.len() % 5 == 0).then_some(())?;
    (0..body.len())
        .step_by(5)
        .map(|pc| {
            let function = body.instruction(pc)?;
            let argument = body.instruction(pc + 1)?;
            let call = body.instruction(pc + 2)?;
            let store = body.instruction(pc + 3)?;
            let result = body.instruction(pc + 4)?;
            (is_local_load(function)
                && is_local_load(argument)
                && call.opcode == crate::ir::Opcode::Call
                && call.flags == 1
                && (call.b, call.c) == (function.a, argument.a)
                && is_local_store(store)
                && (store.a, store.b) == (argument.b, call.a)
                && result.opcode == crate::ir::Opcode::Move
                && (result.a, result.b) == (dst, call.a))
                .then_some(())?;
            environment.plan_word_add_constant(function.b, argument.b, store.a, registers, dst)
        })
        .collect()
}

#[inline(always)]
fn trace_counted_call_iteration(loop_shape: u64, calls: usize) {
    crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
    crate::execution_trace::loop_shape_iteration(loop_shape);
    crate::execution_trace::event(crate::execution_trace::Event::CountedForHit);
    crate::execution_trace::kernel("counted_for", false);
    for _ in 0..calls {
        crate::execution_trace::event(crate::execution_trace::Event::LeafAttempt);
        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
        crate::execution_trace::kernel("word_call_add_constant", false);
    }
}

fn proven_word_move_body(
    environment: &crate::environment::Environment,
    body: crate::machine::CodeView<'_>,
) -> Option<Vec<crate::register_file::ImmediateCopyPlan>> {
    (!body.is_empty()).then_some(())?;
    (0..body.len())
        .map(|pc| {
            let instruction = body.instruction(pc)?;
            (instruction.opcode == crate::ir::Opcode::Move && instruction.flags == 1)
                .then_some(())?;
            crate::locals::can_move_proven_local(environment, instruction.b, instruction.c)
                .then_some(())?;
            environment.plan_immediate_move(instruction.b, instruction.c)
        })
        .collect()
}

fn execute_counted_body(
    body: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &crate::vm::VmContext,
    word_body: Option<&[crate::register_file::ImmediateCopyPlan]>,
) -> Result<crate::completion::LoopTransition, crate::execute::VmError> {
    let Some(plans) = word_body else {
        return execute_loop_body_with_context(registers, &None, body, context);
    };
    for (pc, plan) in plans.iter().enumerate() {
        let instruction = body.instruction(pc).expect("word body was validated");
        let _decode_guard = crate::execution_trace::compact(instruction.opcode);
        crate::execution_trace::compact_site(body, pc);
        crate::execution_trace::operands(instruction);
        plan.execute();
        crate::execution_trace::event(crate::execution_trace::Event::RegisterWordCopy);
    }
    Ok(crate::completion::LoopTransition::Continue(None))
}

macro_rules! counted_comparisons {
    ($($variant:ident => $operator:tt),+ $(,)?) => {
        fn counted_comparison(operator: crate::ops::BinaryOp, lhs: f64, rhs: f64) -> bool {
            match operator {
                $(crate::ops::BinaryOp::$variant => lhs $operator rhs,)+
                _ => false,
            }
        }
    };
}

counted_comparisons! {
    LessThan => <,
    LessEqual => <=,
    GreaterThan => >,
    GreaterEqual => >=,
}

#[derive(Clone, Copy, Debug)]
struct CountedForFact {
    slot: u16,
    bound: CountedBound,
    comparison: crate::ops::BinaryOp,
    step: f64,
    timing: CountedStepTiming,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CountedStepTiming {
    BeforeTest,
    AfterBody,
}

#[derive(Clone, Copy, Debug)]
enum CountedBound {
    Constant(f64),
    Slot(u16),
}

impl CountedBound {
    fn number(self, environment: &crate::environment::Environment) -> Option<f64> {
        match self {
            Self::Constant(value) => Some(value),
            Self::Slot(slot) => environment.get_number(slot),
        }
    }
}

impl CountedForFact {
    fn recognize(
        test: crate::machine::CodeView<'_>,
        update: crate::machine::CodeView<'_>,
    ) -> Option<Self> {
        Self::recognize_after_body(test, update)
            .or_else(|| Self::recognize_before_test(test, update))
    }

    fn recognize_after_body(
        test: crate::machine::CodeView<'_>,
        update: crate::machine::CodeView<'_>,
    ) -> Option<Self> {
        if test.len() != 4 {
            return None;
        }
        let (index, slot) = recognized_static_load(test, 0)?;
        let (bound_register, bound) = recognized_counted_bound(test, 1)?;
        let (condition, comparison, lhs, rhs) = test.binary_at(2)?;
        let returned = test.instruction(3)?;
        if returned.opcode != crate::ir::Opcode::Return || returned.a != condition {
            return None;
        }
        if lhs != index || rhs != bound_register {
            return None;
        }
        let step = recognize_counted_update(update, slot)?;
        Some(Self {
            slot,
            bound,
            comparison,
            step,
            timing: CountedStepTiming::AfterBody,
        })
    }

    fn recognize_before_test(
        test: crate::machine::CodeView<'_>,
        update: crate::machine::CodeView<'_>,
    ) -> Option<Self> {
        (test.len() == 4 && update.is_empty()).then_some(())?;
        let decrement = test.instruction(0)?;
        (decrement.opcode == crate::ir::Opcode::UpdateLocal && decrement.flags != 0)
            .then_some(())?;
        let (bound_register, bound) = recognized_counted_bound(test, 1)?;
        let (condition, comparison, lhs, rhs) = test.binary_at(2)?;
        let returned = test.instruction(3)?;
        (lhs == decrement.b && rhs == bound_register).then_some(())?;
        (returned.opcode == crate::ir::Opcode::Return && returned.a == condition).then_some(())?;
        Some(Self {
            slot: decrement.c,
            bound,
            comparison,
            step: -1.0,
            timing: CountedStepTiming::BeforeTest,
        })
    }
}

fn recognized_counted_bound(
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Option<(u16, CountedBound)> {
    if let Some((register, slot)) = recognized_static_load(code, pc) {
        return Some((register, CountedBound::Slot(slot)));
    }
    let (dst, crate::ops::Constant::Number(value)) = code.constant_at(pc)? else {
        return None;
    };
    Some((dst, CountedBound::Constant(*value)))
}

fn recognized_static_load(code: crate::machine::CodeView<'_>, pc: usize) -> Option<(u16, u16)> {
    let instruction = code.instruction(pc)?;
    matches!(
        instruction.opcode,
        crate::ir::Opcode::LoadLocal | crate::ir::Opcode::LoadLocalChecked
    )
    .then_some((instruction.a, instruction.b))
}

fn recognize_counted_update(update: crate::machine::CodeView<'_>, slot: u16) -> Option<f64> {
    if matches!(update.len(), 2 | 3) {
        let instruction = update.instruction(0)?;
        let returned = update.instruction(update.len() - 1)?;
        let valid_return = if update.len() == 2 {
            // Prefix update returns the updated word (`b`); compact postfix
            // update returns the already-ToNumeric old word (`a`).
            returned.a == instruction.a || returned.a == instruction.b
        } else {
            match update.cold_at(1)? {
                Op::Unary {
                    dst,
                    operator: crate::ops::UnaryOp::Void,
                    src,
                } => *src == instruction.b && returned.a == *dst,
                Op::Unary {
                    dst,
                    operator: crate::ops::UnaryOp::ToNumeric,
                    src,
                } => *src == instruction.a && returned.a == *dst,
                _ => false,
            }
        };
        if instruction.opcode == crate::ir::Opcode::UpdateLocal
            && instruction.c == slot
            && returned.opcode == crate::ir::Opcode::Return
            && valid_return
        {
            return Some(if instruction.flags == 0 { 1.0 } else { -1.0 });
        }
    }
    let checked = match update.len() {
        5 => false,
        6 => {
            matches!(update.cold_at(3), Some(Op::CheckInitialized { slot: checked, .. }) if *checked == slot)
        }
        _ => return None,
    };
    if update.len() == 6 && !checked {
        return None;
    }
    let load = update.instruction(0)?;
    if load.opcode != crate::ir::Opcode::LoadLocal || load.b != slot {
        return None;
    }
    let (step_register, crate::ops::Constant::Number(step)) = update.constant_at(1)? else {
        return None;
    };
    let (next, crate::ops::BinaryOp::NumericAdd, lhs, rhs) = update.binary_at(2)? else {
        return None;
    };
    let store_pc = if checked { 4 } else { 3 };
    let Op::StoreLocal {
        slot: stored_slot,
        src: stored,
    } = update.cold_at(store_pc)?
    else {
        return None;
    };
    let returned = update.instruction(store_pc + 1)?;
    (*stored_slot == slot
        && load.a == lhs
        && step_register == rhs
        && next == *stored
        && returned.opcode == crate::ir::Opcode::Return
        && returned.a == next)
        .then_some(*step)
}

#[cfg(test)]
mod counted_update_tests {
    #[test]
    fn compact_postfix_result_is_a_counted_update() {
        let mut arena = crate::machine::CodeArena::new();
        let range = arena.append_slice(&[
            crate::ops::Op::LoadLocal { dst: 2, slot: 7 },
            crate::ops::Op::Const {
                dst: 3,
                value: crate::ops::Constant::Number(1.0),
            },
            crate::ops::Op::Binary {
                dst: 4,
                operator: crate::ops::BinaryOp::NumericAdd,
                lhs: 2,
                rhs: 3,
            },
            crate::ops::Op::StoreLocal { slot: 7, src: 4 },
            crate::ops::Op::Unary {
                dst: 5,
                operator: crate::ops::UnaryOp::ToNumeric,
                src: 2,
            },
            crate::ops::Op::Return { src: 5 },
        ]);
        let store = arena.freeze();
        let update = store.code(range).expect("compact update");
        assert_eq!(update.len(), 2);
        assert_eq!(super::recognize_counted_update(update, 7), Some(1.0));
    }
}

fn store_loop_value(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    value: Option<crate::value::Value>,
) -> Result<(), crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(());
    };
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

fn update_empty_from(
    registers: &crate::register_file::RegisterFile,
    dst: u16,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let value = crate::execute::read_register(registers, dst)?;
    Ok(completion.update_empty(value))
}

fn loop_test(
    test: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<bool, crate::execute::VmError> {
    match crate::vm::execute_code_completion_in_current_frame(test, registers)? {
        crate::completion::Completion::Return(value) => Ok(crate::execute::is_truthy(&value)),
        crate::completion::Completion::Normal => Ok(false),
        completion => completion
            .into_vm_error()
            .map(|value| crate::execute::is_truthy(&value)),
    }
}

/// Run a loop fragment. An empty fragment (no init/update, e.g. a `while`
/// loop) is a no-op; a non-empty fragment must return normally.
fn run_fragment(
    ops: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<(), crate::execute::VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::FragmentEntry);
    if ops.is_empty() {
        return Ok(());
    }
    match crate::vm::execute_code_completion_in_current_frame(ops, registers)? {
        // Loop fragments use Return as their local value carrier. They are
        // not function boundaries, so consume that marker while preserving
        // the current lexical environment for the next fragment.
        crate::completion::Completion::Normal | crate::completion::Completion::Return(_) => Ok(()),
        completion => completion.into_vm_error().map(|_| ()),
    }
}

fn refresh_per_iteration(slots: &[u16]) {
    let environment = crate::locals::current();
    for &slot in slots {
        let value = environment.get(slot);
        let _ = environment.replace_slot(slot, value);
    }
}
