//! The fast lane runs straight-line instructions whose whole effect is a
//! register write, a Wasm linear-memory access or a jump. No frame state is
//! published. The lane stops before any instruction that would collect, trap,
//! throw, call or touch a shared memory, and the general path executes that
//! instruction with its full semantics, so lane execution is always an exact
//! prefix of general execution.
//!
//! The lane runs a function's *lane view*: one [`LaneInstruction`] per
//! bytecode instruction, at the same index, derived once from the bytecode.
//! A record names its opcode's handler and carries the instruction's decoded
//! fields, so handlers neither decode packed fields nor look up a table.
//! Each handler ends by dispatching the next record's handler itself
//! (threaded dispatch), so each opcode predicts its successor at its own
//! branch. Where the build forms sibling calls (`quench_lane_tail_calls`), that
//! dispatch is a jump and a run of handlers shares one stack frame; otherwise
//! every handler returns to the lane loop. Taken safepoint and backward jumps
//! always return to the lane loop, which bounds stack use by the straight-line
//! run length even if a sibling call is not formed.

use super::*;
use crate::bytecode::{
    Function, ImmediateLayout, ImmediateRole, InstructionField, WideInstruction,
};
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator};
use crate::wasm::memory::{MemoryLoad, MemoryStorage, MemoryStore};

/// One instruction of a lane view.
#[repr(C)]
pub(super) struct LaneInstruction {
    /// The opcode's [`Handler`] for the VM that built the view.
    handler: *const (),
    imm: u32,
    /// Byte distance from this record to its jump target, for instructions
    /// whose immediate is a jump target.
    jump: i32,
    a: u16,
    b: u16,
    c: u16,
}

type Ip = *const LaneInstruction;

/// The active frame's registers. Lane instructions never resize registers or
/// change the frame stack, so the base stays valid for the whole lane run.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Registers(*mut Value);

impl Registers {
    #[inline(always)]
    fn get(self, register: u16) -> Value {
        // SAFETY: the view admits only register fields below
        // Function.registers, and frame setup sizes the register file to it.
        unsafe { *self.0.add(usize::from(register)) }
    }

    #[inline(always)]
    fn set(self, register: u16, value: Value) {
        // SAFETY: as for `get`; no reference into the register file is live.
        unsafe { *self.0.add(usize::from(register)) = value }
    }

    #[inline(always)]
    fn i32(self, register: u16) -> i32 {
        self.get(register).wasm_bits32() as i32
    }
}

/// The unshared memory bytes behind the last memory binding the lane
/// resolved. The binding stays in a register for the whole run, and the lane
/// never collects, calls out or grows memory, so the bytes it names stay
/// alive, unborrowed and unresized at a stable address.
#[derive(Clone, Copy)]
struct MemoryView {
    binding: Value,
    bytes: *mut u8,
    len: usize,
}

impl MemoryView {
    const NONE: Self = Self {
        binding: Value::UNDEFINED,
        bytes: std::ptr::null_mut(),
        len: 0,
    };

    /// SAFETY: the view must have been resolved during the current lane run.
    #[inline(always)]
    unsafe fn bytes<'a>(self) -> &'a mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.bytes, self.len) }
    }
}

/// Lane state that handlers read rarely; the hot state travels in handler
/// arguments so it stays in machine registers across dispatches.
struct LaneContext<H: Host> {
    vm: *mut Vm<H>,
    /// The executing program, which every inline call stays within.
    code: *const ResidualProgram,
    program: ProgramId,
    /// The active frame, its registers and the view of its function.
    frame: usize,
    registers: Registers,
    base: Ip,
    /// The general loop's pending inline calls, which lane calls extend and
    /// lane returns complete exactly as the general loop would.
    pending: *mut Vec<PendingGeneralCall>,
    inline_calls: bool,
    /// The view handlers start from when the lane loop dispatches.
    memory: MemoryView,
}

/// A lane handler returns the next record for the lane loop, tagged with
/// [`EXIT_TAG`] when the general path must execute it. The memory view is
/// passed as scalars so every argument stays in a register.
type Handler<H> = fn(*mut LaneContext<H>, Registers, Ip, *mut u8, usize, Value) -> Ip;

/// Records are word aligned, so the low address bit is free to mark a lane
/// exit.
const EXIT_TAG: usize = 1;
const _: () = assert!(std::mem::align_of::<LaneInstruction>() > EXIT_TAG);

/// The general path executes the instruction at `ip`.
#[inline(always)]
fn exit(ip: Ip) -> Ip {
    ip.map_addr(|address| address | EXIT_TAG)
}

/// SAFETY: the context outlives the lane run and no other reference to the
/// VM is live while a handler runs.
#[inline(always)]
unsafe fn vm<'a, H: Host>(cx: *mut LaneContext<H>) -> &'a mut Vm<H> {
    unsafe { &mut *(*cx).vm }
}

/// The handler of the record at `ip`.
#[inline(always)]
fn handler<H: Host>(ip: Ip) -> Handler<H> {
    // SAFETY: a view is only run by the VM type whose handlers built it, and
    // every reachable record is initialized.
    unsafe { std::mem::transmute::<*const (), Handler<H>>((*ip).handler) }
}

/// Continue with the handler of the record at `ip`.
#[cfg(quench_lane_tail_calls)]
macro_rules! next {
    ($cx:expr, $r:expr, $ip:expr, $view:expr) => {{
        let ip: Ip = $ip;
        let view: MemoryView = $view;
        return handler::<H>(ip)($cx, $r, ip, view.bytes, view.len, view.binding);
    }};
}

#[cfg(not(quench_lane_tail_calls))]
macro_rules! next {
    ($cx:expr, $r:expr, $ip:expr, $view:expr) => {{
        let _ = ($r, $view);
        return $ip;
    }};
}

/// Define a handler with the shared signature. `$i` is the record and
/// `$view` the current memory view.
macro_rules! lane_handler {
    ($(#[$meta:meta])* fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        $(#[$meta])*
        #[allow(unused_variables)]
        fn $name<H: Host, $(const $param: $ty),*>(
            $cx: *mut LaneContext<H>,
            $r: Registers,
            $ip: Ip,
            bytes: *mut u8,
            len: usize,
            binding: Value,
        ) -> Ip {
            let $view = MemoryView { binding, bytes, len };
            // SAFETY: the lane only dispatches records of the running view.
            let $i = unsafe { &*$ip };
            $body
        }
    };
}

/// The record after `ip`.
#[inline(always)]
fn following(ip: Ip) -> Ip {
    // SAFETY: validation ends every function in a terminator, so a lane
    // instruction that falls through has a successor.
    unsafe { ip.add(1) }
}

/// The opcode a handler instantiation serves.
const fn opcode(index: u16) -> Op {
    match Op::from_index(index as usize) {
        Some(op) => op,
        None => panic!("lane handler for an opcode outside the opcode table"),
    }
}

lane_handler! {
    /// Not a lane instruction: the general path executes it.
    fn lane_exit<>(cx, r, ip, i, view) {
        exit(ip)
    }
}

lane_handler! {
    fn lane_move<>(cx, r, ip, i, view) {
        r.set(i.a, r.get(i.b));
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_load_const<>(cx, r, ip, i, view) {
        let program = unsafe { (*cx).program };
        let Some(value) = unsafe { vm(cx) }.programs.constant(program, i.imm as usize) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_load_local_plain<>(cx, r, ip, i, view) {
        // SAFETY: validated bytecode bounds the slot by Function.locals, and
        // frame setup sizes locals to that count.
        let value = unsafe { vm(cx).read_validated_local((*cx).frame, i.imm as usize) };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

/// A taken jump. Safepoint jumps (every `Jump`, and backward Wasm
/// conditionals) leave the lane when a collection is due and otherwise
/// return to the lane loop; forward Wasm conditionals dispatch directly.
#[inline(always)]
fn lane_jump<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    safepoint: bool,
    view: MemoryView,
) -> Ip {
    // SAFETY: the view records the distance to a validated jump target.
    let jump = unsafe { (*ip).jump };
    let target = unsafe { ip.byte_offset(jump as isize) };
    if safepoint || jump <= 0 {
        if unsafe { vm(cx) }.heap.should_collect() {
            return exit(ip);
        }
        return target;
    }
    next!(cx, r, target, view)
}

lane_handler! {
    fn lane_jump_always<>(cx, r, ip, i, view) {
        lane_jump(cx, r, ip, true, view)
    }
}

lane_handler! {
    fn lane_jump_false<>(cx, r, ip, i, view) {
        let value = r.get(i.a);
        let truthy = match value.as_int() {
            Some(integer) => integer != 0,
            None if value == Value::TRUE => true,
            None if value == Value::FALSE => false,
            None => return exit(ip),
        };
        if !truthy {
            return lane_jump(cx, r, ip, true, view);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// `WasmJumpI32Zero` and `WasmJumpI32NonZero`.
    fn lane_jump_i32_zero<WHEN_ZERO: bool>(cx, r, ip, i, view) {
        if (r.i32(i.a) == 0) == WHEN_ZERO {
            return lane_jump(cx, r, ip, false, view);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// Fused i32 comparison and branch, against a register or a constant.
    fn lane_compare_jump<OP: u16>(cx, r, ip, i, view) {
        let (comparison, immediate) = const {
            match I32BinaryOperator::from_jump_op(opcode(OP)) {
                Some(comparison) => (comparison, false),
                None => match I32BinaryOperator::from_immediate_jump_op(opcode(OP)) {
                    Some(comparison) => (comparison, true),
                    None => panic!("not an i32 comparison jump"),
                },
            }
        };
        let right = if immediate {
            i32::from(i.b as i16)
        } else {
            r.i32(i.b)
        };
        let Ok(taken) = comparison.evaluate(r.i32(i.a), right) else {
            return exit(ip);
        };
        if taken != 0 {
            return lane_jump(cx, r, ip, false, view);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_instance_binding<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let env = vm.frames[unsafe { (*cx).frame }].env;
        let Some(value) = vm.heap.environment_slot(env, i.imm as usize) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_global_get<>(cx, r, ip, i, view) {
        let Some(Cell::WasmGlobal { value, .. }) = unsafe { vm(cx) }.heap.get(r.get(i.b)) else {
            return exit(ip);
        };
        r.set(i.a, *value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_global_set<>(cx, r, ip, i, view) {
        let Some(Cell::WasmGlobal {
            value,
            mutable: true,
            ..
        }) = unsafe { vm(cx) }.heap.get_mut(r.get(i.b))
        else {
            return exit(ip);
        };
        *value = r.get(i.a);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_select<>(cx, r, ip, i, view) {
        let (condition, _) = ImmediateLayout::register_pair(i.imm);
        let source = if r.i32(condition) != 0 { i.b } else { i.c };
        r.set(i.a, r.get(source));
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_fill_registers<>(cx, r, ip, i, view) {
        let program = unsafe { (*cx).program };
        let Some(value) = unsafe { vm(cx) }.programs.constant(program, i.imm as usize) else {
            return exit(ip);
        };
        for offset in 0..i.c {
            r.set(i.b + offset, value);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_i32_unary<>(cx, r, ip, i, view) {
        let Some(operator) = I32UnaryOperator::from_tag(i.imm) else {
            return exit(ip);
        };
        let crate::WasmValue::I32(value) = operator.apply(r.i32(i.b)) else {
            return exit(ip);
        };
        r.set(i.a, Value::integer(value));
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// An i32 operator over two registers or a register and a constant; a
    /// trap leaves the lane.
    fn lane_i32_binary<OP: u16>(cx, r, ip, i, view) {
        let (operator, immediate) = const {
            match I32BinaryOperator::from_register_op(opcode(OP)) {
                Some(operator) => (operator, false),
                None => match I32BinaryOperator::from_immediate_op(opcode(OP)) {
                    Some(operator) => (operator, true),
                    None => panic!("not an i32 binary operator"),
                },
            }
        };
        let right = if immediate { i.imm as i32 } else { r.i32(i.c) };
        let Ok(value) = operator.evaluate(r.i32(i.b), right) else {
            return exit(ip);
        };
        r.set(i.a, Value::integer(value));
        next!(cx, r, following(ip), view)
    }
}

/// The memory32 effective address of a direct access, or `None` when the
/// view does not hold the accessed binding.
#[inline(always)]
fn lane_address(r: Registers, view: MemoryView, i: &LaneInstruction) -> Option<u64> {
    (r.get(i.b) == view.binding).then(|| u64::from(r.get(i.c).wasm_bits32()) + u64::from(i.imm))
}

/// Resolve the view for the binding the record at `ip` accesses, then resume
/// that record from the lane loop; a shared or borrowed memory leaves the
/// lane.
#[cold]
#[inline(never)]
fn lane_resolve_memory<H: Host>(cx: *mut LaneContext<H>, binding: Value, ip: Ip) -> Ip {
    let Some(Cell::WasmMemory { bytes, .. }) = unsafe { vm(cx) }.heap.get(binding) else {
        return exit(ip);
    };
    let MemoryStorage::Unshared(state) = &**bytes else {
        return exit(ip);
    };
    // An outstanding borrow belongs to a suspended host access; the general
    // path reports it exactly.
    let Ok(mut state) = state.try_borrow_mut() else {
        return exit(ip);
    };
    unsafe {
        (*cx).memory = MemoryView {
            binding,
            bytes: state.bytes.as_mut_ptr(),
            len: state.bytes.len(),
        };
    }
    ip
}

/// Boxing a 64-bit result allocates; kept out of line with scalar arguments
/// so the hot handlers keep their registers and sibling calls.
#[cold]
#[inline(never)]
fn lane_box_bits64<H: Host>(cx: *mut LaneContext<H>, bits: u64) -> Value {
    unsafe { vm(cx) }.heap.alloc(Cell::WasmBits64(bits))
}

#[cold]
#[inline(never)]
fn lane_unbox_bits64<H: Host>(
    cx: *mut LaneContext<H>,
    value: Value,
    ty: crate::WasmType,
) -> Option<u64> {
    match unsafe { vm(cx) }
        .decode_wasm_value(value, ty)
        .ok()?
        .bits()?
    {
        crate::wasm::ScalarBits::Bits64(bits) => Some(bits),
        _ => None,
    }
}

lane_handler! {
    fn lane_load<OP: u16>(cx, r, ip, i, view) {
        let load = const {
            match MemoryLoad::from_direct_op(opcode(OP)) {
                Some(load) => load,
                None => panic!("not a direct load"),
            }
        };
        let Some(address) = lane_address(r, view, i) else {
            return lane_resolve_memory(cx, r.get(i.b), ip);
        };
        // SAFETY: the view holds the live binding just compared; see
        // MemoryView for why its bytes stay valid for the lane run.
        let Ok(value) = load.read(unsafe { view.bytes() }, address) else {
            return exit(ip);
        };
        let value = match value {
            crate::WasmValue::I32(value) => Value::integer(value),
            crate::WasmValue::F32(bits) => Value::integer(bits as i32),
            crate::WasmValue::I64(value) => lane_box_bits64(cx, value as u64),
            crate::WasmValue::F64(bits) => lane_box_bits64(cx, bits),
            _ => return exit(ip),
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_store<OP: u16>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        let value = match store.value_type() {
            crate::WasmType::I32 => crate::WasmValue::I32(r.i32(i.a)),
            crate::WasmType::F32 => match r.get(i.a).as_int() {
                Some(bits) => crate::WasmValue::F32(bits as u32),
                None => return exit(ip),
            },
            ty @ crate::WasmType::I64 => match lane_unbox_bits64(cx, r.get(i.a), ty) {
                Some(bits) => crate::WasmValue::I64(bits as i64),
                None => return exit(ip),
            },
            ty @ crate::WasmType::F64 => match lane_unbox_bits64(cx, r.get(i.a), ty) {
                Some(bits) => crate::WasmValue::F64(bits),
                None => return exit(ip),
            },
            _ => return exit(ip),
        };
        let Some(address) = lane_address(r, view, i) else {
            return lane_resolve_memory(cx, r.get(i.b), ip);
        };
        // SAFETY: as for `lane_load`.
        if store.write(unsafe { view.bytes() }, address, value).is_err() {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

/// Continue in `frame` at `pc`: the record the frame's function resumes at.
#[inline(always)]
fn lane_enter_frame<H: Host>(
    cx: *mut LaneContext<H>,
    frame: usize,
    pc: usize,
    view: MemoryView,
) -> Ip {
    let vm = unsafe { vm(cx) };
    let function = vm.frames[frame].function;
    let program = unsafe { (*cx).program };
    let Some(base) = vm.programs.lane_view(program, function, lane_view::<H>) else {
        unreachable!("an inline Wasm frame runs a function of its program");
    };
    let r = Registers(vm.frames[frame].registers.as_mut_ptr());
    // SAFETY: the view has one record per instruction of the function, and
    // `pc` is the frame's validated resume point.
    let ip = unsafe { base.add(pc) };
    unsafe {
        (*cx).frame = frame;
        (*cx).registers = r;
        (*cx).base = base;
    }
    next!(cx, r, ip, view)
}

lane_handler! {
    /// A Wasm call to a function of the same program, pushed as the general
    /// loop's inline call would be.
    fn lane_call_known<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).inline_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        let caller = unsafe { (*cx).frame };
        // SAFETY: `ip` is a record of the active view.
        let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
        let parent = vm.capture_env(caller, 0).unwrap_or(vm.frames[caller].env);
        let window = crate::bytecode::RegisterWindow {
            base: ImmediateLayout::call_window_base(i.imm),
            count: ImmediateLayout::argument_count(i.imm),
        };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Ok(stack_guard) = vm.push_wasm_frame(code, caller, u32::from(i.b), parent, window) else {
            return exit(ip);
        };
        vm.frames[caller].pc = call_pc + 1;
        unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
            caller,
            call_pc: call_pc as u32,
            destination: i.a,
            stack_guard,
        });
        lane_enter_frame(cx, caller + 1, 0, view)
    }
}

lane_handler! {
    /// Return from an inline call to its caller; returning from the loop's
    /// entry frame belongs to the general loop.
    fn lane_return<>(cx, r, ip, i, view) {
        let pending = unsafe { &mut *(*cx).pending };
        if !unsafe { (*cx).inline_calls } || pending.is_empty() {
            return exit(ip);
        }
        let value = r.get(i.a);
        let Some(call) = pending.pop() else {
            return exit(ip);
        };
        let vm = unsafe { vm(cx) };
        // SAFETY: the context's program outlives the lane run.
        vm.retire_pending_frame(unsafe { &*(*cx).code });
        vm.write(call.caller, call.destination, value);
        drop(call.stack_guard);
        let pc = vm.frames[call.caller].pc;
        lane_enter_frame(cx, call.caller, pc, view)
    }
}

/// The handler serving opcode `OP`, derived from the opcode's semantic
/// family.
const fn handler_for<H: Host, const OP: u16>() -> Handler<H> {
    let Some(op) = Op::from_index(OP as usize) else {
        return lane_exit::<H>;
    };
    if I32BinaryOperator::from_register_op(op).is_some()
        || I32BinaryOperator::from_immediate_op(op).is_some()
    {
        return lane_i32_binary::<H, OP>;
    }
    if I32BinaryOperator::from_jump_op(op).is_some()
        || I32BinaryOperator::from_immediate_jump_op(op).is_some()
    {
        return lane_compare_jump::<H, OP>;
    }
    if MemoryLoad::from_direct_op(op).is_some() {
        return lane_load::<H, OP>;
    }
    if MemoryStore::from_direct_op(op).is_some() {
        return lane_store::<H, OP>;
    }
    match op {
        Op::Move => lane_move::<H>,
        Op::LoadConst => lane_load_const::<H>,
        Op::LoadLocalPlain => lane_load_local_plain::<H>,
        Op::Jump => lane_jump_always::<H>,
        Op::JumpFalse => lane_jump_false::<H>,
        Op::WasmJumpI32Zero => lane_jump_i32_zero::<H, true>,
        Op::WasmJumpI32NonZero => lane_jump_i32_zero::<H, false>,
        Op::WasmInstanceBinding => lane_instance_binding::<H>,
        Op::WasmGlobalGet => lane_global_get::<H>,
        Op::WasmGlobalSet => lane_global_set::<H>,
        Op::WasmSelect => lane_select::<H>,
        Op::WasmFillRegisters => lane_fill_registers::<H>,
        Op::WasmI32Unary => lane_i32_unary::<H>,
        Op::CallKnown => lane_call_known::<H>,
        Op::Return => lane_return::<H>,
        _ => lane_exit::<H>,
    }
}

/// The handler table covers one opcode byte; the opcode set must fit in it.
const TABLE_SLOTS: usize = 1 << u8::BITS;
const _: () = assert!(Op::COUNT <= TABLE_SLOTS);

/// Opcodes per generated table row.
const TABLE_ROW: u16 = 16;
const TABLE_ROWS: usize = TABLE_SLOTS / TABLE_ROW as usize;

macro_rules! table_row {
    ($h:ty, $row:literal) => {
        [
            handler_for::<$h, { $row * TABLE_ROW }>(),
            handler_for::<$h, { $row * TABLE_ROW + 1 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 2 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 3 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 4 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 5 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 6 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 7 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 8 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 9 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 10 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 11 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 12 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 13 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 14 }>(),
            handler_for::<$h, { $row * TABLE_ROW + 15 }>(),
        ]
    };
}

const fn flatten_rows<H: Host>(
    rows: [[Handler<H>; TABLE_ROW as usize]; TABLE_ROWS],
) -> [Handler<H>; TABLE_SLOTS] {
    let mut table = [lane_exit::<H> as Handler<H>; TABLE_SLOTS];
    let mut slot = 0;
    while slot < TABLE_SLOTS {
        table[slot] = rows[slot / TABLE_ROW as usize][slot % TABLE_ROW as usize];
        slot += 1;
    }
    table
}

struct LaneTable<H>(std::marker::PhantomData<H>);

impl<H: Host> LaneTable<H> {
    const HANDLERS: [Handler<H>; TABLE_SLOTS] = flatten_rows([
        table_row!(H, 0),
        table_row!(H, 1),
        table_row!(H, 2),
        table_row!(H, 3),
        table_row!(H, 4),
        table_row!(H, 5),
        table_row!(H, 6),
        table_row!(H, 7),
        table_row!(H, 8),
        table_row!(H, 9),
        table_row!(H, 10),
        table_row!(H, 11),
        table_row!(H, 12),
        table_row!(H, 13),
        table_row!(H, 14),
        table_row!(H, 15),
    ]);
}

/// Whether the lane may run `instruction` from its view record: every
/// register it names lies inside the register file and carries no flag.
fn lane_fields_fit(instruction: WideInstruction, registers: u16) -> bool {
    let op = instruction.op();
    let fields = [
        (InstructionField::A, instruction.a()),
        (InstructionField::B, instruction.b()),
        (InstructionField::C, instruction.c()),
    ];
    let registers_fit = fields
        .iter()
        .all(|&(field, value)| !op.field_layout(field).is_register_field() || value < registers);
    let window_fits = op != Op::WasmFillRegisters
        || u32::from(instruction.b()) + u32::from(instruction.c()) <= u32::from(registers);
    let pair_fits = op.immediate_layout() != ImmediateLayout::RegisterPair || {
        let (first, second) = ImmediateLayout::register_pair(instruction.imm());
        first < registers && second < registers
    };
    registers_fit && window_fits && pair_fits
}

/// Derive a function's lane view from its bytecode.
fn lane_view<H: Host>(function: &Function) -> Box<[LaneInstruction]> {
    let handlers = const { &LaneTable::<H>::HANDLERS };
    let record = std::mem::size_of::<LaneInstruction>() as i64;
    function
        .code
        .iter()
        .enumerate()
        .map(|(pc, packed)| {
            let instruction = if packed.is_wide() {
                function.wide[packed.wide_index()]
            } else {
                packed.as_wide()
            };
            let op = instruction.op();
            let jump = if op.immediate_role() == ImmediateRole::JumpTarget {
                i32::try_from((i64::from(instruction.imm()) - pc as i64) * record).ok()
            } else {
                Some(0)
            };
            let handler = match jump {
                Some(_) if lane_fields_fit(instruction, function.registers) => {
                    handlers[op as usize]
                }
                _ => lane_exit::<H>,
            };
            LaneInstruction {
                handler: handler as *const (),
                imm: instruction.imm(),
                jump: jump.unwrap_or(0),
                a: instruction.a(),
                b: instruction.b(),
                c: instruction.c(),
            }
        })
        .collect()
}

impl<H: Host> Vm<H> {
    /// Run lane instructions from `pc` in `frame` and return the PC of the
    /// first one that needs the general path; lane calls and returns update
    /// `frame` and `pending` as the general loop would. Kept out of line so
    /// the general loop's code generation, which serves JavaScript, does not
    /// depend on the lane.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_fast_lane(
        &mut self,
        code: &ResidualProgram,
        program: ProgramId,
        function: u32,
        frame: &mut usize,
        pc: usize,
        pending: &mut Vec<PendingGeneralCall>,
        inline_calls: bool,
    ) -> usize {
        let Some(base) = self.programs.lane_view(program, function, lane_view::<H>) else {
            return pc;
        };
        let registers = Registers(self.frames[*frame].registers.as_mut_ptr());
        let mut cx = LaneContext {
            vm: self,
            code,
            program,
            frame: *frame,
            registers,
            base,
            pending,
            inline_calls: inline_calls && code.kind == crate::bytecode::ProgramKind::Wasm,
            memory: MemoryView::NONE,
        };
        // SAFETY: the view has one record per instruction, and residual
        // validation establishes every reachable PC.
        let mut ip = unsafe { base.add(pc) };
        loop {
            let view = cx.memory;
            let next = handler::<H>(ip)(
                &mut cx,
                cx.registers,
                ip,
                view.bytes,
                view.len,
                view.binding,
            );
            ip = next.map_addr(|address| address & !EXIT_TAG);
            if next.addr() & EXIT_TAG != 0 {
                *frame = cx.frame;
                // SAFETY: handlers only return records of the active view.
                return unsafe { ip.offset_from(cx.base) } as usize;
            }
        }
    }
}
