//! The running function's own captures in general dispatch code (depth 0): a read or write
//! of an initialized slot of its dense capture slots, which the payload views
//! (`JsStepDispatch::capture_values`); a store also needs their environment to admit every
//! write directly (`capture_stores`), as the lane's store does (`lane_store_own_capture`).
//! An entry whose view is missing asks `general_capture_slots` once before the exact step.
//! A frame's own binding (`LoadEnvLocal`, `StoreEnvLocal`) reads its slot where general code
//! holds it (`EnvView`, the frame's locals), else asks `general_env_local`, the lane's
//! operations on it (`Vm::initialized_frame_binding`, `Vm::store_initialized_frame_binding`),
//! and a closure (`MakeClosure`) `general_closure`, the lane's (`lane_closure_value_of`).
use super::{emit_js_payload_word, emit_lane_helper_call, Host, JsExactFallback, JsStepDispatch, ResidualProgram, Vm};
use crate::bytecode::{Op, WideInstruction};
use crate::value::ValueLayout;
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

/// Lowers the depth-0 `LoadCapture` or `StoreCapture` at the exact fallback's PC; false
/// where only the exact step applies.
pub(super) fn lower_capture(
    builder: &mut FunctionBuilder<'_>,
    (instruction, next): (WideInstruction, ir::Block),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    let store = instruction.op() == Op::StoreCapture;
    if !store && instruction.op() != Op::LoadCapture { return false; }
    if instruction.capture_depth() != 0 || instruction.result_flags() != 0 || instruction.returns_from_frame()
        || instruction.writes_current_this() || instruction.numeric_local_target().is_some() {
        return false;
    }
    let register = if store { instruction.register_a() } else { instruction.result_register() };
    let Some(register) = register_offset(register) else { return false };
    let slot = usize::from(instruction.capture_slot());
    let fallback = builder.create_block();
    let values = emit_viewed_slots(builder, payload, slot, store, fallback);
    let offset = i32::try_from(slot * size_of::<Value>()).expect("capture slot offset fits i32");
    let old = builder.ins().load(types::I64, MemFlagsData::trusted(), values, offset);
    let tag = builder.ins().band_imm_u(old, ValueLayout::TAG_MASK as i64);
    let initialized = builder.ins().icmp_imm_u(IntCC::NotEqual, tag, ValueLayout::INTERNAL_TAG as i64);
    let done = builder.create_block();
    builder.ins().brif(initialized, done, &[], fallback, &[]);
    builder.switch_to_block(done);
    if store {
        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, register);
        builder.ins().store(MemFlagsData::trusted(), value, values, offset);
    } else {
        builder.ins().store(MemFlagsData::trusted(), old, arguments, register);
    }
    builder.ins().jump(next, &[]);
    exact.emit(builder, fallback);
    true
}

/// The payload's capture slots once they hold `slot` (`emit_view`), else `fallback`.
fn emit_viewed_slots(builder: &mut FunctionBuilder<'_>, payload: ir::Value, slot: usize, store: bool, fallback: ir::Block) -> ir::Value {
    let (viewed, retry) = (builder.create_block(), builder.create_block());
    let values = builder.append_block_param(viewed, types::I64);
    let admitted = emit_view(builder, payload, slot, store);
    let current = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_values));
    builder.ins().brif(admitted, viewed, &[ir::BlockArg::Value(current)], retry, &[]);
    // A missing view (a fresh activation's) asks for the slots once, then tests again.
    builder.switch_to_block(retry);
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, captures));
    emit_lane_helper_call(builder, payload, helper, &[payload]);
    let admitted = emit_view(builder, payload, slot, store);
    let current = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_values));
    builder.ins().brif(admitted, viewed, &[ir::BlockArg::Value(current)], fallback, &[]);
    builder.switch_to_block(viewed);
    values
}

/// Whether the payload's view holds `slot`, and admits a direct write for a `store`.
fn emit_view(builder: &mut FunctionBuilder<'_>, payload: ir::Value, slot: usize, store: bool) -> ir::Value {
    let len = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_len));
    let within = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, len, slot as i64);
    if !store { return within; }
    let stores = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_stores));
    let stores = builder.ins().icmp_imm_u(IntCC::NotEqual, stores, 0);
    builder.ins().band(within, stores)
}

/// Views the payload frame's own capture slots (`Vm::cached_direct_capture` takes them into
/// the frame's cache) and whether they admit every write directly, whose barrier this takes
/// once, as `lane_frame_capture` does.
///
/// # Safety
/// As `general_user_call`.
pub(super) unsafe extern "C" fn general_capture_slots<H: Host>(vm: u64, residual: u64, payload: u64) -> u64 {
    // SAFETY: as the caller promises; this reads the frame's environment and allocates nothing.
    let (vm, program, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
        &mut *(payload as *mut JsStepDispatch)) };
    let frame = dispatch.frame;
    // A frameless activation's slots are its admission's (`frameless::admit`).
    if vm.frameless.inside(dispatch) { return 0; }
    vm.cached_direct_capture(frame, 0, 0);
    let Some((slots, None)) = vm.current_capture_slots(frame) else { return 0 };
    // SAFETY: the frame's cache names live slots this collection.
    let Some((values, len)) = unsafe { &*slots }.native_dense_view() else { return 0 };
    (dispatch.capture_values, dispatch.capture_len) = (values, len);
    if let Some(env) = vm.capture_env(frame, 0)
        && vm.admits_every_direct_store(program, dispatch.program_id, env) {
        vm.heap.remember_cell(env);
        dispatch.capture_stores = 1;
    }
    1
}

/// `general_env_local`'s answer to a store it refuses.
const ENV_STORE_REFUSED: u64 = 0;

/// An admitted `LoadEnvLocal` or `StoreEnvLocal`: whether it stores, its value register, the
/// register a store also copies its value to, its slot and whether it initializes the binding.
struct EnvLocalSite { store: bool, register: i32, copy: Option<i32>, slot: usize, initializing: bool }

impl EnvLocalSite {
    /// The site of `instruction`, where `general_env_local` may answer it: a store of a function
    /// that resolves names at run time may write a `with` object instead.
    fn of(instruction: WideInstruction, function: &crate::bytecode::Function, register_offset: &dyn Fn(u16) -> Option<i32>)
        -> Option<Self> {
        // A store's field A is the value it reads; a load's, the register it writes.
        let store = instruction.op().field_layout(crate::bytecode::InstructionField::A) == crate::bytecode::FieldLayout::Register;
        if !instruction.op().addresses_env_local() || (store && function.has_dynamic_scope_access())
            || instruction.result_flags() != 0 || instruction.numeric_local_target().is_some() {
            return None;
        }
        let register = register_offset(if store { instruction.register_a() } else { instruction.result_register() })?;
        // Only a store has field B (`optional_register_b` asserts its layout).
        let copy = match if store { instruction.optional_register_b() } else { None } {
            Some(copy) => Some(register_offset(copy)?),
            None => None,
        };
        let initializing = store && instruction.boolean_field(crate::bytecode::InstructionField::C) == Some(true);
        Some(Self { store, register, copy, slot: instruction.local_slot(), initializing })
    }

    /// `general_env_local`'s word for the site (`EnvLocal`).
    fn word(&self) -> u64 {
        (self.slot as u64) << EnvLocal::SLOT_SHIFT
            | u64::from(self.store) << EnvLocal::STORE_BIT | u64::from(self.initializing) << EnvLocal::INITIALIZING_BIT
    }

    /// Whether the binding's own direct access may answer the site: `arguments` aliasing and
    /// the self binding keep `general_env_local`'s rules, and an initializing store of an
    /// environment-owned slot cannot tell its dead zone from a slot the environment took.
    fn direct(&self, function: &crate::bytecode::Function) -> bool {
        super::super::arguments::mapped_arguments_slot(function, self.slot).is_none()
            && !(self.store && function.is_self_binding_slot(self.slot))
            && !(self.initializing && function.local_slot_uses_environment(self.slot))
    }
}

/// The frame's own environment as general code reads its environment-owned bindings: the
/// dense slots of a captured frame's environment, which hold them for the rest of the frame's
/// life, and whether they admit every write directly, whose barrier `general_env_local` takes
/// once. Empty where the frame is not captured, its slots are not dense, or a lane body runs;
/// taken again with the other views (`JsStepDispatch::refresh_heap_bases`).
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct EnvView { values: *const Value, len: usize, stores: usize }

impl EnvView {
    pub(super) const EMPTY: Self = Self { values: std::ptr::null(), len: 0, stores: 0 };

    pub(super) fn of<H: Host>(vm: &Vm<H>, dispatch: &JsStepDispatch) -> Self {
        let Some(frame) = vm.frames.get(dispatch.frame).filter(|frame| frame.captured && dispatch.lane_context == 0)
            else { return Self::EMPTY };
        let Some(crate::heap::Cell::Environment { slots, .. }) = vm.heap.get(frame.env) else { return Self::EMPTY };
        slots.native_dense_view().map_or(Self::EMPTY, |(values, len)| Self { values, len, stores: 0 })
    }

    /// This view with `previous`'s admitted writes where it names the same slots and `kept`
    /// says its barrier still holds.
    pub(super) fn kept(self, previous: Self, kept: usize) -> Self {
        Self { stores: previous.stores & kept & usize::from(previous.values == self.values), ..self }
    }
}

/// Lowers the `LoadEnvLocal` or `StoreEnvLocal` at the exact fallback's PC: the binding's own
/// slot where general code holds it (`emit_direct`), else `general_env_local`, its miss
/// taking the exact step; false where only the step applies.
pub(super) fn lower_env_local(
    builder: &mut FunctionBuilder<'_>,
    (instruction, next): (WideInstruction, ir::Block),
    function: &crate::bytecode::Function,
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    frame_locals: (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    let Some(site) = EnvLocalSite::of(instruction, function, register_offset) else { return false };
    let asked = builder.create_block();
    if site.direct(function) {
        let layout = (function.local_slot_uses_environment(site.slot), function.selective_capture_slots.is_some());
        emit_direct(builder, &site, layout, (arguments, payload), frame_locals, (next, asked));
    } else {
        builder.ins().jump(asked, &[]);
    }
    builder.switch_to_block(asked);
    let word = builder.ins().iconst(types::I64, site.word() as i64);
    let value = if site.store { builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, site.register) }
        else { builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64) };
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, env_local));
    let answer = emit_lane_helper_call(builder, payload, helper, &[payload, word, value]);
    let refused = if site.store { ENV_STORE_REFUSED } else { Value::DELETED.to_bits() };
    let hit = builder.ins().icmp_imm_u(IntCC::NotEqual, answer, refused as i64);
    let (done, fallback) = (builder.create_block(), builder.create_block());
    builder.ins().brif(hit, done, &[], fallback, &[]);
    builder.switch_to_block(done);
    emit_completed(builder, &site, arguments, if site.store { value } else { answer }, next);
    exact.emit(builder, fallback);
    true
}

/// A load's value in its register, or a store's in the register it copies to, then `next`.
fn emit_completed(builder: &mut FunctionBuilder<'_>, site: &EnvLocalSite, arguments: ir::Value, value: ir::Value, next: ir::Block) {
    if let Some(destination) = if site.store { site.copy } else { Some(site.register) } {
        builder.ins().store(MemFlagsData::trusted(), value, arguments, destination);
    }
    builder.ins().jump(next, &[]);
}

/// The binding's direct access: an environment-owned slot in a captured frame's environment
/// (`EnvView`), else the frame's local, which a selectively captured frame leaves deleted
/// for the slots its environment took (`Vm::promote_frame_environment`); `asked` otherwise.
fn emit_direct(builder: &mut FunctionBuilder<'_>, site: &EnvLocalSite, (owned, selective): (bool, bool),
    (arguments, payload): (ir::Value, ir::Value), (locals, local_count): (ir::Value, ir::Value),
    (next, asked): (ir::Block, ir::Block)) {
    let local = builder.create_block();
    let view = |builder: &mut FunctionBuilder<'_>, field: usize|
        emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, env_view) + field);
    if owned {
        let len = view(builder, std::mem::offset_of!(EnvView, len));
        let mut within = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, len, site.slot as i64);
        if site.store {
            let stores = view(builder, std::mem::offset_of!(EnvView, stores));
            let stores = builder.ins().icmp_imm_u(IntCC::NotEqual, stores, 0);
            within = builder.ins().band(within, stores);
        }
        let held = builder.create_block();
        // Only a selectively captured frame's locals tell which binding they still hold.
        builder.ins().brif(within, held, &[], if selective { local } else { asked }, &[]);
        builder.switch_to_block(held);
        let values = view(builder, std::mem::offset_of!(EnvView, values));
        emit_slot(builder, site, (arguments, values), (next, asked));
    } else {
        builder.ins().jump(local, &[]);
    }
    builder.switch_to_block(local);
    let held = builder.create_block();
    let within = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, local_count, site.slot as i64);
    builder.ins().brif(within, held, &[], asked, &[]);
    builder.switch_to_block(held);
    emit_slot(builder, site, (arguments, locals), (next, asked));
}

/// The site's read or write of its slot in `slots`, an initialized one unless it initializes
/// the binding; `asked` where the slot holds an internal marker (a dead zone, a slot taken).
fn emit_slot(builder: &mut FunctionBuilder<'_>, site: &EnvLocalSite, (arguments, slots): (ir::Value, ir::Value),
    (next, asked): (ir::Block, ir::Block)) {
    let offset = i32::try_from(site.slot * size_of::<Value>()).expect("local slot offset fits i32");
    let old = builder.ins().load(types::I64, MemFlagsData::trusted(), slots, offset);
    if !site.initializing {
        let tag = builder.ins().band_imm_u(old, ValueLayout::TAG_MASK as i64);
        let initialized = builder.ins().icmp_imm_u(IntCC::NotEqual, tag, ValueLayout::INTERNAL_TAG as i64);
        let done = builder.create_block();
        builder.ins().brif(initialized, done, &[], asked, &[]);
        builder.switch_to_block(done);
    }
    let value = if site.store {
        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, site.register);
        builder.ins().store(MemFlagsData::trusted(), value, slots, offset);
        value
    } else { old };
    emit_completed(builder, site, arguments, value, next);
}

/// The layout of `general_env_local`'s word: the slot above two flags.
struct EnvLocal;
impl EnvLocal {
    const STORE_BIT: u32 = 0;
    const INITIALIZING_BIT: u32 = 1;
    const SLOT_SHIFT: u32 = 2;
}

/// `LoadEnvLocal` or `StoreEnvLocal` (`EnvLocal`'s word) of a binding the payload's frame
/// owns, as the lane makes them (`Vm::initialized_frame_binding`,
/// `Vm::store_initialized_frame_binding`): a load's value or `Value::DELETED`, a store's 1 or
/// `ENV_STORE_REFUSED`, before any effect where refused.
///
/// # Safety
/// As `general_capture_slots`.
pub(super) unsafe extern "C" fn general_env_local<H: Host>(vm: u64, residual: u64, payload: u64, word: u64, value: u64) -> u64 {
    // SAFETY: as the caller promises; neither operation allocates or runs guest code.
    let (vm, program, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
        &mut *(payload as *mut JsStepDispatch)) };
    let store = word >> EnvLocal::STORE_BIT & 1 != 0;
    let refused = if store { ENV_STORE_REFUSED } else { Value::DELETED.to_bits() };
    // A frameless activation's or a lane body's bindings are not the payload frame's.
    if vm.frameless.inside(dispatch) || dispatch.lane_context != 0 { return refused; }
    let slot = (word >> EnvLocal::SLOT_SHIFT) as usize;
    if store {
        let initializing = word >> EnvLocal::INITIALIZING_BIT & 1 != 0;
        let stored = vm.store_initialized_frame_binding(program, dispatch.frame, slot, Value::from_bits(value), initializing);
        admit_env_stores(vm, program, dispatch);
        u64::from(stored)
    } else {
        vm.initialized_frame_binding(program, dispatch.frame, slot).map_or(refused, Value::to_bits)
    }
}

/// Admits direct writes to the payload's environment view where its environment takes every
/// write directly, taking the barrier once, as `general_capture_slots` does for captures.
fn admit_env_stores<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, dispatch: &mut JsStepDispatch) {
    let env = vm.frames[dispatch.frame].env;
    if dispatch.env_view.len != 0 && dispatch.env_view.stores == 0
        && vm.admits_every_direct_store(program, dispatch.program_id, env) {
        vm.heap.remember_cell(env);
        dispatch.env_view.stores = 1;
    }
}

/// Lowers the `MakeClosure` at the exact fallback's PC through `general_closure`, its miss
/// taking the exact step, in a function with no binding sites, whose closures' environment is
/// the frame's own (`Vm::capture_binding_environment`); false where only the step applies.
pub(super) fn lower_closure(
    builder: &mut FunctionBuilder<'_>,
    (instruction, next): (WideInstruction, ir::Block),
    function: &crate::bytecode::Function,
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    if instruction.op().immediate_role() != crate::bytecode::ImmediateRole::ClosureFunctionIndex
        || !function.binding_sites.is_empty() || instruction.result_flags() != 0 {
        return false;
    }
    let Some(destination) = register_offset(instruction.result_register()) else { return false };
    let id = builder.ins().iconst(types::I64, i64::from(instruction.closure_function_index()));
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, closure));
    let closure = emit_lane_helper_call(builder, payload, helper, &[payload, id]);
    let made = builder.ins().icmp_imm_u(IntCC::NotEqual, closure, Value::DELETED.to_bits() as i64);
    let (done, fallback) = (builder.create_block(), builder.create_block());
    builder.ins().brif(made, done, &[], fallback, &[]);
    builder.switch_to_block(done);
    builder.ins().store(MemFlagsData::trusted(), closure, arguments, destination);
    builder.ins().jump(next, &[]);
    exact.emit(builder, fallback);
    true
}

/// The closure of function `id` in the payload's frame, as the lane makes it
/// (`lane_closure_value_of`): the frame's environment promoted once, then `Vm::closure`, which
/// runs no guest code; `Value::DELETED` before any effect inside a frameless activation or a
/// lane body, or with a collection due, which the exact step makes. Promotion moves the frame's captured
/// locals into its environment, so the dispatch takes its views again.
///
/// # Safety
/// As `general_capture_slots`.
pub(super) unsafe extern "C" fn general_closure<H: Host>(vm: u64, residual: u64, payload: u64, id: u64) -> u64 {
    // SAFETY: as the caller promises.
    let (vm, program, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
        &mut *(payload as *mut JsStepDispatch)) };
    if vm.frameless.inside(dispatch) || dispatch.lane_context != 0 || vm.heap.should_collect() {
        return Value::DELETED.to_bits();
    }
    let env = vm.promote_frame_environment(dispatch.frame);
    let closure = vm.closure(program, id as u32, env).unwrap_or(Value::DELETED);
    super::super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_heap_bases(vm, program);
    closure.to_bits()
}

/// The helpers' addresses, as `JsStepDispatch` holds them: capture slots, frame bindings and
/// closures.
pub(super) fn helpers<H: Host>() -> (usize, usize, usize) {
    (general_capture_slots::<H> as *const () as usize, general_env_local::<H> as *const () as usize,
        general_closure::<H> as *const () as usize)
}
