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
//! every handler returns to the lane loop. A taken safepoint jump compares the
//! stack pointer with the run's stack floor: should a build not form a sibling
//! call, the stack reaches the floor and the jump returns to the lane loop,
//! which unwinds it; a due collection raises the floor so that the next
//! safepoint jump leaves the lane.

use super::dispatch_frame::PendingGeneralCall;
use super::*;
use crate::bytecode::{
    FieldLayout, Function, ImmediateLayout, ImmediateRole, InstructionField, WideInstruction,
};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{
    F32BinaryOperator, F32UnaryOperator, F64BinaryOperator, F64UnaryOperator,
};
use crate::wasm::integer::{
    I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator,
};
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
    /// Per instruction, whether the general loop enters the lane there: the
    /// straight-line lane run from it is long enough to repay the lane's
    /// entry cost, and its record does not read the accumulator. Kept apart
    /// from the records, which only the lane reads.
    entries: Box<[bool]>,
    memory_register: Option<u16>,
    /// How a lane call activates the function without a frame; `None`
    /// when its activation needs a frame (locals outside registers).
    activation: Option<ActivationShape>,
    /// The general loop enters the lane in this function: a Wasm function, or a JavaScript
    /// one with a loop the lane runs whole (`closes_loop`). Elsewhere JavaScript leaves the
    /// lane at its calls and property operations, and the hand-offs cost more than the lane
    /// saves (Richards +22%, DeltaBlue +15% instructions when every function used it).
    enters: bool,
}

/// Registers a lane call writes as one fixed block when the window fits it:
/// slots past the window lie in spare stack capacity, so the block needs no
/// loop over the window's size.
const ACTIVATION_FILL_BLOCK: usize = 8;

/// A function's register window as a lane activation starts it: every
/// register its initial value, then the parameters at their base.
#[derive(Clone, Copy)]
struct ActivationShape {
    registers: u16,
    parameters: u16,
    params: u16,
    initial: Value,
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
    /// The memory's length less the widest direct access: an access
    /// starting at or below it is in bounds whatever its width, so the hot
    /// path compares the address alone. Negative when the memory is
    /// narrower than that access.
    reach: isize,
}

/// The widest access a lane record makes directly.
const WIDEST_DIRECT_ACCESS: usize = size_of::<u64>();

impl MemoryView {
    const NONE: Self = Self::new(std::ptr::null_mut(), 0);

    const fn new(bytes: *mut u8, len: usize) -> Self {
        Self {
            bytes,
            reach: len as isize - WIDEST_DIRECT_ACCESS as isize,
        }
    }

    #[inline(always)]
    fn len(self) -> usize {
        (self.reach + WIDEST_DIRECT_ACCESS as isize) as usize
    }

    /// Direct load `load` at `address`, when the access lies within the
    /// reach; `None` leaves it to the general path, which also decides the
    /// last bytes before the memory's end exactly.
    ///
    /// SAFETY: the view must have been resolved during the current lane run.
    #[inline(always)]
    unsafe fn read(self, load: MemoryLoad, address: u64) -> Option<crate::WasmValue> {
        // Addresses are at most 33 bits, so the signed comparison is exact.
        if (address as i64) > self.reach as i64 || load.width() > WIDEST_DIRECT_ACCESS {
            // Off the hot path: the access falls through to its dispatch.
            std::hint::cold_path();
            return None;
        }
        // SAFETY: the access ends at or before the memory's end.
        Some(unsafe { load.read_at(self.bytes.add(address as usize)) })
    }

    /// Direct store `store` of `value` at `address`, as for `read`.
    ///
    /// SAFETY: the view must have been resolved during the current lane run.
    #[inline(always)]
    unsafe fn write(self, store: MemoryStore, address: u64, value: crate::WasmValue) -> Option<()> {
        // Addresses are at most 33 bits, so the signed comparison is exact.
        if (address as i64) > self.reach as i64 || store.width() > WIDEST_DIRECT_ACCESS {
            std::hint::cold_path();
            return None;
        }
        // SAFETY: the access ends at or before the memory's end.
        unsafe { store.write_at(self.bytes.add(address as usize), value) };
        Some(())
    }
}

/// The state handlers carry from one to the next in machine registers.
#[derive(Clone, Copy)]
struct Carried {
    memory: MemoryView,
    integers: crate::value::IntegerEncoding,
    /// The value an accumulator producer just wrote to its result register.
    /// A record reads it only where the view proved it equals that register.
    acc: Value,
}

/// A Wasm activation a lane call made without a VM frame. Its registers are
/// a window of the lane stack; it returns to the record after `call` in its
/// caller, whose registers, view and memory it keeps for the return.
#[derive(Clone, Copy)]
struct Activation {
    function: u32,
    call: Ip,
    caller_base: Ip,
    caller_registers: Registers,
    caller_memory: MemoryView,
    /// The lane-stack index of this activation's first register.
    window: usize,
}

/// Wasm activations a lane runs without VM frames, innermost last, over the
/// bottom VM frame of the lane run. They exist only while the lane runs:
/// every exit makes them VM frames with pending calls, exactly as frame
/// pushes would have, so the general loop, traps and collection see the
/// usual frame stack. The register storage grows only between activations,
/// so register windows stay put while any exists.
#[derive(Default)]
pub(super) struct LaneStack {
    values: Vec<Value>,
    activations: Vec<Activation>,
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
    /// The program's lane views by function, which a call reads when its
    /// callee's view is derived.
    views: *const [std::cell::OnceCell<LaneView>],
    /// Lane calls and returns manage activations: the interpreter runs a
    /// Wasm program in a loop that allows inline calls. A compiled callee
    /// runs from the general path, which raises its traps.
    inline_calls: bool,
    /// The view handlers start from when the lane loop dispatches.
    memory: MemoryView,
    /// The last memory binding resolved in this lane run and its view. No
    /// lane instruction grows memory or runs host code, so a binding's view
    /// holds for the whole run; calls and returns within one instance reuse
    /// it instead of resolving the binding again.
    resolved: (Value, MemoryView),
}

/// A lane handler returns the next record for the lane loop, tagged with
/// [`EXIT_TAG`] when the general path must execute it. The carried state is
/// passed as scalars so every argument stays in a register; the context,
/// which hot handlers do not touch, is reached through [`LANE_CONTEXT`].
type Handler = fn(Registers, Ip, *mut u8, isize, crate::value::IntegerEncoding, Value) -> Ip;

thread_local! {
    /// The context of this thread's running lane. A lane run never calls
    /// out, so runs nest only through the general path, and each run
    /// restores the outer context when it returns.
    static LANE_CONTEXT: std::cell::Cell<*mut ()> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

thread_local! {
    /// The running lane's stack floor: a taken safepoint jump below it
    /// returns to the lane loop. The maximum address marks a due collection.
    /// A safepoint only reads it, so loops carry no dependence through it.
    static LANE_FLOOR: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Stack a lane run may use below its entry before a safepoint jump returns
/// to the lane loop; only reachable should a build not form sibling calls.
const LANE_STACK_SLACK: usize = 64 * 1024;

/// The floor that sends the next safepoint jump through the slow path.
const COLLECTION_DUE_FLOOR: usize = usize::MAX;

/// The current stack address.
#[cfg(quench_lane_tail_calls)]
#[inline(always)]
fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reads the stack pointer register; no memory, stack or flags.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    // SAFETY: as above.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    sp
}

/// Without sibling calls every handler returns to the lane loop, so a run's
/// stack cannot grow; this address lies above every stack floor and below
/// the due-collection floor.
#[cfg(not(quench_lane_tail_calls))]
#[inline(always)]
fn stack_pointer() -> usize {
    COLLECTION_DUE_FLOOR - 1
}

/// Whether a taken safepoint jump takes the slow path.
#[inline(always)]
fn below_floor() -> bool {
    stack_pointer() < LANE_FLOOR.with(std::cell::Cell::get)
}

/// After a lane allocation, a due collection raises the floor.
#[inline(always)]
fn note_allocation<H: Host>(vm: &Vm<H>) {
    if vm.heap.should_collect() {
        LANE_FLOOR.with(|floor| floor.set(COLLECTION_DUE_FLOOR));
    }
}

/// The running lane's context.
#[inline(always)]
fn lane_context<H: Host>() -> *mut LaneContext<H> {
    LANE_CONTEXT.with(std::cell::Cell::get).cast()
}

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
fn handler<H: Host>(ip: Ip) -> Handler {
    // SAFETY: a view is only run by the VM type whose handlers built it, and
    // every reachable record is initialized.
    unsafe { std::mem::transmute::<*const (), Handler>((*ip).handler) }
}

/// Continue with the handler of the record at `ip`.
#[cfg(quench_lane_tail_calls)]
macro_rules! next {
    ($cx:expr, $r:expr, $ip:expr, $view:expr) => {{
        let ip: Ip = $ip;
        let view: Carried = $view;
        let _ = $cx;
        return handler::<H>(ip)(
            $r,
            ip,
            view.memory.bytes,
            view.memory.reach,
            view.integers,
            view.acc,
        );
    }};
}

#[cfg(not(quench_lane_tail_calls))]
macro_rules! next {
    ($cx:expr, $r:expr, $ip:expr, $view:expr) => {{
        let _ = ($cx, $r, $view);
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
            $r: Registers,
            $ip: Ip,
            bytes: *mut u8,
            reach: isize,
            integers: crate::value::IntegerEncoding,
            acc: Value,
        ) -> Ip {
            let $cx = lane_context::<H>();
            let $view = Carried {
                memory: MemoryView { bytes, reach },
                integers,
                acc,
            };
            // SAFETY: the lane only dispatches records of the running view.
            let $i = unsafe { &*$ip };
            $body
        }
    };
}

/// Which field of a record reads the accumulator: none, or the field the
/// constant names. The view selects a reading instantiation only where the
/// field's register is the previous record's result register and control
/// reaches the record only by falling through from it.
const ACC_NONE: u8 = 0;
const ACC_A: u8 = 1;
const ACC_B: u8 = 2;
const ACC_C: u8 = 3;
/// The first register of the immediate's register pair: a select's
/// condition.
const ACC_PAIR: u8 = 4;
/// Accumulator positions a record may read: none, A, B, C and the pair.
const ACC_POSITIONS: usize = 5;

impl InstructionField {
    const fn accumulator(self) -> u8 {
        match self {
            Self::A => ACC_A,
            Self::B => ACC_B,
            Self::C => ACC_C,
        }
    }
}

/// The value of register field `FIELD` of a record whose accumulator field
/// is `ACC`.
#[inline(always)]
fn read<const ACC: u8, const FIELD: u8>(r: Registers, register: u16, view: Carried) -> Value {
    if ACC == FIELD {
        view.acc
    } else {
        r.get(register)
    }
}

#[inline(always)]
fn read_i32<const ACC: u8, const FIELD: u8>(r: Registers, register: u16, view: Carried) -> i32 {
    read::<ACC, FIELD>(r, register, view).wasm_bits32() as i32
}

/// Write an accumulator producer's result and continue with it carried.
macro_rules! produce {
    ($cx:expr, $r:expr, $ip:expr, $register:expr, $value:expr, $view:expr) => {{
        let value: Value = $value;
        $r.set($register, value);
        next!(
            $cx,
            $r,
            following($ip),
            Carried {
                acc: value,
                ..$view
            }
        )
    }};
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
    fn lane_move<ACC: u8>(cx, r, ip, i, view) {
        produce!(cx, r, ip, i.a, read::<ACC, ACC_B>(r, i.b, view), view)
    }
}

lane_handler! {
    fn lane_load_const<>(cx, r, ip, i, view) {
        produce!(cx, r, ip, i.a, i.constant(), view)
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
        let Some(value) = vm.number_binary(operator, left, right) else {
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
    /// `Binary` of arithmetic operator `OP` on two registers, its operator and operand
    /// kinds resolved when the view was derived (`specialize_arithmetic`): the fields hold
    /// the operand registers themselves.
    fn lane_arith<OP: u32>(cx, r, ip, i, view) {
        let Some((value, _)) = super::operations::number_operator::<OP>(r.get(i.b), r.get(i.c))
        else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A present dense array element; any other read takes the general path.
    fn lane_get_index<>(cx, r, ip, i, view) {
        let (Some(base), Some(key)) =
            (lane_operand_value(cx, r, i.b), lane_operand_value(cx, r, i.c))
        else {
            return exit(ip);
        };
        let Some(value) = (unsafe { vm(cx) }).dense_array_element(base, key) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// Replacing a present dense element; any other write takes the general path.
    fn lane_set_index<>(cx, r, ip, i, view) {
        let (object, key) = (r.get(i.b), r.get(i.c));
        let vm = unsafe { vm(cx) };
        let Some(index) = vm.replaceable_dense_index(object, key) else {
            return exit(ip);
        };
        vm.replace_dense_element(object, index, r.get(i.a));
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A capture the frame's environment owns directly; any other read takes the general path.
    fn lane_load_capture<>(cx, r, ip, i, view) {
        let imm = i.imm();
        let Some(value) = (unsafe { vm(cx) }).cached_direct_capture(
            unsafe { (*cx).frame },
            ImmediateLayout::capture_depth(imm),
            ImmediateLayout::capture_slot(imm),
        ) else {
            return exit(ip);
        };
        r.set(i.a, value);
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
        // The general loop's probe (`cached_field_hit`): own, megamorphic and holder entries.
        let super::object::FieldCacheProbe::Hit(value) = vm.cached_field_hit(code, object, i.c)
        else {
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
    let vm = unsafe { vm(cx) };
    if !vm.set_field_cache_hit(code, object, i.imm(), r.get(i.a), i.c) {
        return exit(ip);
    }
    // Adding a property can allocate.
    note_allocation(vm);
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
            return lane_jump(cx, r, ip, BACKWARD, view, None);
        }
        next!(cx, r, following(ip), view)
    }
}

/// A taken jump. Backward jumps, the only ones that close a loop, are
/// safepoints: they leave the lane when a collection is due; forward jumps
/// dispatch directly. Every jump's direction is fixed when the view is
/// derived. Leaving hands the whole instruction to the general path, so
/// `commit`, the register write the instruction makes before its jump,
/// applies only when the lane takes the jump itself.
#[inline(always)]
fn lane_jump<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    backward: bool,
    view: Carried,
    commit: Option<(u16, Value)>,
) -> Ip {
    let jump = unsafe { (*ip).jump() };
    // SAFETY: the view records the distance to a validated jump target.
    let target = unsafe { ip.byte_offset(jump) };
    if backward && below_floor() {
        // Scalar arguments only, so the slow path stays a sibling call.
        return match commit {
            None => lane_safepoint::<H>(ip, target),
            Some((register, value)) => lane_safepoint_writing::<H>(r, ip, target, register, value),
        };
    }
    if let Some((register, value)) = commit {
        r.set(register, value);
    }
    next!(cx, r, target, view)
}

lane_handler! {
    fn lane_jump_always<BACKWARD: bool>(cx, r, ip, i, view) {
        lane_jump(cx, r, ip, BACKWARD, view, None)
    }
}

lane_handler! {
    fn lane_jump_false<BACKWARD: bool>(cx, r, ip, i, view) {
        let truthy = unsafe { vm(cx) }.truthy(r.get(i.a));
        if !truthy {
            return lane_jump(cx, r, ip, BACKWARD, view, None);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// Take the selected entry's jump; the view admits a table only when
    /// every entry is a lane jump.
    fn lane_branch_table<BACKWARD: bool>(cx, r, ip, i, view) {
        let case = r.get(i.a).wasm_bits32().min(i.imm());
        // SAFETY: validation places `imm + 1` entries after the table.
        let entry = unsafe { ip.add(1 + case as usize) };
        lane_jump(cx, r, entry, BACKWARD, view, None)
    }
}

lane_handler! {
    fn lane_bit_field<ACC: u8>(cx, r, ip, i, view) {
        let value = crate::wasm::integer::shift_right_unsigned_and(
            read_i32::<ACC, ACC_B>(r, i.b, view),
            i32::from(i.c as i16),
            i.imm() as i32,
        );
        produce!(cx, r, ip, i.a, view.integers.encode(value), view)
    }
}

lane_handler! {
    /// A counted loop's step and test.
    fn lane_add_jump_nonzero<BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let sum = read_i32::<ACC, ACC_B>(r, i.b, view).wrapping_add(i32::from(i.c as i16));
        if sum != 0 {
            return lane_jump(cx, r, ip, BACKWARD, view, Some((i.a, view.integers.encode(sum))));
        }
        // Falling through, the step is an accumulator producer.
        produce!(cx, r, ip, i.a, view.integers.encode(sum), view)
    }
}

lane_handler! {
    /// `WasmJumpI32Zero` and `WasmJumpI32NonZero`. With `CARRY`, the taken
    /// jump carries register `c` (a field the instruction leaves unused) as
    /// the accumulator, for a target that reads it there.
    fn lane_jump_i32_zero<WHEN_ZERO: bool, BACKWARD: bool, ACC: u8, CARRY: bool>(cx, r, ip, i, view) {
        if (read_i32::<ACC, ACC_A>(r, i.a, view) == 0) == WHEN_ZERO {
            let view = if CARRY { Carried { acc: r.get(i.c), ..view } } else { view };
            return lane_jump(cx, r, ip, BACKWARD, view, None);
        }
        next!(cx, r, following(ip), view)
    }
}

/// The zero-test jumps, indexed by whether they jump on zero, direction and
/// accumulator position; `CARRY` as in `lane_jump_i32_zero`.
struct ZeroJumps<H, const CARRY: bool>(std::marker::PhantomData<H>);

impl<H: Host, const CARRY: bool> ZeroJumps<H, CARRY> {
    const HANDLERS: [[[Handler; ACC_POSITIONS]; 2]; 2] = {
        macro_rules! positions {
            ($zero:literal, $backward:literal) => {
                [
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_NONE, CARRY>,
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_A, CARRY>,
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_B, CARRY>,
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_C, CARRY>,
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_PAIR, CARRY>,
                ]
            };
        }
        [
            [positions!(false, false), positions!(false, true)],
            [positions!(true, false), positions!(true, true)],
        ]
    };
}

lane_handler! {
    /// Fused i32 comparison and branch, against a register or a constant.
    fn lane_compare_jump<OP: u16, BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
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
            read_i32::<ACC, ACC_B>(r, i.b, view)
        };
        let Ok(taken) = comparison.evaluate(read_i32::<ACC, ACC_A>(r, i.a, view), right) else {
            return exit(ip);
        };
        if taken != 0 {
            return lane_jump(cx, r, ip, BACKWARD, view, None);
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
            let memory = resolve_memory(cx, value);
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
    fn lane_select<ACC: u8>(cx, r, ip, i, view) {
        let (condition, _) = ImmediateLayout::register_pair(i.imm());
        let condition = if ACC == ACC_PAIR {
            view.acc.wasm_bits32() as i32
        } else {
            r.i32(condition)
        };
        let source = if condition != 0 { i.b } else { i.c };
        produce!(cx, r, ip, i.a, r.get(source), view)
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
    fn lane_i32_unary<ACC: u8>(cx, r, ip, i, view) {
        let Some(operator) = I32UnaryOperator::from_tag(i.imm()) else {
            return exit(ip);
        };
        let crate::WasmValue::I32(value) = operator.apply(read_i32::<ACC, ACC_B>(r, i.b, view)) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, view.integers.encode(value), view)
    }
}

lane_handler! {
    /// An i32 operator over two registers or a register and a constant; a
    /// trap leaves the lane.
    fn lane_i32_binary<OP: u16, ACC: u8>(cx, r, ip, i, view) {
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
            read_i32::<ACC, ACC_C>(r, i.c, view)
        };
        let Ok(value) = operator.evaluate(read_i32::<ACC, ACC_B>(r, i.b, view), right) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, view.integers.encode(value), view)
    }
}

/// A scalar of static Wasm type `ty` read from a slot value.
#[inline(always)]
fn lane_scalar(value: Value, ty: crate::WasmType) -> Option<crate::WasmValue> {
    Some(match ty {
        crate::WasmType::I32 => crate::WasmValue::I32(value.as_int()?),
        crate::WasmType::F32 => crate::WasmValue::F32(value.as_int()? as u32),
        crate::WasmType::I64 => crate::WasmValue::I64(value.bits64()? as i64),
        crate::WasmType::F64 => crate::WasmValue::F64(value.bits64()?),
        _ => return None,
    })
}

/// A numeric scalar as a slot value.
#[inline(always)]
fn lane_encode(view: Carried, value: crate::WasmValue) -> Option<Value> {
    Some(match value {
        crate::WasmValue::I32(value) => view.integers.encode(value),
        crate::WasmValue::F32(bits) => view.integers.encode(bits as i32),
        crate::WasmValue::I64(value) => Value::from_bits64(value as u64)?,
        crate::WasmValue::F64(bits) => Value::from_bits64(bits)?,
        _ => return None,
    })
}

lane_handler! {
    /// An f32 or f64 operator: each width's selector applies its own
    /// semantics (`crate::wasm::float`).
    fn lane_float_binary<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let left = read::<ACC, ACC_B>(r, i.b, view);
        let right = read::<ACC, ACC_C>(r, i.c, view);
        let value = if const { matches!(opcode(OP), Op::WasmF64Binary) } {
            let (Some(left), Some(right), Some(operator)) = (
                left.bits64(),
                right.bits64(),
                F64BinaryOperator::from_tag(i.imm()),
            ) else {
                return exit(ip);
            };
            operator.apply(f64::from_bits(left), f64::from_bits(right))
        } else {
            let (Some(left), Some(right), Some(operator)) = (
                left.as_int(),
                right.as_int(),
                F32BinaryOperator::from_tag(i.imm()),
            ) else {
                return exit(ip);
            };
            operator.apply(f32::from_bits(left as u32), f32::from_bits(right as u32))
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

/// The float operators a lane record runs as one hardware operation or
/// comparison, by index: the operator is part of the handler, not a
/// selector it decodes.
const FLOAT_ADD: u8 = 0;
const FLOAT_SUBTRACT: u8 = 1;
const FLOAT_MULTIPLY: u8 = 2;
const FLOAT_DIVIDE: u8 = 3;
/// Comparisons, whose result is an i32 truth value.
const FLOAT_EQUAL: u8 = 4;
const FLOAT_NOT_EQUAL: u8 = 5;
const FLOAT_LESS: u8 = 6;
const FLOAT_GREATER: u8 = 7;
const FLOAT_LESS_EQUAL: u8 = 8;
const FLOAT_GREATER_EQUAL: u8 = 9;
const FLOAT_ARITHMETIC: usize = 10;

/// The arithmetic index of a float binary selector, when it has one.
fn float_arithmetic(op: Op, selector: u32) -> Option<u8> {
    Some(match op {
        Op::WasmF64Binary => match F64BinaryOperator::from_tag(selector)? {
            F64BinaryOperator::Add => FLOAT_ADD,
            F64BinaryOperator::Subtract => FLOAT_SUBTRACT,
            F64BinaryOperator::Multiply => FLOAT_MULTIPLY,
            F64BinaryOperator::Divide => FLOAT_DIVIDE,
            F64BinaryOperator::Equal => FLOAT_EQUAL,
            F64BinaryOperator::NotEqual => FLOAT_NOT_EQUAL,
            F64BinaryOperator::Less => FLOAT_LESS,
            F64BinaryOperator::Greater => FLOAT_GREATER,
            F64BinaryOperator::LessEqual => FLOAT_LESS_EQUAL,
            F64BinaryOperator::GreaterEqual => FLOAT_GREATER_EQUAL,
            _ => return None,
        },
        Op::WasmF32Binary => match F32BinaryOperator::from_tag(selector)? {
            F32BinaryOperator::Add => FLOAT_ADD,
            F32BinaryOperator::Subtract => FLOAT_SUBTRACT,
            F32BinaryOperator::Multiply => FLOAT_MULTIPLY,
            F32BinaryOperator::Divide => FLOAT_DIVIDE,
            F32BinaryOperator::Equal => FLOAT_EQUAL,
            F32BinaryOperator::NotEqual => FLOAT_NOT_EQUAL,
            F32BinaryOperator::Less => FLOAT_LESS,
            F32BinaryOperator::Greater => FLOAT_GREATER,
            F32BinaryOperator::LessEqual => FLOAT_LESS_EQUAL,
            F32BinaryOperator::GreaterEqual => FLOAT_GREATER_EQUAL,
            _ => return None,
        },
        _ => return None,
    })
}

lane_handler! {
    /// Float arithmetic `ARITH` of width f64 (`WIDE`) or f32, with the
    /// operators' own semantics (`crate::wasm::float`): the hardware result.
    fn lane_float_arithmetic<WIDE: bool, ARITH: u8, ACC: u8>(cx, r, ip, i, view) {
        let left = read::<ACC, ACC_B>(r, i.b, view);
        let right = read::<ACC, ACC_C>(r, i.c, view);
        /// The arithmetic result, or a comparison's truth value.
        enum Outcome<F> {
            Float(F),
            Truth(bool),
        }
        macro_rules! apply {
            ($left:expr, $right:expr) => {{
                let (left, right) = ($left, $right);
                match ARITH {
                    FLOAT_ADD => Outcome::Float(left + right),
                    FLOAT_SUBTRACT => Outcome::Float(left - right),
                    FLOAT_MULTIPLY => Outcome::Float(left * right),
                    FLOAT_DIVIDE => Outcome::Float(left / right),
                    FLOAT_EQUAL => Outcome::Truth(left == right),
                    FLOAT_NOT_EQUAL => Outcome::Truth(left != right),
                    FLOAT_LESS => Outcome::Truth(left < right),
                    FLOAT_GREATER => Outcome::Truth(left > right),
                    FLOAT_LESS_EQUAL => Outcome::Truth(left <= right),
                    _ => Outcome::Truth(left >= right),
                }
            }};
        }
        // Registers hold their static Wasm types: an f32 slot an encoded
        // i32, an f64 slot its bits in place or a box whose bits read as a
        // NaN, which these operators treat as any other NaN.
        let value = if WIDE {
            let left = f64::from_bits(left.float_bits64());
            let right = f64::from_bits(right.float_bits64());
            match apply!(left, right) {
                Outcome::Float(result) => Value::from_float_bits64(result.to_bits()),
                Outcome::Truth(truth) => view.integers.encode(i32::from(truth)),
            }
        } else {
            let left = f32::from_bits(left.wasm_bits32());
            let right = f32::from_bits(right.wasm_bits32());
            match apply!(left, right) {
                Outcome::Float(result) => view.integers.encode(result.to_bits() as i32),
                Outcome::Truth(truth) => view.integers.encode(i32::from(truth)),
            }
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

/// The f64 operator `ARITH` over two operands, as `lane_float_arithmetic`
/// applies it: the hardware result, or a comparison's truth value.
#[inline(always)]
fn f64_operation(arith: u8, left: f64, right: f64) -> Result<f64, bool> {
    match arith {
        FLOAT_ADD => Ok(left + right),
        FLOAT_SUBTRACT => Ok(left - right),
        FLOAT_MULTIPLY => Ok(left * right),
        FLOAT_DIVIDE => Ok(left / right),
        FLOAT_EQUAL => Err(left == right),
        FLOAT_NOT_EQUAL => Err(left != right),
        FLOAT_LESS => Err(left < right),
        FLOAT_GREATER => Err(left > right),
        FLOAT_LESS_EQUAL => Err(left <= right),
        _ => Err(left >= right),
    }
}

/// Which operand of the second fused float record the first one's result
/// is: its left (`b`) or right (`c`) register.
const FUSED_LEFT: bool = true;

lane_handler! {
    /// An f64 arithmetic `FIRST` whose result the next record, f64 operator
    /// `SECOND`, takes as its left (`LEFT`) or right operand: both records
    /// run here, each result written as its own record writes it.
    fn lane_f64_pair<FIRST: u8, SECOND: u8, LEFT: bool, ACC: u8>(cx, r, ip, i, view) {
        let first = match f64_operation(
            FIRST,
            f64::from_bits(read::<ACC, ACC_B>(r, i.b, view).float_bits64()),
            f64::from_bits(read::<ACC, ACC_C>(r, i.c, view).float_bits64()),
        ) {
            Ok(result) => result,
            Err(_) => unreachable!("a fused first operator is arithmetic"),
        };
        r.set(i.a, Value::from_float_bits64(first.to_bits()));
        let next = following(ip);
        // SAFETY: the view fuses a float record only with a following one.
        let second = unsafe { &*next };
        let (left, right) = if LEFT {
            (first, f64::from_bits(r.get(second.c).float_bits64()))
        } else {
            (f64::from_bits(r.get(second.b).float_bits64()), first)
        };
        let value = match f64_operation(SECOND, left, right) {
            Ok(result) => Value::from_float_bits64(result.to_bits()),
            Err(truth) => view.integers.encode(i32::from(truth)),
        };
        produce!(cx, r, next, second.a, value, view)
    }
}

lane_handler! {
    /// An f64 load whose value the next record, f64 operator `SECOND`,
    /// takes as its left (`LEFT`) or right operand: both records run here.
    /// Bits that need a box leave the lane at the load.
    fn lane_f64_load_pair<SECOND: u8, LEFT: bool>(cx, r, ip, i, view) {
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::F64(bits)) = (unsafe { view.memory.read(MemoryLoad::F64Load, lane_address::<ACC_NONE>(r, i, view)) })
        else {
            return exit(ip);
        };
        let Some(loaded) = Value::from_bits64(bits) else {
            return exit(ip);
        };
        r.set(i.a, loaded);
        let next = following(ip);
        // SAFETY: the view fuses a load only with a following record.
        let second = unsafe { &*next };
        let first = f64::from_bits(bits);
        let (left, right) = if LEFT {
            (first, f64::from_bits(r.get(second.c).float_bits64()))
        } else {
            (f64::from_bits(r.get(second.b).float_bits64()), first)
        };
        let value = match f64_operation(SECOND, left, right) {
            Ok(result) => Value::from_float_bits64(result.to_bits()),
            Err(truth) => view.integers.encode(i32::from(truth)),
        };
        produce!(cx, r, next, second.a, value, view)
    }
}

lane_handler! {
    /// An f64 load, the f64 arithmetic `ARITH` taking the loaded value as
    /// its left (`LEFT`) or right operand, and the f64 store writing the
    /// arithmetic's result (`mem[x] += v`): three records run here, each
    /// result written as its own record writes it. An access the general
    /// path must run exits at its record.
    fn lane_f64_update<ARITH: u8, LEFT: bool>(cx, r, ip, i, view) {
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::F64(bits)) = (unsafe {
            view.memory.read(MemoryLoad::F64Load, lane_address::<ACC_NONE>(r, i, view))
        }) else {
            return exit(ip);
        };
        let Some(loaded) = Value::from_bits64(bits) else {
            return exit(ip);
        };
        r.set(i.a, loaded);
        let operator = following(ip);
        // SAFETY: the view fuses a load only with a following operator and
        // store.
        let o = unsafe { &*operator };
        let first = f64::from_bits(bits);
        let (left, right) = if LEFT {
            (first, f64::from_bits(r.get(o.c).float_bits64()))
        } else {
            (f64::from_bits(r.get(o.b).float_bits64()), first)
        };
        let Ok(result) = f64_operation(ARITH, left, right) else {
            unreachable!("a fused update's operator is arithmetic");
        };
        let value = Value::from_float_bits64(result.to_bits());
        r.set(o.a, value);
        let store = following(operator);
        // SAFETY: as above.
        let access = unsafe { &*store };
        // SAFETY: as for `lane_store`.
        if unsafe {
            view.memory.write(
                MemoryStore::F64Store,
                lane_address::<ACC_NONE>(r, access, view),
                crate::WasmValue::F64(value.float_bits64()),
            )
        }
        .is_none()
        {
            return exit(store);
        }
        next!(cx, r, following(store), view)
    }
}

lane_handler! {
    /// An f64 arithmetic `ARITH` whose result the next record, an f64
    /// store, writes to memory: both records run here. A store the general
    /// path must run exits at its record, the result already written.
    fn lane_f64_store_pair<ARITH: u8, ACC: u8>(cx, r, ip, i, view) {
        let result = match f64_operation(
            ARITH,
            f64::from_bits(read::<ACC, ACC_B>(r, i.b, view).float_bits64()),
            f64::from_bits(read::<ACC, ACC_C>(r, i.c, view).float_bits64()),
        ) {
            Ok(result) => result,
            Err(_) => unreachable!("a fused store's operator is arithmetic"),
        };
        let value = Value::from_float_bits64(result.to_bits());
        r.set(i.a, value);
        let next = following(ip);
        // SAFETY: the view fuses an operator only with a following store.
        let store = unsafe { &*next };
        let bits = value.float_bits64();
        // SAFETY: as for `lane_store`.
        if unsafe {
            view.memory.write(
                MemoryStore::F64Store,
                lane_address::<ACC_NONE>(r, store, view),
                crate::WasmValue::F64(bits),
            )
        }
        .is_none()
        {
            return exit(next);
        }
        next!(cx, r, following(next), view)
    }
}

/// The accumulator fields a fused first float record may read: none, its
/// left operand, or its right operand.
const FUSED_ACC_FIELDS: [u8; 3] = [ACC_NONE, ACC_B, ACC_C];

macro_rules! float_store_pair_handlers {
    ($host:ty, $arith:expr) => {
        [
            lane_f64_store_pair::<$host, $arith, ACC_NONE> as Handler,
            lane_f64_store_pair::<$host, $arith, ACC_B> as Handler,
            lane_f64_store_pair::<$host, $arith, ACC_C> as Handler,
        ]
    };
}

macro_rules! float_load_pair_handlers {
    ($host:ty; $($second:expr),+) => {
        [$([
            lane_f64_load_pair::<$host, $second, false> as Handler,
            lane_f64_load_pair::<$host, $second, FUSED_LEFT> as Handler,
        ]),+]
    };
}

struct FloatPairs<H>(std::marker::PhantomData<H>);

macro_rules! float_pair_handlers {
    ($host:ty) => {
        [
            float_pair_handlers!($host, FLOAT_ADD),
            float_pair_handlers!($host, FLOAT_SUBTRACT),
            float_pair_handlers!($host, FLOAT_MULTIPLY),
            float_pair_handlers!($host, FLOAT_DIVIDE),
        ]
    };
    ($host:ty, $first:expr) => {
        [
            float_pair_handlers!($host, $first, FLOAT_ADD),
            float_pair_handlers!($host, $first, FLOAT_SUBTRACT),
            float_pair_handlers!($host, $first, FLOAT_MULTIPLY),
            float_pair_handlers!($host, $first, FLOAT_DIVIDE),
            float_pair_handlers!($host, $first, FLOAT_EQUAL),
            float_pair_handlers!($host, $first, FLOAT_NOT_EQUAL),
            float_pair_handlers!($host, $first, FLOAT_LESS),
            float_pair_handlers!($host, $first, FLOAT_GREATER),
            float_pair_handlers!($host, $first, FLOAT_LESS_EQUAL),
            float_pair_handlers!($host, $first, FLOAT_GREATER_EQUAL),
        ]
    };
    ($host:ty, $first:expr, $second:expr) => {
        [
            float_pair_handlers!($host, $first, $second, false),
            float_pair_handlers!($host, $first, $second, FUSED_LEFT),
        ]
    };
    ($host:ty, $first:expr, $second:expr, $left:expr) => {
        [
            lane_f64_pair::<$host, $first, $second, $left, ACC_NONE> as Handler,
            lane_f64_pair::<$host, $first, $second, $left, ACC_B> as Handler,
            lane_f64_pair::<$host, $first, $second, $left, ACC_C> as Handler,
        ]
    };
}

/// The f64 arithmetic operators a fused pair can begin with: add,
/// subtract, multiply and divide, whose results are floats.
const FLOAT_PAIR_FIRSTS: usize = 4;

impl<H: Host> FloatPairs<H> {
    /// By first operator, second operator, whether the first result is the
    /// second's left operand, and the first's accumulator field.
    const HANDLERS: [[[[Handler; FUSED_ACC_FIELDS.len()]; 2]; FLOAT_ARITHMETIC];
        FLOAT_PAIR_FIRSTS] = float_pair_handlers!(H);
    /// An f64 load fused with the operator taking its value, by second
    /// operator and whether the value is its left operand.
    /// An f64 arithmetic fused with the store of its result, by operator.
    /// By operator and the operator's accumulator field (none, `b`, `c`).
    const STORES: [[Handler; FUSED_ACC_FIELDS.len()]; FLOAT_PAIR_FIRSTS] = [
        float_store_pair_handlers!(H, FLOAT_ADD),
        float_store_pair_handlers!(H, FLOAT_SUBTRACT),
        float_store_pair_handlers!(H, FLOAT_MULTIPLY),
        float_store_pair_handlers!(H, FLOAT_DIVIDE),
    ];
    /// An f64 load, arithmetic and store of its result, by operator and
    /// whether the loaded value is the left operand.
    const UPDATES: [[Handler; 2]; FLOAT_PAIR_FIRSTS] = [
        [
            lane_f64_update::<H, FLOAT_ADD, false>,
            lane_f64_update::<H, FLOAT_ADD, FUSED_LEFT>,
        ],
        [
            lane_f64_update::<H, FLOAT_SUBTRACT, false>,
            lane_f64_update::<H, FLOAT_SUBTRACT, FUSED_LEFT>,
        ],
        [
            lane_f64_update::<H, FLOAT_MULTIPLY, false>,
            lane_f64_update::<H, FLOAT_MULTIPLY, FUSED_LEFT>,
        ],
        [
            lane_f64_update::<H, FLOAT_DIVIDE, false>,
            lane_f64_update::<H, FLOAT_DIVIDE, FUSED_LEFT>,
        ],
    ];
    const LOADS: [[Handler; 2]; FLOAT_ARITHMETIC] = float_load_pair_handlers!(H;
        FLOAT_ADD, FLOAT_SUBTRACT, FLOAT_MULTIPLY, FLOAT_DIVIDE, FLOAT_EQUAL,
        FLOAT_NOT_EQUAL, FLOAT_LESS, FLOAT_GREATER, FLOAT_LESS_EQUAL, FLOAT_GREATER_EQUAL);
}

struct FloatArithmetic<H>(std::marker::PhantomData<H>);

macro_rules! float_arithmetic_handlers {
    ($host:ty, $wide:expr) => {
        [
            float_arithmetic_handlers!($host, $wide, FLOAT_ADD),
            float_arithmetic_handlers!($host, $wide, FLOAT_SUBTRACT),
            float_arithmetic_handlers!($host, $wide, FLOAT_MULTIPLY),
            float_arithmetic_handlers!($host, $wide, FLOAT_DIVIDE),
            float_arithmetic_handlers!($host, $wide, FLOAT_EQUAL),
            float_arithmetic_handlers!($host, $wide, FLOAT_NOT_EQUAL),
            float_arithmetic_handlers!($host, $wide, FLOAT_LESS),
            float_arithmetic_handlers!($host, $wide, FLOAT_GREATER),
            float_arithmetic_handlers!($host, $wide, FLOAT_LESS_EQUAL),
            float_arithmetic_handlers!($host, $wide, FLOAT_GREATER_EQUAL),
        ]
    };
    ($host:ty, $wide:expr, $arith:expr) => {
        [
            lane_float_arithmetic::<$host, $wide, $arith, ACC_NONE> as Handler,
            lane_float_arithmetic::<$host, $wide, $arith, ACC_A> as Handler,
            lane_float_arithmetic::<$host, $wide, $arith, ACC_B> as Handler,
            lane_float_arithmetic::<$host, $wide, $arith, ACC_C> as Handler,
            lane_float_arithmetic::<$host, $wide, $arith, ACC_PAIR> as Handler,
        ]
    };
}

impl<H: Host> FloatArithmetic<H> {
    /// By width (f32, then f64), arithmetic index and accumulator field.
    const HANDLERS: [[[Handler; ACC_POSITIONS]; FLOAT_ARITHMETIC]; 2] = [
        float_arithmetic_handlers!(H, false),
        float_arithmetic_handlers!(H, true),
    ];
}

lane_handler! {
    /// f64 square root: an arithmetic operator, so a NaN operand matters
    /// only as a NaN (see `float_bits64`), and its result is held in place.
    fn lane_f64_sqrt<ACC: u8>(cx, r, ip, i, view) {
        let input = f64::from_bits(read::<ACC, ACC_B>(r, i.b, view).float_bits64());
        produce!(cx, r, ip, i.a, Value::from_float_bits64(input.sqrt().to_bits()), view)
    }
}

lane_handler! {
    fn lane_float_unary<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let input = read::<ACC, ACC_B>(r, i.b, view);
        let value = if const { matches!(opcode(OP), Op::WasmF64Unary) } {
            let (Some(input), Some(operator)) =
                (input.bits64(), F64UnaryOperator::from_tag(i.imm()))
            else {
                return exit(ip);
            };
            operator.apply(f64::from_bits(input))
        } else {
            let (Some(input), Some(operator)) =
                (input.as_int(), F32UnaryOperator::from_tag(i.imm()))
            else {
                return exit(ip);
            };
            operator.apply(f32::from_bits(input as u32))
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    /// An i64 operator; a trap leaves the lane, where the general loop
    /// raises it.
    fn lane_i64_binary<ACC: u8>(cx, r, ip, i, view) {
        let (Some(left), Some(right), Some(operator)) = (
            read::<ACC, ACC_B>(r, i.b, view).bits64(),
            read::<ACC, ACC_C>(r, i.c, view).bits64(),
            I64BinaryOperator::from_tag(i.imm()),
        ) else {
            return exit(ip);
        };
        let Ok(value) = operator.apply(left as i64, right as i64) else {
            return exit(ip);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    fn lane_i64_unary<ACC: u8>(cx, r, ip, i, view) {
        let (Some(input), Some(operator)) = (
            read::<ACC, ACC_B>(r, i.b, view).bits64(),
            I64UnaryOperator::from_tag(i.imm()),
        ) else {
            return exit(ip);
        };
        let Some(value) = lane_encode(view, operator.apply(input as i64)) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    /// A scalar conversion; a trapping truncation leaves the lane.
    fn lane_scalar_convert<ACC: u8>(cx, r, ip, i, view) {
        let Some(operator) = ScalarConversionOperator::from_tag(i.imm()) else {
            return exit(ip);
        };
        let Some(input) = lane_scalar(read::<ACC, ACC_B>(r, i.b, view), operator.source_type())
        else {
            return exit(ip);
        };
        let Ok(value) = operator.apply(input) else {
            return exit(ip);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

/// The memory32 effective address of a direct access through the function's
/// memory register.
#[inline(always)]
fn lane_address<const ACC: u8>(r: Registers, i: &LaneInstruction, view: Carried) -> u64 {
    u64::from(read::<ACC, ACC_C>(r, i.c, view).wasm_bits32()) + u64::from(i.imm())
}

/// The view of the unshared memory `binding` names, or the empty view.
/// The bytes and length of the unshared memory `binding` names, when the
/// lane or compiled code can access it directly.
pub(super) fn unshared_memory<H: Host>(vm: &Vm<H>, binding: Value) -> Option<(*mut u8, usize)> {
    let view = memory_view(vm, binding);
    (!view.bytes.is_null()).then_some((view.bytes, view.len()))
}

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
    MemoryView::new(state.bytes.as_mut_ptr(), state.bytes.len())
}

/// The view of memory binding `binding`, reusing the run's last resolution.
#[inline(always)]
fn resolve_memory<H: Host>(cx: *mut LaneContext<H>, binding: Value) -> MemoryView {
    // SAFETY: handlers run with exclusive access to their context.
    let (resolved, view) = unsafe { (*cx).resolved };
    if resolved == binding {
        return view;
    }
    let view = memory_view(unsafe { vm(cx) }, binding);
    unsafe { (*cx).resolved = (binding, view) };
    view
}

/// The view behind `view`'s memory register in `registers`.
fn frame_memory_view<H: Host>(
    cx: *mut LaneContext<H>,
    view: &LaneView,
    registers: Registers,
) -> MemoryView {
    view.memory_register.map_or(MemoryView::NONE, |register| {
        resolve_memory(cx, registers.get(register))
    })
}

/// A taken safepoint jump below the stack floor: leave the lane when a
/// collection is due, otherwise dispatch the target from the lane loop,
/// which unwinds the stack. Out of line and reached by a sibling call, so a
/// safepoint's fast path neither loads the context nor saves registers.
#[cold]
#[inline(never)]
fn lane_safepoint<H: Host>(ip: Ip, target: Ip) -> Ip {
    // SAFETY: called only while a lane runs, from one of its handlers.
    if unsafe { vm(lane_context::<H>()) }.heap.should_collect() {
        return exit(ip);
    }
    target
}

/// `lane_safepoint` for a jump that writes `register` when taken.
#[cold]
#[inline(never)]
fn lane_safepoint_writing<H: Host>(
    r: Registers,
    ip: Ip,
    target: Ip,
    register: u16,
    value: Value,
) -> Ip {
    let next = lane_safepoint::<H>(ip, target);
    if next == target {
        r.set(register, value);
    }
    next
}

// The lane holds 64-bit payloads only in place (`Value::from_bits64`): a
// boxed operand or a result that needs a box leaves the lane, and the
// general loop, which boxes, runs the instruction. The handlers then make
// no calls and keep their sibling-call shape.

lane_handler! {
    fn lane_load<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let load = const {
            match MemoryLoad::from_direct_op(opcode(OP)) {
                Some(load) => load,
                None => panic!("not a direct load"),
            }
        };
        // SAFETY: the view admits direct accesses only through the memory
        // register, whose bytes `view` holds; see MemoryView.
        let Some(value) = (unsafe { view.memory.read(load, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    /// An i32 shift left by a constant, reading its operand through field
    /// `ACC`, whose result the next record, an i32 add, takes as its left
    /// (`LEFT`) or right operand: a scaled index. Both records run here,
    /// each result written as its own record writes it.
    fn lane_shift_add<ACC: u8, LEFT: bool>(cx, r, ip, i, view) {
        let Ok(shifted) = I32BinaryOperator::ShiftLeft
            .evaluate(read_i32::<ACC, ACC_B>(r, i.b, view), i.imm() as i32)
        else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(shifted));
        let next = following(ip);
        // SAFETY: the view fuses a shift only with a following add record.
        let add = unsafe { &*next };
        let other = r.i32(if LEFT { add.c } else { add.b });
        let (left, right) = if LEFT { (shifted, other) } else { (other, shifted) };
        let Ok(sum) = I32BinaryOperator::Add.evaluate(left, right) else {
            return exit(next);
        };
        produce!(cx, r, next, add.a, view.integers.encode(sum), view)
    }
}

lane_handler! {
    /// A direct i32 load `OP` whose value the next record, a branch table,
    /// selects on: both records run here, the value written as the load
    /// writes it. With `ADD`, the record is a plain add of a constant ahead
    /// of the load (`switch (*pc++)`), and the three run here, each result
    /// written as its own record writes it. A load the general path must
    /// run exits at its record.
    fn lane_load_table<OP: u16, ACC: u8, ADD: bool>(cx, r, ip, i, view) {
        let load = const {
            match MemoryLoad::from_direct_op(opcode(OP)) {
                Some(load) => load,
                None => panic!("not a direct load"),
            }
        };
        // With `ADD` and `ACC_C`, the accumulator holds the register both
        // the add and the load read.
        let ip = if ADD {
            let base = read_i32::<ACC, ACC_C>(r, i.b, view);
            r.set(i.a, view.integers.encode(base.wrapping_add(i.imm() as i32)));
            following(ip)
        } else {
            ip
        };
        // SAFETY: the view fuses an add only with a following load record.
        let i = unsafe { &*ip };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(selector)) =
            (unsafe { view.memory.read(load, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(selector));
        let table = following(ip);
        // SAFETY: the view fuses a load only with a following table record.
        let case = (selector as u32).min(unsafe { (*table).imm() });
        // SAFETY: validation places `imm + 1` entries after the table.
        let entry = unsafe { table.add(1 + case as usize) };
        lane_jump(cx, r, entry, false, view, None)
    }
}

lane_handler! {
    /// An i32 add of a constant whose sum the next record, direct load `OP`,
    /// reads as its address: both records run here, the sum written as the
    /// add writes it. A load the general path must run exits at its record.
    fn lane_add_load<OP: u16>(cx, r, ip, i, view) {
        let load = const {
            match MemoryLoad::from_direct_op(opcode(OP)) {
                Some(load) => load,
                None => panic!("not a direct load"),
            }
        };
        let sum = r.i32(i.b).wrapping_add(i.imm() as i32);
        r.set(i.a, view.integers.encode(sum));
        let next = following(ip);
        // SAFETY: the view fuses an add only with a following load record.
        let access = unsafe { &*next };
        let address = u64::from(sum as u32) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(load, address) }) else {
            return exit(next);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(next);
        };
        produce!(cx, r, next, access.a, value, view)
    }
}

lane_handler! {
    /// A jump absorbing the move (`ADD` false) or i32 add of the constant
    /// `c` (`ADD` true) at its target: the record holds the target's fields
    /// and jumps past it, carrying the result as the target would.
    fn lane_jump_through<ADD: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        let value = if ADD {
            view.integers.encode(r.i32(i.b).wrapping_add(i32::from(i.c as i16)))
        } else {
            r.get(i.b)
        };
        // A safepoint exit resumes at the jump, which runs the target
        // again, so the result is written only when the jump is taken.
        lane_jump(cx, r, ip, BACKWARD, Carried { acc: value, ..view }, Some((i.a, value)))
    }
}

lane_handler! {
    /// Two moves in a row (a parallel copy resolved in order): both
    /// records run here, the second's value carried as its record carries
    /// it. With `JUMP`, the unconditional jump after them runs here too, in
    /// direction `BACKWARD`.
    fn lane_move_pair<JUMP: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        r.set(i.a, r.get(i.b));
        let next = following(ip);
        // SAFETY: the view fuses a move only with a following move, and
        // with `JUMP` a jump after it.
        let second = unsafe { &*next };
        let value = r.get(second.b);
        if JUMP {
            r.set(second.a, value);
            let view = Carried { acc: value, ..view };
            return lane_jump(cx, r, following(next), BACKWARD, view, None);
        }
        produce!(cx, r, next, second.a, value, view)
    }
}

lane_handler! {
    /// A move (`ADD` false) or an i32 add of a constant (`ADD` true) whose
    /// next record is an unconditional jump, as at a loop latch: both
    /// records run here, the result written as its own record writes it.
    fn lane_then_jump<ADD: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        let value = if ADD {
            view.integers.encode(r.i32(i.b).wrapping_add(i.imm() as i32))
        } else {
            r.get(i.b)
        };
        r.set(i.a, value);
        lane_jump(cx, r, following(ip), BACKWARD, Carried { acc: value, ..view }, None)
    }
}

/// The most records before a record that one fused record can run through:
/// an f64 load, arithmetic and store (`FloatPairs::UPDATES`) start two
/// records ahead of the last they run.
const FUSION_REACH: usize = 2;

/// The handlers fusing a move or an add of a constant with a jump, indexed
/// by whether the instruction adds and by the jump's direction.
struct JumpFusions<H>(std::marker::PhantomData<H>);

impl<H: Host> JumpFusions<H> {
    /// A jump absorbing the instruction at its target.
    const THROUGH: [[Handler; 2]; 2] = [
        [
            lane_jump_through::<H, false, false>,
            lane_jump_through::<H, false, true>,
        ],
        [
            lane_jump_through::<H, true, false>,
            lane_jump_through::<H, true, true>,
        ],
    ];
    /// The instruction followed by its next record's jump.
    const THEN: [[Handler; 2]; 2] = [
        [
            lane_then_jump::<H, false, false>,
            lane_then_jump::<H, false, true>,
        ],
        [
            lane_then_jump::<H, true, false>,
            lane_then_jump::<H, true, true>,
        ],
    ];
}

/// The scaled-index handlers, indexed by the shift's operand field (none,
/// `b`) and by whether the add takes the shifted value on its left.
struct ScaledIndex<H>(std::marker::PhantomData<H>);

impl<H: Host> ScaledIndex<H> {
    const HANDLERS: [[Handler; 2]; 2] = [
        [
            lane_shift_add::<H, ACC_NONE, false>,
            lane_shift_add::<H, ACC_NONE, true>,
        ],
        [
            lane_shift_add::<H, ACC_B, false>,
            lane_shift_add::<H, ACC_B, true>,
        ],
    ];
}

lane_handler! {
    /// An i32 add of a constant, the f64 load reading its sum, and the f64
    /// operator `SECOND` taking the loaded value as its left (`LEFT`) or
    /// right operand: three records run here, each result written as its
    /// own record writes it. Bits that need a box leave at the load.
    fn lane_add_load_f64<SECOND: u8, LEFT: bool>(cx, r, ip, i, view) {
        let sum = r.i32(i.b).wrapping_add(i.imm() as i32);
        r.set(i.a, view.integers.encode(sum));
        let load = following(ip);
        // SAFETY: the view fuses an add only with a following load and
        // float operator.
        let access = unsafe { &*load };
        let address = u64::from(sum as u32) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::F64(bits)) =
            (unsafe { view.memory.read(MemoryLoad::F64Load, address) })
        else {
            return exit(load);
        };
        let Some(loaded) = Value::from_bits64(bits) else {
            return exit(load);
        };
        r.set(access.a, loaded);
        let next = following(load);
        let second = unsafe { &*next };
        let first = f64::from_bits(bits);
        let (left, right) = if LEFT {
            (first, f64::from_bits(r.get(second.c).float_bits64()))
        } else {
            (f64::from_bits(r.get(second.b).float_bits64()), first)
        };
        let value = match f64_operation(SECOND, left, right) {
            Ok(result) => Value::from_float_bits64(result.to_bits()),
            Err(truth) => view.integers.encode(i32::from(truth)),
        };
        produce!(cx, r, next, second.a, value, view)
    }
}

macro_rules! add_load_f64_handlers {
    ($host:ty; $($second:expr),+) => {
        [$([
            lane_add_load_f64::<$host, $second, false> as Handler,
            lane_add_load_f64::<$host, $second, FUSED_LEFT> as Handler,
        ]),+]
    };
}

struct AddLoadF64<H>(std::marker::PhantomData<H>);

impl<H: Host> AddLoadF64<H> {
    /// By the float operator and whether the loaded value is its left
    /// operand.
    const HANDLERS: [[Handler; 2]; FLOAT_ARITHMETIC] = add_load_f64_handlers!(H;
        FLOAT_ADD, FLOAT_SUBTRACT, FLOAT_MULTIPLY, FLOAT_DIVIDE, FLOAT_EQUAL,
        FLOAT_NOT_EQUAL, FLOAT_LESS, FLOAT_GREATER, FLOAT_LESS_EQUAL, FLOAT_GREATER_EQUAL);
}

/// The fused add-and-load handler for direct load `op`.
/// The fused handler for direct load `load`, with address field `acc`, and
/// direct store `store` writing its value: copies of one value type.
fn load_store_handler<H: Host>(load: Op, store: Op, acc: u8) -> Option<Handler> {
    macro_rules! copies {
        ($(($load:ident, $store:ident)),+) => {
            match (load, store, acc) {
                $(
                    (Op::$load, Op::$store, ACC_NONE) => Some(lane_load_store::<H, { Op::$load as u16 }, { Op::$store as u16 }, ACC_NONE> as Handler),
                    (Op::$load, Op::$store, ACC_C) => Some(lane_load_store::<H, { Op::$load as u16 }, { Op::$store as u16 }, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    copies!(
        (WasmI32Load, WasmI32Store),
        (WasmI32Load8U, WasmI32Store8),
        (WasmI64Load, WasmI64Store)
    )
}

/// The fused handler for i32 load `op`, with address field `acc`, the
/// branch table selecting on its value, and with `add` the plain add of a
/// constant ahead of the load.
fn load_table_handler<H: Host>(op: Op, acc: u8, add: bool) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match (op, acc, add) {
                $(
                    (Op::$load, ACC_NONE, false) => Some(lane_load_table::<H, { Op::$load as u16 }, ACC_NONE, false> as Handler),
                    (Op::$load, ACC_C, false) => Some(lane_load_table::<H, { Op::$load as u16 }, ACC_C, false> as Handler),
                    (Op::$load, ACC_NONE, true) => Some(lane_load_table::<H, { Op::$load as u16 }, ACC_NONE, true> as Handler),
                    (Op::$load, ACC_C, true) => Some(lane_load_table::<H, { Op::$load as u16 }, ACC_C, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    loads!(
        WasmI32Load,
        WasmI32Load8S,
        WasmI32Load8U,
        WasmI32Load16S,
        WasmI32Load16U
    )
}

const fn add_load_handler<H: Host>(op: Op) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match op {
                $(Op::$load => Some(lane_add_load::<H, { Op::$load as u16 }> as Handler),)+
                _ => None,
            }
        };
    }
    loads!(
        WasmI32Load,
        WasmI64Load,
        WasmF32Load,
        WasmF64Load,
        WasmI32Load8S,
        WasmI32Load8U,
        WasmI32Load16S,
        WasmI32Load16U,
        WasmI64Load8S,
        WasmI64Load8U,
        WasmI64Load16S,
        WasmI64Load16U,
        WasmI64Load32S,
        WasmI64Load32U
    )
}

lane_handler! {
    /// A direct load `LOAD`, with address field `ACC`, whose value the next
    /// record, direct store `STORE`, writes: both records run here, the value
    /// written to its register as the load writes it. An access the general
    /// path must run exits at its record.
    fn lane_load_store<LOAD: u16, STORE: u16, ACC: u8>(cx, r, ip, i, view) {
        let (load, store) = const {
            match (
                MemoryLoad::from_direct_op(opcode(LOAD)),
                MemoryStore::from_direct_op(opcode(STORE)),
            ) {
                (Some(load), Some(store)) => (load, store),
                _ => panic!("not a direct load and store"),
            }
        };
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(load, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        let Some(encoded) = lane_encode(view, value) else {
            return exit(ip);
        };
        r.set(i.a, encoded);
        let next = following(ip);
        // SAFETY: the view fuses a load only with a following store record.
        let access = unsafe { &*next };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC_NONE>(r, access, view), value) }
            .is_none()
        {
            return exit(next);
        }
        next!(cx, r, following(next), view)
    }
}

/// The scalar direct store `store` writes for slot value `value`: registers
/// hold their static Wasm types, and a value whose bits need a box has none.
#[inline(always)]
fn stored_value(store: MemoryStore, value: Value) -> Option<crate::WasmValue> {
    Some(match store.value_type() {
        crate::WasmType::I32 => crate::WasmValue::I32(value.wasm_bits32() as i32),
        crate::WasmType::F32 => crate::WasmValue::F32(value.as_int()? as u32),
        crate::WasmType::I64 => crate::WasmValue::I64(value.bits64()? as i64),
        crate::WasmType::F64 => crate::WasmValue::F64(value.bits64()?),
        _ => return None,
    })
}

lane_handler! {
    fn lane_store<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        let Some(value) = stored_value(store, read::<ACC, ACC_A>(r, i.a, view)) else {
            return exit(ip);
        };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC>(r, i, view), value) }
            .is_none()
        {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// An i32 load, with address field `ACC`, the i32 operator `OP` taking
    /// the loaded value as its left (`LEFT`) or right operand, and the i32
    /// store writing the operator's result (`mem[x] op= v`): three records
    /// run here, each result written as its own record writes it. An access
    /// the general path must run, or a trapping operator, exits at its
    /// record.
    fn lane_i32_update<OP: u16, LEFT: bool, ACC: u8>(cx, r, ip, i, view) {
        let operator = const {
            match I32BinaryOperator::from_register_op(opcode(OP)) {
                Some(operator) => operator,
                None => panic!("not an i32 register operator"),
            }
        };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(loaded)) = (unsafe {
            view.memory.read(MemoryLoad::I32Load, lane_address::<ACC>(r, i, view))
        }) else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(loaded));
        let combine = following(ip);
        // SAFETY: the view fuses a load only with a following operator and
        // store.
        let o = unsafe { &*combine };
        let (left, right) = if LEFT { (loaded, r.i32(o.c)) } else { (r.i32(o.b), loaded) };
        let Ok(result) = operator.evaluate(left, right) else {
            return exit(combine);
        };
        r.set(o.a, view.integers.encode(result));
        let store = following(combine);
        // SAFETY: as above.
        let access = unsafe { &*store };
        // SAFETY: as for `lane_store`.
        if unsafe {
            view.memory.write(
                MemoryStore::I32Store,
                lane_address::<ACC_NONE>(r, access, view),
                crate::WasmValue::I32(result),
            )
        }
        .is_none()
        {
            return exit(store);
        }
        next!(cx, r, following(store), view)
    }
}

/// The i32 update handler for register operator `op`, loaded value side
/// `left` and load address field `acc`.
fn i32_update_handler<H: Host>(op: Op, left: bool, acc: u8) -> Option<Handler> {
    macro_rules! updates {
        ($($op:ident),+) => {
            match (op, left, acc) {
                $(
                    (Op::$op, false, ACC_NONE) => Some(lane_i32_update::<H, { Op::$op as u16 }, false, ACC_NONE> as Handler),
                    (Op::$op, true, ACC_NONE) => Some(lane_i32_update::<H, { Op::$op as u16 }, true, ACC_NONE> as Handler),
                    (Op::$op, false, ACC_C) => Some(lane_i32_update::<H, { Op::$op as u16 }, false, ACC_C> as Handler),
                    (Op::$op, true, ACC_C) => Some(lane_i32_update::<H, { Op::$op as u16 }, true, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    updates!(
        WasmI32Add,
        WasmI32Subtract,
        WasmI32Multiply,
        WasmI32And,
        WasmI32Or,
        WasmI32Xor,
        WasmI32ShiftLeft,
        WasmI32ShiftRightSigned,
        WasmI32ShiftRightUnsigned,
        WasmI32Equal,
        WasmI32NotEqual,
        WasmI32LessSigned,
        WasmI32LessUnsigned
    )
}

lane_handler! {
    /// A constant that the next record, direct store `OP`, writes to
    /// memory: both records run here, the constant written to its register
    /// as its own record writes it.
    fn lane_constant_store<OP: u16>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        let constant = i.constant();
        r.set(i.a, constant);
        let next = following(ip);
        // SAFETY: the view fuses a constant only with a following store.
        let access = unsafe { &*next };
        let Some(value) = stored_value(store, constant) else {
            return exit(next);
        };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC_NONE>(r, access, view), value) }
            .is_none()
        {
            return exit(next);
        }
        next!(cx, r, following(next), view)
    }
}

/// The constant-store handler for direct store `op`.
const fn constant_store_handler<H: Host>(op: Op) -> Option<Handler> {
    macro_rules! stores {
        ($($store:ident),+) => {
            match op {
                $(Op::$store => Some(lane_constant_store::<H, { Op::$store as u16 }> as Handler),)+
                _ => None,
            }
        };
    }
    stores!(
        WasmI32Store,
        WasmI64Store,
        WasmF32Store,
        WasmF64Store,
        WasmI32Store8,
        WasmI32Store16,
        WasmI64Store8,
        WasmI64Store16,
        WasmI64Store32
    )
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
    let view = frame_memory_view(cx, lane_view, r);
    // SAFETY: the view has one record per instruction of the function, and
    // `pc` is the frame's validated resume point.
    let ip = unsafe { base.add(pc) };
    // The general path's register access names the active frame's register file.
    vm.active_registers = r.0;
    unsafe {
        (*cx).frame = frame;
        (*cx).registers = r;
        (*cx).base = base;
        (*cx).memory = view;
    }
    let view = Carried {
        memory: view,
        integers: crate::value::IntegerEncoding::TAG,
        acc: Value::UNDEFINED,
    };
    next!(cx, r, ip, view)
}

impl<H: Host> Vm<H> {
    /// Make the lane's activations VM frames, innermost last, each with the
    /// pending call its push would have recorded; the lane continues in the
    /// innermost one. The bottom caller is the lane run's VM frame.
    fn materialize_lane_activations(&mut self, cx: *mut LaneContext<H>) {
        let activations = std::mem::take(&mut self.lane_stack.activations);
        if activations.is_empty() {
            self.lane_stack.activations = activations;
            return;
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let pending = unsafe { &mut *(*cx).pending };
        let mut caller = unsafe { (*cx).frame };
        for activation in &activations {
            // SAFETY: a call record lies in its caller's view.
            let call_pc = unsafe { activation.call.offset_from(activation.caller_base) } as usize;
            let destination = unsafe { (*activation.call).a };
            self.frames[caller].pc = call_pc + 1;
            let count = usize::from(code.functions[activation.function as usize].registers);
            let window = activation.window;
            let registers = std::mem::take(&mut self.lane_stack.values);
            self.push_wasm_frame_with_registers(
                code,
                caller,
                activation.function,
                &registers[window..window + count],
            );
            self.lane_stack.values = registers;
            pending.push(PendingGeneralCall {
                caller,
                caller_cursor: None,
                call_pc: call_pc as u32,
                destination,
                construct_this: None,
                returns: false,
                // The lane admitted the activation under WASM_ACTIVATION_LIMIT; as a frame it
                // also counts against the guest-call budget while that budget lasts.
                call_guard: crate::stack::GuestCallGuard::enter().ok(),
            });
            caller += 1;
        }
        self.lane_stack.values.clear();
        let mut activations = activations;
        activations.clear();
        self.lane_stack.activations = activations;
        unsafe {
            (*cx).frame = caller;
            (*cx).registers = Registers(self.frames[caller].registers.as_mut_ptr());
        }
    }
}

lane_handler! {
    /// A Wasm call to a function of the same program, pushed as the general
    /// loop's inline call would be.
    fn lane_call_known<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).inline_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        // SAFETY: the program's views outlive the lane run.
        let views = unsafe { &*(*cx).views };
        let stack = &mut vm.lane_stack;
        if let Some(lane_view) = views.get(usize::from(i.b)).and_then(std::cell::OnceCell::get)
            && let Some(shape) = lane_view.activation
            && stack.activations.len() < stack.activations.capacity()
            && vm.frames.len() + stack.activations.len()
                < super::dispatch_frame::WASM_ACTIVATION_LIMIT
            && stack.values.capacity() - stack.values.len()
                >= usize::from(shape.registers).max(ACTIVATION_FILL_BLOCK)
        {
            let window = stack.values.len();
            let count = usize::from(shape.registers);
            let arguments = ImmediateLayout::call_window_base(i.imm());
            let passed = ImmediateLayout::argument_count(i.imm()).min(shape.params);
            // SAFETY: capacity holds the window; each slot is written before
            // the length covers it, and no activation's window moves.
            let callee_registers = unsafe {
                let registers = stack.values.as_mut_ptr().add(window);
                if count <= ACTIVATION_FILL_BLOCK {
                    registers
                        .cast::<[Value; ACTIVATION_FILL_BLOCK]>()
                        .write_unaligned([shape.initial; ACTIVATION_FILL_BLOCK]);
                } else {
                    let mut index = 0;
                    while index < count {
                        registers.add(index).write(shape.initial);
                        index += 1;
                    }
                }
                let mut offset = 0;
                while offset < passed {
                    registers
                        .add(usize::from(shape.parameters + offset))
                        .write(r.get(arguments + offset));
                    offset += 1;
                }
                stack.values.set_len(window + count);
                Registers(registers)
            };
            // SAFETY: capacity holds the activation.
            unsafe {
                let length = stack.activations.len();
                stack.activations.as_mut_ptr().add(length).write(Activation {
                    function: u32::from(i.b),
                    call: ip,
                    caller_base: (*cx).base,
                    caller_registers: r,
                    caller_memory: view.memory,
                    window,
                });
                stack.activations.set_len(length + 1);
            }
            // A fresh activation's memory register holds no memory until
            // the function's instance binding loads it.
            let base = lane_view.records();
            unsafe {
                (*cx).registers = callee_registers;
                (*cx).base = base;
                (*cx).memory = MemoryView::NONE;
            }
            let view = Carried {
                memory: MemoryView::NONE,
                integers: crate::value::IntegerEncoding::TAG,
                acc: Value::UNDEFINED,
            };
            next!(cx, callee_registers, base, view)
        }
        lane_call_frame::<H>(ip)
    }
}

/// A lane call without room for an activation, or to a function whose lane
/// view is not yet derived: the activations
/// become frames and the call pushes one, as the general loop would; the
/// emptied stack then grows so that later calls fit. Out of line so the
/// activation path saves no registers.
#[cold]
#[inline(never)]
fn lane_call_frame<H: Host>(ip: Ip) -> Ip {
    let cx = lane_context::<H>();
    // SAFETY: called only while a lane runs, from its call handler.
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    let code = unsafe { &*(*cx).code };
    let callee = u32::from(i.b);
    let count = usize::from(code.functions[usize::from(i.b)].registers);
    let window = count.max(ACTIVATION_FILL_BLOCK);
    let stack = &vm.lane_stack;
    let values_full = stack.values.capacity() - stack.values.len() < window;
    let activations_full = stack.activations.len() == stack.activations.capacity();
    vm.materialize_lane_activations(cx);
    // Only a lack of room grows the stack, so its size follows the deepest
    // window and activation count a lane has actually needed.
    if values_full {
        let capacity = vm.lane_stack.values.capacity();
        vm.lane_stack.values.reserve((2 * capacity).max(window));
    }
    if activations_full {
        let capacity = vm.lane_stack.activations.capacity();
        vm.lane_stack.activations.reserve((2 * capacity).max(1));
    }
    let caller = unsafe { (*cx).frame };
    // SAFETY: `ip` is a record of the active view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    let window = crate::bytecode::RegisterWindow {
        base: ImmediateLayout::call_window_base(i.imm()),
        count: ImmediateLayout::argument_count(i.imm()),
    };
    let Ok(call_guard) = vm.push_wasm_frame(code, caller, callee, window) else {
        return exit(ip);
    };
    vm.frames[caller].pc = call_pc + 1;
    unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
        caller,
        caller_cursor: None,
        call_pc: call_pc as u32,
        destination: i.a,
        construct_this: None,
        returns: false,
        call_guard: Some(call_guard),
    });
    lane_enter_frame(cx, caller + 1, 0)
}

lane_handler! {
    /// Return from an inline call to its caller; returning from the loop's
    /// entry frame belongs to the general loop.
    fn lane_return<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if let Some(activation) = vm.lane_stack.activations.pop() {
            // SAFETY: the call record belongs to the caller's live view.
            let destination = unsafe { (*activation.call).a };
            activation.caller_registers.set(destination, r.get(i.a));
            vm.lane_stack.values.truncate(activation.window);
            unsafe {
                (*cx).registers = activation.caller_registers;
                (*cx).base = activation.caller_base;
                (*cx).memory = activation.caller_memory;
            }
            let view = Carried {
                memory: activation.caller_memory,
                integers: crate::value::IntegerEncoding::TAG,
                acc: Value::UNDEFINED,
            };
            next!(cx, activation.caller_registers, following(activation.call), view)
        }
        lane_return_frame::<H>(r, ip)
    }
}

/// Return from a frame the general loop or a frame push made. Out of line
/// so the activation return saves no registers.
#[cold]
#[inline(never)]
fn lane_return_frame<H: Host>(r: Registers, ip: Ip) -> Ip {
    let cx = lane_context::<H>();
    // SAFETY: called only while a lane runs, from its return handler.
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    let pending = unsafe { &mut *(*cx).pending };
    // An in-loop `new` substitutes its receiver for a non-object result;
    // the general path owns that [[Construct]] step.
    if !unsafe { (*cx).inline_calls }
        || pending
            .last()
            .is_none_or(|call| call.construct_this.is_some())
    {
        return exit(ip);
    }
    let value = r.get(i.a);
    let Some(call) = pending.pop() else {
        return exit(ip);
    };
    vm.retire_wasm_frame();
    vm.active_registers = vm.frames[call.caller].registers.as_mut_ptr();
    vm.write(call.caller, call.destination, value);
    drop(call.call_guard);
    let pc = vm.frames[call.caller].pc;
    lane_enter_frame(cx, call.caller, pc)
}

/// A lane opcode's handler, and whether that handler is an accumulator
/// producer: whenever it falls through to the next record, it has written
/// its result register and carries the written value into that record.
#[derive(Clone, Copy)]
struct LaneHandler {
    handler: Handler,
    produces: bool,
}

impl LaneHandler {
    const fn producer(handler: Handler) -> Option<Self> {
        Some(Self {
            handler,
            produces: true,
        })
    }
}

/// The handler serving opcode `OP` whose accumulator field is `ACC`, derived
/// from the opcode's semantic family; `None` when the general path executes
/// the opcode.
const fn handler_for<H: Host, const OP: u16, const BACKWARD: bool, const ACC: u8>()
-> Option<LaneHandler> {
    let Some(op) = Op::from_index(OP as usize) else {
        return None;
    };
    if I32BinaryOperator::from_register_op(op).is_some()
        || I32BinaryOperator::from_immediate_op(op).is_some()
    {
        return LaneHandler::producer(lane_i32_binary::<H, OP, ACC>);
    }
    let handler: Handler = if I32BinaryOperator::from_jump_op(op).is_some()
        || I32BinaryOperator::from_immediate_jump_op(op).is_some()
    {
        lane_compare_jump::<H, OP, BACKWARD, ACC>
    } else if MemoryLoad::from_direct_op(op).is_some() {
        return LaneHandler::producer(lane_load::<H, OP, ACC>);
    } else if MemoryStore::from_direct_op(op).is_some() {
        lane_store::<H, OP, ACC>
    } else {
        match op {
            Op::Move => return LaneHandler::producer(lane_move::<H, ACC>),
            Op::LoadConst => return LaneHandler::producer(lane_load_const::<H>),
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                return LaneHandler::producer(lane_bit_field::<H, ACC>);
            }
            Op::WasmSelect => return LaneHandler::producer(lane_select::<H, ACC>),
            Op::WasmI32AddImmediateJumpNonZero => {
                return LaneHandler::producer(lane_add_jump_nonzero::<H, BACKWARD, ACC>);
            }
            Op::WasmI32Unary => return LaneHandler::producer(lane_i32_unary::<H, ACC>),
            Op::WasmF32Binary | Op::WasmF64Binary => {
                return LaneHandler::producer(lane_float_binary::<H, OP, ACC>);
            }
            Op::WasmF32Unary | Op::WasmF64Unary => {
                return LaneHandler::producer(lane_float_unary::<H, OP, ACC>);
            }
            Op::WasmI64Binary => return LaneHandler::producer(lane_i64_binary::<H, ACC>),
            Op::WasmI64Unary => return LaneHandler::producer(lane_i64_unary::<H, ACC>),
            Op::WasmScalarConvert => {
                return LaneHandler::producer(lane_scalar_convert::<H, ACC>);
            }
            _ => match other_handler::<H, BACKWARD, ACC>(op) {
                Some(handler) => handler,
                None => return None,
            },
        }
    };
    Some(LaneHandler {
        handler,
        produces: false,
    })
}

/// Handlers that neither produce nor read the accumulator, apart from the
/// branch tests, which read it.
const fn other_handler<H: Host, const BACKWARD: bool, const ACC: u8>(op: Op) -> Option<Handler> {
    match op {
        Op::StoreLocalPlain => Some(lane_store_local_plain::<H>),
        Op::LoadThis => Some(lane_load_this::<H>),
        Op::GetField => Some(lane_get_field::<H>),
        Op::SetField | Op::SetFieldStrict => Some(lane_set_object_field::<H>),
        Op::SetThisField | Op::SetThisFieldStrict => Some(lane_set_this_field::<H>),
        Op::ToNumeric => Some(lane_to_numeric::<H>),
        Op::IncDec => Some(lane_inc_dec::<H>),
        Op::Binary | Op::NumericAdd | Op::NumericMultiply => Some(lane_binary::<H>),
        Op::GetIndex => Some(lane_get_index::<H>),
        Op::SetIndex => Some(lane_set_index::<H>),
        Op::LoadCapture => Some(lane_load_capture::<H>),
        Op::JumpBinaryFalse => Some(lane_jump_binary_false::<H, BACKWARD>),
        Op::LoadLocalPlain => Some(lane_load_local_plain::<H>),
        Op::Jump => Some(lane_jump_always::<H, BACKWARD>),
        Op::JumpFalse => Some(lane_jump_false::<H, BACKWARD>),
        Op::WasmBranchTable => Some(lane_branch_table::<H, BACKWARD>),
        Op::WasmJumpI32Zero => Some(lane_jump_i32_zero::<H, true, BACKWARD, ACC, false>),
        Op::WasmJumpI32NonZero => Some(lane_jump_i32_zero::<H, false, BACKWARD, ACC, false>),
        Op::WasmInstanceBinding => Some(lane_instance_binding::<H, false>),
        Op::WasmGlobalGet => Some(lane_global_get::<H>),
        Op::WasmGlobalSet => Some(lane_global_set::<H>),
        Op::WasmFillRegisters => Some(lane_fill_registers::<H>),
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
    ($h:ty, $backward:literal, $acc:expr, $row:literal) => {
        [
            handler_for::<$h, { $row * TABLE_ROW }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 1 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 2 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 3 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 4 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 5 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 6 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 7 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 8 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 9 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 10 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 11 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 12 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 13 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 14 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 15 }, $backward, { $acc }>(),
        ]
    };
}

const fn flatten_rows(
    rows: [[Option<LaneHandler>; TABLE_ROW as usize]; TABLE_ROWS],
) -> [Option<LaneHandler>; TABLE_SLOTS] {
    let mut table = [None; TABLE_SLOTS];
    let mut slot = 0;
    while slot < TABLE_SLOTS {
        table[slot] = rows[slot / TABLE_ROW as usize][slot % TABLE_ROW as usize];
        slot += 1;
    }
    table
}

/// One handler table: the opcode's handler for one jump direction and one
/// accumulator field.
type Table = [Option<LaneHandler>; TABLE_SLOTS];

macro_rules! lane_table {
    ($h:ty, $backward:literal, $acc:expr) => {
        flatten_rows([
            table_row!($h, $backward, $acc, 0),
            table_row!($h, $backward, $acc, 1),
            table_row!($h, $backward, $acc, 2),
            table_row!($h, $backward, $acc, 3),
            table_row!($h, $backward, $acc, 4),
            table_row!($h, $backward, $acc, 5),
            table_row!($h, $backward, $acc, 6),
            table_row!($h, $backward, $acc, 7),
            table_row!($h, $backward, $acc, 8),
            table_row!($h, $backward, $acc, 9),
            table_row!($h, $backward, $acc, 10),
            table_row!($h, $backward, $acc, 11),
            table_row!($h, $backward, $acc, 12),
            table_row!($h, $backward, $acc, 13),
            table_row!($h, $backward, $acc, 14),
            table_row!($h, $backward, $acc, 15),
        ])
    };
}

struct LaneTable<H>(std::marker::PhantomData<H>);

impl<H: Host> LaneTable<H> {
    /// Tables indexed by jump direction (forward, then backward: a
    /// safepoint) and by accumulator field.
    const TABLES: [[Table; ACC_POSITIONS]; 2] = [
        [
            lane_table!(H, false, ACC_NONE),
            lane_table!(H, false, ACC_A),
            lane_table!(H, false, ACC_B),
            lane_table!(H, false, ACC_C),
            lane_table!(H, false, ACC_PAIR),
        ],
        [
            lane_table!(H, true, ACC_NONE),
            lane_table!(H, true, ACC_A),
            lane_table!(H, true, ACC_B),
            lane_table!(H, true, ACC_C),
            lane_table!(H, true, ACC_PAIR),
        ],
    ];
}

/// Whether the lane may run `instruction` from its view record: every
/// register it names lies inside the register file and carries no flag.
pub(super) fn lane_fields_fit(instruction: WideInstruction, function: &Function) -> bool {
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
pub(super) fn memory_register(function: &Function) -> Option<u16> {
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

/// The shortest straight-line lane run the general loop enters. Entering
/// builds the lane context and memory view, which one or two lane
/// instructions do not repay: on V8-v7 fixed work, three keeps every
/// JavaScript benchmark within one percent of the general path or better
/// (one and two cost DeltaBlue three percent), and leaves Wasm loops,
/// whose runs are long, unchanged.
const LANE_ENTRY_MIN_RUN: usize = 3;

/// Whether records may read the accumulator. Without sibling calls every
/// handler returns to the lane loop, which carries no accumulator.
const ACCUMULATOR: bool = cfg!(quench_lane_tail_calls);

/// Derive a function's lane view from its bytecode and its program's
/// constants.
fn derive_lane_view<H: Host>(
    function: &Function,
    constants: &[Value],
    kind: crate::bytecode::ProgramKind,
) -> LaneView {
    // A backward jump is a safepoint where the lane can meet a due
    // collection. Wasm records never allocate, so no collection becomes due
    // inside a Wasm run (one due at entry is taken there), and with sibling
    // calls the run's stack never grows: a Wasm function's jumps all take
    // the plain path.
    let safepoints = kind != crate::bytecode::ProgramKind::Wasm;
    let tables = const { &LaneTable::<H>::TABLES };
    let memory_register = memory_register(function);
    let jump_targets = jump_targets(function);
    let mut producer: Option<u16> = None;
    // Per instruction: the general path executes it, and its record reads
    // the accumulator.
    let mut exits = Vec::with_capacity(function.code.len());
    let mut reads_accumulator = Vec::with_capacity(function.code.len());
    // Per instruction: the register its selected record carries as the
    // accumulator to the record after it.
    let mut produced = Vec::with_capacity(function.code.len());
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
            // Control reaches this record only from the previous one when no
            // jump targets it; the general loop never enters at a record that
            // reads the accumulator.
            let carried = producer.take().filter(|_| ACCUMULATOR && !jump_targets[pc]);
            let acc = carried.map_or(ACC_NONE, |register| {
                accumulator_field(instruction, register)
            });
            // A table is backward when one of its entries jumps back.
            let backward_jump = safepoints
                && if op == Op::WasmBranchTable {
                    crate::bytecode::branch_table_entries(pc, instruction).any(|entry| {
                        decoded_instruction(function, function.code[entry]).imm() as usize <= pc
                    })
                } else {
                    op.immediate_role() == ImmediateRole::JumpTarget
                        && instruction.imm() as usize <= pc
                };
            let selected = operand.and(if binds_memory {
                Some(LaneHandler {
                    handler: lane_instance_binding::<H, true> as Handler,
                    produces: false,
                })
            } else if op == Op::WasmF64Unary
                && F64UnaryOperator::from_tag(instruction.imm())
                    == Some(F64UnaryOperator::SquareRoot)
            {
                tables[usize::from(backward_jump)][usize::from(acc)][op as usize].and_then(|_| {
                    LaneHandler::producer(match acc {
                        ACC_NONE => lane_f64_sqrt::<H, ACC_NONE> as Handler,
                        ACC_A => lane_f64_sqrt::<H, ACC_A> as Handler,
                        ACC_B => lane_f64_sqrt::<H, ACC_B> as Handler,
                        ACC_C => lane_f64_sqrt::<H, ACC_C> as Handler,
                        _ => lane_f64_sqrt::<H, ACC_PAIR> as Handler,
                    })
                })
            } else if let Some(arith) = float_arithmetic(op, instruction.imm()) {
                tables[usize::from(backward_jump)][usize::from(acc)][op as usize].and_then(|_| {
                    LaneHandler::producer(
                        FloatArithmetic::<H>::HANDLERS[usize::from(op == Op::WasmF64Binary)]
                            [usize::from(arith)][usize::from(acc)],
                    )
                })
            } else {
                tables[usize::from(backward_jump)][usize::from(acc)][op as usize]
            });
            producer = selected
                .filter(|selected| selected.produces)
                .map(|_| instruction.a());
            produced.push(producer);
            exits.push(selected.is_none());
            reads_accumulator.push(selected.is_some() && acc != ACC_NONE);
            LaneInstruction {
                handler: selected.map_or(lane_exit::<H> as Handler, |selected| selected.handler)
                    as *const (),
                operand: operand.unwrap_or(LaneOperand { imm: 0 }),
                a: instruction.a(),
                b: instruction.b(),
                c: instruction.c(),
            }
        })
        .collect();
    // Each record's handler before any fusion: a record still holding it
    // runs only its own instruction.
    let selected: Vec<*const ()> = records.iter().map(|record| record.handler).collect();
    // Whether a record's handler serves `op` with accumulator field `acc`.
    let serves = |handler: *const (), op: Op, acc: u8| {
        tables[0][usize::from(acc)][op as usize]
            .is_some_and(|served| handler == served.handler as *const ())
    };
    // A table follows its selected entry's jump, so every entry must run in
    // the lane.
    for (pc, packed) in function.code.iter().enumerate() {
        let instruction = decoded_instruction(function, *packed);
        if instruction.op() == Op::WasmBranchTable
            && crate::bytecode::branch_table_entries(pc, instruction).any(|entry| exits[entry])
        {
            records[pc].handler = lane_exit::<H> as Handler as *const ();
            exits[pc] = true;
            reads_accumulator[pc] = false;
        }
    }
    // An add of a constant whose sum the next record loads from runs with
    // that load as one record: the load reads the sum as its carried
    // accumulator, so control reaches it only from the add.
    for pc in 0..records.len().saturating_sub(1) {
        let instruction = decoded_instruction(function, function.code[pc]);
        let next = decoded_instruction(function, function.code[pc + 1]);
        let plain_add = serves(records[pc].handler, Op::WasmI32AddImmediate, ACC_NONE);
        let carried_load = serves(records[pc + 1].handler, next.op(), ACC_C);
        if instruction.op() == Op::WasmI32AddImmediate
            && plain_add
            && carried_load
            && let Some(fused) = add_load_handler::<H>(next.op())
        {
            // A float operator taking the loaded f64 joins the fusion.
            let operator = (next.op() == Op::WasmF64Load && pc + 2 < records.len())
                .then(|| decoded_instruction(function, function.code[pc + 2]))
                .filter(|third| third.op() == Op::WasmF64Binary)
                .and_then(|third| {
                    let second = float_arithmetic(third.op(), third.imm())?;
                    let left = [ACC_B, ACC_C].into_iter().find(|acc| {
                        records[pc + 2].handler
                            == FloatArithmetic::<H>::HANDLERS[1][usize::from(second)]
                                [usize::from(*acc)] as *const ()
                    })? == ACC_B;
                    Some(AddLoadF64::<H>::HANDLERS[usize::from(second)][usize::from(left)])
                });
            records[pc].handler = operator.unwrap_or(fused) as *const ();
        }
    }
    // An i32 load whose value the next record's branch table selects on
    // runs with that table as one record.
    for pc in 0..records.len().saturating_sub(1) {
        let instruction = decoded_instruction(function, function.code[pc]);
        let next = decoded_instruction(function, function.code[pc + 1]);
        let field = [ACC_NONE, ACC_C]
            .into_iter()
            .find(|acc| serves(records[pc].handler, instruction.op(), *acc));
        if let Some(field) = field
            && next.a() == instruction.a()
            && serves(records[pc + 1].handler, Op::WasmBranchTable, ACC_NONE)
            && let Some(fused) = load_table_handler::<H>(instruction.op(), field, false)
        {
            records[pc].handler = fused as *const ();
            // A plain add of a constant ahead of a load reading its own
            // field joins as well.
            if pc > 0
                && field == ACC_NONE
                && serves(records[pc - 1].handler, Op::WasmI32AddImmediate, ACC_NONE)
                && let Some(fused) = load_table_handler::<H>(instruction.op(), ACC_NONE, true)
            {
                records[pc - 1].handler = fused as *const ();
            }
        }
    }
    // A shift left by a constant whose result the next record adds runs
    // with that add as one record.
    for pc in 0..records.len().saturating_sub(1) {
        let variant = |op: Op, acc: u8, at: usize| serves(records[at].handler, op, acc);
        let field = [ACC_NONE, ACC_B]
            .into_iter()
            .position(|acc| variant(Op::WasmI32ShiftLeftImmediate, acc, pc));
        let left = [ACC_C, ACC_B]
            .into_iter()
            .position(|acc| variant(Op::WasmI32Add, acc, pc + 1));
        if let (Some(field), Some(left)) = (field, left) {
            records[pc].handler = ScaledIndex::<H>::HANDLERS[field][left] as *const ();
        }
    }
    // An i32 load, the operator taking the loaded value and the store of
    // its result run as one record.
    for pc in 0..records.len().saturating_sub(2) {
        let load = decoded_instruction(function, function.code[pc]);
        let combine = decoded_instruction(function, function.code[pc + 1]);
        let store = decoded_instruction(function, function.code[pc + 2]);
        if load.op() != Op::WasmI32Load || store.op() != Op::WasmI32Store {
            continue;
        }
        let field = [ACC_NONE, ACC_C]
            .into_iter()
            .find(|&acc| serves(records[pc].handler, Op::WasmI32Load, acc));
        let left = [ACC_C, ACC_B]
            .into_iter()
            .position(|acc| serves(records[pc + 1].handler, combine.op(), acc));
        if let (Some(field), Some(left)) = (field, left)
            && store.a() == combine.a()
            && serves(records[pc + 2].handler, Op::WasmI32Store, ACC_A)
            && let Some(fused) = i32_update_handler::<H>(combine.op(), left == 1, field)
        {
            records[pc].handler = fused as *const ();
        }
    }
    // A constant that the next record stores runs with that store as one
    // record.
    for pc in 0..records.len().saturating_sub(1) {
        let next = decoded_instruction(function, function.code[pc + 1]);
        if serves(records[pc].handler, Op::LoadConst, ACC_NONE)
            && serves(records[pc + 1].handler, next.op(), ACC_A)
            && let Some(fused) = constant_store_handler::<H>(next.op())
        {
            records[pc].handler = fused as *const ();
        }
    }
    // A load whose value the next record stores runs with that store as
    // one record.
    for pc in 0..records.len().saturating_sub(1) {
        let instruction = decoded_instruction(function, function.code[pc]);
        let next = decoded_instruction(function, function.code[pc + 1]);
        let field = [ACC_NONE, ACC_C]
            .into_iter()
            .find(|acc| serves(records[pc].handler, instruction.op(), *acc));
        let carried_store = serves(records[pc + 1].handler, next.op(), ACC_A);
        if let Some(field) = field
            && carried_store
            && let Some(fused) = load_store_handler::<H>(instruction.op(), next.op(), field)
        {
            records[pc].handler = fused as *const ();
        }
    }
    // A record's plain jump, with its direction.
    let jump_direction = |handler: *const ()| {
        [false, true].into_iter().find(|&backward| {
            tables[usize::from(backward)][usize::from(ACC_NONE)][Op::Jump as usize]
                .is_some_and(|jump| handler == jump.handler as *const ())
        })
    };
    // A jump whose target is a plain move or add of a small constant runs
    // that instruction itself and jumps past it: the target's successor
    // then sees the accumulator the target would have carried. A table
    // entry stays a plain jump, since tables take the entry's jump without
    // running its record. This runs before the fusions below, which take a
    // following jump's record as a plain jump.
    let mut entry_jumps = vec![false; records.len()];
    for (pc, packed) in function.code.iter().enumerate() {
        let instruction = decoded_instruction(function, *packed);
        if instruction.op() == Op::WasmBranchTable {
            for entry in crate::bytecode::branch_table_entries(pc, instruction) {
                entry_jumps[entry] = true;
            }
        }
    }
    let record_bytes = size_of::<LaneInstruction>() as isize;
    for (pc, packed) in function.code.iter().enumerate() {
        let instruction = decoded_instruction(function, *packed);
        let target = instruction.imm() as usize;
        if instruction.op() != Op::Jump
            || jump_direction(records[pc].handler).is_none()
            || entry_jumps[pc]
            || target + 1 >= records.len()
        {
            continue;
        }
        let absorbed = decoded_instruction(function, function.code[target]);
        let plain = |op: Op| absorbed.op() == op && serves(records[target].handler, op, ACC_NONE);
        let backward = safepoints && target < pc;
        let (handler, c) = if plain(Op::Move) {
            (JumpFusions::<H>::THROUGH[0][usize::from(backward)], 0)
        } else if plain(Op::WasmI32AddImmediate)
            && let Ok(step) = i16::try_from(absorbed.imm() as i32)
        {
            (
                JumpFusions::<H>::THROUGH[1][usize::from(backward)],
                step as u16,
            )
        } else {
            continue;
        };
        records[pc] = LaneInstruction {
            handler: handler as *const (),
            operand: LaneOperand {
                jump: (target as isize + 1 - pc as isize) * record_bytes,
            },
            a: absorbed.a(),
            b: absorbed.b(),
            c,
        };
    }
    // Two plain moves in a row run as one record.
    let mut pc = 0;
    while pc + 1 < records.len() {
        if serves(records[pc].handler, Op::Move, ACC_NONE)
            && serves(records[pc + 1].handler, Op::Move, ACC_NONE)
        {
            // A jump after the pair joins it.
            let jump = records
                .get(pc + 2)
                .and_then(|record| jump_direction(record.handler));
            records[pc].handler = match jump {
                None => lane_move_pair::<H, false, false> as Handler,
                Some(false) => lane_move_pair::<H, true, false>,
                Some(true) => lane_move_pair::<H, true, true>,
            } as *const ();
            pc += 2;
        } else {
            pc += 1;
        }
    }
    // A plain move or add of a constant whose next record jumps runs with
    // that jump as one record.
    for pc in 0..records.len().saturating_sub(1) {
        let plain = |op: Op| serves(records[pc].handler, op, ACC_NONE);
        let add = if plain(Op::Move) {
            false
        } else if plain(Op::WasmI32AddImmediate) {
            true
        } else {
            continue;
        };
        if let Some(backward) = jump_direction(records[pc + 1].handler) {
            records[pc].handler =
                JumpFusions::<H>::THEN[usize::from(add)][usize::from(backward)] as *const ();
        }
    }
    // An f64 arithmetic record whose result the next f64 operator takes
    // through the accumulator runs with it as one record; pairs tile a
    // chain from its start.
    let mut pc = 0;
    while pc + 1 < records.len() {
        let instruction = decoded_instruction(function, function.code[pc]);
        let next = decoded_instruction(function, function.code[pc + 1]);
        let pair = (|| {
            if next.op() != Op::WasmF64Binary {
                return None;
            }
            let second = float_arithmetic(next.op(), next.imm())?;
            let left_of = |records: &[LaneInstruction]| {
                [ACC_B, ACC_C]
                    .into_iter()
                    .find(|acc| {
                        records[pc + 1].handler
                            == FloatArithmetic::<H>::HANDLERS[1][usize::from(second)]
                                [usize::from(*acc)] as *const ()
                    })
                    .map(|acc| acc == ACC_B)
            };
            if instruction.op() == Op::WasmF64Load {
                let plain = tables[0][usize::from(ACC_NONE)][Op::WasmF64Load as usize]?;
                if records[pc].handler != plain.handler as *const () {
                    return None;
                }
                let left = left_of(&records)?;
                // An arithmetic whose result the record after it stores
                // (`mem[x] += v`) joins as well.
                let updates =
                    pc + 2 < records.len() && usize::from(second) < FLOAT_PAIR_FIRSTS && {
                        let store = decoded_instruction(function, function.code[pc + 2]);
                        store.op() == Op::WasmF64Store
                            && store.a() == next.a()
                            && [ACC_NONE, ACC_A]
                                .into_iter()
                                .any(|acc| serves(records[pc + 2].handler, Op::WasmF64Store, acc))
                    };
                if updates {
                    return Some((
                        FloatPairs::<H>::UPDATES[usize::from(second)][usize::from(left)],
                        3,
                    ));
                }
                return Some((
                    FloatPairs::<H>::LOADS[usize::from(second)][usize::from(left)],
                    2,
                ));
            }
            if instruction.op() != Op::WasmF64Binary {
                return None;
            }
            let first = float_arithmetic(instruction.op(), instruction.imm())?;
            if usize::from(first) >= FLOAT_PAIR_FIRSTS {
                return None;
            }
            let field = FUSED_ACC_FIELDS.iter().position(|acc| {
                records[pc].handler
                    == FloatArithmetic::<H>::HANDLERS[1][usize::from(first)][usize::from(*acc)]
                        as *const ()
            })?;
            let left = left_of(&records)?;
            Some((
                FloatPairs::<H>::HANDLERS[usize::from(first)][usize::from(second)]
                    [usize::from(left)][field],
                2,
            ))
        })();
        // An f64 arithmetic whose result the next record stores, when only
        // the arithmetic reaches that store.
        let pair = pair.or_else(|| {
            let first = float_arithmetic(instruction.op(), instruction.imm())?;
            // The fused record writes the arithmetic's result itself, so the
            // store may read it plainly or through the accumulator.
            let store_runs = [ACC_NONE, ACC_A]
                .into_iter()
                .any(|acc| serves(records[pc + 1].handler, Op::WasmF64Store, acc));
            if instruction.op() != Op::WasmF64Binary
                || usize::from(first) >= FLOAT_PAIR_FIRSTS
                || next.op() != Op::WasmF64Store
                || next.a() != instruction.a()
                || jump_targets[pc + 1]
                || !store_runs
            {
                return None;
            }
            let field = FUSED_ACC_FIELDS.iter().position(|acc| {
                records[pc].handler
                    == FloatArithmetic::<H>::HANDLERS[1][usize::from(first)][usize::from(*acc)]
                        as *const ()
            })?;
            Some((FloatPairs::<H>::STORES[usize::from(first)][field], 2))
        });
        match pair {
            Some((fused, span)) => {
                records[pc].handler = fused as *const ();
                pc += span;
            }
            None => pc += 1,
        }
    }
    // A dispatch head (an add of a constant fused with the load and table
    // after it) reads the register both take from the accumulator when every
    // edge into it carries that register there, as an interpreter's latches
    // carry its program counter.
    let unfused = |records: &[LaneInstruction], from: usize, to: usize| {
        (from..=to).all(|pc| records[pc].handler == selected[pc])
    };
    // The register the record at `pc` carries as the accumulator when it
    // falls through, if it runs only itself and nothing fused runs into it.
    let carried_through = |records: &[LaneInstruction], pc: usize| {
        unfused(records, pc.saturating_sub(FUSION_REACH), pc)
            .then_some(produced[pc])
            .flatten()
    };
    // The register an edge into `target` carries, per edge; `None` when an
    // edge carries none (or one this analysis does not follow).
    // The carrying variant of the zero-test jump `handler`, if it is one.
    let carrying_zero_jump = |handler: *const ()| {
        let plain = ZeroJumps::<H, false>::HANDLERS
            .as_flattened()
            .as_flattened();
        let carrying = ZeroJumps::<H, true>::HANDLERS.as_flattened().as_flattened();
        plain
            .iter()
            .position(|&plain| handler == plain as *const ())
            .map(|index| carrying[index])
    };
    let edges_carry = |records: &[LaneInstruction], target: usize| -> Option<u16> {
        let mut carried: Option<u16> = None;
        let mut agree = |register: Option<u16>| match (register, carried) {
            (None, _) => false,
            (Some(register), None) => {
                carried = Some(register);
                true
            }
            (Some(register), Some(known)) => register == known,
        };
        if target > 0 && !exits[target - 1] {
            use crate::bytecode::ControlFlowLayout as Flow;
            match decoded_instruction(function, function.code[target - 1])
                .op()
                .control_flow_layout()
            {
                Flow::Fallthrough => {
                    if !agree(carried_through(records, target - 1)) {
                        return None;
                    }
                }
                Flow::Jump | Flow::BranchTable | Flow::Terminal => {}
                // A call's return or a branch not taken carries no register.
                Flow::Call | Flow::ConditionalJump => return None,
            }
        }
        for (pc, packed) in function.code.iter().enumerate() {
            let instruction = decoded_instruction(function, *packed);
            if instruction.op().immediate_role() != ImmediateRole::JumpTarget {
                continue;
            }
            let jump = instruction.imm() as usize;
            let through = JumpFusions::<H>::THROUGH
                .iter()
                .flatten()
                .any(|&handler| records[pc].handler == handler as *const ());
            if through {
                // The record runs its target and lands after it.
                if jump + 1 == target
                    && !agree(Some(decoded_instruction(function, function.code[jump]).a()))
                {
                    return None;
                }
                continue;
            }
            if jump != target {
                continue;
            }
            // A zero-test jump can carry any register (`ZeroJumps`).
            if carrying_zero_jump(records[pc].handler).is_some() {
                continue;
            }
            if instruction.op() != Op::Jump || entry_jumps[pc] || jump_targets[pc] || pc == 0 {
                return None;
            }
            let then = JumpFusions::<H>::THEN
                .iter()
                .flatten()
                .any(|&handler| records[pc - 1].handler == handler as *const ());
            let pair = pc >= 2
                && [true, false].into_iter().any(|backward| {
                    records[pc - 2].handler
                        == if backward {
                            lane_move_pair::<H, true, true> as Handler
                        } else {
                            lane_move_pair::<H, true, false> as Handler
                        } as *const ()
                });
            // Entering a pair's second move runs that move's own record.
            if pair && !then && jump_targets[pc - 1] && carried_through(records, pc - 1).is_none() {
                return None;
            }
            let register = if then || pair {
                Some(decoded_instruction(function, function.code[pc - 1]).a())
            } else if jump_direction(records[pc].handler).is_some() {
                carried_through(records, pc - 1)
            } else {
                None
            };
            if !agree(register) {
                return None;
            }
        }
        carried
    };
    for pc in 0..records.len().saturating_sub(1) {
        let add = decoded_instruction(function, function.code[pc]);
        let load = decoded_instruction(function, function.code[pc + 1]);
        let Some(head) = load_table_handler::<H>(load.op(), ACC_NONE, true) else {
            continue;
        };
        if records[pc].handler != head as *const ()
            || load.c() != add.b()
            || add.a() == add.b()
            || edges_carry(&records, pc) != Some(add.b())
        {
            continue;
        }
        if let Some(carried) = load_table_handler::<H>(load.op(), ACC_C, true) {
            records[pc].handler = carried as *const ();
            reads_accumulator[pc] = true;
            for (jump, packed) in function.code.iter().enumerate() {
                let instruction = decoded_instruction(function, *packed);
                if instruction.op().immediate_role() == ImmediateRole::JumpTarget
                    && instruction.imm() as usize == pc
                    && let Some(carrying) = carrying_zero_jump(records[jump].handler)
                {
                    records[jump].handler = carrying as *const ();
                    records[jump].c = add.b();
                }
            }
        }
    }
    specialize_arithmetic::<H>(function, &mut records);
    let mut entries = vec![false; records.len()].into_boxed_slice();
    let mut run = 0usize;
    for pc in (0..records.len()).rev() {
        run = if exits[pc] { 0 } else { run + 1 };
        entries[pc] = run >= LANE_ENTRY_MIN_RUN && !reads_accumulator[pc];
    }
    let activation = function
        .parameter_registers
        .filter(|_| function.locals == 0)
        .map(|parameters| ActivationShape {
            registers: function.registers,
            parameters,
            params: function.params,
            initial: function.initial_register.value(),
        });
    let enters = kind == crate::bytecode::ProgramKind::Wasm || closes_loop(function, &exits);
    LaneView {
        records,
        entries,
        memory_register,
        activation,
        enters,
    }
}

/// Gives each lane `Binary` record of an arithmetic operator on two register operands the
/// handler for that operator, with the decoded registers in its fields: the record then
/// neither decodes operand kinds nor dispatches on the operator.
fn specialize_arithmetic<H: Host>(function: &Function, records: &mut [LaneInstruction]) {
    use super::operations::{ADDITION, DIVISION, MULTIPLICATION, SUBTRACTION};
    let generic = lane_binary::<H> as Handler as *const ();
    for (pc, packed) in function.code.iter().enumerate() {
        let instruction = decoded_instruction(function, *packed);
        let record = &mut records[pc];
        if record.handler != generic || instruction.numeric_local_target().is_some() {
            continue;
        }
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        let (
            Some(crate::bytecode::OperandKind::Register),
            Some(crate::bytecode::OperandKind::Register),
        ) = (left.kind(), right.kind())
        else {
            continue;
        };
        let handler: Handler = match instruction.binary_operator() {
            ADDITION => lane_arith::<H, ADDITION>,
            SUBTRACTION => lane_arith::<H, SUBTRACTION>,
            MULTIPLICATION => lane_arith::<H, MULTIPLICATION>,
            DIVISION => lane_arith::<H, DIVISION>,
            _ => continue,
        };
        record.handler = handler as *const ();
        record.b = left.payload();
        record.c = right.payload();
    }
}

/// Whether some loop of `function`, from a backward jump's target through the jump, has no
/// instruction the lane exits at.
fn closes_loop(function: &Function, exits: &[bool]) -> bool {
    function.code.iter().enumerate().any(|(pc, packed)| {
        let instruction = decoded_instruction(function, *packed);
        let target = instruction.imm() as usize;
        instruction.op().immediate_role() == ImmediateRole::JumpTarget
            && target <= pc
            && !exits[target..=pc].contains(&true)
    })
}

/// The register field of `instruction` that reads `register`, as an
/// accumulator position; `ACC_NONE` when no register field reads it, or more
/// than one does. A register pair's first register is the last resort.
fn accumulator_field(instruction: WideInstruction, register: u16) -> u8 {
    let mut fields = InstructionField::ALL.iter().copied().filter(|&field| {
        instruction.op().field_layout(field).reads_register()
            && instruction.field_value(field) == register
    });
    match (fields.next(), fields.next()) {
        (Some(field), None) => field.accumulator(),
        (None, _)
            if instruction.op().immediate_layout() == ImmediateLayout::RegisterPair
                && ImmediateLayout::register_pair(instruction.imm()).0 == register =>
        {
            ACC_PAIR
        }
        _ => ACC_NONE,
    }
}

/// The instructions some jump of `function` targets.
fn jump_targets(function: &Function) -> Vec<bool> {
    let mut targets = vec![false; function.code.len()];
    for packed in &function.code {
        let instruction = decoded_instruction(function, *packed);
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget
            && let Some(target) = targets.get_mut(instruction.imm() as usize)
        {
            *target = true;
        }
    }
    targets
}

pub(super) fn decoded_instruction(
    function: &Function,
    packed: crate::bytecode::Instr,
) -> WideInstruction {
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
            // SAFETY: the program store keeps a derived view alive with its program.
            .filter(|view| unsafe { (**view).enters })
            .unwrap_or(std::ptr::null())
    }

    /// Whether the instruction at `pc` of `view` runs in the lane.
    #[inline(always)]
    pub(super) fn lane_runs(view: *const LaneView, pc: usize) -> bool {
        // SAFETY: a non-null view has an entry for every validated PC.
        !view.is_null() && unsafe { *(&(*view).entries).get_unchecked(pc) }
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
        let floor = if self.heap.should_collect() {
            COLLECTION_DUE_FLOOR
        } else {
            stack_pointer().saturating_sub(LANE_STACK_SLACK)
        };
        let outer_floor = LANE_FLOOR.with(|current| current.replace(floor));
        let mut context = LaneContext {
            vm: self,
            code,
            program,
            frame: *frame,
            registers,
            base,
            pending,
            views: self
                .programs
                .lane_views(program)
                .map_or(&[][..], |views| views) as *const _,
            inline_calls: inline_calls
                && code.kind == crate::bytecode::ProgramKind::Wasm
                && self.execution_mode != super::compiled::ExecutionMode::Compiled,
            memory: MemoryView::NONE,
            resolved: (Value::UNDEFINED, MemoryView::NONE),
        };
        let cx: *mut LaneContext<H> = &mut context;
        let outer = LANE_CONTEXT.with(|current| current.replace(cx.cast()));
        // SAFETY: `cx` is the only access path to the context during the run.
        unsafe { (*cx).memory = frame_memory_view(cx, view, registers) };
        // SAFETY: the view has one record per instruction, and residual
        // validation establishes every reachable PC.
        let mut ip = unsafe { base.add(pc) };
        loop {
            // SAFETY: handlers have returned, so no other access is live.
            let (view, registers) = unsafe { ((*cx).memory, (*cx).registers) };
            let next = handler::<H>(ip)(
                registers,
                ip,
                view.bytes,
                view.reach,
                crate::value::IntegerEncoding::TAG,
                Value::UNDEFINED,
            );
            ip = next.map_addr(|address| address & !EXIT_TAG);
            if next.addr() & EXIT_TAG != 0 {
                self.materialize_lane_activations(cx);
                LANE_CONTEXT.with(|current| current.set(outer));
                LANE_FLOOR.with(|current| current.set(outer_floor));
                // SAFETY: as above.
                *frame = unsafe { (*cx).frame };
                // SAFETY: handlers only return records of the active view.
                return unsafe { ip.offset_from((*cx).base) } as usize;
            }
        }
    }
}
