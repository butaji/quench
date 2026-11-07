//! Frameless calls between general dispatch bodies. A plain call of an admitted user
//! function calls the callee's native entry directly, its registers a window on this VM's
//! own native stack (`Frameless::windows`) and its activation a shadow record
//! (`ShadowRecord`), as the lane's direct calls run (`DirectCallee`). Nothing but native code
//! sees such an activation: the first exit to the VM from inside one (an exact step, an
//! inlined callee's recovery) makes the whole chain's frames through the shared recovery
//! (`materialize_chain`) and stops every native caller, and the general loop continues in
//! the innermost frame exactly where it left. A function whose activations keep leaving
//! takes the framed call again (`MATERIALIZATION_LIMIT`).
use super::{
    emit_js_payload_word, emit_lane_helper_call, native_entry_signature, Host, JsExactFallback, JsNativeStep,
    JsStepDispatch, ResidualProgram, Vm, CONTEXT_STACK_LIMIT, JS_GENERAL_RETURN_STATUS_BASE,
};
use crate::bytecode::{ControlFlowLayout, WideInstruction};
use crate::value::ValueLayout;
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

/// Values of the windows of all frameless activations one native entry may hold.
const WINDOW_VALUES: usize = 1 << 16;
const SHADOW_RECORDS: usize = 1 << 12;
const CACHE_ENTRIES: usize = 256;
/// Native stack a frameless call leaves above the entry's limit for the exact steps and
/// recovery that may follow inside it.
const STACK_MARGIN: i64 = 128 * 1024;
/// Recoveries of one function's frameless activations after which its calls keep frames.
const MATERIALIZATION_LIMIT: u32 = 16;
/// `CallEntry::this`: the receiver as passed (a strict body), or no `this` slot (an arrow's
/// is lexical). Any other value is the global object a sloppy body binds for `undefined`
/// or `null`.
const THIS_RECEIVER: u64 = 0;
const THIS_NONE: u64 = 1;

/// A frameless activation: its function, the call in its caller that made it, its
/// registers and the `this` its body reads (`JsStepDispatch::fixed_this_slot`).
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct ShadowRecord {
    function: u32,
    call_pc: u32,
    window: *const Value,
    this: Value,
}

/// An admitted callee, by its value, in the collection epoch it was admitted in: its native
/// entry, its closure's dense capture slots and whether they take direct writes, its function
/// (low 32 bits), register count and parameter count (the next 16 bits each), and
/// how it binds `this`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct CallEntry {
    callee: u64,
    epoch: u64,
    code: u64,
    capture_values: u64,
    capture_len: u64,
    capture_stores: u64,
    meta: u64,
    this: u64,
}

#[derive(Clone, Copy)]
struct FunctionUse {
    /// The body has no back edge, so no safepoint or poll stops it inside native code.
    loop_free: bool,
    materializations: u32,
}

/// The per-VM storage of frameless calls. The windows are never scanned: no collection runs
/// while one is live, since every exit from native code makes their frames first.
pub(crate) struct Frameless {
    windows: Box<[Value]>,
    shadow: Box<[ShadowRecord]>,
    cache: Box<[CallEntry]>,
    functions: rustc_hash::FxHashMap<(u32, u32), FunctionUse>,
}

impl Default for Frameless {
    fn default() -> Self {
        let empty = ShadowRecord { function: 0, call_pc: 0, window: std::ptr::null(), this: Value::UNDEFINED };
        Self {
            windows: vec![Value::UNDEFINED; WINDOW_VALUES].into_boxed_slice(),
            shadow: vec![empty; SHADOW_RECORDS].into_boxed_slice(),
            cache: vec![CallEntry::default(); CACHE_ENTRIES].into_boxed_slice(),
            functions: rustc_hash::FxHashMap::default(),
        }
    }
}

impl Frameless {
    /// Binds a general entry's payload to empty stacks: a native entry starts with no
    /// frameless activation live (`leave`).
    pub(super) fn bind(&self, dispatch: &mut JsStepDispatch) {
        let windows = self.windows.as_ptr_range();
        let shadow = self.shadow.as_ptr_range();
        (dispatch.native_top, dispatch.native_end) = (windows.start as usize, windows.end as usize);
        (dispatch.shadow_top, dispatch.shadow_end) = (shadow.start as usize, shadow.end as usize);
        dispatch.call_cache = self.cache.as_ptr() as usize;
    }

    /// Whether the payload runs inside a frameless activation.
    pub(super) fn inside(&self, dispatch: &JsStepDispatch) -> bool {
        dispatch.shadow_top != self.shadow.as_ptr() as usize
    }

    /// Forgets every admission (`Vm::clear_call_targets`).
    pub(in crate::vm) fn clear(&mut self) { self.cache.fill(CallEntry::default()); }

    fn live(&self, dispatch: &JsStepDispatch) -> &[ShadowRecord] {
        let count = (dispatch.shadow_top - self.shadow.as_ptr() as usize) / size_of::<ShadowRecord>();
        &self.shadow[..count]
    }
}

/// A frameless call of the plain `Call` at `pc` where its callee is admitted and the stacks
/// have room; else `framed`, the call's framed path.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_call(
    builder: &mut FunctionBuilder<'_>,
    (pc, instruction, next): (usize, WideInstruction, ir::Block),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
    destination: i32,
    framed: ir::Block,
) -> Option<()> {
    let window = instruction.call_window();
    let passed = (0..window.count).map(|index| register_offset(window.base + index)).collect::<Option<Vec<_>>>()?;
    let callee = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, register_offset(instruction.register_b())?);
    let receiver = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, register_offset(instruction.register_c())?);
    let entry = emit_entry(builder, payload, callee, framed);
    let meta = builder.ins().load(types::I64, MemFlagsData::trusted(), entry, std::mem::offset_of!(CallEntry, meta) as i32);
    let params = builder.ins().ushr_imm_u(meta, 48);
    let fits = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, params, i64::from(window.count));
    let registers = builder.ins().ushr_imm_u(meta, 32);
    let registers = builder.ins().band_imm_u(registers, 0xffff);
    let (top, record) = emit_room(builder, payload, exact.context, registers, fits, framed);
    emit_window(builder, (arguments, top), &passed, registers);
    let this = emit_this(builder, entry, receiver, record);
    emit_record(builder, record, (meta, pc, top));
    let status = emit_enter(builder, (payload, exact.context, exact.result_pointer), entry, (top, registers, record, this));
    let returned = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, status, i64::from(JS_GENERAL_RETURN_STATUS_BASE));
    let done = builder.create_block();
    // A callee that stopped made the chain's frames (`leave`): this caller stops with it.
    builder.ins().brif(returned, done, &[], exact.stopped_block, &[]);
    builder.switch_to_block(done);
    let value = builder.ins().load(types::I64, MemFlagsData::trusted(), exact.result_pointer, 0);
    builder.ins().store(MemFlagsData::trusted(), value, arguments, destination);
    builder.ins().jump(next, &[]);
    Some(())
}

/// The shadow record's function (the low half of `meta`), call PC and window.
fn emit_record(builder: &mut FunctionBuilder<'_>, record: ir::Value, (meta, pc, top): (ir::Value, usize, ir::Value)) {
    let function = builder.ins().ireduce(types::I32, meta);
    builder.ins().store(MemFlagsData::trusted(), function, record, std::mem::offset_of!(ShadowRecord, function) as i32);
    let call_pc = builder.ins().iconst(types::I32, pc as i64);
    builder.ins().store(MemFlagsData::trusted(), call_pc, record, std::mem::offset_of!(ShadowRecord, call_pc) as i32);
    builder.ins().store(MemFlagsData::trusted(), top, record, std::mem::offset_of!(ShadowRecord, window) as i32);
}

/// The cache entry admitting `callee` in this epoch, asking `general_frameless_admit` on a
/// miss; `framed` where it declines.
fn emit_entry(builder: &mut FunctionBuilder<'_>, payload: ir::Value, callee: ir::Value, framed: ir::Block) -> ir::Value {
    let cache = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, call_cache));
    let slot = builder.ins().band_imm_u(callee, (CACHE_ENTRIES - 1) as i64);
    let offset = builder.ins().imul_imm_u(slot, size_of::<CallEntry>() as i64);
    let cached = builder.ins().iadd(cache, offset);
    let named = builder.ins().load(types::I64, MemFlagsData::trusted(), cached, std::mem::offset_of!(CallEntry, callee) as i32);
    let epoch = builder.ins().load(types::I64, MemFlagsData::trusted(), cached, std::mem::offset_of!(CallEntry, epoch) as i32);
    let current = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, epoch));
    let same = builder.ins().icmp(IntCC::Equal, named, callee);
    let fresh = builder.ins().icmp(IntCC::Equal, epoch, current);
    let hit = builder.ins().band(same, fresh);
    let (found, admit) = (builder.create_block(), builder.create_block());
    let entry = builder.append_block_param(found, types::I64);
    builder.ins().brif(hit, found, &[ir::BlockArg::Value(cached)], admit, &[]);
    builder.switch_to_block(admit);
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, frameless));
    let admitted = emit_lane_helper_call(builder, payload, helper, &[payload, callee]);
    let some = builder.ins().icmp_imm_u(IntCC::NotEqual, admitted, 0);
    builder.ins().brif(some, found, &[ir::BlockArg::Value(admitted)], framed, &[]);
    builder.switch_to_block(found);
    entry
}

/// The window and shadow record of the activation, where the call fits its parameters
/// (`fits`), the stacks and the native stack have room; else `framed`.
fn emit_room(builder: &mut FunctionBuilder<'_>, payload: ir::Value, context: ir::Value, registers: ir::Value,
    fits: ir::Value, framed: ir::Block) -> (ir::Value, ir::Value) {
    let top = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, native_top));
    let end = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, native_end));
    let bytes = builder.ins().imul_imm_u(registers, size_of::<Value>() as i64);
    let next = builder.ins().iadd(top, bytes);
    let windows = builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, next, end);
    let record = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, shadow_top));
    let last = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, shadow_end));
    let pushed = builder.ins().iadd_imm_u(record, size_of::<ShadowRecord>() as i64);
    let records = builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, pushed, last);
    let stack = builder.ins().get_stack_pointer(types::I64);
    let limit = builder.ins().load(types::I64, MemFlagsData::trusted(), context, CONTEXT_STACK_LIMIT);
    let limit = builder.ins().iadd_imm_u(limit, STACK_MARGIN);
    let native = builder.ins().icmp(IntCC::UnsignedGreaterThan, stack, limit);
    let room = builder.ins().band(fits, windows);
    let room = builder.ins().band(room, records);
    let room = builder.ins().band(room, native);
    let enter = builder.create_block();
    builder.ins().brif(room, enter, &[], framed, &[]);
    builder.switch_to_block(enter);
    (top, record)
}

/// The window's registers: the passed values, then `undefined` up to the register count, as
/// a plain activation's (`Frame::place_plain_parameters`).
fn emit_window(builder: &mut FunctionBuilder<'_>, (arguments, top): (ir::Value, ir::Value), passed: &[i32], registers: ir::Value) {
    for (index, offset) in passed.iter().enumerate() {
        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, *offset);
        builder.ins().store(MemFlagsData::trusted(), value, top, (index * size_of::<Value>()) as i32);
    }
    let (header, body, filled) = (builder.create_block(), builder.create_block(), builder.create_block());
    let index = builder.append_block_param(header, types::I64);
    let start = builder.ins().iconst(types::I64, passed.len() as i64);
    builder.ins().jump(header, &[ir::BlockArg::Value(start)]);
    builder.switch_to_block(header);
    let more = builder.ins().icmp(IntCC::UnsignedLessThan, index, registers);
    builder.ins().brif(more, body, &[], filled, &[]);
    builder.switch_to_block(body);
    let offset = builder.ins().imul_imm_u(index, size_of::<Value>() as i64);
    let address = builder.ins().iadd(top, offset);
    let undefined = builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
    builder.ins().store(MemFlagsData::trusted(), undefined, address, 0);
    let following = builder.ins().iadd_imm_u(index, 1);
    builder.ins().jump(header, &[ir::BlockArg::Value(following)]);
    builder.switch_to_block(filled);
}

/// The record's `this` slot address for `fixed_this_slot`, or null where the body binds
/// no receiver it was passed (`CallEntry::this`): a strict body's receiver as passed, a
/// sloppy body's direct object, or its global object for `undefined` or `null`.
fn emit_this(builder: &mut FunctionBuilder<'_>, entry: ir::Value, receiver: ir::Value, record: ir::Value) -> ir::Value {
    let mode = builder.ins().load(types::I64, MemFlagsData::trusted(), entry, std::mem::offset_of!(CallEntry, this) as i32);
    let family = builder.ins().band_imm_u(receiver, ValueLayout::HEAP_FAMILY_MASK as i64);
    let object = builder.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_OBJECT_TAG as i64);
    let undefined = builder.ins().icmp_imm_u(IntCC::Equal, receiver, Value::UNDEFINED.to_bits() as i64);
    let null = builder.ins().icmp_imm_u(IntCC::Equal, receiver, Value::NULL.to_bits() as i64);
    let absent = builder.ins().bor(undefined, null);
    let strict = builder.ins().icmp_imm_u(IntCC::Equal, mode, THIS_RECEIVER as i64);
    let lexical = builder.ins().icmp_imm_u(IntCC::Equal, mode, THIS_NONE as i64);
    // A sloppy body: its global object for an absent receiver, else the receiver.
    let sloppy = builder.ins().select(absent, mode, receiver);
    let value = builder.ins().select(strict, receiver, sloppy);
    let bound = builder.ins().bor(object, absent);
    let bound = builder.ins().bor(bound, strict);
    let none = builder.ins().bnot(lexical);
    let bound = builder.ins().band(bound, none);
    builder.ins().store(MemFlagsData::trusted(), value, record, std::mem::offset_of!(ShadowRecord, this) as i32);
    let slot = builder.ins().iadd_imm_u(record, std::mem::offset_of!(ShadowRecord, this) as i64);
    let zero = builder.ins().iconst(types::I64, 0);
    builder.ins().select(bound, slot, zero)
}

/// Pushes the activation onto the payload, calls the callee's entry at its first PC and pops
/// it again: its status.
fn emit_enter(builder: &mut FunctionBuilder<'_>, (payload, context, result): (ir::Value, ir::Value, ir::Value),
    entry: ir::Value, (top, registers, record, this): (ir::Value, ir::Value, ir::Value, ir::Value)) -> ir::Value {
    let offset = |field: usize| field as i32;
    let fields = [
        std::mem::offset_of!(JsStepDispatch, capture_values),
        std::mem::offset_of!(JsStepDispatch, capture_len),
        std::mem::offset_of!(JsStepDispatch, capture_stores),
        std::mem::offset_of!(JsStepDispatch, fixed_this_slot),
        std::mem::offset_of!(JsStepDispatch, native_top),
        std::mem::offset_of!(JsStepDispatch, shadow_top),
    ];
    let saved: Vec<ir::Value> = fields.iter()
        .map(|field| builder.ins().load(types::I64, MemFlagsData::trusted(), payload, offset(*field))).collect();
    let captures = [std::mem::offset_of!(CallEntry, capture_values), std::mem::offset_of!(CallEntry, capture_len),
        std::mem::offset_of!(CallEntry, capture_stores)]
        .map(|field| builder.ins().load(types::I64, MemFlagsData::trusted(), entry, offset(field)));
    let bytes = builder.ins().imul_imm_u(registers, size_of::<Value>() as i64);
    let next = builder.ins().iadd(top, bytes);
    let pushed = builder.ins().iadd_imm_u(record, size_of::<ShadowRecord>() as i64);
    for (field, value) in fields.iter().zip(captures.into_iter().chain([this, next, pushed])) {
        builder.ins().store(MemFlagsData::trusted(), value, payload, offset(*field));
    }
    let zero = builder.ins().iconst(types::I64, 0);
    builder.ins().store(MemFlagsData::trusted(), zero, payload, offset(std::mem::offset_of!(JsStepDispatch, start_pc)));
    let code = builder.ins().load(types::I64, MemFlagsData::trusted(), entry, offset(std::mem::offset_of!(CallEntry, code)));
    let signature = native_entry_signature(types::I64, builder.func.signature.call_conv);
    let signature = builder.import_signature(signature);
    let call = builder.ins().call_indirect(signature, code, &[context, zero, zero, top, result]);
    let status = builder.inst_results(call)[0];
    for (field, value) in fields.iter().zip(saved) {
        builder.ins().store(MemFlagsData::trusted(), value, payload, offset(*field));
    }
    status
}

/// Admits `callee` for frameless calls from the payload's program and caches it: its
/// entry's address, or 0 where its calls keep frames. Admitted are plain user functions of
/// the running program (`CallPlan::in_loop`) whose whole state is their registers
/// (`CallPlan::registers_only`), whose body has no loop and a linked native entry, and whose
/// activations have not kept leaving.
///
/// # Safety
/// As `general_user_call`.
pub(super) unsafe extern "C" fn general_frameless_admit<H: Host>(vm: u64, residual: u64, payload: u64, callee: u64) -> u64 {
    // SAFETY: as the caller promises; admission reads the heap and allocates nothing.
    let (vm, program, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
        &*(payload as *const JsStepDispatch)) };
    admit(vm, program, dispatch, Value::from_bits(callee)).map_or(0, |entry| entry as u64)
}

fn admit<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, dispatch: &JsStepDispatch, callee: Value) -> Option<*const CallEntry> {
    let (owner, function, env) = vm.in_loop_call_target(program, callee)?;
    if owner != dispatch.program_id { return None; }
    let body = program.function_get(function as usize)?;
    let plan = body.call_plan(program);
    if !plan.in_loop || !plan.registers_only || body.rest { return None; }
    let usage = *vm.frameless.functions.entry((owner.raw(), function)).or_insert_with(|| FunctionUse {
        loop_free: body.instructions().enumerate().all(|(pc, instruction)|
            !matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
                || instruction.jump_target() as usize > pc),
        materializations: 0,
    });
    if !usage.loop_free || usage.materializations >= MATERIALIZATION_LIMIT { return None; }
    let code = native_entry(vm, owner, function)?;
    let capture = vm.target_capture(program, owner, env);
    // SAFETY: a live environment's dense slots stay in place through this collection epoch.
    let (values, len) = (!capture.is_null()).then(|| unsafe {
        crate::heap::EnvironmentSlots::native_storage_view(super::super::dispatch_fast::capture_reads(capture)) }).flatten()
        .unwrap_or((std::ptr::null(), 0));
    let this = if body.is_arrow { THIS_NONE } else if body.strict { THIS_RECEIVER } else { vm.realm.globals.to_bits() };
    let entry = CallEntry {
        callee: callee.to_bits(),
        epoch: vm.heap.collection_epoch(),
        code: code as u64,
        capture_values: values as u64,
        capture_len: len as u64,
        capture_stores: u64::from(super::super::dispatch_fast::capture_stores(capture).is_some()),
        meta: u64::from(function) | (u64::from(body.registers) << 32) | (u64::from(body.params) << 48),
        this,
    };
    let slot = (callee.to_bits() as usize) & (CACHE_ENTRIES - 1);
    vm.frameless.cache[slot] = entry;
    Some(&vm.frameless.cache[slot])
}

/// The chain of the payload's frameless activations, outermost first, each `(function, call
/// PC, PC, registers)`, followed by `inlined`: each continues at its callee's call, the
/// innermost at `pc`. Counts the leave against the innermost frameless function
/// (`MATERIALIZATION_LIMIT`).
/// The native entry of `function` in `owner`'s linked code, which only native builds have.
#[cfg(feature = "native-execution")]
fn native_entry<H: Host>(vm: &Vm<H>, owner: crate::vm::ProgramId, function: u32) -> Option<usize> {
    vm.programs.native_js_code(owner)?.entries().get(function as usize).map(|code| *code as usize)
}

#[cfg(not(feature = "native-execution"))]
fn native_entry<H: Host>(_: &Vm<H>, _: crate::vm::ProgramId, _: u32) -> Option<usize> {
    None
}

pub(super) fn chain<H: Host>(vm: &mut Vm<H>, dispatch: &JsStepDispatch, pc: u32,
    inlined: impl Iterator<Item = (u32, u32, u32, *const Value)>) -> Vec<(u32, u32, u32, *const Value)> {
    let live = vm.frameless.live(dispatch);
    let continues = live.iter().skip(1).map(|record| record.call_pc).chain([pc]);
    let records: Vec<_> = live.iter().zip(continues)
        .map(|(record, at)| (record.function, record.call_pc, at, record.window)).collect();
    if let Some(&(function, ..)) = records.last()
        && let Some(usage) = vm.frameless.functions.get_mut(&(dispatch.program_id.raw(), function)) {
        usage.materializations += 1;
        if usage.materializations == MATERIALIZATION_LIMIT {
            // Calls of it keep frames from now on: no cached admission names it.
            vm.frameless.clear();
        }
    }
    records.into_iter().chain(inlined).collect()
}

/// An exact step inside a frameless activation runs in its frame: the step that made the
/// chain's frames, which the general loop continues from (`StepResult::ResumeFrames`).
pub(super) fn leave<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, dispatch: &mut JsStepDispatch, pc: usize) -> Option<JsNativeStep> {
    if !vm.frameless.inside(dispatch) { return None; }
    let chain = chain(vm, dispatch, pc as u32, std::iter::empty());
    super::materialize_chain(vm, program, dispatch, &chain);
    dispatch.outcome.take()
}
