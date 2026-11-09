//! Calls in general dispatch code, asked before the call's exact step: a native's in-place
//! case (`general_native_call`, the lane's one table), else a user function run to
//! completion on the native stack (`general_user_call`). Both tiers that lower general dispatch, the JIT and
//! the linked image, share them: the helpers are the runtime's, read from the payload.
use super::{
    emit_js_payload_word, emit_lane_helper_call, Host, JsExactFallback,
    JsNativeStep, JsStepDispatch, ResidualProgram, Vm, ACTIVE_CONTEXT,
};
use crate::bytecode::{ConstructArguments, JsDenotation, Op, WideInstruction, RETURN_REGISTER};
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

/// `general_user_call`'s outcomes besides a miss: the value is in the call's register, or
/// the payload holds the step's transition.
const USER_CALL_COMPLETED: i64 = 0;
const USER_CALL_STOPPED: i64 = 1;
const USER_CALL_MISSED: u64 = 2;

/// Lowers the instruction at `pc` through the first shortcut that takes it: an own capture or
/// frame binding (`general_capture`), a plain call, or an index store (`general_elements`). Whether its PC
/// is native, or `None` where only the exact step applies. A call stays in a fallback
/// island's batch: native code only resumes past it.
pub(super) fn lower_shortcut(
    builder: &mut FunctionBuilder<'_>,
    (pc, instruction, next): (usize, WideInstruction, Option<ir::Block>),
    function: &crate::bytecode::Function,
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> Option<bool> {
    if lower_call(builder, (pc, instruction, next), function.strict, register_offset, (arguments, payload), exact) {
        return Some(false);
    }
    let next = next?;
    if super::general_capture::lower_capture(builder, (instruction, next), register_offset, (arguments, payload), exact)
        || super::general_capture::lower_env_local(builder, (instruction, next), function, register_offset, (arguments, payload), exact)
        || super::general_capture::lower_closure(builder, (instruction, next), function, register_offset, (arguments, payload), exact) {
        return Some(true);
    }
    if lower_construct(builder, (pc, instruction, next), register_offset, (arguments, payload), exact) {
        return Some(false);
    }
    if lower_literal(builder, (instruction, next), register_offset, (arguments, payload), exact) {
        return Some(true);
    }
    (instruction.op() == Op::SetIndex
        && super::general_elements::lower_store(builder, (instruction, next), register_offset, (arguments, payload), exact))
        .then_some(true)
}

/// Lowers the plain call at `pc` through its shortcuts, the exact step taking any miss;
/// false where only the exact step applies. A call that returns its value (`return f()`,
/// `Recipe::ReturnResult`) is that call and the native `Return` of its register; in a strict
/// function whose host makes terminal calls proper tail calls, its exact step makes the call.
fn lower_call(
    builder: &mut FunctionBuilder<'_>,
    (pc, instruction, next): (usize, WideInstruction, Option<ir::Block>),
    strict: bool,
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    let returns = instruction.returns_from_frame();
    if instruction.op() != Op::Call || instruction.result_flags() != if returns { RETURN_REGISTER } else { 0 }
        || instruction.direct_eval() || instruction.parameter_eval() {
        return false;
    }
    let Some(destination) = register_offset(instruction.result_register()) else { return false };
    let fallback = builder.create_block();
    let next = match (returns, next) {
        (false, Some(next)) => next,
        (false, None) => return false,
        (true, _) => emit_returning_continuation(builder, (arguments, payload), (destination, fallback), strict, exact),
    };
    // A user callee runs frameless first; a native's in-place case is asked only where no
    // frameless activation takes the callee.
    let framed = builder.create_block();
    if super::frameless::emit_call(builder, (pc, instruction, next), register_offset, (arguments, payload), exact,
        destination, framed).is_none() { builder.ins().jump(framed, &[]); }
    builder.switch_to_block(framed);
    if let Some(builtin) = builtin_call(instruction, register_offset) {
        emit_builtin_shortcut(builder, (arguments, payload), builtin, (destination, next));
    }
    emit_user_call(builder, (pc, next, fallback), payload, exact);
    true
}

/// A returning call's continuation, the native `Return` of `destination`; in a strict
/// function, a host with proper tail calls first sends the call to `fallback`.
fn emit_returning_continuation(builder: &mut FunctionBuilder<'_>, (arguments, payload): (ir::Value, ir::Value),
    (destination, fallback): (i32, ir::Block), strict: bool, exact: &JsExactFallback) -> ir::Block {
    if strict {
        let tail_calls = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, proper_tail_calls));
        let call = builder.create_block();
        builder.ins().brif(tail_calls, fallback, &[], call, &[]);
        builder.switch_to_block(call);
    }
    exact.register_return(builder, arguments, destination)
}

/// A native's in-place case stores its value in `destination` and continues at `next`; a
/// miss continues in a fresh current block.
fn emit_builtin_shortcut(builder: &mut FunctionBuilder<'_>, (arguments, payload): (ir::Value, ir::Value),
    builtin: BuiltinCall, (destination, next): (i32, ir::Block)) {
    let value = emit_builtin_call(builder, (arguments, payload), builtin);
    let hit = builder.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
    let (done, user) = (builder.create_block(), builder.create_block());
    builder.ins().brif(hit, done, &[], user, &[]);
    builder.switch_to_block(done);
    builder.ins().store(MemFlagsData::trusted(), value, arguments, destination);
    builder.ins().jump(next, &[]);
    builder.switch_to_block(user);
}

/// Lowers the plain `new` at `pc` frameless, else as an intrinsic array
/// (`general_array_construct`), else framed (`general_user_call`), the exact step taking any miss.
fn lower_construct(
    builder: &mut FunctionBuilder<'_>,
    (pc, instruction, next): (usize, WideInstruction, ir::Block),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    if instruction.op() != Op::Construct || instruction.result_flags() != 0 || instruction.returns_from_frame() {
        return false;
    }
    let Some(destination) = register_offset(instruction.result_register()) else { return false };
    let framed = builder.create_block();
    if super::frameless::emit_call(builder, (pc, instruction, next), register_offset, (arguments, payload), exact,
        destination, framed).is_none() { return false; }
    builder.switch_to_block(framed);
    // A frameless construction takes only plain register windows, every register framed.
    let ConstructArguments::Registers(window) = instruction.construct_arguments() else { unreachable!() };
    let offset = |register| register_offset(register).expect("the frameless construction's register");
    let callee = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, offset(instruction.register_b()));
    let window_base = if window.count == 0 { 0 } else { offset(window.base) };
    let values = builder.ins().iadd_imm_s(arguments, i64::from(window_base));
    let count = builder.ins().iconst(types::I64, i64::from(window.count));
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, array_construct));
    let array = emit_lane_helper_call(builder, payload, helper, &[payload, callee, values, count]);
    let built = builder.ins().icmp_imm_u(IntCC::NotEqual, array, Value::DELETED.to_bits() as i64);
    let (store, user, fallback) = (builder.create_block(), builder.create_block(), builder.create_block());
    builder.ins().brif(built, store, &[], user, &[]);
    builder.switch_to_block(store);
    builder.ins().store(MemFlagsData::trusted(), array, arguments, destination);
    builder.ins().jump(next, &[]);
    builder.switch_to_block(user);
    emit_user_call(builder, (pc, next, fallback), payload, exact);
    true
}

/// Lowers an object literal's allocation (`JsDenotation::ObjectAllocation`, `general_object`) or
/// field definition (`JsDenotation::FieldDefinition`, `general_define_field`), each as the lane
/// makes it, the exact step taking any miss; false where only the step applies.
fn lower_literal(
    builder: &mut FunctionBuilder<'_>,
    (instruction, next): (WideInstruction, ir::Block),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    let denotation = instruction.op().js_denotation();
    if instruction.result_flags() != 0 { return false; }
    let (answer, refused, destination) = match denotation {
        JsDenotation::ObjectAllocation => {
            let Some(destination) = register_offset(instruction.result_register()) else { return false };
            let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, object));
            (emit_lane_helper_call(builder, payload, helper, &[payload]), Value::DELETED.to_bits(), Some(destination))
        }
        JsDenotation::FieldDefinition => {
            let (Some(value), Some(object)) = (register_offset(instruction.register_a()), register_offset(instruction.register_b()))
                else { return false };
            let word = (u64::from(instruction.atom_index()) << u16::BITS) | u64::from(instruction.cache_site_index());
            let [value, object] = [value, object].map(|offset| builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, offset));
            let word = builder.ins().iconst(types::I64, word as i64);
            let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, define_field));
            (emit_lane_helper_call(builder, payload, helper, &[payload, object, word, value]), 0, None)
        }
        _ => return false,
    };
    let hit = builder.ins().icmp_imm_u(IntCC::NotEqual, answer, refused as i64);
    let (done, fallback) = (builder.create_block(), builder.create_block());
    builder.ins().brif(hit, done, &[], fallback, &[]);
    builder.switch_to_block(done);
    if let Some(destination) = destination {
        builder.ins().store(MemFlagsData::trusted(), answer, arguments, destination);
    }
    builder.ins().jump(next, &[]);
    exact.emit(builder, fallback);
    true
}

/// The framed call through `general_user_call`, its miss taking the exact step.
fn emit_user_call(builder: &mut FunctionBuilder<'_>, (pc, next, fallback): (usize, ir::Block, ir::Block),
    payload: ir::Value, exact: &JsExactFallback) {
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, user_call));
    let pc_value = builder.ins().iconst(types::I64, pc as i64);
    let status = emit_lane_helper_call(builder, payload, helper, &[payload, pc_value]);
    let completed = builder.ins().icmp_imm_u(IntCC::Equal, status, USER_CALL_COMPLETED);
    let other = builder.create_block();
    builder.ins().brif(completed, next, &[], other, &[]);
    builder.switch_to_block(other);
    let stopped = builder.ins().icmp_imm_u(IntCC::Equal, status, USER_CALL_STOPPED);
    builder.ins().brif(stopped, exact.stopped_block, &[], fallback, &[]);
    exact.emit(builder, fallback);
}

/// The registers of a call with at most one argument, which may be a native's in-place case
/// (`Vm::in_place_native_value`, which tests the callee itself): callee, receiver and the
/// argument, if any.
struct BuiltinCall {
    callee: i32,
    receiver: i32,
    argument: Option<i32>,
    count: u16,
}

fn builtin_call(instruction: WideInstruction, register_offset: &dyn Fn(u16) -> Option<i32>) -> Option<BuiltinCall> {
    let window = instruction.call_window();
    if window.count > 1 { return None; }
    Some(BuiltinCall {
        callee: register_offset(instruction.register_b())?,
        receiver: register_offset(instruction.register_c())?,
        argument: if window.count == 1 { Some(register_offset(window.base)?) } else { None },
        count: window.count,
    })
}

fn emit_builtin_call(builder: &mut FunctionBuilder<'_>, (arguments, payload): (ir::Value, ir::Value), call: BuiltinCall) -> ir::Value {
    let callee = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, call.callee);
    let receiver = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, call.receiver);
    let argument = match call.argument {
        Some(offset) => builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, offset),
        None => builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64),
    };
    let count = builder.ins().iconst(types::I64, i64::from(call.count));
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, native_call));
    emit_lane_helper_call(builder, payload, helper, &[payload, callee, receiver, argument, count])
}

/// A native's in-place case (`Vm::in_place_native_value`): its value, else `Value::DELETED`
/// before any effect.
pub(super) unsafe extern "C" fn general_native_call<H: Host>(vm: u64, residual: u64, payload: u64, callee: u64, receiver: u64,
    argument: u64, count: u64) -> u64 {
    // SAFETY: as `lane_field_add`.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &mut *(payload as *mut JsStepDispatch)) };
    let native = match vm.heap.function(Value::from_bits(callee)).map(|record| &record.kind) {
        Some(crate::heap::FunctionKind::Native(native)) => *native,
        _ => return Value::DELETED.to_bits(),
    };
    let argument = [Value::from_bits(argument)];
    let Some(args) = argument.get(..usize::try_from(count).unwrap_or(usize::MAX)) else { return Value::DELETED.to_bits() };
    // SAFETY: the running entry's program, live for the call.
    let code = unsafe { &*(residual as *const ResidualProgram) };
    let Some(value) = vm.in_place_native_value(code, native, Value::from_bits(callee), Value::from_bits(receiver), args)
        else { return Value::DELETED.to_bits() };
    // As `lane_store_miss`: a due collection waits for the next back edge's safepoint.
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}

/// The plain call or construction at `pc` of the payload's frame, as the exact step's in-loop
/// push runs it (`Vm::try_inline_user_call`, `Vm::try_inline_construct`,
/// `Vm::complete_pushed_call`) without the step around it:
/// `USER_CALL_COMPLETED` with the value in its register, `USER_CALL_STOPPED` with the
/// transition in the payload, or `USER_CALL_MISSED` before any effect.
///
/// # Safety
/// `vm`, `residual` and `payload` are the running entry's, live and unborrowed for the call.
pub(super) unsafe extern "C" fn general_user_call<H: Host>(vm: u64, residual: u64, payload: u64, pc: u64) -> u64 {
    // Rust semantics run outside the native code's fault binding, as a callout's do.
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    // SAFETY: as the caller promises.
    let status = unsafe { user_call(&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
        &mut *(payload as *mut JsStepDispatch), pc as usize) };
    ACTIVE_CONTEXT.with(|current| current.set(active));
    status
}

fn user_call<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, dispatch: &mut JsStepDispatch, pc: usize) -> u64 {
    // A frameless activation's call runs in its frame, which only its exact step makes.
    if vm.frameless.inside(dispatch) { return USER_CALL_MISSED; }
    // SAFETY: the entry's activation owns its function for the run.
    let body = unsafe { &*dispatch.body };
    let instruction = super::super::dispatch_fast::decoded_instruction(body, body.code[pc]);
    let (caller, next) = (dispatch.frame, pc + 1);
    vm.frames[caller].pc = pc;
    vm.frames[caller].pc_is_resume = false;
    if let Err(error) = vm.check_execution_interrupt() { return stop(dispatch, pc, Err(error)); }
    // Only calls and constructions take the framed path.
    let pushed = if instruction.op().is_ordinary_call() { vm.try_inline_user_call(program, caller, instruction, next) }
        else if dispatch.allow_inline_calls { vm.try_inline_construct(program, caller, instruction, next) } else { None };
    let Some(pushed) = pushed else { return USER_CALL_MISSED };
    let step = vm.complete_pushed_call(program, caller, instruction, pushed);
    if !matches!(step, Ok(super::super::StepResult::Continue)) { return stop(dispatch, pc, step); }
    let frame = &mut vm.frames[caller];
    frame.pc = next;
    frame.pc_is_resume = true;
    // Native code continues only on the storage it was entered with, in its own frame.
    if !super::js_native_frame_storage_unchanged(frame, dispatch) {
        frame.js_jit_skip_entry_once = next == 0;
        vm.active_registers = frame.registers.as_mut_ptr();
        return stop(dispatch, pc, step);
    }
    if vm.frames.len() != caller + 1 { return stop(dispatch, pc, step); }
    dispatch.refresh_heap_bases(vm, program);
    USER_CALL_COMPLETED as u64
}

fn stop(dispatch: &mut JsStepDispatch, pc: usize, result: Result<super::super::StepResult, crate::JsError>) -> u64 {
    dispatch.outcome = Some(JsNativeStep { instruction_pc: pc, next_pc: pc + 1, result });
    USER_CALL_STOPPED as u64
}
