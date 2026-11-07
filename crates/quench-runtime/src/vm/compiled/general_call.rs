//! Calls in general dispatch code, asked before the call's exact step: an array builtin's
//! dense shortcut (`general_native_call`), else a user function run to completion on the
//! native stack (`general_user_call`). Both tiers that lower general dispatch, the JIT and
//! the linked image, share them: the helpers are the runtime's, read from the payload.
use super::{
    emit_js_payload_word, emit_lane_helper_call, FieldLookup, Host, JsExactFallback,
    JsNativeStep, JsStepDispatch, ResidualProgram, Vm, ACTIVE_CONTEXT,
};
use crate::bytecode::{FieldLayout, Function, InstructionField, Op, WideInstruction};
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

/// `general_user_call`'s outcomes besides a miss: the value is in the call's register, or
/// the payload holds the step's transition.
const USER_CALL_COMPLETED: i64 = 0;
const USER_CALL_STOPPED: i64 = 1;
const USER_CALL_MISSED: u64 = 2;

/// Lowers the plain call at `pc` through its shortcuts, the exact step taking any miss;
/// false where only the exact step applies.
pub(super) fn lower_call(
    builder: &mut FunctionBuilder<'_>,
    program: &ResidualProgram,
    function: &Function,
    (pc, instruction, next): (usize, WideInstruction, ir::Block),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    if instruction.op() != Op::Call || instruction.result_flags() != 0 || instruction.returns_from_frame()
        || instruction.direct_eval() || instruction.parameter_eval() {
        return false;
    }
    let Some(destination) = register_offset(instruction.result_register()) else { return false };
    let fallback = builder.create_block();
    if let Some(builtin) = builtin_call(program, function, pc, instruction, register_offset) {
        let value = emit_builtin_call(builder, (arguments, payload), builtin);
        let hit = builder.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
        let (done, user) = (builder.create_block(), builder.create_block());
        builder.ins().brif(hit, done, &[], user, &[]);
        builder.switch_to_block(done);
        builder.ins().store(MemFlagsData::trusted(), value, arguments, destination);
        builder.ins().jump(next, &[]);
        builder.switch_to_block(user);
    }
    let framed = builder.create_block();
    if super::frameless::emit_call(builder, (pc, instruction, next), register_offset, (arguments, payload), exact,
        destination, framed).is_none() { builder.ins().jump(framed, &[]); }
    builder.switch_to_block(framed);
    emit_user_call(builder, (pc, next, fallback), payload, exact);
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

/// The registers of a `push` call with one argument or a `pop` call with none: callee,
/// receiver and the argument, if any.
struct BuiltinCall {
    callee: i32,
    receiver: i32,
    argument: Option<i32>,
    count: u16,
}

fn builtin_call(program: &ResidualProgram, function: &Function, pc: usize, instruction: WideInstruction,
    register_offset: &dyn Fn(u16) -> Option<i32>) -> Option<BuiltinCall> {
    let window = instruction.call_window();
    if window.count > 1 { return None; }
    let read = function.code[..pc].iter().rev()
        .map(|packed| super::super::dispatch_fast::decoded_instruction(function, *packed))
        .find(|earlier| earlier.op().field_layout(InstructionField::A) == FieldLayout::ResultRegister
            && earlier.result_register() == instruction.register_b())?;
    let Some(FieldLookup::Atom { atom, .. }) = (read.op() == Op::GetField).then(|| read.field_lookup()).flatten() else { return None };
    let name = program.atoms.get(atom as usize)?;
    if !((name == "push" && window.count == 1) || (name == "pop" && window.count == 0)) { return None; }
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

/// The dense shortcut of an array builtin call: its value, else `Value::DELETED` before any
/// effect.
pub(super) unsafe extern "C" fn general_native_call<H: Host>(vm: u64, _residual: u64, payload: u64, callee: u64, receiver: u64,
    argument: u64, count: u64) -> u64 {
    // SAFETY: as `lane_field_add`.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &mut *(payload as *mut JsStepDispatch)) };
    let native = match vm.heap.function(Value::from_bits(callee)).map(|record| &record.kind) {
        Some(crate::heap::FunctionKind::Native(native)) => *native,
        _ => return Value::DELETED.to_bits(),
    };
    let receiver = Value::from_bits(receiver);
    let value = match (native, count) {
        (crate::heap::Native::ArrayPush, 1) => vm.array_push_dense(receiver, &[Value::from_bits(argument)]),
        (crate::heap::Native::ArrayPop, 0) => vm.array_pop_dense(receiver),
        _ => None,
    };
    let Some(value) = value else { return Value::DELETED.to_bits() };
    // As `lane_store_miss`: a due collection waits for the next back edge's safepoint.
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}

/// The plain call at `pc` of the payload's frame, as the exact step's in-loop call runs it
/// (`Vm::try_inline_user_call`, `Vm::complete_pushed_call`) without the step around it:
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
    #[cfg(feature = "native-execution")]
    let guarded = vm.native_guarded_call(program, dispatch.program_id, caller, instruction);
    #[cfg(not(feature = "native-execution"))]
    let guarded: Option<Value> = None;
    let pushed = if guarded.is_some() { None } else {
        Some(match vm.try_inline_user_call(program, caller, instruction, next) { Some(step) => step, None => return USER_CALL_MISSED })
    };
    let step = match (guarded, pushed) {
        (Some(value), _) => {
            vm.frames[caller].registers[usize::from(instruction.result_register())] = value;
            Ok(super::super::StepResult::Continue)
        }
        (None, step) => vm.complete_pushed_call(program, caller, instruction, step.expect("a pushed call")),
    };
    if !matches!(step, Ok(super::super::StepResult::Continue)) { return stop(dispatch, pc, step); }
    let frame = &mut vm.frames[caller];
    frame.pc = next;
    frame.pc_is_resume = true;
    // A guarded hit allocates nothing and keeps every storage address (`guarded_call_values`).
    if guarded.is_some() { return USER_CALL_COMPLETED as u64; }
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
