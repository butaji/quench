//! Straight-line Wasm runs that one lane record executes with their vectors unboxed.
//!
//! A register holds a vector as a boxed cell (`Heap::alloc_v128`), so each SIMD record
//! reads its operands through the heap and allocates its result. Within a run of vector,
//! address and i32 instructions the view fixes which registers hold vectors: the run's
//! record (`lane_vector_region`) keeps each one unboxed in a bank slot named by its
//! register, enters a register into the bank where the run first reads it, and boxes it
//! back only where liveness still reads it: after the run, and before an instruction the
//! general path must execute (a trap, or memory the lane cannot borrow). A constant the run
//! loads is written to its register only where a slot read or liveness needs it, so a
//! shift count folds into its shift. Every other record of the run keeps its own handler,
//! so control arriving inside the run runs it as before. Each step computes its
//! instruction's own semantics (`SimdKernel`, `BitKernel`, `ShuffleForm`,
//! `I32BinaryOperator`, the vector load and store bounds), on bits instead of cells.

use super::*;
use crate::compile::liveness::{RegisterMasks, RegisterSet, register_in};
use crate::wasm::simd::{BitKernel, BitShift, ShuffleForm, SimdKernel};

/// The registers a bank names: liveness' width, which every region register lies within.
const BANK_REGISTERS: usize = RegisterSet::BITS as usize;

/// The fewest vector instructions a run needs to become a region: its record then saves at
/// least one boxed result and one operand read per instruction, against one entry.
const REGION_MIN_VECTORS: usize = 2;

/// The steps before a step that `Builder::fused` folds into it.
const FUSED_STEPS: usize = 2;

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

/// The right operand of an i32 operator.
#[derive(Clone, Copy, Debug)]
enum I32Right {
    Register(u16),
    Immediate(i32),
}

/// One instruction of a run, or the entry of a register into the bank. A step that may
/// leave names its record (`at`, from the run's first) and the registers boxed first.
#[derive(Clone, Copy)]
enum Step {
    Unbox { register: u16, at: u32, leave: Leave },
    Bits { kernel: BitKernel, a: u16, b: u16, c: u16 },
    Count { kernel: BitKernel, a: u16, b: u16, count: u32 },
    Lanes { kernel: &'static SimdKernel, lane: u8, a: Place, b: Place, c: Option<Place> },
    /// A lane operator whose operands and result are all vectors the bank holds: its
    /// kernel's bits form (`SimdKernel::bits`) on bank slots, `c` being `b` for a unary one.
    Vector { bits: fn(u128, u128, u8) -> u128, lane: u8, a: u16, b: u16, c: u16 },
    Shuffle { form: ShuffleForm, a: u16, b: u16, c: u16 },
    /// Two one-operand u32 shuffles and the extending multiply of their results
    /// (`crate::wasm::simd::multiply_words`).
    MultiplyWords { a: u16, b: u16, c: u16, left: [u8; 2], right: [u8; 2] },
    VectorMove { a: u16, b: u16 },
    Move { a: u16, b: u16 },
    Constant { a: u16, value: Value },
    VectorConstant { a: u16, bits: u128 },
    I32 { operator: I32BinaryOperator, a: u16, b: u16, right: I32Right, at: u32, leave: Leave },
    Address { a: u16, index: u16, offset: u64 },
    Load { a: u16, address: u16, at: u32, leave: Leave },
    Store { value: u16, address: u16, at: u32, leave: Leave },
    /// An `Address` whose register only the `Load` or `Store` after it reads, folded into
    /// it: the address is computed in place and written to its register only where the
    /// access leaves, for the general path to read.
    LoadAt { a: u16, address: u16, index: u16, offset: u64, at: u32, leave: Leave },
    StoreAt { value: u16, address: u16, index: u16, offset: u64, at: u32, leave: Leave },
}

/// A run of records one record executes (`lane_vector_region`).
pub(super) struct VectorRegion {
    /// The run's first record.
    first: usize,
    steps: Box<[Step]>,
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

impl VectorRegion {
    pub(super) fn first(&self) -> usize {
        self.first
    }

    /// Box the registers of `leave` from the bank.
    #[cold]
    fn box_out<H: Host>(&self, leave: Leave, bank: &Bank, vm: &mut Vm<H>, r: Registers) {
        for &register in &self.spills[leave.start as usize..leave.end as usize] {
            // SAFETY: a leaving set names registers the bank holds there.
            let bits = unsafe { bank[usize::from(register)].assume_init() };
            r.set(register, vm.heap.alloc_v128(bits.to_le_bytes()));
        }
        note_allocation(vm);
    }

    /// Whether the run's own back edge repeats it: unless a safepoint is due there (`may_loop`
    /// false), the closing comparison holds and no fuel is out (`below_floor`).
    #[inline(always)]
    fn repeats(&self, r: Registers, may_loop: bool, below_floor: impl Fn() -> bool) -> bool {
        let Some(looping) = self.looping.filter(|_| may_loop) else {
            return false;
        };
        let right = match looping.right {
            I32Right::Register(register) => r.i32(register),
            I32Right::Immediate(value) => value,
        };
        looping.comparison.evaluate(r.i32(looping.left), right) == Ok(1) && !below_floor()
    }

    /// Run the steps; `Err(at)` names the record the general path executes, once the
    /// registers it may read hold their values.
    #[inline(always)]
    fn run<H: Host>(
        &self,
        vm: &mut Vm<H>,
        r: Registers,
        memory: MemoryView,
        integers: crate::value::IntegerEncoding,
        may_loop: bool,
        below_floor: impl Fn() -> bool,
    ) -> Result<(), u32> {
        let mut exec = Exec { bank: [std::mem::MaybeUninit::uninit(); BANK_REGISTERS], repeated: false, vm, r, memory, integers };
        loop {
            for step in &self.steps {
                exec.step(self, step)?;
            }
            if !self.repeats(r, may_loop, &below_floor) {
                break;
            }
            exec.repeated = true;
        }
        if self.end.start != self.end.end {
            self.box_out(self.end, &exec.bank, exec.vm, r);
        }
        Ok(())
    }
}

/// A run's execution state: its bank of unboxed vectors, the registers and memory its steps
/// read and write, and whether the run is on a repeated iteration of its loop.
struct Exec<'a, H: Host> {
    bank: Bank,
    repeated: bool,
    vm: &'a mut Vm<H>,
    r: Registers,
    memory: MemoryView,
    integers: crate::value::IntegerEncoding,
}

impl<H: Host> Exec<'_, H> {
    /// The bits of a bank register.
    // SAFETY (bank reads): the builder enters every register before a step reads it from the
    // bank, and names only registers below `BANK_REGISTERS`.
    #[inline(always)]
    fn get(&self, register: u16) -> u128 {
        unsafe { self.bank.get_unchecked(usize::from(register)).assume_init() }
    }

    #[inline(always)]
    fn put(&mut self, register: u16, bits: u128) {
        let bits: u128 = bits;
        unsafe { self.bank.get_unchecked_mut(usize::from(register)).write(bits) };
    }

    /// The bits of a lane operand, from the bank or a slot.
    #[inline(always)]
    fn place(&self, place: Place) -> u128 {
        match place {
            // SAFETY: as for `get`.
            Place::Bank(register) => self.get(register),
            Place::I32(register) => u128::from(self.r.i32(register) as u32),
        }
    }

    /// Leaves the run at record `at`, boxing what the leave names; a repeated iteration also
    /// boxes the registers its loop carries. Returns the record.
    #[inline(always)]
    fn leave(&mut self, region: &VectorRegion, leave: Leave, at: u32) -> u32 {
        if self.repeated && let Some(looping) = &region.looping {
            region.box_out(looping.carried, &self.bank, self.vm, self.r);
        }
        region.box_out(leave, &self.bank, self.vm, self.r);
        at
    }

    /// Runs one step; `Err(at)` leaves the run at record `at`.
    #[inline(always)]
    fn step(&mut self, region: &VectorRegion, step: &Step) -> Result<(), u32> {
        match *step {
            // A repeated iteration's bank already holds every register the run unboxes,
            // newest (`vector_regions` admits only such loops).
            Step::Unbox { .. } if self.repeated => Ok(()),
            Step::Unbox { register, at, leave } => match self.vm.heap.v128(self.r.get(register)) {
                Some(bytes) => {
                    self.put(register, u128::from_le_bytes(bytes));
                    Ok(())
                }
                None => Err(self.leave(region, leave, at)),
            },
            Step::Bits { .. }
            | Step::Count { .. }
            | Step::Vector { .. }
            | Step::Shuffle { .. }
            | Step::MultiplyWords { .. }
            | Step::VectorMove { .. }
            | Step::VectorConstant { .. } => {
                self.vector_step(step);
                Ok(())
            }
            Step::Lanes { .. } | Step::Move { .. } | Step::Constant { .. } | Step::I32 { .. } | Step::Address { .. } => self.scalar_step(region, step),
            Step::Load { .. } | Step::LoadAt { .. } | Step::StoreAt { .. } | Step::Store { .. } => self.memory_step(region, step),
        }
    }

    /// A step that computes a vector in the bank from bank registers.
    #[inline(always)]
    fn vector_step(&mut self, step: &Step) {
        match *step {
            Step::Bits { kernel, a, b, c } => self.put(a, kernel.apply(self.get(b), self.get(c))),
            Step::Count { kernel, a, b, count } => self.put(a, kernel.apply(self.get(b), u128::from(count))),
            Step::Vector { bits, lane, a, b, c } => self.put(a, bits(self.get(b), self.get(c), lane)),
            Step::Shuffle { form, a, b, c } => self.put(a, form.apply(self.get(b), self.get(c))),
            Step::MultiplyWords { a, b, c, left, right } => {
                self.put(a, crate::wasm::simd::multiply_words(self.get(b), self.get(c), left, right))
            }
            Step::VectorMove { a, b } => self.put(a, self.get(b)),
            Step::VectorConstant { a, bits } => self.put(a, bits),
            _ => {}
        }
    }

    /// A step on scalars and lanes: moves and constants, lane operators, i32 operators that may
    /// leave, and addresses.
    #[inline(always)]
    fn scalar_step(&mut self, region: &VectorRegion, step: &Step) -> Result<(), u32> {
        match *step {
            Step::Lanes { kernel, lane, a, b, c } => {
                let left = self.place(b);
                let right = c.map_or(0, |c| self.place(c));
                let bits = kernel.apply_bits(left, right, lane);
                match a {
                    Place::Bank(register) => self.put(register, bits),
                    Place::I32(register) => self.r.set(register, self.integers.encode(bits as u32 as i32)),
                }
            }
            Step::Move { a, b } => self.r.set(a, self.r.get(b)),
            Step::Constant { a, value } => self.r.set(a, value),
            Step::I32 { operator, a, b, right, at, leave } => {
                let right = match right {
                    I32Right::Register(register) => self.r.i32(register),
                    I32Right::Immediate(value) => value,
                };
                match operator.evaluate(self.r.i32(b), right) {
                    Ok(value) => self.r.set(a, self.integers.encode(value)),
                    Err(_) => return Err(self.leave(region, leave, at)),
                }
            }
            Step::Address { a, index, offset } => {
                // A memory32 index zero-extends; with the static offset it stays below
                // 2^33, which a slot holds untagged as `Vm::encode_bits64` would.
                let address = u64::from(self.r.i32(index) as u32) + offset;
                self.r.set(a, Value::from_bits64(address).expect("an untagged memory32 address"));
            }
            _ => {}
        }
        Ok(())
    }

    /// A step that accesses memory: a vector load or store, direct or at an address the run
    /// computed. An access outside the memory leaves the run.
    #[inline(always)]
    fn memory_step(&mut self, region: &VectorRegion, step: &Step) -> Result<(), u32> {
        let memory = self.memory;
        match *step {
            Step::Load { a, address, at, leave } => match self.r.get(address).bits64().and_then(|address| vector_bytes(memory, address)) {
                // SAFETY: `vector_bytes` keeps the vector's bytes in the memory.
                Some(bytes) => self.put(a, u128::from_le_bytes(unsafe { bytes.read() })),
                None => return Err(self.leave(region, leave, at)),
            },
            Step::LoadAt { a, address, index, offset, at, leave } => {
                let effective = u64::from(self.r.i32(index) as u32) + offset;
                match vector_bytes(memory, effective) {
                    // SAFETY: `vector_bytes` keeps the vector's bytes in the memory.
                    Some(bytes) => self.put(a, u128::from_le_bytes(unsafe { bytes.read() })),
                    None => {
                        self.r.set(address, Value::from_bits64(effective).expect("an untagged memory32 address"));
                        return Err(self.leave(region, leave, at));
                    }
                }
            }
            Step::StoreAt { value, address, index, offset, at, leave } => {
                let effective = u64::from(self.r.i32(index) as u32) + offset;
                match vector_bytes(memory, effective) {
                    Some(bytes) => {
                        let bits = self.get(value);
                        // SAFETY: as for `Step::Load`.
                        unsafe { bytes.write(bits.to_le_bytes()) }
                    }
                    None => {
                        self.r.set(address, Value::from_bits64(effective).expect("an untagged memory32 address"));
                        return Err(self.leave(region, leave, at));
                    }
                }
            }
            Step::Store { value, address, at, leave } => match self.r.get(address).bits64().and_then(|address| vector_bytes(memory, address)) {
                Some(bytes) => {
                    let bits = self.get(value);
                    // SAFETY: as for `Step::Load`.
                    unsafe { bytes.write(bits.to_le_bytes()) }
                }
                None => return Err(self.leave(region, leave, at)),
            },
            _ => {}
        }
        Ok(())
    }
}

lane_handler! {
    /// A vector region (`VectorRegion`): its run of records as one, continuing after them,
    /// or at the record the general path executes.
    fn lane_vector_region<>(cx, r, ip, i, view) {
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
pub(super) fn region_handler<H: Host>() -> Handler {
    lane_vector_region::<H>
}

/// What an admitted instruction is to a run.
#[derive(Clone, Copy)]
enum Admitted {
    Simd { kernel: &'static SimdKernel, lane: u8 },
    Shuffle(ShuffleForm),
    Move,
    Constant(Value),
    VectorConstant(u128),
    I32(I32BinaryOperator, Option<i32>),
    Address(u64),
    Load,
    Store,
}

/// The vector and i32 types a region step reads and writes.
fn admitted_type(ty: crate::WasmType) -> bool {
    matches!(ty, crate::WasmType::V128 | crate::WasmType::I32)
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
        return Some(Admitted::I32(operator, None));
    }
    if let Some(operator) = I32BinaryOperator::from_immediate_op(op) {
        return Some(Admitted::I32(operator, Some(instruction.imm() as i32)));
    }
    Some(match op.class() {
        Class::WasmSimd => {
            let (kernel, lane) = SimdKernel::of(instruction.imm())?;
            (admitted_type(kernel.left)
                && kernel.right.is_none_or(admitted_type)
                && admitted_type(kernel.result_type(lane)))
            .then_some(Admitted::Simd { kernel, lane })?
        }
        Class::WasmSimdShuffle => Admitted::Shuffle(ShuffleForm::of(heap.v128(constant()?)?)),
        Class::Move => Admitted::Move,
        Class::LoadConst => {
            let value = constant()?;
            match heap.v128(value) {
                Some(bytes) => Admitted::VectorConstant(u128::from_le_bytes(bytes)),
                None => Admitted::Constant(value),
            }
        }
        Class::WasmMemoryAddress if Some(instruction.c()) == memory => {
            Admitted::Address(constant()?.bits64()?)
        }
        Class::WasmMemoryLoad
            if Some(instruction.b()) == memory
                && MemoryLoad::from_tag(instruction.imm()) == Some(MemoryLoad::V128Load) =>
        {
            Admitted::Load
        }
        Class::WasmMemoryStore
            if Some(instruction.b()) == memory
                && MemoryStore::from_tag(instruction.imm()) == Some(MemoryStore::V128Store) =>
        {
            Admitted::Store
        }
        _ => return None,
    })
}

/// A run under construction: the registers its bank holds after the steps so far, the
/// constants registers hold, and those whose slots still lack their constant (`pending`).
struct Builder<'a> {
    live: &'a RegisterMasks,
    start: usize,
    steps: Vec<Step>,
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

    /// The index and offset of the `Address` step just before an access at `pc` through
    /// `address`, removed, when nothing after the access reads its register.
    fn folded_address(&mut self, pc: usize, address: u16) -> Option<(u16, u64)> {
        let &Step::Address { a, index, offset } = self.steps.last()? else { return None };
        let live = self.live.get(pc + 1).unwrap_or(RegisterSet::MAX);
        (a == address && index != address && !register_in(live, usize::from(address))).then(|| {
            self.steps.pop();
            (index, offset)
        })
    }

    /// `step`, with the two steps before it, as one when together they compute one
    /// operator whose intermediate vectors liveness drops after the instruction at `pc`: a
    /// rotation (`x << k | x >> (64 - k)`), or `_mm_mul_epu32` (two one-operand u32
    /// shuffles and the extending multiply of their results). Each intermediate is written
    /// after both its sources are read, so the fused step reads what they read.
    fn fused(&self, pc: usize, step: Step) -> Option<Step> {
        let &[.., first, second] = self.steps.as_slice() else {
            return None;
        };
        let live = self.live.get(pc + 1).unwrap_or(RegisterSet::MAX);
        // An intermediate is the result, or dead after the instruction.
        let dropped = |register: u16, result: u16| register == result || !register_in(live, usize::from(register));
        match step {
            Step::Bits { kernel: BitKernel::Or, a, b, c } => {
                let (
                    Step::Count { kernel: BitKernel::Shift(first_shift), a: p, b: x, count: first_count },
                    Step::Count { kernel: BitKernel::Shift(second_shift), a: q, b: y, count: second_count },
                ) = (first, second)
                else {
                    return None;
                };
                let (left, right) = match (first_shift, second_shift) {
                    (BitShift::I64x2Shl, BitShift::I64x2ShrU) => (first_count, second_count),
                    (BitShift::I64x2ShrU, BitShift::I64x2Shl) => (second_count, first_count),
                    _ => return None,
                };
                (((b, c) == (p, q) || (b, c) == (q, p))
                    && p != q
                    && x == y
                    && p != x
                    && q != x
                    && dropped(p, a)
                    && dropped(q, a)
                    && (left % u64::BITS + right % u64::BITS) % u64::BITS == 0)
                    .then_some(Step::Count { kernel: BitKernel::Shift(BitShift::I64x2Rotl), a, b: x, count: left })
            }
            Step::Bits { kernel: BitKernel::I64x2ExtMulLowI32x4U, a, b, c } => {
                let (
                    Step::Shuffle { form: ShuffleForm::Words(first_words), a: p, b: x, c: x_again },
                    Step::Shuffle { form: ShuffleForm::Words(second_words), a: q, b: y, c: y_again },
                ) = (first, second)
                else {
                    return None;
                };
                let one_operand = x == x_again && y == y_again;
                let intermediates = p != q && ![x, y].contains(&p) && ![x, y].contains(&q);
                let (left, right) = if (b, c) == (p, q) {
                    ((x, first_words), (y, second_words))
                } else if (b, c) == (q, p) {
                    ((y, second_words), (x, first_words))
                } else {
                    return None;
                };
                (one_operand && intermediates && dropped(p, a) && dropped(q, a)).then_some(Step::MultiplyWords {
                    a,
                    b: left.0,
                    c: right.0,
                    left: [left.1[0], left.1[1]],
                    right: [right.1[0], right.1[1]],
                })
            }
            _ => None,
        }
    }

    fn in_bank(&self, register: u16) -> bool {
        register_in(self.bank, usize::from(register))
    }

    /// Add the instruction at `pc`; `None` ends the run before it, having added nothing.
    fn add(&mut self, pc: usize, instruction: WideInstruction, admitted: Admitted) -> Option<()> {
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        if [a, b, c].into_iter().any(|register| usize::from(register) >= BANK_REGISTERS) {
            return None;
        }
        // A scalar read of a register the bank holds would need it boxed mid-run.
        if scalar_reads(admitted, b, c).into_iter().flatten().any(|register| self.in_bank(register)) {
            return None;
        }
        match admitted {
            Admitted::Simd { kernel, lane } => self.add_simd(pc, instruction, kernel, lane),
            Admitted::Shuffle(form) => {
                self.vector(pc, b);
                self.vector(pc, c);
                self.steps.push(Step::Shuffle { form, a, b, c });
                self.write(a, true, None);
            }
            Admitted::Move => self.add_move(a, b),
            Admitted::Constant(value) => {
                self.write(a, false, Some(value));
                self.pending |= RegisterSet::bit(usize::from(a));
            }
            Admitted::VectorConstant(bits) => {
                self.steps.push(Step::VectorConstant { a, bits });
                self.write(a, true, None);
            }
            Admitted::I32(operator, immediate) => self.add_i32(pc, instruction, operator, immediate),
            Admitted::Address(offset) => {
                self.scalar(b);
                self.steps.push(Step::Address { a, index: b, offset });
                self.write(a, false, None);
            }
            Admitted::Load => self.add_load(pc, a, c),
            Admitted::Store => self.add_store(pc, a, c),
        }
        Some(())
    }

    /// Add a Simd instruction: its operands are read as vectors or scalars, then one step (fused
    /// with the steps before it where that computes the same) writes the result.
    fn add_simd(&mut self, pc: usize, instruction: WideInstruction, kernel: &'static SimdKernel, lane: u8) {
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        let result = kernel.result_type(lane);
        let count = kernel.right.filter(|&ty| ty == crate::WasmType::I32).and_then(|_| self.constants[usize::from(c)]?.as_int());
        let bits = (is_vector(kernel.left) && kernel.right.is_none_or(|ty| is_vector(ty) || count.is_some()) && is_vector(result))
            .then(|| BitKernel::of(instruction.imm(), count))
            .flatten();
        self.read_simd_operands(pc, kernel, b, c, bits);
        let step = match (bits, count) {
            (Some(kernel @ BitKernel::Shift(_)), Some(count)) => Step::Count { kernel, a, b, count: count as u32 },
            (Some(kernel), _) => Step::Bits { kernel, a, b, c },
            (None, _) if is_vector(result) && is_vector(kernel.left) && kernel.right.is_none_or(is_vector) => Step::Vector {
                bits: kernel.bits,
                lane,
                a,
                b,
                c: if kernel.right.is_some() { c } else { b },
            },
            (None, _) => Step::Lanes {
                kernel,
                lane,
                a: place_of(a, result),
                b: place_of(b, kernel.left),
                c: kernel.right.map(|ty| place_of(c, ty)),
            },
        };
        match self.fused(pc, step) {
            Some(fused) => {
                self.steps.truncate(self.steps.len() - FUSED_STEPS);
                self.steps.push(fused);
            }
            None => self.steps.push(step),
        }
        self.write(a, is_vector(result), None);
    }

    /// Read the operands of a Simd instruction: vectors enter the bank, and scalars are settled
    /// (a folded count reads no slot).
    fn read_simd_operands(&mut self, pc: usize, kernel: &SimdKernel, b: u16, c: u16, bits: Option<BitKernel>) {
        for (register, ty) in [(b, Some(kernel.left)), (c, kernel.right)] {
            match ty {
                Some(ty) if is_vector(ty) => self.vector(pc, register),
                Some(_) if !matches!(bits, Some(BitKernel::Shift(_))) => self.scalar(register),
                _ => {}
            }
        }
    }

    /// Add a move of `b` to `a`: a vector moves in the bank, a pending constant stays pending,
    /// and anything else is copied.
    fn add_move(&mut self, a: u16, b: u16) {
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
            self.steps.push(Step::Move { a, b });
            self.write(a, false, constant);
        }
    }

    /// Add an i32 operator. Only a division can trap, so only then are its operands boxed for
    /// the leave.
    fn add_i32(&mut self, pc: usize, instruction: WideInstruction, operator: I32BinaryOperator, immediate: Option<i32>) {
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        self.scalar(b);
        if immediate.is_none() {
            self.scalar(c);
        }
        // Only a division can trap, which its operator shows dividing by zero; any
        // other operator never leaves, so its step needs no registers boxed.
        let leave = if operator.evaluate(1, 0).is_err() { self.leave(pc) } else { Leave { start: 0, end: 0 } };
        let right = immediate.map_or(I32Right::Register(c), I32Right::Immediate);
        self.steps.push(Step::I32 { operator, a, b, right, at: self.at(pc), leave });
        self.write(a, false, None);
    }

    /// Add a load of `a` from the address in `c`, folded into the `Address` step before it
    /// where that step only computes the address.
    fn add_load(&mut self, pc: usize, a: u16, c: u16) {
        self.scalar(c);
        let leave = self.leave(pc);
        let step = match self.folded_address(pc, c) {
            Some((index, offset)) => Step::LoadAt { a, address: c, index, offset, at: self.at(pc), leave },
            None => Step::Load { a, address: c, at: self.at(pc), leave },
        };
        self.steps.push(step);
        self.write(a, true, None);
    }

    /// Add a store of the vector `a` to the address in `c`, folded as a load's address is.
    fn add_store(&mut self, pc: usize, a: u16, c: u16) {
        self.scalar(c);
        self.vector(pc, a);
        let leave = self.leave(pc);
        let step = match self.folded_address(pc, c) {
            Some((index, offset)) => Step::StoreAt { value: a, address: c, index, offset, at: self.at(pc), leave },
            None => Step::Store { value: a, address: c, at: self.at(pc), leave },
        };
        self.steps.push(step);
    }
}

/// Whether a wasm type is a vector.
fn is_vector(ty: crate::WasmType) -> bool {
    ty == crate::WasmType::V128
}

/// The place a lane operand of type `ty` is read from or written to: the bank for a vector,
/// the slot for a scalar.
fn place_of(register: u16, ty: crate::WasmType) -> Place {
    if is_vector(ty) { Place::Bank(register) } else { Place::I32(register) }
}

/// The registers an admitted instruction reads as scalars, which must not be in the bank.
fn scalar_reads(admitted: Admitted, b: u16, c: u16) -> [Option<u16>; 2] {
    match admitted {
        Admitted::Simd { kernel, .. } => match (is_vector(kernel.left), kernel.right.map(is_vector)) {
            (false, Some(false)) => [Some(b), Some(c)],
            (false, _) => [Some(b), None],
            (true, Some(false)) => [Some(c), None],
            (true, _) => [None, None],
        },
        Admitted::I32(_, None) => [Some(b), Some(c)],
        Admitted::I32(_, Some(_)) | Admitted::Address(_) => [Some(b), None],
        Admitted::Load | Admitted::Store => [Some(c), None],
        _ => [None, None],
    }
}

impl<'a> Builder<'a> {
    /// An empty run starting at record `start`.
    fn new(live: &'a RegisterMasks, start: usize) -> Self {
        Builder {
            live,
            start,
            steps: Vec::new(),
            spills: Vec::new(),
            bank: RegisterSet::default(),
            constants: [None; BANK_REGISTERS],
            pending: RegisterSet::default(),
            vectors: 0,
            last_leave: None,
            unboxed: RegisterSet::default(),
            scalar_written: RegisterSet::default(),
        }
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
        let mut builder = Builder::new(live, pc);
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
mod tests;
