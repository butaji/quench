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
use super::dispatch_frame::PendingGeneralCall;
use crate::bytecode::{
    FieldLayout, Function, ImmediateLayout, ImmediateRole, InstructionField, WideInstruction,
};
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator};
use crate::wasm::memory::{MemoryLoad, MemoryStorage, MemoryStore};

/// One instruction of a lane view.
#[repr(C)]
pub(super) struct LaneInstruction {
    /// The opcode's [`Handler`] for the VM that built the view.
    handler: *const (),
    operand: LaneOperand,
    a: u16,
    b: u16,
    c: u16,
    /// The general path executes this instruction; `handler` only reports
    /// that if the lane reaches it.
    exits: bool,
    /// The general loop enters the lane here: the straight-line lane run
    /// from this record is long enough to repay the lane's entry cost.
    enters: bool,
}

/// The immediate, resolved by its role: a jump target becomes the byte
/// distance from the record, a constant index becomes the constant.
#[derive(Clone, Copy)]
union LaneOperand {
    imm: u32,
    jump: isize,
    constant: Value,
}

impl LaneInstruction {
    #[inline(always)]
    fn imm(&self) -> u32 {
        // SAFETY: the view writes `imm` for every role other than a jump
        // target or constant index, and only those handlers read it.
        unsafe { self.operand.imm }
    }

    #[inline(always)]
    fn jump(&self) -> isize {
        // SAFETY: the view writes `jump` for every jump-target immediate.
        unsafe { self.operand.jump }
    }

    #[inline(always)]
    fn constant(&self) -> Value {
        // SAFETY: the view writes `constant` for every constant index.
        unsafe { self.operand.constant }
    }
}

type Ip = *const LaneInstruction;

/// A function's lane view: one record per instruction, and the one register
/// its direct memory accesses go through. Accesses through any other
/// register, and lane instructions other than the binding load that would
/// write this one, take the general path, so the memory view tracks the
/// register exactly.
pub(super) struct LaneView {
    records: Box<[LaneInstruction]>,
    memory_register: Option<u16>,
}

impl LaneView {
    pub(super) fn records(&self) -> Ip {
        self.records.as_ptr()
    }
}

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

/// The unshared memory bytes behind the active function's memory register.
/// The view is resolved whenever that register can change under the lane:
/// at lane entry, when a lane call or return switches functions, and when
/// the binding instruction loads the register; no other lane instruction
/// writes it (see `LaneView`). The lane never collects, calls out or grows
/// memory, so the bytes stay alive, unborrowed and unresized at a stable
/// address. A memory the lane cannot borrow resolves to the empty view, on
/// which every access leaves the lane.
#[derive(Clone, Copy)]
struct MemoryView {
    bytes: *mut u8,
    len: usize,
}

impl MemoryView {
    const NONE: Self = Self {
        bytes: std::ptr::null_mut(),
        len: 0,
    };

    /// SAFETY: the view must have been resolved during the current lane run.
    #[inline(always)]
    unsafe fn bytes<'a>(self) -> &'a mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.bytes, self.len) }
    }
}

/// The state handlers carry from one to the next in machine registers.
#[derive(Clone, Copy)]
struct Carried {
    memory: MemoryView,
    integers: crate::value::IntegerEncoding,
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
    /// Safepoint jumps left to dispatch before returning to the lane loop.
    direct_jumps: u32,
    /// The view handlers start from when the lane loop dispatches.
    memory: MemoryView,
}

/// A lane handler returns the next record for the lane loop, tagged with
/// [`EXIT_TAG`] when the general path must execute it. The memory view is
/// passed as scalars so every argument stays in a register.
type Handler<H> =
    fn(*mut LaneContext<H>, Registers, Ip, *mut u8, usize, crate::value::IntegerEncoding) -> Ip;

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
        let view: Carried = $view;
        return handler::<H>(ip)(
            $cx,
            $r,
            ip,
            view.memory.bytes,
            view.memory.len,
            view.integers,
        );
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
            integers: crate::value::IntegerEncoding,
        ) -> Ip {
            let $view = Carried {
                memory: MemoryView { bytes, len },
                integers,
            };
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
        r.set(i.a, i.constant());
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_load_local_plain<>(cx, r, ip, i, view) {
        // SAFETY: validated bytecode bounds the slot by Function.locals, and
        // frame setup sizes locals to that count.
        let value = unsafe { vm(cx).read_validated_local((*cx).frame, i.imm() as usize) };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

/// Taken safepoint jumps dispatched directly between returns to the lane
/// loop. Sibling calls keep a lane run in one stack frame; the periodic
/// return bounds the stack should a build not form one.
const DIRECT_SAFEPOINT_JUMPS: u32 = 64;

/// The safepoint budget a lane run starts with. A collection can only
/// become due inside the lane through a lane allocation, which zeroes the
/// budget, so the heap is consulted only when the budget runs out.
#[inline(always)]
fn safepoint_budget<H: Host>(vm: &Vm<H>) -> u32 {
    if vm.heap.should_collect() {
        0
    } else {
        DIRECT_SAFEPOINT_JUMPS
    }
}

/// The value of a register or constant operand; other operand kinds take
/// the general path.
#[inline(always)]
fn lane_operand_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    encoded: u16,
) -> Option<Value> {
    let operand = crate::bytecode::Operand(encoded);
    match operand.kind()? {
        crate::bytecode::OperandKind::Register => Some(r.get(operand.payload())),
        crate::bytecode::OperandKind::Constant => {
            let program = unsafe { (*cx).program };
            unsafe { vm(cx) }
                .programs
                .constant(program, usize::from(operand.payload()))
        }
        _ => None,
    }
}

lane_handler! {
    fn lane_store_local_plain<>(cx, r, ip, i, view) {
        let value = r.get(i.a);
        // SAFETY: validated bytecode bounds the slot by Function.locals.
        unsafe { vm(cx).write_validated_local((*cx).frame, i.imm() as usize, value) };
        if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
            r.set(register, value);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_load_this<>(cx, r, ip, i, view) {
        // An uninitialized `this` throws on the general path.
        let Some(value) = (unsafe { vm(cx).current_this((*cx).frame) }) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// Numbers convert to themselves; other values take the general path.
    fn lane_to_numeric<>(cx, r, ip, i, view) {
        let Some(number) = r.get(i.b).as_number() else {
            return exit(ip);
        };
        r.set(i.a, Value::number(number));
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_inc_dec<>(cx, r, ip, i, view) {
        let input = r.get(i.b);
        let decrement = i.imm() != 0;
        let value = match input.as_int() {
            Some(integer) => if decrement { integer.checked_sub(1) } else { integer.checked_add(1) }
                .map(Value::integer)
                .unwrap_or_else(|| Value::number(f64::from(integer) + if decrement { -1.0 } else { 1.0 })),
            None => match input.as_number() {
                Some(number) => Value::number(number + if decrement { -1.0 } else { 1.0 }),
                None => return exit(ip),
            },
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// Numeric operands on the integer fast path; the regional site profile
    /// records the execution exactly as the general path does.
    fn lane_binary<>(cx, r, ip, i, view) {
        let (Some(left), Some(right)) =
            (lane_operand_value(cx, r, i.b), lane_operand_value(cx, r, i.c))
        else {
            return exit(ip);
        };
        let operator = i.imm();
        let vm = unsafe { vm(cx) };
        let Some(value) = vm.numeric_binary(operator, left, right) else {
            return exit(ip);
        };
        if i.a & crate::bytecode::NUMERIC_LOCAL_TARGET != 0 {
            let local = usize::from(i.a & crate::bytecode::REGISTER_MASK);
            // SAFETY: the view admits a numeric local target only below
            // Function.locals.
            unsafe { vm.write_validated_local((*cx).frame, local, value) };
        } else {
            r.set(i.a, value);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A field read that hits its cache; any other read takes the general
    /// path, which repeats the side-effect-free lookup.
    fn lane_get_field<>(cx, r, ip, i, view) {
        if i.b == crate::bytecode::FieldBase::NESTED {
            return exit(ip);
        }
        let base = crate::bytecode::FieldBase(i.b);
        let vm = unsafe { vm(cx) };
        let object = match base.register_index() {
            Some(register) => r.get(register),
            None => match vm.current_this(unsafe { (*cx).frame }) {
                Some(this) => this,
                None => return exit(ip),
            },
        };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = vm.field_cache_hit(code, object, i.imm(), i.c) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

/// Store through the field cache; any other store takes the general path.
#[inline(always)]
fn lane_set_field<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    object: Value,
    i: &LaneInstruction,
    view: Carried,
) -> Ip {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if !unsafe { vm(cx) }.set_field_cache_hit(code, object, i.imm(), r.get(i.a), i.c) {
        return exit(ip);
    }
    next!(cx, r, following(ip), view)
}

lane_handler! {
    /// `SetField` and `SetFieldStrict`.
    fn lane_set_object_field<>(cx, r, ip, i, view) {
        lane_set_field(cx, r, ip, r.get(i.b), i, view)
    }
}

lane_handler! {
    /// `SetThisField` and `SetThisFieldStrict`.
    fn lane_set_this_field<>(cx, r, ip, i, view) {
        let Some(this) = (unsafe { vm(cx).current_this((*cx).frame) }) else {
            return exit(ip);
        };
        lane_set_field(cx, r, ip, this, i, view)
    }
}

lane_handler! {
    /// Integer relational operands on the general path's fast path.
    fn lane_jump_binary_false<BACKWARD: bool>(cx, r, ip, i, view) {
        let (Some(left), Some(right)) =
            (lane_operand_value(cx, r, i.b), lane_operand_value(cx, r, i.c))
        else {
            return exit(ip);
        };
        let Some(holds) = super::operations::integer_relation(u32::from(i.a), left, right) else {
            return exit(ip);
        };
        if !holds {
            return lane_jump(cx, r, ip, BACKWARD, view);
        }
        next!(cx, r, following(ip), view)
    }
}

/// A taken jump. Safepoint jumps (every `Jump`, and backward conditionals,
/// whose direction the view fixes per record) leave the lane when a
/// collection is due; forward conditionals dispatch directly.
#[inline(always)]
fn lane_jump<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    safepoint: bool,
    view: Carried,
) -> Ip {
    let jump = unsafe { (*ip).jump() };
    // SAFETY: the view records the distance to a validated jump target.
    let target = unsafe { ip.byte_offset(jump) };
    if safepoint {
        let budget = unsafe { &mut (*cx).direct_jumps };
        let (remaining, exhausted) = budget.overflowing_sub(1);
        *budget = remaining;
        if exhausted {
            let vm = unsafe { vm(cx) };
            if vm.heap.should_collect() {
                return exit(ip);
            }
            unsafe { (*cx).direct_jumps = safepoint_budget(vm) };
            return target;
        }
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
        let truthy = unsafe { vm(cx) }.truthy(r.get(i.a));
        if !truthy {
            return lane_jump(cx, r, ip, true, view);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// Take the selected entry's jump; the view admits a table only when
    /// every entry is a lane jump.
    fn lane_branch_table<>(cx, r, ip, i, view) {
        let case = r.get(i.a).wasm_bits32().min(i.imm());
        // SAFETY: validation places `imm + 1` entries after the table.
        let entry = unsafe { ip.add(1 + case as usize) };
        lane_jump(cx, r, entry, true, view)
    }
}

lane_handler! {
    fn lane_bit_field<>(cx, r, ip, i, view) {
        let value = crate::wasm::integer::shift_right_unsigned_and(
            r.i32(i.b),
            i32::from(i.c as i16),
            i.imm() as i32,
        );
        r.set(i.a, view.integers.encode(value));
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A counted loop's step and test.
    fn lane_add_jump_nonzero<BACKWARD: bool>(cx, r, ip, i, view) {
        let sum = r.i32(i.b).wrapping_add(i32::from(i.c as i16));
        r.set(i.a, view.integers.encode(sum));
        if sum != 0 {
            return lane_jump(cx, r, ip, BACKWARD, view);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// `WasmJumpI32Zero` and `WasmJumpI32NonZero`.
    fn lane_jump_i32_zero<WHEN_ZERO: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        if (r.i32(i.a) == 0) == WHEN_ZERO {
            return lane_jump(cx, r, ip, BACKWARD, view);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// Fused i32 comparison and branch, against a register or a constant.
    fn lane_compare_jump<OP: u16, BACKWARD: bool>(cx, r, ip, i, view) {
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
            return lane_jump(cx, r, ip, BACKWARD, view);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// Load an instance binding; loading the function's memory register
    /// resolves the memory view.
    fn lane_instance_binding<MEMORY: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let env = vm.frames[unsafe { (*cx).frame }].env;
        let Some(value) = vm.heap.environment_slot(env, i.imm() as usize) else {
            return exit(ip);
        };
        r.set(i.a, value);
        let view = if MEMORY {
            let memory = memory_view(vm, value);
            unsafe { (*cx).memory = memory };
            Carried { memory, ..view }
        } else {
            view
        };
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
        let (condition, _) = ImmediateLayout::register_pair(i.imm());
        let source = if r.i32(condition) != 0 { i.b } else { i.c };
        r.set(i.a, r.get(source));
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_fill_registers<>(cx, r, ip, i, view) {
        let value = i.constant();
        for offset in 0..i.c {
            r.set(i.b + offset, value);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_i32_unary<>(cx, r, ip, i, view) {
        let Some(operator) = I32UnaryOperator::from_tag(i.imm()) else {
            return exit(ip);
        };
        let crate::WasmValue::I32(value) = operator.apply(r.i32(i.b)) else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(value));
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
        let right = if immediate { i.imm() as i32 } else { r.i32(i.c) };
        let Ok(value) = operator.evaluate(r.i32(i.b), right) else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(value));
        next!(cx, r, following(ip), view)
    }
}

/// The memory32 effective address of a direct access through the function's
/// memory register.
#[inline(always)]
fn lane_address(r: Registers, i: &LaneInstruction) -> u64 {
    u64::from(r.get(i.c).wasm_bits32()) + u64::from(i.imm())
}

/// The view of the unshared memory `binding` names, or the empty view.
fn memory_view<H: Host>(vm: &Vm<H>, binding: Value) -> MemoryView {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap.get(binding) else {
        return MemoryView::NONE;
    };
    let MemoryStorage::Unshared(state) = &**bytes else {
        return MemoryView::NONE;
    };
    // An outstanding borrow belongs to a suspended host access; the general
    // path reports it exactly.
    let Ok(mut state) = state.try_borrow_mut() else {
        return MemoryView::NONE;
    };
    MemoryView {
        bytes: state.bytes.as_mut_ptr(),
        len: state.bytes.len(),
    }
}

/// The view behind `view`'s memory register in `registers`.
fn frame_memory_view<H: Host>(vm: &Vm<H>, view: &LaneView, registers: Registers) -> MemoryView {
    view.memory_register.map_or(MemoryView::NONE, |register| {
        memory_view(vm, registers.get(register))
    })
}

/// Boxing a 64-bit result allocates; kept out of line with scalar arguments
/// so the hot handlers keep their registers and sibling calls.
#[cold]
#[inline(never)]
fn lane_box_bits64<H: Host>(cx: *mut LaneContext<H>, bits: u64) -> Value {
    let vm = unsafe { vm(cx) };
    let value = vm.heap.alloc(Cell::WasmBits64(bits));
    // A due collection waits for the next safepoint, which checks the heap.
    unsafe { (*cx).direct_jumps = safepoint_budget(vm) };
    value
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
        // SAFETY: the view admits direct accesses only through the memory
        // register, whose bytes `view` holds; see MemoryView.
        let Ok(value) = load.read(unsafe { view.memory.bytes() }, lane_address(r, i)) else {
            return exit(ip);
        };
        let value = match value {
            crate::WasmValue::I32(value) => view.integers.encode(value),
            crate::WasmValue::F32(bits) => view.integers.encode(bits as i32),
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
        // SAFETY: as for `lane_load`.
        if store.write(unsafe { view.memory.bytes() }, lane_address(r, i), value).is_err() {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

/// Continue in `frame` at `pc`: the record the frame's function resumes at.
#[inline(always)]
fn lane_enter_frame<H: Host>(cx: *mut LaneContext<H>, frame: usize, pc: usize) -> Ip {
    let vm = unsafe { vm(cx) };
    let function = vm.frames[frame].function;
    let program = unsafe { (*cx).program };
    let Some(lane_view) = vm
        .programs
        .lane_view(program, function, derive_lane_view::<H>)
    else {
        unreachable!("an inline Wasm frame runs a function of its program");
    };
    // SAFETY: views live as long as their program entry.
    let lane_view = unsafe { &*lane_view };
    let base = lane_view.records();
    let r = Registers(vm.frames[frame].registers.as_mut_ptr());
    let view = frame_memory_view(vm, lane_view, r);
    // SAFETY: the view has one record per instruction of the function, and
    // `pc` is the frame's validated resume point.
    let ip = unsafe { base.add(pc) };
    unsafe {
        (*cx).frame = frame;
        (*cx).registers = r;
        (*cx).base = base;
        (*cx).memory = view;
    }
    let view = Carried {
        memory: view,
        integers: crate::value::IntegerEncoding::TAG,
    };
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
            base: ImmediateLayout::call_window_base(i.imm()),
            count: ImmediateLayout::argument_count(i.imm()),
        };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Ok(stack_guard) = vm.push_wasm_frame(code, caller, u32::from(i.b), parent, window) else {
            return exit(ip);
        };
        vm.frames[caller].pc = call_pc + 1;
        let caller_cursor =
            vm.general_cursor(unsafe { (*cx).program }, code, vm.frames[caller].function, true);
        unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
            caller,
            caller_cursor,
            call_pc: call_pc as u32,
            destination: i.a,
            construct_this: None,
            stack_guard,
        });
        lane_enter_frame(cx, caller + 1, 0)
    }
}

lane_handler! {
    /// Return from an inline call to its caller; returning from the loop's
    /// entry frame belongs to the general loop.
    fn lane_return<>(cx, r, ip, i, view) {
        let pending = unsafe { &mut *(*cx).pending };
        // An in-loop `new` substitutes its receiver for a non-object result;
        // the general path owns that [[Construct]] step.
        if !unsafe { (*cx).inline_calls }
            || pending.last().is_none_or(|call| call.construct_this.is_some())
        {
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
        lane_enter_frame(cx, call.caller, pc)
    }
}

/// The handler serving opcode `OP`, derived from the opcode's semantic
/// family; `None` when the general path executes the opcode.
const fn handler_for<H: Host, const OP: u16, const BACKWARD: bool>() -> Option<Handler<H>> {
    let Some(op) = Op::from_index(OP as usize) else {
        return None;
    };
    if I32BinaryOperator::from_register_op(op).is_some()
        || I32BinaryOperator::from_immediate_op(op).is_some()
    {
        return Some(lane_i32_binary::<H, OP>);
    }
    if I32BinaryOperator::from_jump_op(op).is_some()
        || I32BinaryOperator::from_immediate_jump_op(op).is_some()
    {
        return Some(lane_compare_jump::<H, OP, BACKWARD>);
    }
    if MemoryLoad::from_direct_op(op).is_some() {
        return Some(lane_load::<H, OP>);
    }
    if MemoryStore::from_direct_op(op).is_some() {
        return Some(lane_store::<H, OP>);
    }
    match op {
        Op::Move => Some(lane_move::<H>),
        Op::StoreLocalPlain => Some(lane_store_local_plain::<H>),
        Op::LoadThis => Some(lane_load_this::<H>),
        Op::GetField => Some(lane_get_field::<H>),
        Op::SetField | Op::SetFieldStrict => Some(lane_set_object_field::<H>),
        Op::SetThisField | Op::SetThisFieldStrict => Some(lane_set_this_field::<H>),
        Op::ToNumeric => Some(lane_to_numeric::<H>),
        Op::IncDec => Some(lane_inc_dec::<H>),
        Op::Binary => Some(lane_binary::<H>),
        Op::JumpBinaryFalse => Some(lane_jump_binary_false::<H, BACKWARD>),
        Op::LoadConst => Some(lane_load_const::<H>),
        Op::LoadLocalPlain => Some(lane_load_local_plain::<H>),
        Op::Jump => Some(lane_jump_always::<H>),
        Op::JumpFalse => Some(lane_jump_false::<H>),
        Op::WasmBranchTable => Some(lane_branch_table::<H>),
        Op::WasmI32ShiftRightUnsignedAndImmediate => Some(lane_bit_field::<H>),
        Op::WasmI32AddImmediateJumpNonZero => Some(lane_add_jump_nonzero::<H, BACKWARD>),
        Op::WasmJumpI32Zero => Some(lane_jump_i32_zero::<H, true, BACKWARD>),
        Op::WasmJumpI32NonZero => Some(lane_jump_i32_zero::<H, false, BACKWARD>),
        Op::WasmInstanceBinding => Some(lane_instance_binding::<H, false>),
        Op::WasmGlobalGet => Some(lane_global_get::<H>),
        Op::WasmGlobalSet => Some(lane_global_set::<H>),
        Op::WasmSelect => Some(lane_select::<H>),
        Op::WasmFillRegisters => Some(lane_fill_registers::<H>),
        Op::WasmI32Unary => Some(lane_i32_unary::<H>),
        Op::CallKnown => Some(lane_call_known::<H>),
        Op::Return => Some(lane_return::<H>),
        _ => None,
    }
}

/// The handler table covers one opcode byte; the opcode set must fit in it.
const TABLE_SLOTS: usize = 1 << u8::BITS;
const _: () = assert!(Op::COUNT <= TABLE_SLOTS);

/// Opcodes per generated table row.
const TABLE_ROW: u16 = 16;
const TABLE_ROWS: usize = TABLE_SLOTS / TABLE_ROW as usize;

macro_rules! table_row {
    ($h:ty, $backward:literal, $row:literal) => {
        [
            handler_for::<$h, { $row * TABLE_ROW }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 1 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 2 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 3 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 4 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 5 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 6 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 7 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 8 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 9 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 10 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 11 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 12 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 13 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 14 }, $backward>(),
            handler_for::<$h, { $row * TABLE_ROW + 15 }, $backward>(),
        ]
    };
}

const fn flatten_rows<H: Host>(
    rows: [[Option<Handler<H>>; TABLE_ROW as usize]; TABLE_ROWS],
) -> [Option<Handler<H>>; TABLE_SLOTS] {
    let mut table = [None; TABLE_SLOTS];
    let mut slot = 0;
    while slot < TABLE_SLOTS {
        table[slot] = rows[slot / TABLE_ROW as usize][slot % TABLE_ROW as usize];
        slot += 1;
    }
    table
}

struct LaneTable<H>(std::marker::PhantomData<H>);

impl<H: Host> LaneTable<H> {
    /// Handlers for instructions whose jump, if any, goes forward.
    const FORWARD: [Option<Handler<H>>; TABLE_SLOTS] = flatten_rows([
        table_row!(H, false, 0),
        table_row!(H, false, 1),
        table_row!(H, false, 2),
        table_row!(H, false, 3),
        table_row!(H, false, 4),
        table_row!(H, false, 5),
        table_row!(H, false, 6),
        table_row!(H, false, 7),
        table_row!(H, false, 8),
        table_row!(H, false, 9),
        table_row!(H, false, 10),
        table_row!(H, false, 11),
        table_row!(H, false, 12),
        table_row!(H, false, 13),
        table_row!(H, false, 14),
        table_row!(H, false, 15),
    ]);
    /// Handlers for instructions whose jump goes backward: a safepoint.
    const BACKWARD: [Option<Handler<H>>; TABLE_SLOTS] = flatten_rows([
        table_row!(H, true, 0),
        table_row!(H, true, 1),
        table_row!(H, true, 2),
        table_row!(H, true, 3),
        table_row!(H, true, 4),
        table_row!(H, true, 5),
        table_row!(H, true, 6),
        table_row!(H, true, 7),
        table_row!(H, true, 8),
        table_row!(H, true, 9),
        table_row!(H, true, 10),
        table_row!(H, true, 11),
        table_row!(H, true, 12),
        table_row!(H, true, 13),
        table_row!(H, true, 14),
        table_row!(H, true, 15),
    ]);
}

/// Whether the lane may run `instruction` from its view record: every
/// register it names lies inside the register file and carries no flag.
fn lane_fields_fit(instruction: WideInstruction, function: &Function) -> bool {
    let registers = function.registers;
    let op = instruction.op();
    let fields = [
        (InstructionField::A, instruction.a()),
        (InstructionField::B, instruction.b()),
        (InstructionField::C, instruction.c()),
    ];
    let numeric_local = instruction.writes_numeric_local()
        && instruction.a() & crate::bytecode::RETURN_REGISTER == 0
        && instruction.result_register() < function.locals;
    let registers_fit = fields
        .iter()
        .all(|&(field, value)| match op.field_layout(field) {
            FieldLayout::ResultRegister if field == InstructionField::A && numeric_local => true,
            FieldLayout::FieldBase => {
                value == crate::bytecode::FieldBase::NESTED
                    || crate::bytecode::FieldBase(value)
                        .register_index()
                        .is_none_or(|register| register < registers)
            }
            FieldLayout::OptionalRegister => value
                .checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS)
                .is_none_or(|register| register < registers),
            FieldLayout::Operand => {
                let operand = crate::bytecode::Operand(value);
                operand.kind() != Some(crate::bytecode::OperandKind::Register)
                    || operand.payload() < registers
            }
            layout => !layout.is_register_field() || value < registers,
        });
    let window_fits = op != Op::WasmFillRegisters
        || u32::from(instruction.b()) + u32::from(instruction.c()) <= u32::from(registers);
    let pair_fits = op.immediate_layout() != ImmediateLayout::RegisterPair || {
        let (first, second) = ImmediateLayout::register_pair(instruction.imm());
        first < registers && second < registers
    };
    registers_fit && window_fits && pair_fits
}

/// The record operand of `instruction` at `pc`, or `None` when its jump or
/// constant cannot be resolved.
fn lane_operand(
    instruction: WideInstruction,
    pc: usize,
    constants: &[Value],
) -> Option<LaneOperand> {
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    Some(match instruction.op().immediate_role() {
        ImmediateRole::JumpTarget => {
            let distance = isize::try_from(instruction.imm()).ok()? - isize::try_from(pc).ok()?;
            LaneOperand {
                jump: distance.checked_mul(record)?,
            }
        }
        ImmediateRole::ConstantIndex => LaneOperand {
            constant: *constants.get(instruction.imm() as usize)?,
        },
        _ => LaneOperand {
            imm: instruction.imm(),
        },
    })
}

/// The register every direct memory access of `function` goes through, when
/// there is exactly one.
fn memory_register(function: &Function) -> Option<u16> {
    let mut registers = function
        .code
        .iter()
        .map(|packed| decoded_instruction(function, *packed))
        .filter(|instruction| {
            MemoryLoad::from_direct_op(instruction.op()).is_some()
                || MemoryStore::from_direct_op(instruction.op()).is_some()
        })
        .map(|instruction| instruction.b());
    let first = registers.next()?;
    registers.all(|register| register == first).then_some(first)
}

/// Whether a lane run of `instruction` could write `register`.
fn lane_writes(instruction: WideInstruction, register: u16) -> bool {
    let op = instruction.op();
    let result = matches!(
        op.field_layout(InstructionField::A),
        FieldLayout::ResultRegister | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister
    ) && instruction.a() == register;
    let optional = op.field_layout(InstructionField::B) == FieldLayout::OptionalRegister
        && instruction
            .b()
            .checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS)
            == Some(register);
    let filled = op == Op::WasmFillRegisters
        && (instruction.b()..instruction.b().saturating_add(instruction.c())).contains(&register);
    result || optional || filled
}

/// Derive a function's lane view from its bytecode and its program's
/// constants.
/// The shortest straight-line lane run the general loop enters. Entering
/// builds the lane context and memory view, which one or two lane
/// instructions do not repay: on V8-v7 fixed work, three keeps every
/// JavaScript benchmark within one percent of the general path or better
/// (one and two cost DeltaBlue three percent), and leaves Wasm loops,
/// whose runs are long, unchanged.
const LANE_ENTRY_MIN_RUN: usize = 3;

fn derive_lane_view<H: Host>(function: &Function, constants: &[Value]) -> LaneView {
    let forward = const { &LaneTable::<H>::FORWARD };
    let backward = const { &LaneTable::<H>::BACKWARD };
    let memory_register = memory_register(function);
    let mut records: Box<[LaneInstruction]> = function
        .code
        .iter()
        .enumerate()
        .map(|(pc, packed)| {
            let instruction = decoded_instruction(function, *packed);
            let op = instruction.op();
            let binds_memory =
                op == Op::WasmInstanceBinding && Some(instruction.a()) == memory_register;
            let memory_access = MemoryLoad::from_direct_op(op).is_some()
                || MemoryStore::from_direct_op(op).is_some();
            let keeps_view = if memory_access {
                Some(instruction.b()) == memory_register
            } else {
                binds_memory
                    || memory_register.is_none_or(|register| !lane_writes(instruction, register))
            };
            let operand = lane_operand(instruction, pc, constants)
                .filter(|_| keeps_view && lane_fields_fit(instruction, function));
            let handler = operand.and(if binds_memory {
                Some(lane_instance_binding::<H, true> as Handler<H>)
            } else {
                let backward_jump = op.immediate_role() == ImmediateRole::JumpTarget
                    && instruction.imm() as usize <= pc;
                (if backward_jump { backward } else { forward })[op as usize]
            });
            LaneInstruction {
                handler: handler.unwrap_or(lane_exit::<H>) as *const (),
                operand: operand.unwrap_or(LaneOperand { imm: 0 }),
                a: instruction.a(),
                b: instruction.b(),
                c: instruction.c(),
                exits: handler.is_none(),
                enters: false,
            }
        })
        .collect();
    // A table follows its selected entry's jump, so every entry must run in
    // the lane.
    for (pc, packed) in function.code.iter().enumerate() {
        let instruction = decoded_instruction(function, *packed);
        if instruction.op() == Op::WasmBranchTable
            && crate::bytecode::branch_table_entries(pc, instruction)
                .any(|entry| records[entry].exits)
        {
            records[pc].handler = lane_exit::<H> as Handler<H> as *const ();
            records[pc].exits = true;
        }
    }
    let mut run = 0usize;
    for record in records.iter_mut().rev() {
        run = if record.exits { 0 } else { run + 1 };
        record.enters = run >= LANE_ENTRY_MIN_RUN;
    }
    LaneView {
        records,
        memory_register,
    }
}

fn decoded_instruction(function: &Function, packed: crate::bytecode::Instr) -> WideInstruction {
    if packed.is_wide() {
        function.wide[packed.wide_index()]
    } else {
        packed.as_wide()
    }
}

impl<H: Host> Vm<H> {
    /// The lane view of `function`, or null when its program has none.
    pub(super) fn lane_view(&self, program: ProgramId, function: u32) -> *const LaneView {
        self.programs
            .lane_view(program, function, derive_lane_view::<H>)
            .unwrap_or(std::ptr::null())
    }

    /// Whether the instruction at `pc` of `view` runs in the lane.
    #[inline(always)]
    pub(super) fn lane_runs(view: *const LaneView, pc: usize) -> bool {
        // SAFETY: a non-null view has a record for every validated PC.
        !view.is_null() && unsafe { (*(*view).records().add(pc)).enters }
    }

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
        view: *const LaneView,
        frame: &mut usize,
        pc: usize,
        pending: &mut Vec<PendingGeneralCall>,
        inline_calls: bool,
    ) -> usize {
        // SAFETY: the cursor's view outlives the lane run.
        let view = unsafe { &*view };
        let base = view.records();
        let registers = Registers(self.frames[*frame].registers.as_mut_ptr());
        let direct_jumps = safepoint_budget(self);
        let memory = frame_memory_view(self, view, registers);
        let mut cx = LaneContext {
            vm: self,
            code,
            program,
            frame: *frame,
            registers,
            base,
            pending,
            inline_calls: inline_calls && code.kind == crate::bytecode::ProgramKind::Wasm,
            direct_jumps,
            memory,
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
                crate::value::IntegerEncoding::TAG,
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
