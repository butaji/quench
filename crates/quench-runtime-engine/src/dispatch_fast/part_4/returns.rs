//! Returns from a lane: a return's value through its activations and chains, the wasm completion of a return, and the returning handler rows.
use super::*;

/// `call_return::step_activation_return_of` of the innermost activation, a script one whose call keeps its
/// result: of the caller's program (`Completion::Plain`), or, where `CROSSES`, of another
/// (`Completion::Cross`), which enters the caller's program first, or, where `CONSTRUCTS`, a
/// `new` of its program (`Completion::Constructs`), or, where `SETTLES`, a settling async
/// call (`Completion::SettlesPromise`), whose promise the value resolves and the call yields.
/// `INTO_BODY` admits a returning call too.
#[inline(always)]
pub(in super::super) fn return_from_in_lane<const CROSSES: bool, const INTO_BODY: bool, const CONSTRUCTS: bool, const SETTLES: bool>(cx: *mut LaneContext, mut value: Value) -> Option<(Registers, Ip)> {
    debug_assert!(unsafe { (*cx).leaf.call }.is_null(), "a leaf's own returns complete it");
    let vm = unsafe { vm(cx) };
    // A resolution that would observe the value returns it from the frame instead.
    let then = if SETTLES { settled_then_in_lane(vm, value)? } else { Value::UNDEFINED };
    // A construction yields its receiver, the activation's `this`, for a non-object result.
    // The construct family's completion row (`staged::call_construct::complete_construct`): the
    // receiver is the returning window's `this` context word, read only for a non-object.
    if CONSTRUCTS {
        let window = unsafe { (*cx).registers }.0 as u64;
        value = Value::from_bits(quench_runtime::staged::call_construct::concrete_complete_construct(window, value.to_bits(), vm.is_object_like(value)));
    }
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // A plain header's own return (`completions::lane_return_header`) ran before this one
    // (`call_return::step_activation_return_of`, `return_to_body_in_lane`): what reaches here is a side record's
    // completion, a returning call's chain or a record.
    // The innermost read where it is, a header with its side record or a record (no spill:
    // one-frame-one-call.md F4).
    let top = stack.top_frame()?;
    let expected = returning_completion(CROSSES, CONSTRUCTS, SETTLES);
    if top.completion() != Some(expected) { return None; }
    // SAFETY: the call record belongs to the caller's live view.
    let destination = unsafe { (*top.call()).a };
    if !INTO_BODY && destination & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        if CROSSES || CONSTRUCTS || SETTLES {
            return None;
        }
        return return_chain_in_lane(cx, value);
    }
    let destination = destination & quench_runtime::bytecode::REGISTER_MASK;
    let (call, caller_registers, caller_base, window) = (top.call(), top.caller_registers(), top.caller_base(), top.window_word());
    let caller_program = top.caller_program();
    // SAFETY: a script activation holds no drop glue and no Wasm storage
    // (`Activation::frame_storage_bytes`); the values hold no drop glue.
    unsafe {
        stack.discard_top_frame();
        stack.values.release_window(window);
    }
    // A return from inside a `with` leaves its scope without an exit.
    stack.drop_with_objects_from(stack.activation_count());
    // The caller's memory view reads its program's records (`MemoryView::script`), which an
    // activation of the caller's own JavaScript program left in place.
    if CROSSES {
        enter_program_in_lane(cx, caller_program);
        unsafe { (*cx).memory = MemoryView::script(cx) };
    }
    debug_assert!(unsafe { (*cx).memory.bytes } == cx.cast::<u8>(), "a JavaScript run carries its context");
    unsafe { (*cx).registers = caller_registers; (*cx).base = caller_base; }
    if SETTLES { value = settle_promise_in_lane(cx, value, then, top.payload().settles()); }
    caller_registers.set(destination, value);
    Some((caller_registers, following(call)))
}

/// `call_return::step_activation_return_of` of a plain activation that returns the completion of its call, and
/// whose callers do in turn (`return f(...)`), down to the plain activation whose call takes
/// `value`: that activation's caller resumes with it, as the run's completion one activation
/// at a time (`complete_plain_chain_in_lane`) makes it. `None`, which changes nothing, for a run
/// another completion ends; the general completions take it.
#[inline(always)]
pub(in super::super) fn return_chain_in_lane(cx: *mut LaneContext, value: Value) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // The run walked where its frames are, headers with their side records, then records: no
    // spill and no record built (one-frame-one-call.md F4). `above` counts the frames inside
    // the one whose call takes the value.
    let (outer, destination, above) = {
        let mut frames = stack.frames_innermost();
        // The innermost returns its call's completion, which the caller of `return_from_in_lane` proved.
        let innermost = frames.next()?;
        debug_assert!(innermost.completion() == Some(Completion::Plain));
        let mut above = 1;
        loop {
            let frame = frames.next()?;
            if frame.completion() != Some(Completion::Plain) {
                return None;
            }
            // SAFETY: the call record belongs to the caller's live view.
            let destination = unsafe { (*frame.call()).a };
            if destination & quench_runtime::bytecode::RETURN_REGISTER == 0 {
                break (frame, destination, above);
            }
            above += 1;
        }
    };
    let (call, caller_registers, caller_base, window) = (outer.call(), outer.caller_registers(), outer.caller_base(), outer.window_word());
    let index = stack.activation_count() - 1 - above;
    stack.drop_owned_from(index + 1);
    // SAFETY: script activations hold no drop glue or Wasm storage; the values hold no drop
    // glue. The frames from the outer one in go where they are.
    unsafe {
        for _ in 0..=above { stack.discard_top_frame(); }
        stack.values.release_window(window);
        (*cx).registers = caller_registers; (*cx).base = caller_base;
    }
    caller_registers.set(destination & quench_runtime::bytecode::REGISTER_MASK, value);
    Some((caller_registers, following(call)))
}

/// Complete the innermost lane activation with `value`, which the record at `ip` of that
/// activation returns, as a return from it does: its caller resumes after the call with the
/// value in the call's result register, or, for a call that returns its callee's
/// completion, completes in turn. `None` when no activation runs: the running function is a
/// VM frame.
#[inline(always)]
pub(in super::super) fn complete_activation_in_lane(cx: *mut LaneContext, ip: Ip, mut value: Value) -> Option<Resumed> {
    if let Some(resumed) = complete_plain_chain_in_lane(cx, value) {
        return Some(resumed);
    }
    let vm = unsafe { vm(cx) };
    // The innermost's completion read where it is, a header with its side record or a record:
    // no spill and no record built (one-frame-one-call.md F4).
    let top = vm.lane_stack.top_frame()?;
    let completion = top.completion();
    if completion == Some(Completion::ToWasm) {
        return Some(lane_complete_to_wasm(cx, ip, value));
    }
    if proxy_traps::trap_result_needs_general_path(vm, top.payload(), value) {
        return Some(Resumed::Exit(frame_return(vm, ip, value)));
    }
    // A settling activation whose promise's resolution would observe the value returns it
    // from its frame, whose pending call resolves the promise (`settled_then_in_lane`); else the
    // `then` read here is the one its resolution uses.
    let mut then = Value::UNDEFINED;
    if completion == Some(Completion::SettlesPromise) {
        let Some(read) = settled_then_in_lane(vm, value) else { return Some(Resumed::Exit(frame_return(vm, ip, value))) };
        then = read;
    }
    // Each frame popped where it is (`ActivationView`): a header's pop builds no record.
    let mut frame = vm.lane_stack.pop_activation()?;
    loop {
        let (caller_memory, this) = restore_caller_in_lane(cx, &frame);
        if let Some(completion) = frame.completion() {
            // A native's callback ends its run, whatever the native's own call record is.
            if let Some(ended) = body_calls::ends_run(cx, &frame, completion, this, value) {
                return Some(ended);
            }
            if completion == Completion::ArrayStep {
                return Some(step_array_iteration_in_lane(cx, &frame, value));
            }
            if completion.is_generator() {
                return Some(generator_returned_in_lane(cx, &frame, caller_memory, value));
            }
            if completion == Completion::AsyncRequest {
                return Some(async_requests::request_returned_in_lane(cx, &frame, caller_memory, value));
            }
            // [[Construct]] of an ordinary constructor yields its returned object,
            // otherwise the receiver it allocated. A skipped forwarding constructor returns
            // `undefined` after the method, so its construction yields the receiver.
            // A trap of another program (`AccessorCall::trap_target`) completes as a
            // trap, not as `Completion::Cross`, and leaves its program the same way.
            if completion.crosses() || frame.caller_program() != unsafe { (*cx).program } {
                enter_program_in_lane(cx, frame.caller_program());
            }
            value = setters::completed_value(vm, &frame, completion, this, value);
            if completion == Completion::Forwarder {
                value = Value::from_bits(quench_runtime::staged::call_construct::concrete_forwarder_value(value.to_bits(), this.to_bits()));
            }
            if completion == Completion::SettlesPromise {
                value = settle_promise_in_lane(cx, value, then, frame.payload().settles());
            }
        } else if let Some(result) = frame.export_result() {
            // The export's caller runs its own program again, as the run's JavaScript.
            enter_program_in_lane(cx, frame.caller_program());
            unsafe {
                (*cx).inline_calls = false;
                set_script_calls_in_lane(cx, true);
                (*cx).instance = Value::DELETED;
            }
            value = vm.wasm_export_result(value, result);
        }
        // SAFETY: the call record belongs to the caller's live view.
        if unsafe { (*frame.call()).a } & quench_runtime::bytecode::RETURN_REGISTER == 0 {
            return Some(resume_after_call_in_lane(&frame, caller_memory, value));
        }
        // The call returns its callee's completion: its caller completes in turn, an
        // activation here or the frame in the general loop, which the exit hands it to.
        if caller_returns_in_frame(vm, value, &mut then) {
            return Some(Resumed::Exit(frame_return(vm, frame.call(), value)));
        }
        frame = vm.lane_stack.pop_activation().expect("the caller activation is the innermost");
    }
}

/// The return of a JavaScript import's callee (`Completion::ToWasm`) (`step_import_return_of`).
#[inline(never)]
pub(in super::super) fn lane_complete_to_wasm(cx: *mut LaneContext, ip: Ip, value: Value) -> Resumed {
    quench_runtime::staged::wasm_activation::step_import_return_of(&mut LaneReturn { cx, ip }, value)
}

/// The return of a Wasm activation whose call keeps its result (`step_wasm_return_of`), else `None`.
#[inline(always)]
pub(in super::super) fn lane_complete_wasm_activation(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    quench_runtime::staged::wasm_activation::step_wasm_return_of(&mut LaneReturn { cx, ip: std::ptr::null() }, value)
}

/// The `returning_miss` kind of a value record that returns its result from the frame.
pub(in super::super) fn returning_kind(instruction: WideInstruction) -> Option<u8> {
    if !instruction.returns_from_frame() {
        return None;
    }
    let traits = instruction.op().traits();
    if traits.has(Traits::RETURNING_FIELD) { Some(RETURNING_FIELD) }
    else if traits.has(Traits::RETURNING_UNARY) { Some(RETURNING_UNARY) }
    else if traits.has(Traits::RETURNING_BINARY) { Some(RETURNING_BINARY) }
    else if traits.has(Traits::RETURNING_OBJECT) { Some(RETURNING_OBJECT) }
    else if traits.has(Traits::RETURNING_OBJECT_LITERAL) { Some(RETURNING_OBJECT_LITERAL) }
    else if traits.has(Traits::RETURNING_LITERAL) { Some(RETURNING_LITERAL) }
    else { None }
}

/// The value of a `Unary` (`unary`) or `Binary` record whose operands the lane's own values
/// decline, made by the general operation in place (`unwound_in_place`): it may reach guest
/// code (an object's `ToPrimitive` hook) or throw, which is raised at the record. `Err` is the
/// record the run goes on at: the handler a throw entered, or the general path, also for an
/// operand the lane cannot read. Out of line, so the returning handlers keep their code.
#[cold]
#[inline(never)]
pub(in super::super) fn operation_in_place(cx: *mut LaneContext, ip: Ip, r: Registers, i: &LaneInstruction, unary: bool) -> Result<Value, Ip> {
    let operator = i.imm();
    if unary {
        let operand = r.get(i.b);
        return unwound_in_place(cx, ip, |vm, code| vm.unary(code, operator, operand));
    }
    let (Some(left), Some(right)) = (operand_value_in_lane(cx, r, i.b), operand_value_in_lane(cx, r, i.c)) else {
        return Err(exit(ip));
    };
    unwound_in_place(cx, ip, |vm, code| vm.binary(code, operator, left, right))
}

/// The protocol miss body of a returning constant-right `Binary` of operator `operator` (a
/// `BinaryOperator`'s value) at leaf form `LEAF` (`protocol_misses::returns`): `returning_operator_miss`
/// of that operator; `None` for a value no arithmetic operator has.
pub(in super::super) const fn returning_operator_miss_of<const LEAF: u8>(operator: u8) -> Option<Handler> {
    macro_rules! pick {
        ($op:expr) => {
            returning_operator_miss::<{ $op }, true, LEAF> as Handler
        };
    }
    arithmetic_operators!(operator as u32, pick)
}

/// The kind a returning `GetField` takes on its first run (`row_returning_field_first`),
/// which its record and site alone decide.
pub(in super::super) fn returning_field_first_kind<const LEAF: u8>(
    vm: &Vm,
    code: &ResidualProgram,
    i: &LaneInstruction,
) -> Handler {
    if i.b != quench_runtime::bytecode::FieldBase::NESTED && vm.is_private_name(i.imm()) {
        return field_return_row::<LEAF>(RETURNING_FIELD_PRIVATE);
    }
    if is_single_field_base(i.b) && !vm.reads_function_info(i.imm()) {
        by_leaf!(LEAF, |L| single_base_handler(i.b, generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(true, L)),
            generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(false, L)),
            returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD, L) as Handler))
    } else if field_site_tier_in_lane(vm, code, i) == FieldSiteTier::Nested {
        field_return_row::<LEAF>(RETURNING_FIELD_NESTED)
    } else {
        field_return_row::<LEAF>(RETURNING_FIELD)
    }
}

/// The protocol row of a returning `GetField` of `kind` (a `RETURNING_FIELD*` kind) at leaf form
/// `LEAF`: its RETURN code (`returning_code`), which `returning_miss` completes.
pub(in super::super) fn field_return_row<const LEAF: u8>(kind: u8) -> Handler {
    by_leaf!(LEAF, |L| match kind {
        RETURNING_FIELD => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD, L) as Handler,
        RETURNING_FIELD_OWN => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_OWN, L) as Handler,
        RETURNING_FIELD_POLYMORPHIC => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_POLYMORPHIC, L) as Handler,
        RETURNING_FIELD_HOLDER => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_HOLDER, L) as Handler,
        RETURNING_FIELD_INHERITED => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_INHERITED, L) as Handler,
        RETURNING_FIELD_NESTED => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_NESTED, L) as Handler,
        RETURNING_FIELD_PRIVATE => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_PRIVATE, L) as Handler,
        RETURNING_FIELD_GETTER => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_GETTER, L) as Handler,
        _ => unreachable!("a returning GetField's kind is a field kind"),
    })
}
