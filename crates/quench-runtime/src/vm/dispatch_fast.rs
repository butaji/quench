//! The fast lane runs straight-line instructions whose whole effect is a
//! register write, a Wasm linear-memory access or a jump. Fields decode at
//! fixed positions and no frame state is published. The lane stops before any
//! instruction that would collect, trap, throw, call or touch a shared memory,
//! and the general path executes that instruction with its full semantics, so
//! lane execution is always an exact prefix of general execution.

use super::*;
use crate::bytecode::{Instr, WideInstruction};
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator};
use crate::wasm::memory::{MemoryLoad, MemoryStorage, MemoryStore};

/// Operand access shared by narrow and wide encodings of lane opcodes.
pub(super) trait LaneFields: Copy {
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

/// The active frame's registers. Lane instructions never resize registers or
/// change the frame stack, so the base stays valid for the whole lane run.
#[derive(Clone, Copy)]
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

/// The unshared memory behind the last memory binding the lane resolved.
/// The binding stays in a register for the whole run and the lane never
/// collects, so the backing it names stays alive at a stable address.
#[derive(Clone, Copy)]
struct MemoryView {
    binding: Value,
    state: *const std::cell::RefCell<crate::wasm::memory::MemoryState>,
}

impl<H: Host> Vm<H> {
    /// Run lane instructions from `pc` until one needs the general path.
    /// Kept out of line so the general loop's code generation, which serves
    /// JavaScript, does not depend on the lane.
    #[inline(never)]
    pub(super) fn run_fast_lane(
        &mut self,
        program: ProgramId,
        frame: usize,
        code: *const Instr,
        wide: *const WideInstruction,
        pc: &mut usize,
    ) {
        let r = Registers(self.frames[frame].registers.as_mut_ptr());
        let mut memory = MemoryView {
            binding: Value::UNDEFINED,
            state: std::ptr::null(),
        };
        loop {
            // SAFETY: residual validation establishes every reachable PC and
            // wide index; the caller passes the active function's code.
            let packed = unsafe { *code.add(*pc) };
            let next = if packed.is_wide() {
                let instruction = unsafe { *wide.add(packed.wide_index()) };
                self.lane_step(
                    program,
                    frame,
                    r,
                    &mut memory,
                    instruction.op(),
                    instruction,
                    *pc,
                )
            } else {
                self.lane_step(
                    program,
                    frame,
                    r,
                    &mut memory,
                    packed.op(),
                    Narrow(packed),
                    *pc,
                )
            };
            match next {
                Some(next) => *pc = next,
                None => return,
            }
        }
    }

    /// The next PC after completing one instruction, or `None` to leave it
    /// to the general path untouched.
    #[inline(always)]
    fn lane_step<I: LaneFields>(
        &mut self,
        program: ProgramId,
        f: usize,
        r: Registers,
        memory: &mut MemoryView,
        op: Op,
        i: I,
        pc: usize,
    ) -> Option<usize> {
        let next = pc + 1;
        match op {
            Op::Move => r.set(i.a(), r.get(i.b())),
            Op::LoadConst => {
                let value = self.programs.constant(program, i.imm() as usize)?;
                r.set(i.a(), value);
            }
            Op::LoadLocalPlain => {
                // SAFETY: validated bytecode bounds the slot by Function.locals,
                // and frame setup sizes locals to that count.
                let value = unsafe { self.read_validated_local(f, i.imm() as usize) };
                r.set(i.a(), value);
            }
            Op::Jump => return self.lane_jump(i.imm(), pc, true),
            Op::JumpFalse => {
                let value = r.get(i.a());
                let truthy = match value.as_int() {
                    Some(integer) => integer != 0,
                    None if value == Value::TRUE => true,
                    None if value == Value::FALSE => false,
                    None => return None,
                };
                if !truthy {
                    return self.lane_jump(i.imm(), pc, true);
                }
            }
            Op::WasmJumpI32Zero => {
                if r.i32(i.a()) == 0 {
                    return self.lane_jump(i.imm(), pc, false);
                }
            }
            Op::WasmJumpI32NonZero => {
                if r.i32(i.a()) != 0 {
                    return self.lane_jump(i.imm(), pc, false);
                }
            }
            Op::WasmJumpI32Equal => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::Equal);
            }
            Op::WasmJumpI32NotEqual => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::NotEqual);
            }
            Op::WasmJumpI32LessSigned => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::LessSigned);
            }
            Op::WasmJumpI32LessUnsigned => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::LessUnsigned);
            }
            Op::WasmJumpI32GreaterSigned => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::GreaterSigned);
            }
            Op::WasmJumpI32GreaterUnsigned => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::GreaterUnsigned);
            }
            Op::WasmJumpI32LessEqualSigned => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::LessEqualSigned);
            }
            Op::WasmJumpI32LessEqualUnsigned => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::LessEqualUnsigned);
            }
            Op::WasmJumpI32GreaterEqualSigned => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::GreaterEqualSigned);
            }
            Op::WasmJumpI32GreaterEqualUnsigned => {
                return self.lane_compare_jump(r, i, pc, I32BinaryOperator::GreaterEqualUnsigned);
            }
            Op::WasmJumpI32EqualImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::Equal);
            }
            Op::WasmJumpI32NotEqualImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::NotEqual);
            }
            Op::WasmJumpI32LessSignedImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::LessSigned);
            }
            Op::WasmJumpI32LessUnsignedImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::LessUnsigned);
            }
            Op::WasmJumpI32GreaterSignedImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::GreaterSigned);
            }
            Op::WasmJumpI32GreaterUnsignedImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::GreaterUnsigned);
            }
            Op::WasmJumpI32LessEqualSignedImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::LessEqualSigned);
            }
            Op::WasmJumpI32LessEqualUnsignedImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::LessEqualUnsigned);
            }
            Op::WasmJumpI32GreaterEqualSignedImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::GreaterEqualSigned);
            }
            Op::WasmJumpI32GreaterEqualUnsignedImmediate => {
                return self.lane_constant_jump(r, i, pc, I32BinaryOperator::GreaterEqualUnsigned);
            }
            Op::WasmInstanceBinding => {
                let value = self
                    .heap
                    .environment_slot(self.frames[f].env, i.imm() as usize)?;
                r.set(i.a(), value);
            }
            Op::WasmGlobalGet => {
                let Some(Cell::WasmGlobal { value, .. }) = self.heap.get(r.get(i.b())) else {
                    return None;
                };
                r.set(i.a(), *value);
            }
            Op::WasmGlobalSet => {
                let Some(Cell::WasmGlobal {
                    value,
                    mutable: true,
                    ..
                }) = self.heap.get_mut(r.get(i.b()))
                else {
                    return None;
                };
                *value = r.get(i.a());
            }
            Op::WasmSelect => {
                let (condition, _) = i.pair();
                let source = if r.i32(condition) != 0 { i.b() } else { i.c() };
                r.set(i.a(), r.get(source));
            }
            Op::WasmFillRegisters => {
                let value = self.programs.constant(program, i.imm() as usize)?;
                for offset in 0..i.c() {
                    r.set(i.b() + offset, value);
                }
            }
            Op::WasmI32Unary => {
                let operator = I32UnaryOperator::from_tag(i.imm())?;
                let crate::WasmValue::I32(value) = operator.apply(r.i32(i.b())) else {
                    return None;
                };
                r.set(i.a(), Value::integer(value));
            }
            Op::WasmI32Add => Self::lane_i32(r, i, I32BinaryOperator::Add, false)?,
            Op::WasmI32Subtract => Self::lane_i32(r, i, I32BinaryOperator::Subtract, false)?,
            Op::WasmI32Multiply => Self::lane_i32(r, i, I32BinaryOperator::Multiply, false)?,
            Op::WasmI32DivideSigned => {
                Self::lane_i32(r, i, I32BinaryOperator::DivideSigned, false)?
            }
            Op::WasmI32DivideUnsigned => {
                Self::lane_i32(r, i, I32BinaryOperator::DivideUnsigned, false)?
            }
            Op::WasmI32RemainderSigned => {
                Self::lane_i32(r, i, I32BinaryOperator::RemainderSigned, false)?
            }
            Op::WasmI32RemainderUnsigned => {
                Self::lane_i32(r, i, I32BinaryOperator::RemainderUnsigned, false)?
            }
            Op::WasmI32And => Self::lane_i32(r, i, I32BinaryOperator::And, false)?,
            Op::WasmI32Or => Self::lane_i32(r, i, I32BinaryOperator::Or, false)?,
            Op::WasmI32Xor => Self::lane_i32(r, i, I32BinaryOperator::Xor, false)?,
            Op::WasmI32ShiftLeft => Self::lane_i32(r, i, I32BinaryOperator::ShiftLeft, false)?,
            Op::WasmI32ShiftRightSigned => {
                Self::lane_i32(r, i, I32BinaryOperator::ShiftRightSigned, false)?
            }
            Op::WasmI32ShiftRightUnsigned => {
                Self::lane_i32(r, i, I32BinaryOperator::ShiftRightUnsigned, false)?
            }
            Op::WasmI32RotateLeft => Self::lane_i32(r, i, I32BinaryOperator::RotateLeft, false)?,
            Op::WasmI32RotateRight => Self::lane_i32(r, i, I32BinaryOperator::RotateRight, false)?,
            Op::WasmI32Equal => Self::lane_i32(r, i, I32BinaryOperator::Equal, false)?,
            Op::WasmI32NotEqual => Self::lane_i32(r, i, I32BinaryOperator::NotEqual, false)?,
            Op::WasmI32LessSigned => Self::lane_i32(r, i, I32BinaryOperator::LessSigned, false)?,
            Op::WasmI32LessUnsigned => {
                Self::lane_i32(r, i, I32BinaryOperator::LessUnsigned, false)?
            }
            Op::WasmI32GreaterSigned => {
                Self::lane_i32(r, i, I32BinaryOperator::GreaterSigned, false)?
            }
            Op::WasmI32GreaterUnsigned => {
                Self::lane_i32(r, i, I32BinaryOperator::GreaterUnsigned, false)?
            }
            Op::WasmI32LessEqualSigned => {
                Self::lane_i32(r, i, I32BinaryOperator::LessEqualSigned, false)?
            }
            Op::WasmI32LessEqualUnsigned => {
                Self::lane_i32(r, i, I32BinaryOperator::LessEqualUnsigned, false)?
            }
            Op::WasmI32GreaterEqualSigned => {
                Self::lane_i32(r, i, I32BinaryOperator::GreaterEqualSigned, false)?
            }
            Op::WasmI32GreaterEqualUnsigned => {
                Self::lane_i32(r, i, I32BinaryOperator::GreaterEqualUnsigned, false)?
            }
            Op::WasmI32AddImmediate => Self::lane_i32(r, i, I32BinaryOperator::Add, true)?,
            Op::WasmI32SubtractImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::Subtract, true)?
            }
            Op::WasmI32MultiplyImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::Multiply, true)?
            }
            Op::WasmI32DivideSignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::DivideSigned, true)?
            }
            Op::WasmI32DivideUnsignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::DivideUnsigned, true)?
            }
            Op::WasmI32RemainderSignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::RemainderSigned, true)?
            }
            Op::WasmI32RemainderUnsignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::RemainderUnsigned, true)?
            }
            Op::WasmI32AndImmediate => Self::lane_i32(r, i, I32BinaryOperator::And, true)?,
            Op::WasmI32OrImmediate => Self::lane_i32(r, i, I32BinaryOperator::Or, true)?,
            Op::WasmI32XorImmediate => Self::lane_i32(r, i, I32BinaryOperator::Xor, true)?,
            Op::WasmI32ShiftLeftImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::ShiftLeft, true)?
            }
            Op::WasmI32ShiftRightSignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::ShiftRightSigned, true)?
            }
            Op::WasmI32ShiftRightUnsignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::ShiftRightUnsigned, true)?
            }
            Op::WasmI32RotateLeftImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::RotateLeft, true)?
            }
            Op::WasmI32RotateRightImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::RotateRight, true)?
            }
            Op::WasmI32EqualImmediate => Self::lane_i32(r, i, I32BinaryOperator::Equal, true)?,
            Op::WasmI32NotEqualImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::NotEqual, true)?
            }
            Op::WasmI32LessSignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::LessSigned, true)?
            }
            Op::WasmI32LessUnsignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::LessUnsigned, true)?
            }
            Op::WasmI32GreaterSignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::GreaterSigned, true)?
            }
            Op::WasmI32GreaterUnsignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::GreaterUnsigned, true)?
            }
            Op::WasmI32LessEqualSignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::LessEqualSigned, true)?
            }
            Op::WasmI32LessEqualUnsignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::LessEqualUnsigned, true)?
            }
            Op::WasmI32GreaterEqualSignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::GreaterEqualSigned, true)?
            }
            Op::WasmI32GreaterEqualUnsignedImmediate => {
                Self::lane_i32(r, i, I32BinaryOperator::GreaterEqualUnsigned, true)?
            }
            Op::WasmI32Load => self.lane_load(r, memory, i, MemoryLoad::I32Load)?,
            Op::WasmI64Load => self.lane_load(r, memory, i, MemoryLoad::I64Load)?,
            Op::WasmF32Load => self.lane_load(r, memory, i, MemoryLoad::F32Load)?,
            Op::WasmF64Load => self.lane_load(r, memory, i, MemoryLoad::F64Load)?,
            Op::WasmI32Load8S => self.lane_load(r, memory, i, MemoryLoad::I32Load8S)?,
            Op::WasmI32Load8U => self.lane_load(r, memory, i, MemoryLoad::I32Load8U)?,
            Op::WasmI32Load16S => self.lane_load(r, memory, i, MemoryLoad::I32Load16S)?,
            Op::WasmI32Load16U => self.lane_load(r, memory, i, MemoryLoad::I32Load16U)?,
            Op::WasmI64Load8S => self.lane_load(r, memory, i, MemoryLoad::I64Load8S)?,
            Op::WasmI64Load8U => self.lane_load(r, memory, i, MemoryLoad::I64Load8U)?,
            Op::WasmI64Load16S => self.lane_load(r, memory, i, MemoryLoad::I64Load16S)?,
            Op::WasmI64Load16U => self.lane_load(r, memory, i, MemoryLoad::I64Load16U)?,
            Op::WasmI64Load32S => self.lane_load(r, memory, i, MemoryLoad::I64Load32S)?,
            Op::WasmI64Load32U => self.lane_load(r, memory, i, MemoryLoad::I64Load32U)?,
            Op::WasmI32Store => self.lane_store(r, memory, i, MemoryStore::I32Store)?,
            Op::WasmI64Store => self.lane_store(r, memory, i, MemoryStore::I64Store)?,
            Op::WasmF32Store => self.lane_store(r, memory, i, MemoryStore::F32Store)?,
            Op::WasmF64Store => self.lane_store(r, memory, i, MemoryStore::F64Store)?,
            Op::WasmI32Store8 => self.lane_store(r, memory, i, MemoryStore::I32Store8)?,
            Op::WasmI32Store16 => self.lane_store(r, memory, i, MemoryStore::I32Store16)?,
            Op::WasmI64Store8 => self.lane_store(r, memory, i, MemoryStore::I64Store8)?,
            Op::WasmI64Store16 => self.lane_store(r, memory, i, MemoryStore::I64Store16)?,
            Op::WasmI64Store32 => self.lane_store(r, memory, i, MemoryStore::I64Store32)?,
            _ => return None,
        }
        Some(next)
    }

    /// A taken jump stays in the lane unless it is a collection safepoint
    /// with a collection due: every `Jump`, and backward Wasm conditionals.
    #[inline(always)]
    fn lane_jump(&self, target: u32, pc: usize, safepoint: bool) -> Option<usize> {
        let target = target as usize;
        if (safepoint || target <= pc) && self.heap.should_collect() {
            return None;
        }
        Some(target)
    }

    #[inline(always)]
    fn lane_compare_jump<I: LaneFields>(
        &self,
        r: Registers,
        i: I,
        pc: usize,
        comparison: I32BinaryOperator,
    ) -> Option<usize> {
        if comparison.evaluate(r.i32(i.a()), r.i32(i.b())).ok()? != 0 {
            self.lane_jump(i.imm(), pc, false)
        } else {
            Some(pc + 1)
        }
    }

    #[inline(always)]
    fn lane_constant_jump<I: LaneFields>(
        &self,
        r: Registers,
        i: I,
        pc: usize,
        comparison: I32BinaryOperator,
    ) -> Option<usize> {
        let right = i32::from(i.signed_b());
        if comparison.evaluate(r.i32(i.a()), right).ok()? != 0 {
            self.lane_jump(i.imm(), pc, false)
        } else {
            Some(pc + 1)
        }
    }

    /// An i32 operator whose result is defined; a trap leaves the lane.
    #[inline(always)]
    fn lane_i32<I: LaneFields>(
        r: Registers,
        i: I,
        operator: I32BinaryOperator,
        immediate: bool,
    ) -> Option<()> {
        let right = if immediate {
            i.imm() as i32
        } else {
            r.i32(i.c())
        };
        let value = operator.evaluate(r.i32(i.b()), right).ok()?;
        r.set(i.a(), Value::integer(value));
        Some(())
    }

    /// The unshared backing and memory32 effective address of a direct access.
    #[inline(always)]
    fn lane_memory<I: LaneFields>(
        &self,
        r: Registers,
        view: &mut MemoryView,
        i: I,
    ) -> Option<(&std::cell::RefCell<crate::wasm::memory::MemoryState>, u64)> {
        let binding = r.get(i.b());
        if view.binding != binding {
            let Some(Cell::WasmMemory { bytes, .. }) = self.heap.get(binding) else {
                return None;
            };
            let MemoryStorage::Unshared(state) = &**bytes else {
                return None;
            };
            *view = MemoryView {
                binding,
                state: std::ptr::from_ref(state),
            };
        }
        let address = u64::from(r.get(i.c()).wasm_bits32()) + u64::from(i.imm());
        // SAFETY: `view.state` came from the live binding just compared; see
        // MemoryView for why the backing outlives the lane run.
        Some((unsafe { &*view.state }, address))
    }

    #[inline(always)]
    fn lane_load<I: LaneFields>(
        &mut self,
        r: Registers,
        view: &mut MemoryView,
        i: I,
        load: MemoryLoad,
    ) -> Option<()> {
        let value = {
            let (state, address) = self.lane_memory(r, view, i)?;
            load.read(&state.try_borrow().ok()?.bytes, address).ok()?
        };
        let value = match value {
            crate::WasmValue::I32(value) => Value::integer(value),
            value => self.encode_wasm_value(value),
        };
        r.set(i.a(), value);
        Some(())
    }

    #[inline(always)]
    fn lane_store<I: LaneFields>(
        &mut self,
        r: Registers,
        view: &mut MemoryView,
        i: I,
        store: MemoryStore,
    ) -> Option<()> {
        let value = if store.value_type() == crate::WasmType::I32 {
            crate::WasmValue::I32(r.i32(i.a()))
        } else {
            self.decode_wasm_value(r.get(i.a()), store.value_type())
                .ok()?
        };
        let (state, address) = self.lane_memory(r, view, i)?;
        store
            .write(&mut state.try_borrow_mut().ok()?.bytes, address, value)
            .ok()
    }
}
