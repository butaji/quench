//! Straight-line Wasm runs that one lane record executes with their vectors unboxed.
//!
//! A register holds a vector as a boxed cell (`Heap::alloc_v128`), so each SIMD record
//! reads its operands through the heap and allocates its result. Within a run of vector,
//! address and i32 instructions the view fixes which registers hold vectors: the run's
//! record (`vector_region_row`) keeps each one unboxed in a bank slot named by its
//! register, enters a register into the bank where the run first reads it, and boxes it
//! back only where liveness still reads it: after the run, and before a record the general
//! path must execute (a trap, or memory the lane cannot borrow). A constant the run loads is
//! written to its register only where a slot read or liveness needs it.
//!
//! Each record computes its staged row: its `Operation`, decoded once when the region is
//! built, runs on a `RegionFrame` (`staged::step_out_of_line` and the Wasm row entries). The frame reads
//! and writes the bank for the vector registers and the register file for the rest. A record
//! no staged row answers keeps its own computation (`Step::Lanes`, `VectorMove`,
//! `VectorConstant`, `Load`, `Store`). Every other record of the run keeps its own handler, so
//! control arriving inside the run runs it as before.

use super::*;
use crate::bytecode::{Operand, Register};
use crate::compile::liveness::{RegisterMasks, RegisterSet, register_in};
use crate::staged;
use crate::wasm::simd::SimdKernel;
use std::convert::Infallible;

/// The registers a bank names: liveness' width, which every region register lies within.
const BANK_REGISTERS: usize = RegisterSet::BITS as usize;

/// The fewest vector instructions a run needs to become a region: its record then saves at
/// least one boxed result and one operand read per instruction, against one entry.
const REGION_MIN_VECTORS: usize = 2;

/// The registers boxed where the run leaves its record: a range of `VectorRegion::spills`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Leave {
    start: u32,
    end: u32,
}

/// A scalar or vector operand of a lane operator.
#[derive(Clone, Copy, Debug)]
enum Place {
    Bank(u16),
    I32(u16),
}

/// The right operand of an i32 comparison: a register or an immediate.
#[derive(Clone, Copy, Debug)]
enum I32Right {
    Register(u16),
    Immediate(i32),
}

/// A record's staged operation, decoded once when the region is built (`Operation::decode`).
type Row = staged::Operation<u64, ()>;

/// One step of a run: the entry of a register into the bank, a record's staged row, or the
/// computation of a record no staged row answers. A step that may leave names its record (`at`,
/// from the run's first) and the registers boxed first.
#[derive(Clone, Copy)]
enum Step {
    Unbox { register: u16, at: u32, leave: Leave },
    /// The staged row `rows[op]` of the record `at`.
    Row { op: u32, at: u32, leave: Leave },
    /// A SIMD operator with an i32 result, which no staged row answers: its kernel's bits.
    Lanes { kernel: &'static SimdKernel, lane: u8, a: u16, b: Place, c: Option<Place> },
    /// A whole-vector copy of a bank register, which no staged row answers.
    VectorMove { a: u16, b: u16 },
    /// A constant written to its register's slot where a slot read or liveness needs it.
    Constant { a: u16, value: Value },
    /// A v128 constant, which no staged row answers.
    VectorConstant { a: u16, bits: u128 },
    /// A v128 load of the memory in `address`, which no staged row answers.
    Load { a: u16, address: u16, at: u32, leave: Leave },
    /// A v128 store to the memory in `address`, which no staged row answers.
    Store { value: u16, address: u16, at: u32, leave: Leave },
}

/// A run of records one record executes (`vector_region_row`).
pub(super) struct VectorRegion {
    /// The run's first record.
    first: usize,
    steps: Box<[Step]>,
    /// The staged operations the run's `Step::Row`s run.
    rows: Box<[Row]>,
    spills: Box<[u16]>,
    /// The records the run covers; the record after them continues.
    records: usize,
    end: Leave,
    /// The register the record after the run reads as its carried accumulator, which its
    /// producer, the run's last record, wrote; `NO_ACCUMULATOR` for none.
    acc: u16,
    /// The i32 comparison jump after the run when it closes a loop the run begins: the run
    /// then takes the back edge itself, its bank kept across iterations.
    looping: Option<RegionLoop>,
}

/// A loop a region runs itself (`VectorRegion::looping`): its closing jump's comparison of
/// register `left` and `right`, and the bank registers boxed before leaving from a repeated
/// iteration, where registers the first iteration had not yet entered may be newer in the
/// bank than in their slots.
#[derive(Clone, Copy)]
struct RegionLoop {
    comparison: I32BinaryOperator,
    left: u16,
    right: I32Right,
    carried: Leave,
}

type Bank = [std::mem::MaybeUninit<u128>; BANK_REGISTERS];

/// The vector at `address` of `memory`, when all its bytes lie within the memory.
#[inline(always)]
fn vector_bytes(memory: MemoryView, address: u64) -> Option<*mut [u8; crate::wasm::V128_BYTES]> {
    // The view's reach is the length less the widest direct access (`MemoryView::new`).
    // SAFETY: a Wasm view's reach is set.
    let last = unsafe { memory.reach.get() } as i64 + WIDEST_DIRECT_ACCESS as i64
        - crate::wasm::V128_BYTES as i64;
    (last >= 0 && address <= last as u64).then(|| {
        // SAFETY: the vector ends at or before the memory's end.
        unsafe { memory.bytes.add(address as usize).cast() }
    })
}

/// The register frame a run's staged rows run on. The bank holds the vector registers the run
/// has unboxed, which the builder enters before any row reads them; the register file holds
/// the rest. The memory is the view's, which the run's admitted accesses address.
struct RegionFrame<'a> {
    bank: &'a mut Bank,
    registers: Registers,
    heap: &'a crate::heap::Heap,
    memory: MemoryView,
    integers: crate::value::IntegerEncoding,
}

impl staged::Frame<staged::Concrete> for RegionFrame<'_> {
    type Fact = ();
    type Control = Infallible;

    fn fact(&self, _register: Register) {}

    fn read(&mut self, _stage: &mut staged::Concrete, register: Register) -> u64 {
        self.registers.get(register).to_bits()
    }

    fn write(&mut self, _stage: &mut staged::Concrete, register: Register, value: u64, _fact: ()) {
        self.registers.set(register, Value::from_bits(value));
    }

    /// The region's activation holds no `this` of its own: a run has no `LoadThis`.
    fn this(&mut self, _stage: &mut staged::Concrete) -> u64 {
        Value::DELETED.to_bits()
    }

    fn define_this(&mut self, _stage: &mut staged::Concrete, destination: Register, this: u64) {
        self.registers.set(destination, Value::from_bits(this));
    }

    fn constant(&mut self, _stage: &mut staged::Concrete, constant: staged::Constant) -> (u64, ()) {
        (constant.value.to_bits(), ())
    }

    /// A run holds no truth test: its rows decide no condition from a heap value.
    fn truthiness(&mut self, _stage: &mut staged::Concrete, _register: Register) -> Option<bool> {
        None
    }

    fn branch(&mut self, _stage: &mut staged::Concrete, _condition: bool) -> Infallible {
        unreachable!("a run holds no conditional jump")
    }

    fn jump(&mut self, _stage: &mut staged::Concrete) -> Infallible {
        unreachable!("a run holds no jump")
    }

    fn boolean(&mut self, _stage: &mut staged::Concrete, condition: bool) -> (u64, ()) {
        (Value::boolean(condition).to_bits(), ())
    }

    fn wasm_read(&mut self, _stage: &mut staged::Concrete, register: Register, rep: staged::WasmRep) -> Option<u64> {
        let value = self.registers.get(register);
        Some(match rep {
            staged::WasmRep::Int32 => u64::from(value.as_int()? as u32),
            staged::WasmRep::Bits64 => value.bits64()?,
            staged::WasmRep::RawBits64 | staged::WasmRep::Tagged => value.to_bits(),
        })
    }

    fn wasm_write(&mut self, _stage: &mut staged::Concrete, register: Register, word: u64, rep: staged::WasmRep) -> Option<u64> {
        let value = match rep {
            staged::WasmRep::Int32 => self.integers.encode(word as u32 as i32),
            staged::WasmRep::Bits64 => Value::from_bits64(word)?,
            staged::WasmRep::RawBits64 | staged::WasmRep::Tagged => Value::from_bits(word),
        };
        self.registers.set(register, value);
        Some(value.to_bits())
    }

    /// The 16 bytes of the vector `register` holds in the bank, which the builder entered before
    /// the row reads it.
    fn vector(&mut self, _stage: &mut staged::Concrete, register: Register) -> Option<[u8; crate::wasm::V128_BYTES]> {
        // SAFETY: the builder enters every register a row reads as a vector before the row runs.
        Some(unsafe { self.bank[usize::from(register)].assume_init() }.to_le_bytes())
    }

    /// The 16 bytes of the v128 constant `constant` names.
    fn vector_constant(&mut self, _stage: &mut staged::Concrete, constant: staged::Constant) -> Option<[u8; crate::wasm::V128_BYTES]> {
        self.heap.v128(constant.value)
    }

    /// Define `register` with the vector `bytes` in the bank. No tagged word is made: the run
    /// reads the vector from the bank, so the word the row answers is a placeholder.
    fn set_vector(&mut self, _stage: &mut staged::Concrete, register: Register, bytes: [u8; crate::wasm::V128_BYTES]) -> Option<u64> {
        self.bank[usize::from(register)].write(u128::from_le_bytes(bytes));
        Some(0)
    }

    /// The memory an admitted effective address names: the run's view, memory32 as admitted
    /// (`admit`). Its bytes are the view's bytes; the register names the same memory.
    fn wasm_memory(&mut self, _stage: &mut staged::Concrete, _register: Register) -> Option<(&mut [u8], bool)> {
        // SAFETY: the view was resolved in this lane run, and this frame holds no other borrow of it.
        let (bytes, length) = unsafe { self.memory.extent() };
        if length == 0 {
            return Some((Default::default(), false));
        }
        // SAFETY: as above; the view's bytes are `length` long.
        Some((unsafe { std::slice::from_raw_parts_mut(bytes, length) }, false))
    }
}

impl staged::GlobalFrame<staged::Concrete> for RegionFrame<'_> {}

impl staged::CallFrame<staged::Concrete> for RegionFrame<'_> {}

impl staged::PropertyFrame<staged::Concrete> for RegionFrame<'_> {
    type Plans = staged::NoPlans;

    fn plans(&self) -> staged::NoPlans {
        staged::NoPlans
    }
}

impl staged::ElementFrame<staged::Concrete> for RegionFrame<'_> {}

/// The numeric operands of a run: its rows read none, since a run's numeric work is its Wasm
/// rows and its moves, which read the register file through `Frame`.
impl staged::NumericFrame<staged::Concrete> for RegionFrame<'_> {
    fn int_operand(&mut self, _stage: &mut staged::Concrete, _operand: Operand, _miss: ()) -> Option<u64> {
        None
    }
    fn number_operand(&mut self, _stage: &mut staged::Concrete, _operand: Operand, _miss: ()) -> Option<u64> {
        None
    }
    fn prove_number(&mut self, _stage: &mut staged::Concrete, _register: Register, _miss: ()) -> Option<()> {
        None
    }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

/// The staged row of `operation` on the run's frame: the Wasm vector, integer and access rows by
/// their own entries, every other row out of line (`staged::step_out_of_line`), so the region
/// loop does not inline the interpreter's whole row dispatch.
#[inline(always)]
fn run_row(stage: &mut staged::Concrete, frame: &mut RegionFrame<'_>, operation: Row) -> staged::Step<u64, Infallible> {
    match operation {
        Row::WasmVector(_) => staged::step_wasm_vector(stage, frame, operation),
        Row::WasmInt(_) => staged::step_wasm_int(stage, frame, operation),
        Row::WasmAccess(_) => staged::step_wasm_access(stage, frame, operation),
        _ => staged::step_out_of_line(stage, frame, operation),
    }
}

impl VectorRegion {
    pub(super) fn first(&self) -> usize {
        self.first
    }

    /// Box the registers of `leave` from the bank.
    #[cold]
    fn box_out(&self, leave: Leave, bank: &Bank, vm: &mut Vm, r: Registers) {
        for &register in &self.spills[leave.start as usize..leave.end as usize] {
            // SAFETY: a leaving set names registers the bank holds there.
            let bits = unsafe { bank[usize::from(register)].assume_init() };
            r.set(register, vm.heap.alloc_v128(bits.to_le_bytes()));
        }
        note_allocation(vm);
    }

    /// Run the steps; `Err(at)` names the record the general path executes, once the
    /// registers it may read hold their values.
    #[inline]
    fn run(
        &self,
        vm: &mut Vm,
        r: Registers,
        memory: MemoryView,
        integers: crate::value::IntegerEncoding,
        may_loop: bool,
        below_floor: impl Fn() -> bool,
    ) -> Result<(), u32> {
        let mut bank: Bank = [std::mem::MaybeUninit::uninit(); BANK_REGISTERS];
        // SAFETY (bank reads): the builder enters every register before a step reads it
        // from the bank, and names only registers below `BANK_REGISTERS`.
        macro_rules! get {
            ($register:expr) => {
                unsafe { bank.get_unchecked(usize::from($register)).assume_init() }
            };
        }
        macro_rules! put {
            ($register:expr, $bits:expr) => {{
                let bits: u128 = $bits;
                unsafe { bank.get_unchecked_mut(usize::from($register)).write(bits) };
            }};
        }
        // Whether this is a repeated iteration of the run's own loop (`RegionLoop`).
        let mut repeated = false;
        macro_rules! leave {
            ($leave:expr, $at:expr) => {{
                if repeated && let Some(looping) = &self.looping {
                    self.box_out(looping.carried, &bank, vm, r);
                }
                self.box_out($leave, &bank, vm, r);
                return Err($at);
            }};
        }
        let place = |bank: &Bank, place: Place| match place {
            // SAFETY: as for `get!`.
            Place::Bank(register) => unsafe { bank.get_unchecked(usize::from(register)).assume_init() },
            Place::I32(register) => u128::from(r.i32(register) as u32),
        };
        let mut stage = staged::Concrete::new(std::ptr::null());
        loop {
            for step in &self.steps {
                match *step {
                    // A repeated iteration's bank already holds every register the run unboxes,
                    // newest (`vector_regions` admits only such loops).
                    Step::Unbox { .. } if repeated => {}
                    Step::Unbox { register, at, leave } => match vm.heap.v128(r.get(register)) {
                        Some(bytes) => put!(register, u128::from_le_bytes(bytes)),
                        None => leave!(leave, at),
                    },
                    Step::Row { op, at, leave } => {
                        let outcome = {
                            let mut frame = RegionFrame { bank: &mut bank, registers: r, heap: &vm.heap, memory, integers };
                            run_row(&mut stage, &mut frame, self.rows[op as usize])
                        };
                        match outcome {
                            staged::Step::Value(_) => {}
                            staged::Step::Unresolved => leave!(leave, at),
                            staged::Step::Control(never) => match never {},
                        }
                    }
                    Step::Lanes { kernel, lane, a, b, c } => {
                        let left = place(&bank, b);
                        let right = c.map_or(0, |c| place(&bank, c));
                        r.set(a, integers.encode(kernel.apply_bits(left, right, lane) as u32 as i32));
                    }
                    Step::VectorMove { a, b } => put!(a, get!(b)),
                    Step::Constant { a, value } => r.set(a, value),
                    Step::VectorConstant { a, bits } => put!(a, bits),
                    Step::Load { a, address, at, leave } => {
                        match r.get(address).bits64().and_then(|address| vector_bytes(memory, address)) {
                            // SAFETY: `vector_bytes` keeps the vector's bytes in the memory.
                            Some(bytes) => put!(a, u128::from_le_bytes(unsafe { bytes.read() })),
                            None => leave!(leave, at),
                        }
                    }
                    Step::Store { value, address, at, leave } => {
                        match r.get(address).bits64().and_then(|address| vector_bytes(memory, address)) {
                            Some(bytes) => {
                                let bits = get!(value);
                                // SAFETY: as for `Step::Load`.
                                unsafe { bytes.write(bits.to_le_bytes()) }
                            }
                            None => leave!(leave, at),
                        }
                    }
                }
            }
            // The run's own back edge, unless a safepoint is due there: then, as when the loop
            // exits, the closing jump's record runs after the run.
            let Some(looping) = self.looping.filter(|_| may_loop) else { break };
            let right = match looping.right {
                I32Right::Register(register) => r.i32(register),
                I32Right::Immediate(value) => value,
            };
            if looping.comparison.evaluate(r.i32(looping.left), right) != Ok(1) || below_floor() {
                break;
            }
            repeated = true;
        }
        if self.end.start != self.end.end {
            self.box_out(self.end, &bank, vm, r);
        }
        Ok(())
    }
}

lane_handler! {
    /// A vector region (`VectorRegion`): its run of records as one, continuing after them,
    /// or at the record the general path executes.
    fn vector_region_row<>(cx, r, ip, i, view) {
        // SAFETY: the view owns the region its record names (`LaneView::vector_regions`).
        let region = unsafe { &*i.vector_region() };
        let vm = unsafe { vm(cx) };
        // A run that credits back edges to the Wasm tier (`LaneContext::backedges_left`)
        // leaves them to the closing jump.
        let may_loop = unsafe { (*cx).backedges_left }.is_none();
        if let Err(at) = region.run(vm, r, view.memory, view.integers, may_loop, || {
            // A loop iteration is a backward jump: it spends fuel as one would, leaving the
            // jump that spends the last unit (`script_fuel_out`) to the record itself.
            // SAFETY: the context outlives the lane run.
            unsafe { if (*cx).fuel == 0 { true } else { (*cx).fuel -= 1; false } }
        }) {
            // SAFETY: `at` is a record of the run.
            return exit(unsafe { ip.add(at as usize) });
        }
        let acc = if region.acc == NO_ACCUMULATOR { view.acc } else { Slot::of(r.get(region.acc)) };
        // SAFETY: a run ends before its function's terminator.
        next!(cx, r, unsafe { ip.add(region.records) }, Carried { acc, ..view })
    }
}

/// The handler of a region's first record.
pub(super) fn region_handler() -> Handler {
    vector_region_row
}

/// What an admitted instruction is to a run: the admission decision, with the record's staged
/// operation decoded once (`decode_row`) where a row computes it.
#[derive(Clone, Copy)]
enum Admitted {
    /// A SIMD operator: its row where it yields a vector; the kernel plans the bank and the
    /// operands, and an i32 result is computed by `Step::Lanes`.
    Simd { kernel: &'static SimdKernel, lane: u8, row: Row },
    Shuffle { row: Row },
    Move { row: Row },
    Constant(Value),
    VectorConstant(u128),
    I32 { operator: I32BinaryOperator, immediate: Option<i32>, row: Row },
    Address { row: Row },
    Load,
    Store,
}

/// The vector and i32 types a region step reads and writes.
fn admitted_type(ty: crate::WasmType) -> bool {
    matches!(ty, crate::WasmType::V128 | crate::WasmType::I32)
}

/// The staged operation `instruction` denotes, decoded with the run's constants: its constant
/// operands index `constants` as the lane's own do.
fn decode_row(instruction: WideInstruction, constants: &[Value]) -> Option<Row> {
    let constant_at = |index: usize| constants.get(index).map(|&value| staged::Constant::untyped(value));
    let constant_at: &dyn Fn(usize) -> Option<staged::Constant> = &constant_at;
    staged::Operation::decode(
        instruction,
        staged::DecodeContext { constant_at: Some(constant_at), ..staged::DecodeContext::EMPTY },
    )
}

/// What `instruction` is to a run, or `None` for an instruction that ends one.
fn admit(
    instruction: WideInstruction,
    constants: &[Value],
    heap: &crate::heap::Heap,
    memory: Option<u16>,
) -> Option<Admitted> {
    let op = instruction.op();
    let constant = || constants.get(instruction.constant_index()).copied();
    if let Some(operator) = I32BinaryOperator::from_register_op(op) {
        return Some(Admitted::I32 { operator, immediate: None, row: decode_row(instruction, constants)? });
    }
    if let Some(operator) = I32BinaryOperator::from_immediate_op(op) {
        let immediate = Some(instruction.imm() as i32);
        return Some(Admitted::I32 { operator, immediate, row: decode_row(instruction, constants)? });
    }
    let admitted = if staged::wasm_simd(op) {
        let (kernel, lane) = SimdKernel::of(instruction.imm())?;
        if !(admitted_type(kernel.left)
            && kernel.right.is_none_or(admitted_type)
            && admitted_type(kernel.result_type(lane)))
        {
            return None;
        }
        Admitted::Simd { kernel, lane, row: decode_row(instruction, constants)? }
    } else if staged::wasm_simd_shuffle(op) {
        // The lane indices are a v128 constant: the row reads them from the heap.
        heap.v128(constant()?)?;
        Admitted::Shuffle { row: decode_row(instruction, constants)? }
    } else if staged::copies_register(op) {
        Admitted::Move { row: decode_row(instruction, constants)? }
    } else if staged::loads_constant(op) {
        let value = constant()?;
        match heap.v128(value) {
            Some(bytes) => Admitted::VectorConstant(u128::from_le_bytes(bytes)),
            None => Admitted::Constant(value),
        }
    } else if staged::wasm_memory_address(op) && Some(instruction.c()) == memory {
        Admitted::Address { row: decode_row(instruction, constants)? }
    } else if staged::wasm_memory_load(op)
        && Some(instruction.b()) == memory
        && MemoryLoad::from_tag(instruction.imm()) == Some(MemoryLoad::V128Load)
    {
        Admitted::Load
    } else if staged::wasm_memory_store(op)
        && Some(instruction.b()) == memory
        && MemoryStore::from_tag(instruction.imm()) == Some(MemoryStore::V128Store)
    {
        Admitted::Store
    } else {
        return None;
    };
    Some(admitted)
}

/// A run under construction: the registers its bank holds after the steps so far, the
/// constants registers hold, and those whose slots still lack their constant (`pending`).
struct Builder<'a> {
    live: &'a RegisterMasks,
    start: usize,
    steps: Vec<Step>,
    rows: Vec<Row>,
    spills: Vec<u16>,
    bank: RegisterSet,
    constants: [Option<Value>; BANK_REGISTERS],
    pending: RegisterSet,
    vectors: usize,
    last_leave: Option<Leave>,
    /// Registers the run unboxes, and registers it writes as scalars.
    unboxed: RegisterSet,
    scalar_written: RegisterSet,
}

impl Builder<'_> {
    fn at(&self, pc: usize) -> u32 {
        (pc - self.start) as u32
    }

    /// Write the pending constants of `registers` to their slots.
    fn settle(&mut self, registers: RegisterSet) {
        let settled = self.pending & registers;
        for register in (0..BANK_REGISTERS as u16).filter(|&register| register_in(settled, usize::from(register))) {
            let value = self.constants[usize::from(register)].expect("a pending register holds a constant");
            self.steps.push(Step::Constant { a: register, value });
        }
        self.pending &= !settled;
    }

    /// Before the instruction at `pc`, where the general path may execute it: settle the
    /// constants liveness reads, and name the bank's registers it reads, which leaving
    /// boxes. Leaving points with the same set share its range.
    fn leave(&mut self, pc: usize) -> Leave {
        let live = self.live.get(pc).unwrap_or(RegisterSet::MAX);
        self.settle(live);
        let boxed = self.bank & live;
        let registers =
            (0..BANK_REGISTERS as u16).filter(|&register| register_in(boxed, usize::from(register)));
        if let Some(last) = self.last_leave
            && self.spills[last.start as usize..last.end as usize].iter().copied().eq(registers.clone())
        {
            return last;
        }
        let start = self.spills.len() as u32;
        self.spills.extend(registers);
        let leave = Leave { start, end: self.spills.len() as u32 };
        self.last_leave = Some(leave);
        leave
    }

    /// The loop `jump`, the instruction after the run, closes when it jumps back to the run's
    /// first record `start` on an i32 comparison of registers outside the bank, and every
    /// register the run unboxes ends the run in the bank, never written as a scalar: a
    /// repeated iteration then finds each newest in the bank. Settles every pending constant,
    /// so a repeated iteration's slot reads see what the previous one loaded.
    fn closing_loop(&mut self, jump: WideInstruction, start: usize) -> Option<RegionLoop> {
        let op = jump.op();
        let (comparison, right) = match I32BinaryOperator::from_jump_op(op) {
            Some(comparison) => (comparison, I32Right::Register(jump.b())),
            None => (
                I32BinaryOperator::from_immediate_jump_op(op)?,
                I32Right::Immediate(i32::from(jump.b() as i16)),
            ),
        };
        let left = jump.a();
        let in_bank = |register: u16| usize::from(register) < BANK_REGISTERS && register_in(self.bank, usize::from(register));
        let right_in_bank = matches!(right, I32Right::Register(register) if in_bank(register));
        if jump.jump_target() as usize != start
            || in_bank(left)
            || right_in_bank
            || self.unboxed & !self.bank != RegisterSet::default()
            || self.unboxed & self.scalar_written != RegisterSet::default()
        {
            return None;
        }
        self.settle(self.pending);
        let start = self.spills.len() as u32;
        let bank = self.bank;
        self.spills.extend((0..BANK_REGISTERS as u16).filter(|&register| register_in(bank, usize::from(register))));
        let carried = Leave { start, end: self.spills.len() as u32 };
        self.last_leave = None;
        Some(RegionLoop { comparison, left, right, carried })
    }

    /// Read `register` as a vector: enter it into the bank first.
    fn vector(&mut self, pc: usize, register: u16) {
        if !register_in(self.bank, usize::from(register)) {
            let leave = self.leave(pc);
            self.steps.push(Step::Unbox { register, at: self.at(pc), leave });
            self.bank |= RegisterSet::bit(usize::from(register));
            self.unboxed |= RegisterSet::bit(usize::from(register));
        }
    }

    /// Read `register`'s slot as a scalar: settle its constant first.
    fn scalar(&mut self, register: u16) {
        self.settle(RegisterSet::bit(usize::from(register)));
    }

    fn write(&mut self, register: u16, vector: bool, constant: Option<Value>) {
        let bit = RegisterSet::bit(usize::from(register));
        self.pending &= !bit;
        if vector {
            self.bank |= bit;
            self.vectors += 1;
        } else {
            self.bank &= !bit;
            self.scalar_written |= bit;
        }
        self.constants[usize::from(register)] = constant;
    }

    fn in_bank(&self, register: u16) -> bool {
        register_in(self.bank, usize::from(register))
    }

    /// Add the staged row `row` of the record at `pc`, which leaves with `leave` boxed.
    fn row(&mut self, pc: usize, row: Row, leave: Leave) {
        self.rows.push(row);
        self.steps.push(Step::Row { op: self.rows.len() as u32 - 1, at: self.at(pc), leave });
    }

    /// Add the instruction at `pc`; `None` ends the run before it, having added nothing.
    fn add(&mut self, pc: usize, instruction: WideInstruction, admitted: Admitted) -> Option<()> {
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        if [a, b, c].into_iter().any(|register| usize::from(register) >= BANK_REGISTERS) {
            return None;
        }
        let vector = |ty| ty == crate::WasmType::V128;
        // A scalar read of a register the bank holds would need it boxed mid-run.
        let scalar_reads: &[u16] = match admitted {
            Admitted::Simd { kernel, .. } => match (vector(kernel.left), kernel.right.map(vector)) {
                (false, Some(false)) => &[b, c],
                (false, _) => &[b],
                (true, Some(false)) => &[c],
                (true, _) => &[],
            },
            Admitted::I32 { immediate: None, .. } => &[b, c],
            Admitted::I32 { immediate: Some(_), .. } | Admitted::Address { .. } => &[b],
            Admitted::Load | Admitted::Store => &[c],
            _ => &[],
        };
        if scalar_reads.iter().any(|&register| self.in_bank(register)) {
            return None;
        }
        match admitted {
            Admitted::Simd { kernel, lane, row } => {
                let result = kernel.result_type(lane);
                for (register, ty) in [(b, Some(kernel.left)), (c, kernel.right)] {
                    match ty {
                        Some(ty) if vector(ty) => self.vector(pc, register),
                        Some(_) => self.scalar(register),
                        None => {}
                    }
                }
                if vector(result) {
                    let leave = self.leave(pc);
                    self.row(pc, row, leave);
                } else {
                    let place = |register, ty| if vector(ty) { Place::Bank(register) } else { Place::I32(register) };
                    let c = kernel.right.map(|ty| place(c, ty));
                    self.steps.push(Step::Lanes { kernel, lane, a, b: place(b, kernel.left), c });
                }
                self.write(a, vector(result), None);
            }
            Admitted::Shuffle { row } => {
                self.vector(pc, b);
                self.vector(pc, c);
                let leave = self.leave(pc);
                self.row(pc, row, leave);
                self.write(a, true, None);
            }
            Admitted::Move { row } => {
                let constant = self.constants[usize::from(b)];
                if self.in_bank(b) {
                    self.steps.push(Step::VectorMove { a, b });
                    self.write(a, true, None);
                    self.vectors -= 1;
                } else if register_in(self.pending, usize::from(b)) {
                    // The copy is the constant, pending as its source is.
                    self.write(a, false, constant);
                    self.pending |= RegisterSet::bit(usize::from(a));
                } else {
                    let leave = self.leave(pc);
                    self.row(pc, row, leave);
                    self.write(a, false, constant);
                }
            }
            Admitted::Constant(value) => {
                self.write(a, false, Some(value));
                self.pending |= RegisterSet::bit(usize::from(a));
            }
            Admitted::VectorConstant(bits) => {
                self.steps.push(Step::VectorConstant { a, bits });
                self.write(a, true, None);
            }
            Admitted::I32 { operator, immediate, row } => {
                self.scalar(b);
                if immediate.is_none() {
                    self.scalar(c);
                }
                // Only a division can trap, which its operator shows dividing by zero; any
                // other operator never leaves, so its row needs no registers boxed.
                let leave = if operator.evaluate(1, 0).is_err() {
                    self.leave(pc)
                } else {
                    Leave { start: 0, end: 0 }
                };
                self.row(pc, row, leave);
                self.write(a, false, None);
            }
            Admitted::Address { row } => {
                self.scalar(b);
                let leave = self.leave(pc);
                self.row(pc, row, leave);
                self.write(a, false, None);
            }
            Admitted::Load => {
                self.scalar(c);
                let leave = self.leave(pc);
                self.steps.push(Step::Load { a, address: c, at: self.at(pc), leave });
                self.write(a, true, None);
            }
            Admitted::Store => {
                self.scalar(c);
                self.vector(pc, a);
                let leave = self.leave(pc);
                self.steps.push(Step::Store { value: a, address: c, at: self.at(pc), leave });
            }
        }
        Some(())
    }
}

/// The vector regions of a Wasm function's records, by their first record: each maximal
/// run of admitted instructions that a jump enters only at its start and that computes at
/// least `REGION_MIN_VECTORS` vectors. A run holds no record the general path executes
/// (`exits`); `produced` is each record's carried accumulator register, as the view
/// selected it.
pub(super) fn vector_regions(
    decoded: &[WideInstruction],
    constants: &[Value],
    heap: &crate::heap::Heap,
    memory_register: Option<u16>,
    live: &RegisterMasks,
    jump_targets: &InstructionSet,
    exits: &[bool],
    produced: &[Option<u16>],
) -> Vec<VectorRegion> {
    let mut regions = Vec::new();
    let mut pc = 0;
    while pc < decoded.len() {
        let mut builder = Builder {
            live,
            start: pc,
            steps: Vec::new(),
            rows: Vec::new(),
            spills: Vec::new(),
            bank: RegisterSet::default(),
            constants: [None; BANK_REGISTERS],
            pending: RegisterSet::default(),
            vectors: 0,
            last_leave: None,
            unboxed: RegisterSet::default(),
            scalar_written: RegisterSet::default(),
        };
        let mut end = pc;
        while end < decoded.len()
            && (end == pc || !jump_targets.contains(end))
            && !exits[end]
            && let Some(admitted) = admit(decoded[end], constants, heap, memory_register)
            && builder.add(end, decoded[end], admitted).is_some()
        {
            end += 1;
        }
        if builder.vectors >= REGION_MIN_VECTORS {
            let looping = decoded.get(end).and_then(|&jump| builder.closing_loop(jump, pc));
            let leave = builder.leave(end);
            let acc = produced[end - 1].unwrap_or(NO_ACCUMULATOR);
            regions.push(VectorRegion {
                first: pc,
                steps: builder.steps.into_boxed_slice(),
                rows: builder.rows.into_boxed_slice(),
                spills: builder.spills.into_boxed_slice(),
                records: end - pc,
                end: leave,
                acc,
                looping,
            });
        }
        pc = end.max(pc + 1);
    }
    regions
}

#[cfg(test)]
mod tests {
    use super::super::super::Vm;
    use crate::{WasmTrap, WasmValue};
    use wasmparser::{MemArg, Operator, ValType};

    const SOURCE: i32 = 64;
    const TARGET: i32 = 256;
    const PAGE: i32 = 65_536;
    /// The lanes the source vectors hold: carries out of every lane, sign bits, and a low
    /// word whose products overflow u32.
    const LEFT: [u64; 2] = [u64::MAX, 0x8000_0000_0000_0001];
    const RIGHT: [u64; 2] = [1, 0x8000_0000_ffff_ffff];

    fn vector(offset: u64) -> MemArg {
        MemArg { align: 4, max_align: 4, offset, memory: 0 }
    }
    fn word(offset: u64) -> MemArg {
        MemArg { align: 2, max_align: 2, offset, memory: 0 }
    }
    fn local(index: u32) -> Operator<'static> {
        Operator::LocalGet { local_index: index }
    }
    fn constant(value: i32) -> Operator<'static> {
        Operator::I32Const { value }
    }
    fn lanes([low, high]: [u64; 2]) -> u128 {
        u128::from(low) | (u128::from(high) << 64)
    }
    /// `_mm_mul_epu32`'s shuffle: the low u32 of each u64 lane, twice.
    fn low_words() -> [u8; 16] {
        std::array::from_fn(|byte| [0u8, 2][(byte / 4) % 2] * 4 + (byte % 4) as u8)
    }

    /// `body` storing the vector its operators leave at `TARGET + slot * 16`.
    fn store_at(slot: u64, mut body: Vec<Operator<'static>>) -> Vec<Operator<'static>> {
        let mut operators = vec![local(1)];
        operators.append(&mut body);
        operators.push(Operator::V128Store { memarg: vector(slot * 16) });
        operators
    }

    /// Straight-line i64x2 arithmetic, shifts past the lane width (folded and dynamic),
    /// rotations by 0, 64 and 24 in their shift-or form, and the shuffled extending
    /// multiply: `lanes(source, target, count)`, the source vectors in locals 3 and 4.
    fn region_body_in_lane() -> Vec<Operator<'static>> {
        let (v, w) = (|| local(3), || local(4));
        let rotation = |left: i32, right: i32| {
            vec![v(), constant(left), Operator::I64x2Shl, v(), constant(right), Operator::I64x2ShrU, Operator::V128Or]
        };
        let mut body = vec![
            local(0),
            Operator::V128Load { memarg: vector(0) },
            Operator::LocalSet { local_index: 3 },
            local(0),
            Operator::V128Load { memarg: vector(16) },
            Operator::LocalSet { local_index: 4 },
        ];
        for (slot, operators) in [
            vec![v(), w(), Operator::I64x2Add],
            vec![v(), w(), Operator::I64x2Sub],
            vec![v(), w(), Operator::I64x2Mul],
            vec![v(), constant(69), Operator::I64x2Shl],
            vec![v(), local(2), Operator::I64x2ShrS],
            vec![v(), constant(64), Operator::I64x2ShrU],
            rotation(0, 64),
            rotation(64, 0),
            rotation(40, 24),
            vec![
                v(),
                v(),
                Operator::I8x16Shuffle { lanes: low_words() },
                w(),
                w(),
                Operator::I8x16Shuffle { lanes: low_words() },
                Operator::I64x2ExtMulLowI32x4U,
            ],
            vec![v(), w(), Operator::I32x4Add],
        ]
        .into_iter()
        .enumerate()
        {
            body.extend(store_at(slot as u64, operators));
        }
        body.push(Operator::End);
        body
    }

    /// What `region_body_in_lane` stores, slot by slot, for shift count `count`.
    fn expected_lanes(count: i32) -> Vec<u128> {
        let map = |operation: &dyn Fn(u64, u64) -> u64| {
            lanes([operation(LEFT[0], RIGHT[0]), operation(LEFT[1], RIGHT[1])])
        };
        let words = |value: u64| [value as u32, (value >> 32) as u32];
        let (left, right) = (lanes(LEFT), lanes(RIGHT));
        let quads: [u32; 4] = std::array::from_fn(|lane| {
            ((left >> (32 * lane)) as u32).wrapping_add((right >> (32 * lane)) as u32)
        });
        vec![
            map(&|a, b| a.wrapping_add(b)),
            map(&|a, b| a.wrapping_sub(b)),
            map(&|a, b| a.wrapping_mul(b)),
            map(&|a, _| a << (69 % 64)),
            map(&|a, _| ((a as i64) >> (count as u32 % 64)) as u64),
            map(&|a, _| a),
            map(&|a, _| a),
            map(&|a, _| a),
            map(&|a, _| a.rotate_left(40)),
            map(&|a, b| u64::from(words(a)[0]) * u64::from(words(b)[0])),
            quads.iter().enumerate().fold(0, |bits, (lane, &value)| bits | (u128::from(value) << (32 * lane))),
        ]
    }

    /// A vector doubled and stored, an i32 division whose trap leaves it stored, and the
    /// vector, live across the division, stored again plus one:
    /// `divide(target, dividend, divisor)`.
    fn division_body() -> Vec<Operator<'static>> {
        vec![
            local(0),
            Operator::V128Load { memarg: vector(0) },
            Operator::LocalTee { local_index: 3 },
            local(3),
            Operator::I64x2Add,
            Operator::LocalSet { local_index: 3 },
            local(0),
            local(3),
            Operator::V128Store { memarg: vector(0) },
            local(1),
            local(2),
            Operator::I32DivS,
            Operator::LocalSet { local_index: 4 },
            local(0),
            local(3),
            local(3),
            Operator::I64x2Add,
            Operator::V128Store { memarg: vector(16) },
            local(0),
            local(4),
            Operator::I32Store { memarg: word(32) },
            Operator::End,
        ]
    }

    /// A vector loaded from `source` and stored at `target`: `copy(source, target)`.
    fn copy_body() -> Vec<Operator<'static>> {
        vec![
            local(1),
            local(0),
            Operator::V128Load { memarg: vector(0) },
            local(0),
            Operator::V128Load { memarg: vector(0) },
            Operator::I64x2Add,
            Operator::V128Store { memarg: vector(0) },
            Operator::End,
        ]
    }

    struct Fixture {
        vm: Vm,
        instance: crate::WasmInstance,
        module: crate::WasmModule,
    }

    const LANES: u32 = 0;
    const STORE: u32 = 1;
    const LOAD: u32 = 2;
    const DIVIDE: u32 = 3;
    const COPY: u32 = 4;

    fn fixture(mode: crate::ExecutionMode) -> Fixture {
        let body = |params: usize, locals: Vec<ValType>, result: bool, operators: Vec<Operator<'static>>| {
            crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                    params: vec![crate::WasmType::I32; params],
                    results: if result { vec![crate::WasmType::I32] } else { vec![] },
                }),
                locals,
                operators: operators.into_iter().map(Ok),
            }
        };
        let module = crate::Engine::lower_wasm_module_with_state(
            "vector-regions",
            [
                body(3, vec![ValType::V128, ValType::V128], false, region_body_in_lane()),
                body(2, vec![], false, vec![local(0), local(1), Operator::I32Store { memarg: word(0) }, Operator::End]),
                body(1, vec![], true, vec![local(0), Operator::I32Load { memarg: word(0) }, Operator::End]),
                body(3, vec![ValType::V128, ValType::I32], false, division_body()),
                body(2, vec![], false, copy_body()),
            ],
            &crate::WasmTypes::default(),
            &[],
            &[crate::WasmMemory {
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(1),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
                import: None,
            }],
            &[],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.programs.compile_wasm_modules_at_first_entry();
        vm.set_execution_mode(mode);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        Fixture { vm, instance, module }
    }

    impl Fixture {
        fn call(&mut self, function: u32, args: &[i32]) -> Result<Option<WasmValue>, crate::JsError> {
            let args: Vec<WasmValue> = args.iter().copied().map(WasmValue::I32).collect();
            self.vm.invoke_wasm(&self.instance, function, &args)
        }
        fn write(&mut self, address: i32, bits: u128) {
            for index in 0..4 {
                self.call(STORE, &[address + index * 4, (bits >> (32 * index)) as u32 as i32]).unwrap();
            }
        }
        fn read(&mut self, address: i32) -> u128 {
            (0..4).fold(0, |bits, index| {
                let Some(WasmValue::I32(word)) = self.call(LOAD, &[address + index * 4]).unwrap() else {
                    unreachable!("an i32 load")
                };
                bits | (u128::from(word as u32) << (32 * index))
            })
        }
        /// How many vector regions `function`'s lane view holds.
        fn regions(&self, function: u32) -> usize {
            let program = self.vm.programs.find_shared(&self.module.program).expect("the module's program");
            let view = self.vm.view_any_in_lane(program, function).expect("a lane view");
            // SAFETY: views live as long as their program entry.
            unsafe { &*view }.vector_regions.len()
        }
    }

    #[test]
    fn vector_regions_compute_lanes_exactly() {
        for count in [0, 5, 63, 64, 70, -1] {
            for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
                let mut fixture = fixture(mode);
                fixture.write(SOURCE, lanes(LEFT));
                fixture.write(SOURCE + 16, lanes(RIGHT));
                fixture.call(LANES, &[SOURCE, TARGET, count]).unwrap();
                let stored: Vec<u128> = (0..11).map(|slot| fixture.read(TARGET + slot * 16)).collect();
                assert_eq!(stored, expected_lanes(count), "count {count} in {mode:?}");
            }
        }
        let fixture = fixture(crate::ExecutionMode::Interpreted);
        assert!(fixture.regions(LANES) > 0, "the lane body runs as a vector region");
    }

    #[test]
    fn vector_region_traps_leave_earlier_effects_and_boxed_vectors() {
        let doubled = lanes([LEFT[0].wrapping_add(LEFT[0]), LEFT[1].wrapping_add(LEFT[1])]);
        let quadrupled = lanes([(doubled as u64).wrapping_mul(2), ((doubled >> 64) as u64).wrapping_mul(2)]);
        for (dividend, divisor, trap) in [
            (7, 0, Some(WasmTrap::IntegerDivideByZero)),
            (i32::MIN, -1, Some(WasmTrap::IntegerOverflow)),
            (i32::MIN, 1, None),
            (-7, 2, None),
        ] {
            let mut fixture = fixture(crate::ExecutionMode::Interpreted);
            fixture.write(TARGET, lanes(LEFT));
            fixture.write(TARGET + 16, 0);
            let result = fixture.call(DIVIDE, &[TARGET, dividend, divisor]);
            assert_eq!(fixture.read(TARGET), doubled, "the store before the division");
            match trap {
                Some(trap) => {
                    assert_eq!(result.unwrap_err().wasm_trap(), Some(trap), "{dividend} / {divisor}");
                    assert_eq!(fixture.read(TARGET + 16), 0, "nothing after the trap");
                }
                None => {
                    result.unwrap();
                    assert_eq!(fixture.read(TARGET + 16), quadrupled, "the vector live across the division");
                    let Some(WasmValue::I32(quotient)) = fixture.call(LOAD, &[TARGET + 32]).unwrap() else {
                        unreachable!("an i32 load")
                    };
                    assert_eq!(quotient, dividend.wrapping_div(divisor));
                }
            }
        }
        assert!(fixture(crate::ExecutionMode::Interpreted).regions(DIVIDE) > 0);
    }

    #[test]
    fn vector_region_accesses_trap_out_of_bounds() {
        let mut fixture = fixture(crate::ExecutionMode::Interpreted);
        fixture.write(SOURCE, lanes(LEFT));
        let last = PAGE - 16;
        fixture.write(last, lanes(RIGHT));
        // A store whose last bytes pass the end traps and writes nothing; a load does too.
        for (source, target) in [(SOURCE, last + 1), (SOURCE, PAGE - 8), (SOURCE, -16), (last + 1, TARGET), (PAGE, TARGET)] {
            let error = fixture.call(COPY, &[source, target]).unwrap_err();
            assert_eq!(error.wasm_trap(), Some(WasmTrap::OutOfBoundsMemory), "copy {source} -> {target}");
            assert_eq!(fixture.read(last), lanes(RIGHT), "memory past the trap is unchanged");
        }
        // The last whole vector of the memory is in bounds.
        fixture.call(COPY, &[SOURCE, last]).unwrap();
        let doubled = lanes([LEFT[0].wrapping_add(LEFT[0]), LEFT[1].wrapping_add(LEFT[1])]);
        assert_eq!(fixture.read(last), doubled);
        assert!(fixture.regions(COPY) > 0);
    }
}
