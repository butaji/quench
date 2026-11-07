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

    /// Run the steps; `Err(at)` names the record the general path executes, once the
    /// registers it may read hold their values.
    #[inline(always)]
    fn run<H: Host>(
        &self,
        vm: &mut Vm<H>,
        r: Registers,
        memory: MemoryView,
        integers: crate::value::IntegerEncoding,
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
        macro_rules! leave {
            ($leave:expr, $at:expr) => {{
                self.box_out($leave, &bank, vm, r);
                return Err($at);
            }};
        }
        let place = |bank: &Bank, place: Place| match place {
            // SAFETY: as for `get!`.
            Place::Bank(register) => unsafe { bank.get_unchecked(usize::from(register)).assume_init() },
            Place::I32(register) => u128::from(r.i32(register) as u32),
        };
        for step in &self.steps {
            match *step {
                Step::Unbox { register, at, leave } => match vm.heap.v128(r.get(register)) {
                    Some(bytes) => put!(register, u128::from_le_bytes(bytes)),
                    None => leave!(leave, at),
                },
                Step::Bits { kernel, a, b, c } => put!(a, kernel.apply(get!(b), get!(c))),
                Step::Count { kernel, a, b, count } => put!(a, kernel.apply(get!(b), u128::from(count))),
                Step::Lanes { kernel, lane, a, b, c } => {
                    let left = place(&bank, b);
                    let right = c.map_or(0, |c| place(&bank, c));
                    let bits = kernel.apply_bits(left, right, lane);
                    match a {
                        Place::Bank(register) => put!(register, bits),
                        Place::I32(register) => r.set(register, integers.encode(bits as u32 as i32)),
                    }
                }
                Step::Shuffle { form, a, b, c } => put!(a, form.apply(get!(b), get!(c))),
                Step::MultiplyWords { a, b, c, left, right } => {
                    put!(a, crate::wasm::simd::multiply_words(get!(b), get!(c), left, right))
                }
                Step::VectorMove { a, b } => put!(a, get!(b)),
                Step::Move { a, b } => r.set(a, r.get(b)),
                Step::Constant { a, value } => r.set(a, value),
                Step::VectorConstant { a, bits } => put!(a, bits),
                Step::I32 { operator, a, b, right, at, leave } => {
                    let right = match right {
                        I32Right::Register(register) => r.i32(register),
                        I32Right::Immediate(value) => value,
                    };
                    match operator.evaluate(r.i32(b), right) {
                        Ok(value) => r.set(a, integers.encode(value)),
                        Err(_) => leave!(leave, at),
                    }
                }
                Step::Address { a, index, offset } => {
                    // A memory32 index zero-extends; with the static offset it stays below
                    // 2^33, which a slot holds untagged as `Vm::encode_bits64` would.
                    let address = u64::from(r.i32(index) as u32) + offset;
                    r.set(a, Value::from_bits64(address).expect("an untagged memory32 address"));
                }
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
        if self.end.start != self.end.end {
            self.box_out(self.end, &bank, vm, r);
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
        if let Err(at) = region.run(vm, r, view.memory, view.integers) {
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
    Some(match op {
        Op::WasmSimd => {
            let (kernel, lane) = SimdKernel::of(instruction.imm())?;
            (admitted_type(kernel.left)
                && kernel.right.is_none_or(admitted_type)
                && admitted_type(kernel.result_type(lane)))
            .then_some(Admitted::Simd { kernel, lane })?
        }
        Op::WasmSimdShuffle => Admitted::Shuffle(ShuffleForm::of(heap.v128(constant()?)?)),
        Op::Move => Admitted::Move,
        Op::LoadConst => {
            let value = constant()?;
            match heap.v128(value) {
                Some(bytes) => Admitted::VectorConstant(u128::from_le_bytes(bytes)),
                None => Admitted::Constant(value),
            }
        }
        Op::WasmMemoryAddress if Some(instruction.c()) == memory => {
            Admitted::Address(constant()?.bits64()?)
        }
        Op::WasmMemoryLoad
            if Some(instruction.b()) == memory
                && MemoryLoad::from_tag(instruction.imm()) == Some(MemoryLoad::V128Load) =>
        {
            Admitted::Load
        }
        Op::WasmMemoryStore
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

    /// Read `register` as a vector: enter it into the bank first.
    fn vector(&mut self, pc: usize, register: u16) {
        if !register_in(self.bank, usize::from(register)) {
            let leave = self.leave(pc);
            self.steps.push(Step::Unbox { register, at: self.at(pc), leave });
            self.bank |= 1 << register;
        }
    }

    /// Read `register`'s slot as a scalar: settle its constant first.
    fn scalar(&mut self, register: u16) {
        self.settle(1 << register);
    }

    fn write(&mut self, register: u16, vector: bool, constant: Option<Value>) {
        let bit: RegisterSet = 1 << register;
        self.pending &= !bit;
        if vector {
            self.bank |= bit;
            self.vectors += 1;
        } else {
            self.bank &= !bit;
        }
        self.constants[usize::from(register)] = constant;
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
        let vector = |ty| ty == crate::WasmType::V128;
        // A scalar read of a register the bank holds would need it boxed mid-run.
        let scalar_reads: &[u16] = match admitted {
            Admitted::Simd { kernel, .. } => match (vector(kernel.left), kernel.right.map(vector)) {
                (false, Some(false)) => &[b, c],
                (false, _) => &[b],
                (true, Some(false)) => &[c],
                (true, _) => &[],
            },
            Admitted::I32(_, None) => &[b, c],
            Admitted::I32(_, Some(_)) | Admitted::Address(_) => &[b],
            Admitted::Load | Admitted::Store => &[c],
            _ => &[],
        };
        if scalar_reads.iter().any(|&register| self.in_bank(register)) {
            return None;
        }
        match admitted {
            Admitted::Simd { kernel, lane } => {
                let result = kernel.result_type(lane);
                let count = kernel
                    .right
                    .filter(|&ty| ty == crate::WasmType::I32)
                    .and_then(|_| self.constants[usize::from(c)]?.as_int());
                let bits = (vector(kernel.left)
                    && kernel.right.is_none_or(|ty| vector(ty) || count.is_some())
                    && vector(result))
                .then(|| BitKernel::of(instruction.imm(), count))
                .flatten();
                for (register, ty) in [(b, Some(kernel.left)), (c, kernel.right)] {
                    match ty {
                        Some(ty) if vector(ty) => self.vector(pc, register),
                        // A folded count reads no slot.
                        Some(_) if !matches!(bits, Some(BitKernel::Shift(_))) => self.scalar(register),
                        _ => {}
                    }
                }
                let place = |register, ty| if vector(ty) { Place::Bank(register) } else { Place::I32(register) };
                let step = match (bits, count) {
                    (Some(kernel @ BitKernel::Shift(_)), Some(count)) => {
                        Step::Count { kernel, a, b, count: count as u32 }
                    }
                    (Some(kernel), _) => Step::Bits { kernel, a, b, c },
                    (None, _) => Step::Lanes {
                        kernel,
                        lane,
                        a: place(a, result),
                        b: place(b, kernel.left),
                        c: kernel.right.map(|ty| place(c, ty)),
                    },
                };
                match self.fused(pc, step) {
                    Some(fused) => {
                        self.steps.truncate(self.steps.len() - FUSED_STEPS);
                        self.steps.push(fused);
                    }
                    None => self.steps.push(step),
                }
                self.write(a, vector(result), None);
            }
            Admitted::Shuffle(form) => {
                self.vector(pc, b);
                self.vector(pc, c);
                self.steps.push(Step::Shuffle { form, a, b, c });
                self.write(a, true, None);
            }
            Admitted::Move => {
                let constant = self.constants[usize::from(b)];
                if self.in_bank(b) {
                    self.steps.push(Step::VectorMove { a, b });
                    self.write(a, true, None);
                    self.vectors -= 1;
                } else if register_in(self.pending, usize::from(b)) {
                    // The copy is the constant, pending as its source is.
                    self.write(a, false, constant);
                    self.pending |= 1 << a;
                } else {
                    self.steps.push(Step::Move { a, b });
                    self.write(a, false, constant);
                }
            }
            Admitted::Constant(value) => {
                self.write(a, false, Some(value));
                self.pending |= 1 << a;
            }
            Admitted::VectorConstant(bits) => {
                self.steps.push(Step::VectorConstant { a, bits });
                self.write(a, true, None);
            }
            Admitted::I32(operator, immediate) => {
                self.scalar(b);
                if immediate.is_none() {
                    self.scalar(c);
                }
                // Only a division can trap, which its operator shows dividing by zero; any
                // other operator never leaves, so its step needs no registers boxed.
                let leave = if operator.evaluate(1, 0).is_err() {
                    self.leave(pc)
                } else {
                    Leave { start: 0, end: 0 }
                };
                let right = immediate.map_or(I32Right::Register(c), I32Right::Immediate);
                self.steps.push(Step::I32 { operator, a, b, right, at: self.at(pc), leave });
                self.write(a, false, None);
            }
            Admitted::Address(offset) => {
                self.scalar(b);
                self.steps.push(Step::Address { a, index: b, offset });
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
            spills: Vec::new(),
            bank: 0,
            constants: [None; BANK_REGISTERS],
            pending: 0,
            vectors: 0,
            last_leave: None,
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
            let leave = builder.leave(end);
            let acc = produced[end - 1].unwrap_or(NO_ACCUMULATOR);
            regions.push(VectorRegion {
                first: pc,
                steps: builder.steps.into_boxed_slice(),
                spills: builder.spills.into_boxed_slice(),
                records: end - pc,
                end: leave,
                acc,
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
    fn lane_body() -> Vec<Operator<'static>> {
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

    /// What `lane_body` stores, slot by slot, for shift count `count`.
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
        vm: Vm<crate::SystemHost>,
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
                body(3, vec![ValType::V128, ValType::V128], false, lane_body()),
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
            let view = self.vm.lane_view_any(program, function).expect("a lane view");
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
