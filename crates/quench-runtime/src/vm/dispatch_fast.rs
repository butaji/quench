//! The fast lane runs straight-line instructions whose whole effect is a
//! register write, a Wasm linear-memory access or a jump. Fields decode at
//! fixed positions and no frame state is published. The lane stops before any
//! instruction that would collect, trap, throw, call or touch a shared memory,
//! and the general path executes that instruction with its full semantics, so
//! lane execution is always an exact prefix of general execution.
//!
//! Every lane opcode has a handler, and each handler ends by dispatching the
//! next instruction's handler itself (threaded dispatch), so each opcode's
//! successor is predicted at its own branch. Where the build forms sibling
//! calls (`quench_lane_tail_calls`), that dispatch is a jump and a run of
//! handlers shares one stack frame; otherwise every handler returns to the
//! lane loop. Taken safepoint and backward jumps always return to the lane
//! loop, which bounds stack use by the straight-line run length even if a
//! sibling call is not formed.

use super::*;
use crate::bytecode::{Instr, WideInstruction};
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator};
use crate::wasm::memory::{MemoryLoad, MemoryStorage, MemoryStore};

/// Operand access shared by narrow and wide encodings of lane opcodes.
trait LaneFields: Copy {
    fn a(self) -> u16;
    fn b(self) -> u16;
    fn c(self) -> u16;
    fn imm(self) -> u32;
    fn pair(self) -> (u16, u16);
    /// The B field as a 16-bit two's-complement constant.
    fn signed_b(self) -> i16;
}

/// A narrow instruction of a lane opcode, decoded without layout checks.
#[derive(Clone, Copy)]
struct Narrow(Instr);

impl LaneFields for Narrow {
    #[inline(always)]
    fn a(self) -> u16 {
        self.0.plain_a()
    }
    #[inline(always)]
    fn b(self) -> u16 {
        self.0.plain_b()
    }
    #[inline(always)]
    fn c(self) -> u16 {
        self.0.plain_c()
    }
    #[inline(always)]
    fn imm(self) -> u32 {
        self.0.plain_imm()
    }
    #[inline(always)]
    fn pair(self) -> (u16, u16) {
        self.0.register_pair()
    }
    #[inline(always)]
    fn signed_b(self) -> i16 {
        self.0.full_b() as i16
    }
}

impl LaneFields for WideInstruction {
    #[inline(always)]
    fn a(self) -> u16 {
        WideInstruction::a(self)
    }
    #[inline(always)]
    fn b(self) -> u16 {
        WideInstruction::b(self)
    }
    #[inline(always)]
    fn c(self) -> u16 {
        WideInstruction::c(self)
    }
    #[inline(always)]
    fn imm(self) -> u32 {
        WideInstruction::imm(self)
    }
    #[inline(always)]
    fn pair(self) -> (u16, u16) {
        self.register_pair()
    }
    #[inline(always)]
    fn signed_b(self) -> i16 {
        WideInstruction::b(self) as i16
    }
}

/// How a handler reads its instruction's fields: from the packed word, or
/// from the wide side table the packed word indexes.
trait LaneFetch {
    type Fields: LaneFields;
    /// SAFETY: `ip` is a validated instruction of the lane's function.
    unsafe fn fetch<H: Host>(cx: *mut LaneContext<H>, ip: Ip) -> Self::Fields;
}

struct NarrowFetch;

impl LaneFetch for NarrowFetch {
    type Fields = Narrow;
    #[inline(always)]
    unsafe fn fetch<H: Host>(_: *mut LaneContext<H>, ip: Ip) -> Narrow {
        Narrow(unsafe { *ip })
    }
}

struct WideFetch;

impl LaneFetch for WideFetch {
    type Fields = WideInstruction;
    #[inline(always)]
    unsafe fn fetch<H: Host>(cx: *mut LaneContext<H>, ip: Ip) -> WideInstruction {
        // SAFETY: residual validation bounds every wide index by the table.
        unsafe { *(*cx).wide.add((*ip).wide_index()) }
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
        // SAFETY: compiler-issued registers are below Function.registers, and
        // frame setup sizes the register file to that count.
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
    program: ProgramId,
    frame: usize,
    code: *const Instr,
    wide: *const WideInstruction,
    /// The view handlers start from when the lane loop dispatches.
    memory: MemoryView,
}

type Ip = *const Instr;

/// A lane handler returns the next instruction for the lane loop, tagged with
/// [`EXIT_TAG`] when the general path must execute it. The memory view is
/// passed as scalars so every argument stays in a register.
type Handler<H> = fn(*mut LaneContext<H>, Registers, Ip, *mut u8, usize, Value) -> Ip;

/// Instructions are word aligned, so the low address bit is free to mark a
/// lane exit.
const EXIT_TAG: usize = 1;
const _: () = assert!(std::mem::align_of::<Instr>() > EXIT_TAG);

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

/// Continue with the handler of the instruction at `ip`.
#[cfg(quench_lane_tail_calls)]
macro_rules! next {
    ($cx:expr, $r:expr, $ip:expr, $view:expr) => {{
        let ip: Ip = $ip;
        let view: MemoryView = $view;
        return lane_handler::<H>(ip)($cx, $r, ip, view.bytes, view.len, view.binding);
    }};
}

#[cfg(not(quench_lane_tail_calls))]
macro_rules! next {
    ($cx:expr, $r:expr, $ip:expr, $view:expr) => {{
        let _ = ($r, $view);
        return $ip;
    }};
}

/// Define a handler with the shared signature. `$i` is the decoded
/// instruction and `$view` the current memory view.
macro_rules! handler {
    ($(#[$meta:meta])* fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        $(#[$meta])*
        #[allow(unused_variables)]
        fn $name<H: Host, F: LaneFetch, $(const $param: $ty),*>(
            $cx: *mut LaneContext<H>,
            $r: Registers,
            $ip: Ip,
            bytes: *mut u8,
            len: usize,
            binding: Value,
        ) -> Ip {
            let $view = MemoryView { binding, bytes, len };
            // SAFETY: the lane only dispatches validated, reachable instructions.
            let $i = unsafe { F::fetch($cx, $ip) };
            $body
        }
    };
}

/// The instruction after `ip`.
#[inline(always)]
fn following(ip: Ip) -> Ip {
    // SAFETY: validation ends every function in a terminator, so a lane
    // instruction that falls through has a successor.
    unsafe { ip.add(1) }
}

#[inline(always)]
fn lane_handler<H: Host>(ip: Ip) -> Handler<H> {
    let table = const { &LaneTables::<H>::NARROW };
    // SAFETY: `ip` is a validated instruction; the table covers every
    // encodable opcode.
    unsafe { *table.get_unchecked((*ip).opcode_index()) }
}

/// The opcode a handler instantiation serves.
const fn opcode(index: u16) -> Op {
    match Op::from_index(index as usize) {
        Some(op) => op,
        None => panic!("lane handler for an opcode outside the opcode table"),
    }
}

handler! {
    /// Not a lane opcode: the general path executes it.
    fn lane_exit<>(cx, r, ip, i, view) {
        exit(ip)
    }
}

handler! {
    /// The narrow word names a wide instruction: dispatch on its opcode.
    fn lane_wide<>(cx, r, ip, i, view) {
        // SAFETY: `ip` holds a wide marker; validation bounds its index.
        let wide = unsafe { *(*cx).wide.add((*ip).wide_index()) };
        let table = const { &LaneTables::<H>::WIDE };
        table[wide.op() as usize](cx, r, ip, view.bytes, view.len, view.binding)
    }
}

handler! {
    fn lane_move<>(cx, r, ip, i, view) {
        r.set(i.a(), r.get(i.b()));
        next!(cx, r, following(ip), view)
    }
}

handler! {
    fn lane_load_const<>(cx, r, ip, i, view) {
        let program = unsafe { (*cx).program };
        let Some(value) = unsafe { vm(cx) }.programs.constant(program, i.imm() as usize) else {
            return exit(ip);
        };
        r.set(i.a(), value);
        next!(cx, r, following(ip), view)
    }
}

handler! {
    fn lane_load_local_plain<>(cx, r, ip, i, view) {
        // SAFETY: validated bytecode bounds the slot by Function.locals, and
        // frame setup sizes locals to that count.
        let value = unsafe { vm(cx).read_validated_local((*cx).frame, i.imm() as usize) };
        r.set(i.a(), value);
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
    target: u32,
    safepoint: bool,
    view: MemoryView,
) -> Ip {
    // SAFETY: residual validation establishes every jump target.
    let target = unsafe { (*cx).code.add(target as usize) };
    if safepoint || target <= ip {
        if unsafe { vm(cx) }.heap.should_collect() {
            return exit(ip);
        }
        return target;
    }
    next!(cx, r, target, view)
}

handler! {
    fn lane_jump_always<>(cx, r, ip, i, view) {
        lane_jump(cx, r, ip, i.imm(), true, view)
    }
}

handler! {
    fn lane_jump_false<>(cx, r, ip, i, view) {
        let value = r.get(i.a());
        let truthy = match value.as_int() {
            Some(integer) => integer != 0,
            None if value == Value::TRUE => true,
            None if value == Value::FALSE => false,
            None => return exit(ip),
        };
        if !truthy {
            return lane_jump(cx, r, ip, i.imm(), true, view);
        }
        next!(cx, r, following(ip), view)
    }
}

handler! {
    /// `WasmJumpI32Zero` and `WasmJumpI32NonZero`.
    fn lane_jump_i32_zero<WHEN_ZERO: bool>(cx, r, ip, i, view) {
        if (r.i32(i.a()) == 0) == WHEN_ZERO {
            return lane_jump(cx, r, ip, i.imm(), false, view);
        }
        next!(cx, r, following(ip), view)
    }
}

handler! {
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
            i32::from(i.signed_b())
        } else {
            r.i32(i.b())
        };
        let Ok(taken) = comparison.evaluate(r.i32(i.a()), right) else {
            return exit(ip);
        };
        if taken != 0 {
            return lane_jump(cx, r, ip, i.imm(), false, view);
        }
        next!(cx, r, following(ip), view)
    }
}

handler! {
    fn lane_instance_binding<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let env = vm.frames[unsafe { (*cx).frame }].env;
        let Some(value) = vm.heap.environment_slot(env, i.imm() as usize) else {
            return exit(ip);
        };
        r.set(i.a(), value);
        next!(cx, r, following(ip), view)
    }
}

handler! {
    fn lane_global_get<>(cx, r, ip, i, view) {
        let Some(Cell::WasmGlobal { value, .. }) = unsafe { vm(cx) }.heap.get(r.get(i.b())) else {
            return exit(ip);
        };
        r.set(i.a(), *value);
        next!(cx, r, following(ip), view)
    }
}

handler! {
    fn lane_global_set<>(cx, r, ip, i, view) {
        let Some(Cell::WasmGlobal {
            value,
            mutable: true,
            ..
        }) = unsafe { vm(cx) }.heap.get_mut(r.get(i.b()))
        else {
            return exit(ip);
        };
        *value = r.get(i.a());
        next!(cx, r, following(ip), view)
    }
}

handler! {
    fn lane_select<>(cx, r, ip, i, view) {
        let (condition, _) = i.pair();
        let source = if r.i32(condition) != 0 { i.b() } else { i.c() };
        r.set(i.a(), r.get(source));
        next!(cx, r, following(ip), view)
    }
}

handler! {
    fn lane_fill_registers<>(cx, r, ip, i, view) {
        let program = unsafe { (*cx).program };
        let Some(value) = unsafe { vm(cx) }.programs.constant(program, i.imm() as usize) else {
            return exit(ip);
        };
        for offset in 0..i.c() {
            r.set(i.b() + offset, value);
        }
        next!(cx, r, following(ip), view)
    }
}

handler! {
    fn lane_i32_unary<>(cx, r, ip, i, view) {
        let Some(operator) = I32UnaryOperator::from_tag(i.imm()) else {
            return exit(ip);
        };
        let crate::WasmValue::I32(value) = operator.apply(r.i32(i.b())) else {
            return exit(ip);
        };
        r.set(i.a(), Value::integer(value));
        next!(cx, r, following(ip), view)
    }
}

handler! {
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
        let right = if immediate {
            i.imm() as i32
        } else {
            r.i32(i.c())
        };
        let Ok(value) = operator.evaluate(r.i32(i.b()), right) else {
            return exit(ip);
        };
        r.set(i.a(), Value::integer(value));
        next!(cx, r, following(ip), view)
    }
}

/// The memory32 effective address of a direct access, or `None` when the
/// view does not hold the accessed binding.
#[inline(always)]
fn lane_address<I: LaneFields>(r: Registers, view: MemoryView, i: I) -> Option<u64> {
    (r.get(i.b()) == view.binding)
        .then(|| u64::from(r.get(i.c()).wasm_bits32()) + u64::from(i.imm()))
}

/// Resolve the view for the binding the instruction at `ip` accesses, then
/// resume that instruction from the lane loop; a shared or borrowed memory
/// leaves the lane.
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

handler! {
    fn lane_load<OP: u16>(cx, r, ip, i, view) {
        let load = const {
            match MemoryLoad::from_direct_op(opcode(OP)) {
                Some(load) => load,
                None => panic!("not a direct load"),
            }
        };
        let Some(address) = lane_address(r, view, i) else {
            return lane_resolve_memory(cx, r.get(i.b()), ip);
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
        r.set(i.a(), value);
        next!(cx, r, following(ip), view)
    }
}

handler! {
    fn lane_store<OP: u16>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        let value = match store.value_type() {
            crate::WasmType::I32 => crate::WasmValue::I32(r.i32(i.a())),
            crate::WasmType::F32 => match r.get(i.a()).as_int() {
                Some(bits) => crate::WasmValue::F32(bits as u32),
                None => return exit(ip),
            },
            ty @ crate::WasmType::I64 => match lane_unbox_bits64(cx, r.get(i.a()), ty) {
                Some(bits) => crate::WasmValue::I64(bits as i64),
                None => return exit(ip),
            },
            ty @ crate::WasmType::F64 => match lane_unbox_bits64(cx, r.get(i.a()), ty) {
                Some(bits) => crate::WasmValue::F64(bits),
                None => return exit(ip),
            },
            _ => return exit(ip),
        };
        let Some(address) = lane_address(r, view, i) else {
            return lane_resolve_memory(cx, r.get(i.b()), ip);
        };
        // SAFETY: as for `lane_load`.
        if store.write(unsafe { view.bytes() }, address, value).is_err() {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

/// The handler serving opcode `OP` under encoding `F`, derived from the
/// opcode's semantic family.
const fn handler_for<H: Host, F: LaneFetch, const OP: u16>() -> Handler<H> {
    let Some(op) = Op::from_index(OP as usize) else {
        return lane_exit::<H, F>;
    };
    if op.is_wide_marker() {
        return lane_wide::<H, F>;
    }
    if I32BinaryOperator::from_register_op(op).is_some()
        || I32BinaryOperator::from_immediate_op(op).is_some()
    {
        return lane_i32_binary::<H, F, OP>;
    }
    if I32BinaryOperator::from_jump_op(op).is_some()
        || I32BinaryOperator::from_immediate_jump_op(op).is_some()
    {
        return lane_compare_jump::<H, F, OP>;
    }
    if MemoryLoad::from_direct_op(op).is_some() {
        return lane_load::<H, F, OP>;
    }
    if MemoryStore::from_direct_op(op).is_some() {
        return lane_store::<H, F, OP>;
    }
    match op {
        Op::Move => lane_move::<H, F>,
        Op::LoadConst => lane_load_const::<H, F>,
        Op::LoadLocalPlain => lane_load_local_plain::<H, F>,
        Op::Jump => lane_jump_always::<H, F>,
        Op::JumpFalse => lane_jump_false::<H, F>,
        Op::WasmJumpI32Zero => lane_jump_i32_zero::<H, F, true>,
        Op::WasmJumpI32NonZero => lane_jump_i32_zero::<H, F, false>,
        Op::WasmInstanceBinding => lane_instance_binding::<H, F>,
        Op::WasmGlobalGet => lane_global_get::<H, F>,
        Op::WasmGlobalSet => lane_global_set::<H, F>,
        Op::WasmSelect => lane_select::<H, F>,
        Op::WasmFillRegisters => lane_fill_registers::<H, F>,
        Op::WasmI32Unary => lane_i32_unary::<H, F>,
        _ => lane_exit::<H, F>,
    }
}

/// Handler tables cover one opcode byte; the opcode set must fit in it.
const TABLE_SLOTS: usize = 1 << u8::BITS;
const _: () = assert!(Instr::OPCODE_SLOTS <= TABLE_SLOTS);

/// Opcodes per generated table row.
const TABLE_ROW: u16 = 16;

macro_rules! table_row {
    ($h:ty, $f:ty, $row:literal) => {
        [
            handler_for::<$h, $f, { $row * TABLE_ROW }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 1 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 2 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 3 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 4 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 5 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 6 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 7 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 8 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 9 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 10 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 11 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 12 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 13 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 14 }>(),
            handler_for::<$h, $f, { $row * TABLE_ROW + 15 }>(),
        ]
    };
}

macro_rules! handler_table {
    ($h:ty, $f:ty) => {
        flatten_rows([
            table_row!($h, $f, 0),
            table_row!($h, $f, 1),
            table_row!($h, $f, 2),
            table_row!($h, $f, 3),
            table_row!($h, $f, 4),
            table_row!($h, $f, 5),
            table_row!($h, $f, 6),
            table_row!($h, $f, 7),
            table_row!($h, $f, 8),
            table_row!($h, $f, 9),
            table_row!($h, $f, 10),
            table_row!($h, $f, 11),
            table_row!($h, $f, 12),
            table_row!($h, $f, 13),
            table_row!($h, $f, 14),
            table_row!($h, $f, 15),
        ])
    };
}

const ROWS: usize = TABLE_SLOTS / TABLE_ROW as usize;

const fn flatten_rows<H: Host>(
    rows: [[Handler<H>; TABLE_ROW as usize]; ROWS],
) -> [Handler<H>; TABLE_SLOTS] {
    let mut table = [lane_exit::<H, NarrowFetch> as Handler<H>; TABLE_SLOTS];
    let mut slot = 0;
    while slot < TABLE_SLOTS {
        table[slot] = rows[slot / TABLE_ROW as usize][slot % TABLE_ROW as usize];
        slot += 1;
    }
    table
}

struct LaneTables<H>(std::marker::PhantomData<H>);

impl<H: Host> LaneTables<H> {
    const NARROW: [Handler<H>; TABLE_SLOTS] = handler_table!(H, NarrowFetch);
    const WIDE: [Handler<H>; TABLE_SLOTS] = handler_table!(H, WideFetch);
}

impl<H: Host> Vm<H> {
    /// Run lane instructions from `pc` and return the PC of the first one
    /// that needs the general path. Kept out of line so the general loop's
    /// code generation, which serves JavaScript, does not depend on the lane.
    #[inline(never)]
    pub(super) fn run_fast_lane(
        &mut self,
        program: ProgramId,
        frame: usize,
        code: *const Instr,
        wide: *const WideInstruction,
        pc: usize,
    ) -> usize {
        let registers = Registers(self.frames[frame].registers.as_mut_ptr());
        let mut cx = LaneContext {
            vm: self,
            program,
            frame,
            code,
            wide,
            memory: MemoryView::NONE,
        };
        // SAFETY: residual validation establishes every reachable PC.
        let mut ip = unsafe { code.add(pc) };
        loop {
            let view = cx.memory;
            let next =
                lane_handler::<H>(ip)(&mut cx, registers, ip, view.bytes, view.len, view.binding);
            ip = next.map_addr(|address| address & !EXIT_TAG);
            if next.addr() & EXIT_TAG != 0 {
                // SAFETY: handlers only return instructions of this function.
                return unsafe { ip.offset_from(code) } as usize;
            }
        }
    }
}
